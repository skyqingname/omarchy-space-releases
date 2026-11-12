use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};

#[derive(Clone, Deserialize, Serialize)]
pub struct Resources { pub cpus: u32, pub memory_mib: u64, pub disk_gib: u64 }
#[derive(Clone, Deserialize, Serialize)]
pub struct Limits { pub min_cpus: u32, pub min_memory_mib: u64, pub max_memory_mib: u64, pub host_memory_mib: u64, pub min_free_disk_gib: u64, pub max_disk_gib: u64 }
#[derive(Clone, Deserialize, Serialize)]
pub struct Subscription { pub timeout_seconds: u64, pub user_agent: String }
#[derive(Clone, Deserialize, Serialize)]
pub struct VmConfig {
    pub zstd_memory_mib: u64,
    pub provision_port_name: String, pub clipboard_port_name: String, pub boot_timeout_seconds: u64,
    pub provision_timeout_seconds: u64, pub shutdown_timeout_seconds: u64, pub shutdown_retry_seconds: u64,
    pub provision_service_timeout: String,
    pub status_poll_milliseconds: u64, pub desktop_poll_milliseconds: u64,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct NetworkConfig {
    pub dns_server: String, pub bootstrap_dns: String, pub dns_strategy: String, pub mixed_port: u16,
    pub tun_interface: String, pub tun_addresses: Vec<String>, pub tun_stack: String,
    pub probe_url: String, pub probe_timeout_seconds: u64,
    pub probe_attempts: u32, pub probe_retry_seconds: u64,
    pub startup_timeout_seconds: u64, pub startup_poll_seconds: f64,
    pub guard: NetworkGuard,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct NetworkGuard {
    pub version: u32, pub table: String, pub service_name: String, pub service_user: String,
    pub rules_path: String, pub reconnect_hook_path: String, pub dns_helper_path: String,
    pub configuration_lock_path: String,
    pub restart_seconds: u64, pub dhcp_server: String, pub blocked_destination_cidrs: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct HostAccessConfig { pub address: String, pub port: u16, pub alias: String, pub key_comment: String, pub key_options: String, pub check_timeout_seconds: u64 }
#[derive(Clone, Deserialize, Serialize)]
pub struct TerminalConfig {
    pub guest_port: u16, pub forward_address: String, pub ssh_service: String, pub host_key_alias: String,
    pub key_name: String, pub key_comment: String, pub authorized_keys_marker: String, pub known_hosts_name: String,
    pub session_record_name: String, pub follow_log_name: String, pub ssh_options: Vec<String>, pub term: String,
    pub working_directory: String, pub claude_arguments: Vec<String>, pub claude_settings: Value,
    pub claude_projects_path: String, pub claude_credentials_path: String, pub claude_state_path: String,
    pub claude_user_settings_path: String, pub runtime_directory: String, pub settings_name: String,
    pub hooks_name: String, pub meter_name: String, pub hook_events: Vec<String>, pub hook_fields: Vec<String>,
    pub meter_fields: Vec<String>, pub record_types: Vec<String>, pub attachment_types: Vec<String>, pub skipped_system_subtypes: Vec<String>,
    pub link_schemes: Vec<String>, pub link_command: String,
    pub read_chunk_bytes: usize, pub history_limit: usize, pub history_title_chars: usize,
    pub follow_poll_seconds: f64, pub status_poll_seconds: f64, pub submit_delay_milliseconds: u64,
    pub read_buffer_bytes: usize, pub output_gather_milliseconds: u64, pub output_gather_bytes: usize,
    pub tool_output_max_chars: usize, pub tool_summary_max_chars: usize,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct BrowserConfig {
    pub package: String, pub output: String, pub guest_port: u16, pub window_class: String,
    pub launch_command: String, pub refresh_rate: u32, pub max_fps: u32, pub poll_seconds: f64, pub relaunch_seconds: f64,
    pub ready_timeout_seconds: u64, pub log_name: String, pub read_buffer_bytes: usize,
    pub initial_width: u32, pub initial_height: u32, pub resize_debounce_milliseconds: u64,
    pub quality_level: u32, pub compression_level: u32,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct ClipboardConfig { pub poll_milliseconds: u64, pub max_bytes: usize }
#[derive(Clone, Deserialize, Serialize)]
pub struct AttachmentConfig { pub directory: String, pub max_file_bytes: u64, pub max_files: usize, pub chunk_bytes: usize, pub timeout_seconds: u64 }
#[derive(Clone, Deserialize, Serialize)]
pub struct DownloadConfig { pub user_agent: String, pub connect_timeout_seconds: u64, pub buffer_bytes: usize, pub report_milliseconds: u64 }
#[derive(Clone, Deserialize, Serialize)]
pub struct ProductConfig {
    pub name: String, pub full_name: String, pub version: String, pub defaults: Resources, pub limits: Limits,
    pub subscription: Subscription, pub download: DownloadConfig, pub clipboard: ClipboardConfig, pub attachments: AttachmentConfig, pub host_access: HostAccessConfig, pub terminal: TerminalConfig, pub browser: BrowserConfig, pub vm: VmConfig, pub network: NetworkConfig, pub guest: Value, pub region: Value,
}
impl ProductConfig {
    pub fn load() -> Self { serde_json::from_str(include_str!("../../product.config.json")).expect("product.config.json") }
}

// Client policy comes from the bundled source profile, not an external pack's
// manifest: existing offline packs must remain usable without new metadata.
#[derive(Clone, Deserialize, Serialize)]
pub struct RuntimeProfile {
    pub min_disk_gib: u64, pub host_access_service_hint: String, pub terminal: TerminalTools,
    pub software_graphics: Option<SoftwareGraphics>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct TerminalTools { pub ssh: String, pub ssh_keygen: String }

#[derive(Clone, Deserialize, Serialize)]
pub struct SoftwareGraphics {
    pub device: String, pub display: String, pub vga: String, pub kernel_parameter: String,
    pub environment: BTreeMap<String, String>,
}

impl SoftwareGraphics {
    // Overlay only display-related options. Existing offline pack manifests
    // keep their image, disk, CPU, firmware, network and control-channel data.
    pub fn arguments(&self, original: &[String]) -> Result<Vec<String>, String> {
        let mut args = original.to_vec();
        let mut devices = 0;
        let mut displays = 0;
        let mut kernels = 0;
        let mut vga = false;
        let (kernel_key, _) = self.kernel_parameter.split_once('=').ok_or("兼容显示的内核参数配置不正确")?;
        let prefix = format!("{kernel_key}=");
        for i in 0..args.len().saturating_sub(1) {
            match args[i].as_str() {
                "-device" if matches!(args[i + 1].split(',').next(), Some("virtio-vga-gl" | "virtio-gpu-gl-pci" | "virtio-vga" | "virtio-gpu-pci")) => {
                    args[i + 1] = self.device.clone(); devices += 1;
                },
                "-display" => { args[i + 1] = self.display.clone(); displays += 1; },
                "-vga" => { args[i + 1] = self.vga.clone(); vga = true; },
                "-append" => {
                    let mut parameters: Vec<_> = args[i + 1].split_whitespace().filter(|part| !part.starts_with(&prefix)).map(str::to_owned).collect();
                    parameters.push(self.kernel_parameter.clone());
                    args[i + 1] = parameters.join(" "); kernels += 1;
                },
                _ => (),
            }
        }
        if devices != 1 || displays != 1 || kernels != 1 {
            return Err("资源包的图形或内核参数不符合兼容显示要求，尚未启动虚拟机".into());
        }
        if !vga { args.extend(["-vga".into(), self.vga.clone()]); }
        Ok(args)
    }
}
impl RuntimeProfile {
    pub fn load(platform: &str) -> Result<Self, String> {
        let mut profiles: BTreeMap<String, Self> = serde_json::from_str(include_str!("../../runtime-profiles.json"))
            .map_err(|error| format!("平台运行配置格式不正确：{error}"))?;
        profiles.remove(platform).ok_or_else(|| format!("缺少平台运行配置：{platform}"))
    }

    pub fn validate_disk(&self, disk_gib: u64, free_disk_gib: u64, limits: &Limits) -> Result<(), String> {
        if disk_gib < self.min_disk_gib {
            return Err(format!("当前平台的虚拟磁盘至少需要 {} GB", self.min_disk_gib));
        }
        let maximum = free_disk_gib.min(limits.max_disk_gib);
        if disk_gib > maximum {
            return Err(format!("所选位置当前最多可分配 {maximum} GB 虚拟磁盘，请调整容量或更换存放位置"));
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Download { pub url: String, pub bytes: u64 }
#[derive(Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub platform: String, pub image: String, pub image_format: String, pub image_version: String,
    pub qemu: String, pub zstd: String, pub sing_box_package: String,
    pub efi_variables_bytes: u64, pub arguments: Vec<String>,
    pub network_device_id: String,
    pub downloads: BTreeMap<String, Vec<Download>>,
}
#[derive(Clone)]
pub struct AssetPack { pub root: PathBuf, pub images: PathBuf, pub manifest: Manifest }
impl AssetPack {
    pub fn load(root: &Path, data_dir: &Path) -> Result<Self, String> {
        let bytes = fs::read(root.join("manifest.json")).map_err(|_| "未找到离线资源包的 manifest.json".to_string())?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| "资源包清单格式不正确".to_string())?;
        if manifest.platform != platform() { return Err("资源包与当前电脑的平台不匹配".into()); }
        for name in [&manifest.qemu, &manifest.sing_box_package] {
            if !root.join(name).is_file() { return Err(format!("离线资源包缺少文件：{name}")); }
        }
        if manifest.image_format == "zstd" && !root.join(&manifest.zstd).is_file() {
            return Err("离线资源包缺少解压程序".into());
        }
        let images = if root.join(&manifest.image).is_file() { root.to_path_buf() } else { data_dir.join("omarchy-image") };
        Ok(Self { root: root.to_path_buf(), images, manifest })
    }
    pub fn image_ready(&self) -> bool { self.manifest.downloads.keys().all(|name| self.images.join(name).is_file()) }
    pub fn download_bytes(&self) -> u64 { self.manifest.downloads.values().flatten().map(|part| part.bytes).sum() }
}
pub fn platform() -> String { format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_disk_boundaries_apply_without_changing_global_defaults() {
        let cfg = ProductConfig::load();
        let windows = RuntimeProfile::load("windows-x86_64").unwrap();
        let mac = RuntimeProfile::load("macos-aarch64").unwrap();
        assert!(windows.validate_disk(16, 64, &cfg.limits).is_ok());
        assert!(windows.validate_disk(15, 64, &cfg.limits).is_err());
        assert!(mac.validate_disk(16, 64, &cfg.limits).is_err());
        assert!(mac.validate_disk(31, 64, &cfg.limits).is_err());
        assert!(mac.validate_disk(32, 64, &cfg.limits).is_ok());
        assert_eq!(cfg.defaults.disk_gib, 64);
        assert_eq!(cfg.limits.min_free_disk_gib, 20);
        assert!(RuntimeProfile::load("unsupported-platform").is_err());
    }

    #[test]
    fn disk_limit_still_follows_selected_volume_and_product_maximum() {
        let cfg = ProductConfig::load();
        for name in ["windows-x86_64", "macos-aarch64"] {
            let profile = RuntimeProfile::load(name).unwrap();
            assert!(profile.validate_disk(40, 40, &cfg.limits).is_ok());
            assert!(profile.validate_disk(41, 40, &cfg.limits).is_err());
            assert!(profile.validate_disk(512, 1024, &cfg.limits).is_ok());
            assert!(profile.validate_disk(513, 1024, &cfg.limits).is_err());
            assert!(profile.validate_disk(profile.min_disk_gib, profile.min_disk_gib - 1, &cfg.limits).is_err());
        }
    }

    #[test]
    fn software_graphics_uses_cpu_path_and_preserves_disk_and_network_arguments() {
        let profiles: Value = serde_json::from_str(include_str!("../../runtime-profiles.json")).unwrap();
        let manifest: Manifest = serde_json::from_value(profiles["windows-x86_64"].clone()).unwrap();
        let graphics = RuntimeProfile::load("windows-x86_64").unwrap().software_graphics.unwrap();
        let args = graphics.arguments(&manifest.arguments).unwrap();
        let value = |args: &[String], flag: &str| args[args.iter().position(|part| part == flag).unwrap() + 1].clone();
        assert_eq!(value(&args, "-display"), "sdl,gl=off,show-cursor=off");
        assert_eq!(value(&args, "-vga"), "none");
        assert!(args.contains(&"virtio-gpu-pci,id=gpu0".into()));
        assert!(!args.iter().any(|part| part.contains("virtio-vga-gl") || part.contains("venus=on")));
        assert!(value(&args, "-append").contains("tryomarchy.render=cpu"));
        assert!(!value(&args, "-append").contains("tryomarchy.render=gpu"));
        for flag in ["-machine", "-cpu", "-smp", "-m", "-drive", "-kernel", "-initrd", "-netdev"] {
            assert_eq!(value(&args, flag), value(&manifest.arguments, flag), "{flag}");
        }
        for device in ["virtio-keyboard-pci", "virtio-tablet-pci", "virtio-net-pci,id=omarchy-nic,netdev=n0,romfile=", "virtio-serial-pci,id=virtioserial0"] {
            assert!(args.contains(&device.to_string()), "{device}");
        }
        assert_eq!(graphics.environment["SDL_RENDER_DRIVER"], "software");
        assert!(RuntimeProfile::load("macos-aarch64").unwrap().software_graphics.is_none());
    }

    #[test]
    fn software_graphics_rejects_ambiguous_display_devices() {
        let profiles: Value = serde_json::from_str(include_str!("../../runtime-profiles.json")).unwrap();
        let manifest: Manifest = serde_json::from_value(profiles["windows-x86_64"].clone()).unwrap();
        let graphics = RuntimeProfile::load("windows-x86_64").unwrap().software_graphics.unwrap();
        let mut duplicate = manifest.arguments.clone();
        duplicate.extend(["-device".into(), "virtio-gpu-pci".into()]);
        assert!(graphics.arguments(&duplicate).is_err());
        assert!(graphics.arguments(&["-display".into(), "sdl".into()]).is_err());
    }
}
