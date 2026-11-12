use serde::Serialize;
use std::{path::Path, process::Command};

#[derive(Serialize)]
pub struct Hardware {
    pub cpus: usize,
    pub memory_mib: u64,
    pub free_disk_gib: u64,
    pub virtualization_ready: bool,
    pub prerequisite: String,
}

fn output(command: &mut Command) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let result = command.output().map_err(|_| "无法读取电脑资源信息".to_string())?;
    if !result.status.success() { return Err("电脑资源检测未完成".into()); }
    String::from_utf8(result.stdout).map_err(|_| "电脑资源信息无法读取".into())
}

pub fn inspect(data_dir: &Path) -> Result<Hardware, String> {
    let cpus = std::thread::available_parallelism().map_err(|_| "无法读取处理器数量")?.get();
    let free_disk_gib = free_disk_gib(data_dir)?;
    #[cfg(target_os = "macos")]
    {
        let memory = output(Command::new("/usr/sbin/sysctl").args(["-n","hw.memsize"]))?.trim().parse::<u64>().map_err(|_| "无法读取内存容量")?;
        let hv = output(Command::new("/usr/sbin/sysctl").args(["-n","kern.hv_support"]))?.trim() == "1";
        Ok(Hardware { cpus, memory_mib: memory/1024/1024, free_disk_gib, virtualization_ready: hv,
            prerequisite: if hv { "Apple 硬件虚拟化已就绪" } else { "此 Mac 未提供硬件虚拟化支持" }.into() })
    }
    #[cfg(target_os = "windows")]
    {
        let script = "$ErrorActionPreference='Stop'; $c=Get-CimInstance Win32_ComputerSystem; $f=Get-CimInstance Win32_OptionalFeature -Filter \"Name='HypervisorPlatform'\"; [pscustomobject]@{memory=[uint64]$c.TotalPhysicalMemory; ready=([bool]$c.HypervisorPresent -and $f.InstallState -eq 1)} | ConvertTo-Json -Compress";
        let text = output(Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",script]))?;
        let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| "Windows 资源检测结果无法读取")?;
        let ready = value["ready"].as_bool().ok_or("无法读取 Windows 虚拟化状态")?;
        Ok(Hardware { cpus, memory_mib: value["memory"].as_u64().ok_or("无法读取 Windows 内存")?/1024/1024,
            free_disk_gib,
            virtualization_ready: ready,
            prerequisite: if ready { "Windows Hypervisor Platform 已就绪" } else { "请在“启用或关闭 Windows 功能”中启用“Windows 虚拟机监控程序平台”，并重启电脑；固件也需要开启硬件虚拟化" }.into() })
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { let _ = (cpus, free_disk_gib); Err("当前版本仅提供 macOS 和 Windows 客户端".into()) }
}

pub fn free_disk_gib(directory: &Path) -> Result<u64, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        // Resolve for the volume query only. A C: junction can store its data on D:.
        let physical = directory.canonicalize().map_err(|error| format!("存放位置无法访问：{error}"))?;
        let path: Vec<u16> = physical.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available = 0u64;
        let success = unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut available, std::ptr::null_mut(), std::ptr::null_mut()) };
        if success == 0 { return Err(format!("无法读取所选位置的可用空间：{}", std::io::Error::last_os_error())); }
        Ok(available / 1024 / 1024 / 1024)
    }
    #[cfg(target_os = "macos")]
    {
        let disk = output(Command::new("/bin/df").arg("-Pk").arg(directory))?;
        let free_kib = disk.lines().last().and_then(|line| line.split_whitespace().nth(3)).ok_or("无法读取所选位置的可用空间")?.parse::<u64>().map_err(|_| "无法读取所选位置的可用空间")?;
        Ok(free_kib / 1024 / 1024)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { let _ = directory; Err("当前版本仅提供 macOS 和 Windows 客户端".into()) }
}
