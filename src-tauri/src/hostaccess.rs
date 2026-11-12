use crate::config::HostAccessConfig;
use std::{fs, net::{SocketAddr, TcpStream}, path::Path, time::Duration};

#[cfg(any(target_os = "windows", test))]
#[path = "hostaccess_windows.rs"]
mod windows;

// The key comes from inside the VM, so it is rebuilt field by field: anything
// else in this line would become sshd options or a second authorized key.
fn entry(key: &str, cfg: &HostAccessConfig) -> Result<String, String> {
    let fields: Vec<&str> = key.split_whitespace().collect();
    let valid = matches!(fields.as_slice(), [kind, blob, comment] if *kind == "ssh-ed25519" && *comment == cfg.key_comment
        && blob.chars().all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c)));
    if !valid { return Err("虚拟机返回的 SSH 公钥格式不正确".into()); }
    Ok(format!("{} {} {} {}", cfg.key_options, fields[0], fields[1], fields[2]))
}

fn rewrite(home: &Path, cfg: &HostAccessConfig, added: Option<String>) -> Result<(), String> {
    let directory = home.join(".ssh");
    let path = directory.join("authorized_keys");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let current = if path.exists() { fs::read_to_string(&path).map_err(|e| e.to_string())? } else { String::new() };
    let mut lines: Vec<String> = current.lines().filter(|line| !line.ends_with(&cfg.key_comment)).map(str::to_owned).collect();
    lines.extend(added);
    let pending = directory.join("authorized_keys.omarchy-space");
    fs::write(&pending, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&pending, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    fs::rename(pending, path).map_err(|e| e.to_string())
}

pub fn authorize(home: &Path, key: &str, cfg: &HostAccessConfig) -> Result<(), String> { rewrite(home, cfg, Some(entry(key, cfg)?)) }
pub fn revoke(home: &Path, cfg: &HostAccessConfig) -> Result<(), String> { rewrite(home, cfg, None) }

pub fn service_ready(cfg: &HostAccessConfig) -> Result<(), String> {
    let address = SocketAddr::from(([127, 0, 0, 1], cfg.port));
    let profile = crate::config::RuntimeProfile::load(&crate::config::platform())?;
    TcpStream::connect_timeout(&address, Duration::from_secs(cfg.check_timeout_seconds))
        .map_err(|error| format!("{}（{error}）", profile.host_access_service_hint))?;
    Ok(())
}

pub fn ensure_service(cfg: &HostAccessConfig, setup_directory: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    { windows::ensure(cfg, setup_directory) }
    #[cfg(not(target_os = "windows"))]
    { let _ = setup_directory; service_ready(cfg) }
}

pub fn host_user() -> Result<String, String> {
    std::env::var(if cfg!(windows) { "USERNAME" } else { "USER" }).map_err(|_| "无法读取本机用户名".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authorization_uses_original_home_with_spaces_and_preserves_other_keys() {
        let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evidence/2026-11-04-windows-host-ssh/key-fixtures")
            .join(uuid::Uuid::new_v4().to_string()).join("Example User");
        fs::create_dir_all(home.join(".ssh")).unwrap();
        let path = home.join(".ssh/authorized_keys");
        let unrelated = "ssh-ed25519 AAAA unrelated-test-key\n";
        fs::write(&path, unrelated).unwrap();
        let cfg = crate::config::ProductConfig::load().host_access;
        authorize(&home, "ssh-ed25519 AAAA omarchy-space-host", &cfg).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(unrelated));
        assert!(text.contains(&format!("{} ssh-ed25519 AAAA omarchy-space-host", cfg.key_options)));
        revoke(&home, &cfg).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), unrelated);
    }
}
