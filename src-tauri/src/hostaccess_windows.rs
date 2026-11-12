use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::Path;

fn configuration_ready(text: &str, port: u16) -> bool {
    let text = text.replace("\r\n", "\n");
    let header = format!("# Omarchy Space: local host access\nListenAddress 127.0.0.1\nPort {port}\nAuthorizedKeysFile .ssh/authorized_keys\n# Omarchy Space: end\n");
    if !text.starts_with(&header) { return false; }
    let mut administrator = false;
    let mut global = true;
    for line in text[header.len()..].lines() {
        let active = line.split('#').next().unwrap_or("").trim();
        let fields: Vec<_> = active.split_whitespace().collect();
        if fields.is_empty() { continue; }
        match fields[0].to_ascii_lowercase().as_str() {
            "include" | "listenaddress" | "port" => return false,
            "match" => {
                global = false;
                administrator = fields.len() == 3 && fields[1].eq_ignore_ascii_case("group") && fields[2].eq_ignore_ascii_case("administrators");
            },
            "authorizedkeysfile" if global || administrator => {
                if fields.get(1) != Some(&".ssh/authorized_keys") { return false; }
            },
            _ => (),
        }
    }
    true
}

fn setup_command(result_path: &Path, port: u16) -> String {
    let request = serde_json::json!({"result_path":result_path,"port":port});
    let script = include_str!("../../scripts/windows-host-ssh.ps1")
        .replace("__REQUEST_JSON_BASE64__", &STANDARD.encode(request.to_string().as_bytes()));
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!("-NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand {}", STANDARD.encode(bytes))
}

#[cfg(target_os = "windows")]
pub fn ensure(cfg: &crate::config::HostAccessConfig, directory: &Path) -> Result<(), String> {
    use std::{fs, path::PathBuf};
    use serde::Deserialize;
    let program_data = std::env::var_os("ProgramData").ok_or("无法读取 Windows ProgramData 目录")?;
    let config_path = PathBuf::from(program_data).join("ssh/sshd_config");
    if super::service_ready(cfg).is_ok() && fs::read_to_string(&config_path).map(|text| configuration_ready(&text, cfg.port)).unwrap_or(false) {
        return Ok(());
    }
    fs::create_dir_all(directory).map_err(|error| format!("无法创建 SSH 设置记录目录：{error}"))?;
    let result_path = directory.join(format!("{}.json", uuid::Uuid::new_v4()));
    let exit_code = run_elevated(&setup_command(&result_path, cfg.port))?;
    #[derive(Deserialize)]
    struct Outcome { success: bool, stage: String, error: Option<String> }
    let bytes = fs::read(&result_path).map_err(|error| format!("Windows SSH 设置未返回结果（退出码 {exit_code}）：{error}"))?;
    let result: Outcome = serde_json::from_slice(&bytes).map_err(|error| format!("Windows SSH 设置结果无法读取：{error}"))?;
    if !result.success {
        return Err(format!("{}失败：{}", result.stage, result.error.unwrap_or_else(|| format!("进程退出码 {exit_code}"))));
    }
    if exit_code != 0 { return Err(format!("Windows SSH 设置进程异常退出：{exit_code}")); }
    super::service_ready(cfg)
}

#[cfg(target_os = "windows")]
fn run_elevated(arguments: &str) -> Result<u32, String> {
    use std::{os::windows::ffi::OsStrExt, ffi::OsStr, path::PathBuf};
    use windows_sys::Win32::{Foundation::{CloseHandle, WAIT_FAILED},
        System::{Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE}, Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE}},
        UI::{Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NOASYNC, SEE_MASK_FLAG_NO_UI}, WindowsAndMessaging::SW_HIDE}};
    fn wide(value: &OsStr) -> Vec<u16> { value.encode_wide().chain(Some(0)).collect() }
    let system_root = std::env::var_os("SystemRoot").ok_or("无法读取 Windows 系统目录")?;
    let executable = PathBuf::from(system_root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let verb = wide(OsStr::new("runas"));
    let file = wide(executable.as_os_str());
    let parameters = wide(OsStr::new(arguments));
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_HIDE;
    let initialized = unsafe { CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32) };
    if initialized < 0 { return Err(format!("无法初始化 Windows 管理员授权（HRESULT 0x{:08X}）", initialized as u32)); }
    let launched = unsafe { ShellExecuteExW(&mut info) };
    let launch_error = if launched == 0 { Some(std::io::Error::last_os_error()) } else { None };
    unsafe { CoUninitialize(); }
    if let Some(error) = launch_error {
        return Err(if error.raw_os_error() == Some(1223) {
            format!("已取消 Windows 管理员授权，本机访问未打开：{error}")
        } else { format!("无法启动 Windows SSH 管理员设置：{error}") });
    }
    let mut exit_code = 0;
    let result = if unsafe { WaitForSingleObject(info.hProcess, INFINITE) } == WAIT_FAILED {
        Err(format!("等待 Windows SSH 设置失败：{}", std::io::Error::last_os_error()))
    } else if unsafe { GetExitCodeProcess(info.hProcess, &mut exit_code) } == 0 {
        Err(format!("无法读取 Windows SSH 设置退出状态：{}", std::io::Error::last_os_error()))
    } else { Ok(exit_code) };
    unsafe { CloseHandle(info.hProcess); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_managed_loopback_configuration_skips_elevation() {
        let managed = "# Omarchy Space: local host access\nListenAddress 127.0.0.1\nPort 22\nAuthorizedKeysFile .ssh/authorized_keys\n# Omarchy Space: end\n";
        assert!(configuration_ready(managed, 22));
        assert!(configuration_ready(&managed.replace('\n', "\r\n"), 22));
        assert!(!configuration_ready(managed, 2222));
        for extra in ["ListenAddress 0.0.0.0\n", "Include other.conf\n", "Match Group administrators\n AuthorizedKeysFile __PROGRAMDATA__/ssh/administrators_authorized_keys\n"] {
            assert!(!configuration_ready(&(managed.to_owned() + extra), 22));
        }
        assert!(!configuration_ready("#Port 22\nMatch Group administrators\n AuthorizedKeysFile __PROGRAMDATA__/ssh/administrators_authorized_keys\n", 22));
    }
    #[test]
    fn elevated_request_keeps_space_and_unicode_paths_out_of_command_syntax() {
        let path = Path::new("C:\\Users\\Example User\\文档\\result.json");
        let command = setup_command(path, 22);
        assert!(command.len() < 30000);
        let bytes = STANDARD.decode(command.split_whitespace().last().unwrap()).unwrap();
        let utf16: Vec<u16> = bytes.chunks_exact(2).map(|p| u16::from_le_bytes([p[0],p[1]])).collect();
        let script = String::from_utf16(&utf16).unwrap();
        let marker = "FromBase64String('";
        let encoded = script.split(marker).nth(1).unwrap().split('\'').next().unwrap();
        let request: serde_json::Value = serde_json::from_slice(&STANDARD.decode(encoded).unwrap()).unwrap();
        assert_eq!(request["result_path"], path.to_str().unwrap());
        assert_eq!(request["port"], 22);
        assert!(!script.contains("Example User"));
    }
}
