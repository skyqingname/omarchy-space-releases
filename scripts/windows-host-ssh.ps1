param([switch]$DefinitionsOnly)

# Only the standard administrator key override is removed. Other Match blocks
# keep their scope and settings; unrelated existing directives are preserved.
function ConvertTo-OmarchySshdConfig {
    param([string]$Text, [int]$Port)
    if ($Text -match '(?im)^\s*Include\s+') {
        throw 'sshd_config 含 Include 外部配置，无法保证只监听本机；尚未修改配置。请先合并外部配置后重试。'
    }
    $lines = $Text -split '\r?\n'
    $output = [System.Collections.Generic.List[string]]::new()
    $output.Add("# Omarchy Space: local host access`r`nListenAddress 127.0.0.1`r`nPort $Port`r`nAuthorizedKeysFile .ssh/authorized_keys`r`n# Omarchy Space: end")
    $globalScope = $true
    for ($i = 0; $i -lt $lines.Length; $i++) {
        $line = $lines[$i]
        if ($line -match '^# Omarchy Space: (local host access|end)$') { continue }
        if ($line -match '^\s*Match\s+Group\s+administrators\s*(#.*)?$') {
            $globalScope = $false
            $block = [System.Collections.Generic.List[string]]::new()
            $hasOtherSettings = $false
            while ($i + 1 -lt $lines.Length -and $lines[$i + 1] -notmatch '^\s*Match\s+') {
                $i++
                $entry = $lines[$i]
                if ($entry -match '^\s*AuthorizedKeysFile\s+') { $entry = '# ' + $entry }
                elseif ($entry -match '^\s*[^#\s]') { $hasOtherSettings = $true }
                $block.Add($entry)
            }
            if ($hasOtherSettings) { $output.Add($line) } else { $output.Add('# ' + $line) }
            $output.AddRange($block)
            continue
        }
        if ($line -match '^\s*Match\s+') { $globalScope = $false }
        if ($globalScope -and $line -match '^\s*(ListenAddress|Port|AuthorizedKeysFile)\s+') { $line = '# ' + $line }
        $output.Add($line)
    }
    return ($output -join "`r`n").TrimEnd() + "`r`n"
}

function Invoke-OmarchyHostSshSetup {
    param([int]$Port)
    $stage = '检查 OpenSSH 服务器'
    $backup = $null
    $previousStartType = $null
    $previousStatus = $null
    try {
        $capabilityName = 'OpenSSH.Server~~~~0.0.1.0'
        $capability = Get-WindowsCapability -Online -Name $capabilityName -ErrorAction Stop
        if ($capability.State -ne 'Installed') {
            $stage = '安装 OpenSSH 服务器'
            $installation = Add-WindowsCapability -Online -Name $capabilityName -ErrorAction Stop
            if ($installation.RestartNeeded) { throw 'Windows 已安装组件，但要求重启电脑；重启后请再次打开本机访问。' }
        }
        $stage = '准备仅本机可访问的 SSH 配置'
        $directory = Join-Path $env:ProgramData 'ssh'
        $config = Join-Path $directory 'sshd_config'
        $template = Join-Path $env:SystemRoot 'System32\OpenSSH\sshd_config_default'
        if (-not (Test-Path -LiteralPath $directory)) {
            $stage = '创建 OpenSSH 数据目录并设置权限'
            New-Item -ItemType Directory -Path $directory -ErrorAction Stop | Out-Null
            # Match sshd's own create_prgdata_ssh_folder permissions. Existing
            # directories retain their administrator-managed ACL unchanged.
            $acl = Get-Acl -LiteralPath $directory -ErrorAction Stop
            $acl.SetSecurityDescriptorSddlForm('O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;AU)')
            Set-Acl -LiteralPath $directory -AclObject $acl -ErrorAction Stop
        }
        $stage = '准备仅本机可访问的 SSH 配置'
        $existing = Test-Path -LiteralPath $config
        $source = if ($existing) { $config } else { $template }
        $text = [System.IO.File]::ReadAllText($source)
        $updated = ConvertTo-OmarchySshdConfig -Text $text -Port $Port
        if ($existing) {
            $backup = $config + '.omarchy-space-' + (Get-Date -Format 'yyyyMMddHHmmssfff') + '.bak'
            Copy-Item -LiteralPath $config -Destination $backup -ErrorAction Stop
        }
        $service = Get-Service -Name sshd -ErrorAction Stop
        $previousStartType = $service.StartType.ToString()
        $previousStatus = $service.Status.ToString()
        if ($service.Status -eq 'Running') { Stop-Service -Name sshd -ErrorAction Stop }
        [System.IO.File]::WriteAllText($config, $updated, [System.Text.UTF8Encoding]::new($false))
        # Write the loopback-only config before the first start; sshd generates
        # its host keys without ever listening on a LAN interface.
        $stage = '首次启动 sshd'
        Start-Service -Name sshd -ErrorAction Stop
        $stage = '设置 sshd 自动启动'
        Set-Service -Name sshd -StartupType Automatic -ErrorAction Stop
        $stage = '重启 sshd'
        Restart-Service -Name sshd -ErrorAction Stop
        return @{success=$true; stage='完成'; error=$null; config_backup=$backup; previous_start_type=$previousStartType; previous_status=$previousStatus}
    } catch {
        return @{success=$false; stage=$stage; error=($_ | Out-String).Trim(); config_backup=$backup; previous_start_type=$previousStartType; previous_status=$previousStatus}
    }
}

if ($DefinitionsOnly) { return }
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$request = [System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String('__REQUEST_JSON_BASE64__')) | ConvertFrom-Json
$result = Invoke-OmarchyHostSshSetup -Port $request.port
[System.IO.File]::WriteAllText($request.result_path, ($result | ConvertTo-Json -Depth 4), [System.Text.UTF8Encoding]::new($false))
if (-not $result.success) { exit 1 }
