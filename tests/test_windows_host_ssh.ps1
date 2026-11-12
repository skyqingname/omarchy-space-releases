$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../scripts/windows-host-ssh.ps1') -DefinitionsOnly
function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
$stock = @'
#Port 22
ListenAddress 0.0.0.0
ListenAddress ::
AuthorizedKeysFile .ssh/authorized_keys
Subsystem sftp sftp-server.exe
Match Group administrators
    AuthorizedKeysFile __PROGRAMDATA__/ssh/administrators_authorized_keys
'@
$converted = ConvertTo-OmarchySshdConfig $stock 22
Assert ($converted -match '(?m)^ListenAddress 127\.0\.0\.1\r?$') 'Loopback binding missing'
Assert ($converted -notmatch '(?m)^ListenAddress (0\.0\.0\.0|::)') 'Broad listener retained'
Assert ($converted -notmatch '(?m)^Match Group administrators') 'Default administrator Match still active'
Assert ($converted -notmatch '(?m)^\s*AuthorizedKeysFile __PROGRAMDATA__') 'Administrator key override still active'
Assert ($converted -match 'Subsystem sftp sftp-server.exe') 'Unrelated directive lost'
$custom = ConvertTo-OmarchySshdConfig ($stock + "`n    AllowTcpForwarding no`nMatch User other`n    ForceCommand internal-sftp") 22
Assert ($custom -match '(?m)^Match Group administrators') 'Custom administrator policy lost scope'
Assert ($custom -match 'AllowTcpForwarding no') 'Custom administrator policy changed'
Assert ($custom -match 'Match User other\r?\n    ForceCommand internal-sftp') 'Other Match block changed'
try { ConvertTo-OmarchySshdConfig 'Include other.conf' 22; throw 'Expected Include rejection' }
catch { Assert ($_.Exception.Message -match 'Include 外部配置') 'Unexpected Include rejection' }

$fixture = Join-Path $PSScriptRoot ('../evidence/2026-11-04-windows-host-ssh/ps-fixtures/' + [guid]::NewGuid().ToString())
$env:ProgramData = Join-Path $fixture 'Program Data'
$env:SystemRoot = Join-Path $fixture 'Windows'
$template = Join-Path $env:SystemRoot 'System32\OpenSSH\sshd_config_default'
New-Item -ItemType Directory -Path (Split-Path $template) -Force | Out-Null
[IO.File]::WriteAllText($template, $stock)
$script:events = [Collections.Generic.List[string]]::new()
$script:installed = $false
$script:failure = ''
function Get-WindowsCapability { param([switch]$Online, $Name, $ErrorAction); @{State=$(if($script:installed){'Installed'}else{'NotPresent'})} }
function Add-WindowsCapability { param([switch]$Online, $Name, $ErrorAction); $script:events.Add('install'); if($script:failure -eq 'install'){throw 'DISM 0x800f0954: Windows Update unavailable'}; @{RestartNeeded=$false} }
function Get-Service { param($Name, $ErrorAction); @{Status='Stopped';StartType='Manual'} }
function Stop-Service { param($Name, $ErrorAction); $script:events.Add('stop') }
function Get-Acl {
    param($LiteralPath, $ErrorAction)
    $acl = [pscustomobject]@{Sddl=$null}
    $acl | Add-Member -MemberType ScriptMethod -Name SetSecurityDescriptorSddlForm -Value { param($sddl); $this.Sddl = $sddl }
    return $acl
}
function Set-Acl {
    param($LiteralPath, $AclObject, $ErrorAction)
    $script:events.Add('acl')
    Assert ($AclObject.Sddl -eq 'O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;AU)') 'Directory ACL differs from sshd default'
    Assert ($LiteralPath -eq (Join-Path $env:ProgramData 'ssh')) 'Wrong directory ACL target'
    Assert (-not (Test-Path -LiteralPath (Join-Path $LiteralPath 'sshd_config'))) 'ACL must be set before configuration and host key creation'
    if ($script:failure -eq 'acl') { throw 'Access denied while setting the OpenSSH directory ACL' }
}
function Start-Service {
    param($Name, $ErrorAction)
    $script:events.Add('start')
    $text = [IO.File]::ReadAllText((Join-Path $env:ProgramData 'ssh/sshd_config'))
    Assert ($text.StartsWith("# Omarchy Space: local host access`r`nListenAddress 127.0.0.1")) 'Service started before loopback configuration'
}
function Set-Service { param($Name, $StartupType, $ErrorAction); Assert ($StartupType -eq 'Automatic') 'Wrong startup type'; $script:events.Add('automatic') }
function Restart-Service { param($Name, $ErrorAction); $script:events.Add('restart'); if($script:failure -eq 'restart'){throw 'Access denied to sshd restart'} }
$result = Invoke-OmarchyHostSshSetup 22
Assert ($result.success) ('First installation failed: '+$result.error)
Assert (($script:events -join ',') -eq 'install,acl,start,automatic,restart') 'New directory ACL must precede first service start'
$original = [IO.File]::ReadAllText((Join-Path $env:ProgramData 'ssh/sshd_config'))
$script:installed = $true
$script:events.Clear()
$result = Invoke-OmarchyHostSshSetup 22
Assert ($result.success) ('Existing installation failed: '+$result.error)
Assert (($script:events -join ',') -eq 'start,automatic,restart') 'Existing directory ACL changed or installed capability was reinstalled'
Assert ([IO.File]::ReadAllText($result.config_backup) -eq $original) 'Original config backup is incorrect'
$script:installed = $false
$script:failure = 'install'
$script:events.Clear()
$result = Invoke-OmarchyHostSshSetup 22
Assert (-not $result.success -and $result.stage -eq '安装 OpenSSH 服务器' -and $result.error.Contains('0x800f0954')) 'Installation error lost'
Assert (($script:events -join ',') -eq 'install') 'Service started after failed installation'
$savedProgramData = $env:ProgramData
$env:ProgramData = Join-Path $fixture 'ACL Failure Data'
$script:installed = $true
$script:failure = 'acl'
$script:events.Clear()
$result = Invoke-OmarchyHostSshSetup 22
Assert (-not $result.success -and $result.stage -eq '创建 OpenSSH 数据目录并设置权限' -and $result.error.Contains('Access denied')) 'ACL failure was hidden'
Assert (($script:events -join ',') -eq 'acl') 'Service started after ACL failure'
Assert (-not (Test-Path -LiteralPath (Join-Path $env:ProgramData 'ssh/sshd_config'))) 'Configuration was written after ACL failure'
$env:ProgramData = $savedProgramData
$script:installed = $true
$script:failure = 'restart'
$result = Invoke-OmarchyHostSshSetup 22
Assert (-not $result.success -and $result.stage -eq '重启 sshd' -and $result.error.Contains('Access denied')) 'Restart failure was hidden'
Assert ($result.previous_start_type -eq 'Manual' -and $result.previous_status -eq 'Stopped') 'Failure lost the original service state needed for rollback'
Write-Output 'PASS: configuration scopes, new-directory ACL before writes/start, existing-directory ACL preserved, ACL failure stops, loopback-before-start, install/no-reinstall, backup, install failure, restart failure. Windows cmdlets and ACL mocked; no system SSH modified.'
