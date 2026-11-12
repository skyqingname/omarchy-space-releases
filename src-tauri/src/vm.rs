use crate::{browser, clipboard, config::{self, AssetPack, Resources}, desktop, download, hostaccess, subscription, terminal, AppState, View};
use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::{write::ZlibEncoder, Compression};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs::{self, File, OpenOptions}, io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write}, net::{TcpListener, TcpStream}, path::{Path, PathBuf}, process::{Child, Command, Stdio}, sync::{Arc, Mutex}, thread, time::{Duration, Instant}};
use tauri::{AppHandle, Emitter, Manager};
use std::sync::atomic::{AtomicBool, Ordering};

// desktop_hidden: a normal start keeps QEMU's window hidden until the boot
// finishes or the user opens the desktop.
pub struct Session { pub child: Arc<Mutex<Child>>, pub qmp: Arc<Mutex<TcpStream>>, pub ssh_port: u16, pub pid: u32, pub desktop_hidden: Arc<AtomicBool> }
#[derive(Serialize, Deserialize)]
pub struct Machine { pub resources: Resources, pub image_version: String, pub asset_root: String, pub configured: bool, #[serde(default)] pub host_access: bool, #[serde(default)] pub software_rendering: bool }

pub fn read_machine(data_dir: &Path) -> Result<Machine, String> {
    let bytes = fs::read(data_dir.join("vm/machine.json")).map_err(|_| "还没有创建环境".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "虚拟机信息无法读取".into())
}

fn read_delivery(data_dir: &Path) -> Result<Value, String> {
    let path = data_dir.join("vm/delivery.json");
    if !path.exists() { return Ok(Value::Null); }
    let bytes = fs::read(path).map_err(|_| "无法读取客机配置回执")?;
    serde_json::from_slice(&bytes).map_err(|_| "客机配置回执格式不正确".into())
}

// The one derivation of "what exists on disk": the interface reads this and
// keeps no separate notion of where the user is in the product.
pub fn lifecycle(data_dir: &Path) -> Result<(&'static str, Value), String> {
    if !data_dir.join("vm/machine.json").is_file() { return Ok(("none", Value::Null)); }
    let configured = read_machine(data_dir)?.configured;
    let delivery = read_delivery(data_dir)?;
    let environment = if configured { "ready" } else if delivery["phase"] == "restart_required" { "restart_required" } else { "incomplete" };
    Ok((environment, delivery))
}

fn write_machine(vm_dir: &Path, machine: &Machine) -> Result<(), String> {
    let pending = vm_dir.join("machine.pending");
    fs::write(&pending, serde_json::to_vec_pretty(machine).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(pending, vm_dir.join("machine.json")).map_err(|e| e.to_string())
}

pub fn set_software_rendering(data_dir: &Path, enabled: bool) -> Result<(), String> {
    let mut machine = read_machine(data_dir)?;
    machine.software_rendering = enabled;
    write_machine(&data_dir.join("vm"), &machine)
}

fn update(app: &AppHandle, phase: &str, message: &str) {
    let state = app.state::<AppState>();
    let data_dir = state.data_dir();
    let mut view = state.view.lock().unwrap();
    view.phase = phase.into(); view.message = message.into();
    if phase != "failed" { view.failure = None; }
    if phase != "download" { view.progress = None; }
    match lifecycle(&data_dir) {
        Ok((environment, delivery)) => { view.environment = environment.into(); view.delivery = delivery; },
        Err(error) => { view.phase = "failed".into(); view.message = error; },
    }
    view.host_access = read_machine(&data_dir).map(|machine| machine.host_access).unwrap_or(false);
    let _ = fs::write(data_dir.join("deployment.json"), serde_json::to_vec_pretty(&*view).unwrap());
    let _ = app.emit("deployment", view.clone());
}

fn update_run(app: &AppHandle, run_id: u64, phase: &str, message: &str) {
    if app.state::<AppState>().run_id.load(Ordering::SeqCst) == run_id {
        update(app, phase, message);
    }
}

pub fn set_host_access(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let data_dir = state.data_dir();
    let mut machine = read_machine(&data_dir)?;
    machine.host_access = enabled;
    write_machine(&data_dir.join("vm"), &machine)?;
    let (phase, message) = { let view = state.view.lock().unwrap(); (view.phase.clone(), view.message.clone()) };
    update(app, &phase, &message);
    Ok(())
}

pub fn download_progress(app: &AppHandle, progress: f64, message: &str) {
    app.state::<AppState>().view.lock().unwrap().progress = Some(progress);
    update(app, "download", message);
}

pub fn begin_download(app: AppHandle, pack: AssetPack) {
    app.state::<AppState>().view.lock().unwrap().task = Some("download".into());
    update(&app, "download", "正在连接下载地址");
    thread::spawn(move || {
        let cfg = app.state::<AppState>().config.download.clone();
        match download::fetch(&app, &pack, &cfg) {
            Ok(()) => finish(&app, "downloaded", "Omarchy 已下载完成"),
            Err(error) => finish(&app, "failed", &error),
        }
    });
}

// Only disk creation is busy without a session, and it cannot be interrupted.
pub fn hold_close(app: &AppHandle) {
    let phase = app.state::<AppState>().view.lock().unwrap().phase.clone();
    update(app, &phase, "正在创建独立磁盘，完成后才能关闭窗口");
}

// Every task ends here, so the interface always returns to the environment.
fn finish(app: &AppHandle, phase: &str, message: &str) {
    let state = app.state::<AppState>();
    let _operation = state.operation.lock().unwrap();
    state.view.lock().unwrap().task = None;
    state.busy.store(false, Ordering::SeqCst);
    update(app, phase, message);
}

#[cfg(not(target_os = "windows"))]
fn image_command_stderr(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(target_os = "windows")]
fn image_command_stderr(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::{GetACP, MultiByteToWideChar, MB_ERR_INVALID_CHARS};
    if bytes.is_empty() { return String::new(); }
    // The bundled zstd uses msvcrt's ANSI stderr. QEMU's UTF-8 manifest
    // does not apply to this separate image-decompression process.
    let code_page = unsafe { GetACP() };
    let mut wide = vec![0u16; bytes.len()];
    let written = unsafe { MultiByteToWideChar(code_page, MB_ERR_INVALID_CHARS, bytes.as_ptr(), bytes.len() as i32, wide.as_mut_ptr(), wide.len() as i32) };
    if written == 0 {
        return format!("无法读取解压工具错误信息（Windows 代码页 {code_page}）：{}", std::io::Error::last_os_error());
    }
    String::from_utf16_lossy(&wide[..written as usize])
}

#[cfg(target_os = "windows")]
fn enable_sparse_disk(file: &File) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_SPARSE};
    let mut returned = 0;
    // zstd's seek-over-zero implementation does not mark its Windows output
    // sparse. Mark the existing factory disk before growing it.
    let success = unsafe { DeviceIoControl(file.as_raw_handle(), FSCTL_SET_SPARSE,
        std::ptr::null(), 0, std::ptr::null_mut(), 0, &mut returned, std::ptr::null_mut()) };
    if success == 0 {
        return Err(format!("无法启用虚拟磁盘稀疏存储，已停止扩容。系统错误：{}。所选存放位置需要位于 NTFS 格式的磁盘上，exFAT 和 FAT32 不支持稀疏文件。请通过“选择存放位置”更换到 NTFS 磁盘后重试。", std::io::Error::last_os_error()));
    }
    Ok(())
}

fn checked(command: &mut Command, description: &str) -> Result<(), String> {
    // These commands only clone/decompress public factory images; stderr contains
    // useful filesystem/decoder diagnostics, never subscription credentials.
    let output = command.stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|error| format!("{description}无法启动：{error}"))?;
    if !output.status.success() {
        let status = output.status.code().map(|code| format!("退出码 {code}")).unwrap_or_else(|| "进程被系统终止".into());
        let detail = image_command_stderr(&output.stderr);
        let detail = detail.trim();
        return Err(if detail.is_empty() { format!("{description}失败（{status}）") } else { format!("{description}失败（{status}）：{detail}") });
    }
    Ok(())
}

pub fn prepare(app: &AppHandle, run_id: u64, assets: &AssetPack, vm_dir: &Path, resources: &Resources) -> Result<(), String> {
    let disk = vm_dir.join("disk.img");
    fs::create_dir_all(vm_dir).map_err(|e| e.to_string())?;
    if disk.exists() { return Ok(()); }
    update_run(app, run_id, "image", "正在从离线镜像创建独立磁盘");
    let source = assets.images.join(&assets.manifest.image);
    let partial = vm_dir.join("disk.preparing");
    match assets.manifest.image_format.as_str() {
        "zstd" => {
            let memory_mib = app.state::<AppState>().config.vm.zstd_memory_mib;
            checked(Command::new(assets.root.join(&assets.manifest.zstd)).arg("-d").arg(format!("--memory={}MiB", memory_mib)).arg("--sparse").arg("-f").arg(&source).arg("-o").arg(&partial), "镜像解压")?;
        },
        "raw" => {
            #[cfg(target_os = "macos")]
            checked(Command::new("/bin/cp").arg("-c").arg(&source).arg(&partial), "镜像克隆")?;
            #[cfg(not(target_os = "macos"))]
            fs::copy(&source, &partial).map_err(|_| "镜像复制失败".to_string())?;
        },
        _ => return Err("资源包镜像格式不支持".into()),
    }
    let file = OpenOptions::new().write(true).open(&partial).map_err(|e| e.to_string())?;
    let capacity = resources.disk_gib * 1024 * 1024 * 1024;
    if capacity < file.metadata().map_err(|e| e.to_string())?.len() { return Err("所选磁盘容量小于出厂镜像".into()); }
    #[cfg(target_os = "windows")]
    enable_sparse_disk(&file)?;
    file.set_len(capacity).map_err(|e| e.to_string())?;
    if assets.manifest.efi_variables_bytes > 0 {
        File::create(vm_dir.join("efi-vars.fd")).and_then(|f| f.set_len(assets.manifest.efi_variables_bytes)).map_err(|e| e.to_string())?;
    }
    let machine = Machine { resources: resources.clone(), image_version: assets.manifest.image_version.clone(), asset_root: assets.root.to_string_lossy().into(), configured: false, host_access: false, software_rendering: false };
    write_machine(vm_dir, &machine)?;
    fs::rename(&partial, &disk).map_err(|e| e.to_string())?;
    Ok(())
}

fn accept(listener: &TcpListener, child: &Arc<Mutex<Child>>, seconds: u64) -> Result<TcpStream, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        match listener.accept() {
            Ok((socket, _)) => { socket.set_nonblocking(false).map_err(|e| e.to_string())?; return Ok(socket); },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(_) => return Err("虚拟机控制通道连接失败".into()),
        }
        if child.lock().unwrap().try_wait().map_err(|e| e.to_string())?.is_some() { return Err("虚拟机启动后提前退出，请检查运行时日志".into()); }
        if Instant::now() >= deadline { return Err("等待虚拟机启动超时".into()); }
        thread::sleep(Duration::from_millis(100));
    }
}

fn initialize_qmp(mut socket: TcpStream, seconds: u64) -> Result<TcpStream, String> {
    socket.set_read_timeout(Some(Duration::from_secs(seconds))).map_err(|e| e.to_string())?;
    socket.set_write_timeout(Some(Duration::from_secs(seconds))).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(socket.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    if reader.read_line(&mut line).map_err(|error| format!("QMP 启动信息读取失败：{error}"))? == 0 { return Err("QMP 在发送启动信息前关闭了连接".into()); }
    let greeting: Value = serde_json::from_str(&line).map_err(|error| format!("QMP 启动信息无效：{error}"))?;
    if greeting.get("QMP").is_none() { return Err("QMP 控制通道未就绪".into()); }
    qmp_command(&mut socket, "qmp_capabilities", None)?;
    Ok(socket)
}

fn qmp_command(socket: &mut TcpStream, name: &str, arguments: Option<Value>) -> Result<Value, String> {
    let started = Instant::now();
    let mut request = json!({"execute":name,"id":name});
    if let Some(arguments) = arguments { request["arguments"] = arguments; }
    serde_json::to_writer(&mut *socket, &request).map_err(|error| format!("QMP {name} 命令发送失败：{error}"))?;
    socket.write_all(b"\n").map_err(|error| format!("QMP {name} 命令发送失败：{error}"))?;
    let mut reader = BufReader::new(socket.try_clone().map_err(|error| format!("QMP {name} 控制连接读取失败：{error}"))?);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(|error| format!("QMP {name} 等待响应失败（{:.1} 秒）：{error}；请求：{request}", started.elapsed().as_secs_f64()))? == 0 {
            return Err(format!("QMP {name} 等待响应时连接关闭（{:.1} 秒）", started.elapsed().as_secs_f64()));
        }
        let response: Value = serde_json::from_str(&line).map_err(|error| format!("QMP {name} 响应无效：{error}"))?;
        // QMP permits protocol errors without an id. There is only one
        // outstanding command here; waiting for a tagged reply would hide it.
        if let Some(error) = response.get("error") {
            if response["id"] == name || response.get("id").is_none() {
                return Err(format!("QMP {name} 控制命令被拒绝：{error}"));
            }
        }
        if response["id"] == name {
            return response.get("return").cloned().ok_or_else(|| format!("QMP {name} 响应缺少执行结果：{response}"));
        }
    }
}

fn runtime_log_tail(path: &Path) -> Result<String, io::Error> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(8192)))?;
    let mut bytes = Vec::new();
    file.take(8192).read_to_end(&mut bytes)?;
    // The bundled Windows QEMU declares activeCodePage=UTF-8.
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<_> = text.lines().rev().take(20).collect();
    lines.reverse();
    Ok(lines.join("\n"))
}

fn smbios_credential(vm_dir: &Path, name: &str, content: &str) -> Result<String, String> {
    let value = format!("io.systemd.credential.binary:{name}={}", STANDARD.encode(content));
    #[cfg(windows)]
    {
        // The bootstrap alone can exceed CreateProcess's 32,767-character limit.
        // QEMU's type=11,path reads the same OEM string from a file instead.
        let path = vm_dir.join(format!("{name}.smbios"));
        fs::write(&path, value).map_err(|e| format!("无法写入虚拟机启动数据：{e}"))?;
        Ok(format!("type=11,path={}", path.to_string_lossy().replace(',', ",,")))
    }
    #[cfg(not(windows))]
    { let _ = vm_dir; Ok(format!("type=11,value={value}")) }
}

pub fn boot(app: &AppHandle, run_id: u64, assets: AssetPack, data_dir: &Path, vm_dir: &Path, resources: Resources, network: Option<Value>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let cfg = &state.config;
    let software = if read_machine(data_dir)?.software_rendering {
        Some(config::RuntimeProfile::load(&config::platform())?.software_graphics.ok_or("当前平台没有兼容显示配置")?)
    } else { None };
    let runtime_arguments = match &software {
        Some(graphics) => graphics.arguments(&assets.manifest.arguments)?,
        None => assets.manifest.arguments.clone(),
    };
    let qmp_listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let provision_listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let clipboard_listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let mut arguments: Vec<String> = runtime_arguments.iter().map(|arg| arg
        .replace("{assets}", &assets.root.to_string_lossy().replace(',', ",,"))
        .replace("{images}", &assets.images.to_string_lossy().replace(',', ",,"))
        .replace("{vm}", &vm_dir.to_string_lossy().replace(',', ",,"))
        .replace("{cpus}", &resources.cpus.to_string())
        .replace("{memory_mib}", &resources.memory_mib.to_string())).collect();
    // Pause CPUs until the NIC is disconnected: even the very first boot
    // cannot send packets before the guest's persistent filter is installed.
    arguments.extend(["-S".into(), "-name".into(), cfg.name.clone(), "-qmp".into(), format!("tcp:127.0.0.1:{},server=off", qmp_listener.local_addr().unwrap().port()),
        "-chardev".into(), format!("socket,id=provision,host=127.0.0.1,port={},server=off", provision_listener.local_addr().unwrap().port()),
        "-device".into(), format!("virtserialport,chardev=provision,name={}", cfg.vm.provision_port_name),
        "-chardev".into(), format!("socket,id=clipboard,host=127.0.0.1,port={},server=off", clipboard_listener.local_addr().unwrap().port()),
        "-device".into(), format!("virtserialport,chardev=clipboard,name={}", cfg.vm.clipboard_port_name)]);
    let source = format!("import json\nCONFIG=json.loads({:?})\n{}\n{}\n{}", serde_json::to_string(cfg).unwrap(), include_str!("../../guest/network.py"), include_str!("../../guest/region.py"), include_str!("../../guest/provision.py"));
    let mut compressed = ZlibEncoder::new(Vec::new(), Compression::default());
    compressed.write_all(source.as_bytes()).map_err(|e| e.to_string())?;
    let bootstrap = compressed.finish().map_err(|e| e.to_string())?;
    let unit = format!("[Unit]\nDescription=Omarchy Space Provisioning\nConditionPathExists=!/etc/initrd-release\nAfter=systemd-udev-settle.service\nWants=systemd-udev-settle.service\n[Service]\nType=oneshot\nExecStart=/usr/bin/python3 -c \"import base64,zlib;exec(zlib.decompress(base64.b64decode('{}')))\"\nTimeoutStartSec={}\nStandardOutput=null\nStandardError=null\n", STANDARD.encode(bootstrap), cfg.vm.provision_service_timeout);
    for (name, content) in [("systemd.extra-unit.omarchy-space-provision.service", unit.as_str()), ("systemd.unit-dropin.multi-user.target", "[Unit]\nWants=omarchy-space-provision.service\n")] {
        arguments.extend(["-smbios".into(), smbios_credential(vm_dir, name, content)?]);
    }
    update_run(app, run_id, "boot", if software.is_some() { "正在使用 CPU 兼容显示启动 Omarchy" } else { "正在启动 Omarchy" });
    let log = File::create(vm_dir.join("runtime.log")).map_err(|e| e.to_string())?;
    let mut command = Command::new(assets.root.join(&assets.manifest.qemu));
    command.args(&arguments).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
    if let Some(graphics) = &software { command.envs(&graphics.environment); }
    let child = command.spawn().map_err(|e| format!("虚拟机运行时未启动：{e}"))?;
    let pid = child.id();
    let child = Arc::new(Mutex::new(child));
    let desktop_hidden = Arc::new(AtomicBool::new(network.is_none()));
    if network.is_none() { hide_desktop(app, run_id, pid, desktop_hidden.clone()); }
    let ssh_port = TcpListener::bind((cfg.terminal.forward_address.as_str(), 0)).and_then(|listener| listener.local_addr()).map_err(|e| e.to_string())?.port();
    let qmp = match accept(&qmp_listener, &child, cfg.vm.boot_timeout_seconds).and_then(|socket| {
        let mut socket = initialize_qmp(socket, cfg.vm.boot_timeout_seconds)?;
        qmp_command(&mut socket, "set_link", Some(json!({"name":assets.manifest.network_device_id,"up":false})))?;
        // Added at run time so existing offline manifests need no new field.
        let forward = format!("hostfwd_add tcp:{}:{ssh_port}-:{}", cfg.terminal.forward_address, cfg.terminal.guest_port);
        let reply = qmp_command(&mut socket, "human-monitor-command", Some(json!({"command-line":forward})))?;
        if reply != "" { return Err(format!("虚拟机终端端口转发失败：{reply}")); }
        qmp_command(&mut socket, "cont", None)?;
        Ok(socket)
    }) {
        Ok(socket) => socket,
        Err(error) => {
            let mut process = child.lock().unwrap();
            let before_cleanup = match process.try_wait() {
                Ok(Some(status)) => format!("QEMU 已自行退出：{status}"),
                Ok(None) => "QEMU 在启动清理前仍运行".into(),
                Err(error) => format!("QEMU 进程状态读取失败：{error}"),
            };
            let _ = process.kill();
            let _ = process.wait();
            let runtime = match runtime_log_tail(&vm_dir.join("runtime.log")) {
                Ok(log) if log.trim().is_empty() => "QEMU 未写入运行日志".into(),
                Ok(log) => format!("QEMU 运行日志末尾：\n{log}"),
                Err(error) => format!("无法读取 QEMU 运行日志：{error}"),
            };
            let hint = if cfg!(windows) && software.is_none() && runtime.contains("eglGetCurrentDisplay() not found") {
                "\n图形加速初始化失败。请在“我的环境”开启“兼容显示”，再启动环境。"
            } else { "" };
            return Err(format!("{error}\n{before_cleanup}\n{runtime}{hint}"));
        }
    };
    let mut line = String::new();
    let qmp = Arc::new(Mutex::new(qmp));
    *state.session.lock().unwrap() = Some(Session { child: child.clone(), qmp: qmp.clone(), ssh_port, pid, desktop_hidden });
    state.view.lock().unwrap().running = true;
    update_run(app, run_id, "boot", "Omarchy 正在启动，等待配置通道");
    let watch_app = app.clone();
    let watch_child = child.clone();
    let poll = cfg.vm.status_poll_milliseconds;
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(poll));
        let status = watch_child.lock().unwrap().try_wait();
        if let Ok(Some(status)) = status {
            let s = watch_app.state::<AppState>();
            let mut session = s.session.lock().unwrap();
            if !session.as_ref().map(|v| Arc::ptr_eq(&v.child, &watch_child)).unwrap_or(false) { break; }
            *session = None;
            drop(session);
            terminal::close(&s);
            browser::close(&s);
            s.run_id.fetch_add(1, Ordering::SeqCst);
            s.view.lock().unwrap().running = false;
            if status.success() { finish(&watch_app, "stopped", "虚拟机已关闭，数据保留在独立磁盘"); } else { finish(&watch_app, "failed", "虚拟机进程已退出，请查看运行时日志"); }
            break;
        }
    });
    let stream = accept(&provision_listener, &child, cfg.vm.boot_timeout_seconds)?;
    clipboard::forward(accept(&clipboard_listener, &child, cfg.vm.boot_timeout_seconds)?, child.clone(), cfg.clipboard.clone());
    stream.set_read_timeout(Some(Duration::from_secs(cfg.vm.provision_timeout_seconds))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(cfg.vm.provision_timeout_seconds))).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    line.clear(); reader.read_line(&mut line).map_err(|_| "客机配置程序未在规定时间内连接".to_string())?;
    let hello: Value = serde_json::from_str(&line).map_err(|_| "客机配置通道未返回有效状态".to_string())?;
    if hello["phase"] != "connected" { return Err("客机配置通道响应不正确".into()); }
    let configuring = network.is_some();
    let host = if read_machine(data_dir)?.host_access { Some(json!({"user":hostaccess::host_user()?,"platform":config::platform()})) } else { None };
    let terminal = json!({"public_key":terminal::public_key(&state)?});
    let payload = match network {
        Some(network) => json!({"action":"configure","network":network,"host":host,"terminal":terminal,
            "sing_box_bytes": fs::metadata(assets.root.join(&assets.manifest.sing_box_package)).map_err(|_| "无法读取 sing-box 离线包")?.len()}),
        None => { if hello["installed"] != true { return Err("此环境尚未配置完成，请重新导入节点后继续安装".into()); } json!({"action":"status","host":host,"terminal":terminal}) },
    };
    let mut writer = stream;
    serde_json::to_writer(&mut writer, &payload).map_err(|_| "客机配置发送失败".to_string())?;
    writer.write_all(b"\n").map_err(|_| "客机配置发送失败".to_string())?;
    drop(payload);
    if configuring {
        update_run(app, run_id, "network_install", "正在向客机传输离线组件");
        let mut file = File::open(assets.root.join(&assets.manifest.sing_box_package)).map_err(|_| "无法读取离线组件")?;
        io::copy(&mut file, &mut writer).map_err(|_| "离线组件传输中断")?;
    }
    let mut protected_link_enabled = false;
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(|_| "等待客机配置结果超时".to_string())? == 0 { return Err("客机配置通道已关闭，尚未收到完成结果".into()); }
        let event: Value = serde_json::from_str(&line).map_err(|_| "客机返回的安装状态无法读取".to_string())?;
        let phase = event["phase"].as_str().ok_or("客机状态缺少阶段")?;
        let message = event["message"].as_str().ok_or("客机状态缺少说明")?;
        if phase == "network_guard_ready" {
            if protected_link_enabled || event["network_guard_version"] != cfg.network.guard.version {
                return Err("客机断网保护回执不正确，网络未放行".into());
            }
            if state.run_id.load(Ordering::SeqCst) != run_id { return Ok(()); }
            qmp_command(&mut qmp.lock().unwrap(), "set_link", Some(json!({"name":assets.manifest.network_device_id,"up":true})))?;
            writer.write_all(b"{\"network_up\":true}\n").map_err(|_| "网络链路确认发送失败")?;
            protected_link_enabled = true;
            update_run(app, run_id, "network_install", message);
            continue;
        }
        if phase == "ready" || phase == "restart_required" {
            if !protected_link_enabled || event["proxy_required"] != true || event["network_guard_active"] != true {
                return Err("未收到完整的代理强制联网保护回执".into());
            }
            if state.run_id.load(Ordering::SeqCst) != run_id { return Ok(()); }
            fs::write(vm_dir.join("delivery.json"), serde_json::to_vec_pretty(&event).unwrap()).map_err(|e| e.to_string())?;
            let mut machine = read_machine(data_dir)?;
            machine.configured = phase == "ready";
            write_machine(vm_dir, &machine)?;
            if let (true, Some(key)) = (machine.host_access, event["host_public_key"].as_str()) {
                hostaccess::authorize(&app.path().home_dir().map_err(|e| e.to_string())?, key, &cfg.host_access)?;
            }
            // The nodes outlive a failed attempt so another one can be chosen.
            if configuring { state.nodes.lock().unwrap().clear(); }
            finish(app, phase, message);
            return Ok(());
        }
        if phase == "failed" {
            state.view.lock().unwrap().failure = event.get("failure").cloned();
            return Err(message.to_owned());
        }
        update_run(app, run_id, phase, message);
    }
}

// QEMU may raise its window again while it starts, so keep it hidden until
// this start finishes; afterwards the window belongs to the user.
fn hide_desktop(app: &AppHandle, run_id: u64, pid: u32, hidden: Arc<AtomicBool>) {
    let app = app.clone();
    thread::spawn(move || {
        let state = app.state::<AppState>();
        let poll = Duration::from_millis(state.config.vm.desktop_poll_milliseconds);
        while hidden.load(Ordering::SeqCst) && state.run_id.load(Ordering::SeqCst) == run_id && state.view.lock().unwrap().task.is_some() {
            desktop::hide(pid);
            thread::sleep(poll);
        }
    });
}

pub fn show_desktop(state: &AppState) -> Result<(), String> {
    let session = state.session.lock().unwrap();
    let session = session.as_ref().ok_or("虚拟机当前没有运行")?;
    session.desktop_hidden.store(false, Ordering::SeqCst);
    desktop::show(session.pid)
}

pub fn shutdown(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (child, qmp) = {
        let guard = state.session.lock().unwrap();
        let session = guard.as_ref().ok_or("虚拟机当前没有运行")?;
        (session.child.clone(), session.qmp.clone())
    };
    terminal::close(&state);
    browser::close(&state);
    state.run_id.fetch_add(1, Ordering::SeqCst);
    state.view.lock().unwrap().task = None;
    update(app, "stopping", "正在请求 Omarchy 正常关机");
    let press = || qmp.lock().unwrap().write_all(b"{\"execute\":\"system_powerdown\"}\n");
    let retry = Duration::from_secs(state.config.vm.shutdown_retry_seconds);
    let deadline = Instant::now() + Duration::from_secs(state.config.vm.shutdown_timeout_seconds);
    let mut requested = press();
    let mut next_press = Instant::now() + retry;
    while requested.is_ok() && Instant::now() < deadline {
        if child.lock().unwrap().try_wait().map_err(|e| e.to_string())?.is_some() {
            *state.session.lock().unwrap() = None;
            state.view.lock().unwrap().running = false;
            finish(app, "stopped", "虚拟机已关闭，数据保留在独立磁盘");
            return Ok(());
        }
        // A power button press is lost while the guest is still in firmware or early boot.
        if Instant::now() >= next_press { requested = press(); next_press = Instant::now() + retry; }
        thread::sleep(Duration::from_millis(state.config.vm.status_poll_milliseconds));
    }
    let message = if requested.is_ok() { "正常关机尚未完成，请在 Omarchy 窗口内关闭系统后再退出" } else { "关机请求发送失败" };
    finish(app, "failed", message);
    Err(message.into())
}

pub fn begin(app: AppHandle, assets: AssetPack, data_dir: PathBuf, resources: Resources, node_index: Option<usize>) {
    let state = app.state::<AppState>();
    let run_id = state.run_id.fetch_add(1, Ordering::SeqCst) + 1;
    let (task, phase, message) = match node_index {
        Some(_) if data_dir.join("vm/machine.json").is_file() => ("reconfigure", "boot", "正在启动 Omarchy"),
        Some(_) => ("install", "image", "正在准备独立磁盘"),
        None => ("start", "boot", "正在启动 Omarchy"),
    };
    state.view.lock().unwrap().task = Some(task.into());
    update(&app, phase, message);
    thread::spawn(move || {
        let state = app.state::<AppState>();
        let result = (|| {
            let network = match node_index {
                Some(index) => {
                    let nodes = state.nodes.lock().unwrap();
                    let node = nodes.get(index).ok_or("节点已失效，请重新导入")?;
                    Some(subscription::sing_box(node, &state.config.network))
                }, None => None,
            };
            let vm_dir = data_dir.join("vm");
            prepare(&app, run_id, &assets, &vm_dir, &resources)?;
            if network.is_some() {
                let mut machine = read_machine(&data_dir)?;
                machine.configured = false;
                write_machine(&vm_dir, &machine)?;
                // The receipt described the previous network configuration.
                let delivery = vm_dir.join("delivery.json");
                if delivery.exists() { fs::remove_file(delivery).map_err(|e| e.to_string())?; }
            }
            boot(&app, run_id, assets, &data_dir, &vm_dir, resources, network)
        })();
        if state.run_id.load(Ordering::SeqCst) == run_id {
            if let Err(error) = result { finish(&app, "failed", &error); }
        }
    });
}

pub fn initial_view(data_dir: &Path) -> View {
    match lifecycle(data_dir) {
        Ok((environment, delivery)) => {
            let (phase, message) = match environment {
                "restart_required" => ("restart_required", "请启动环境，完成出口地区设置的重启核验"),
                "ready" => ("stopped", "环境已保留，可以启动"),
                "incomplete" => ("stopped", "环境尚未配置完成"),
                _ => ("idle", "准备创建你的独立环境"),
            };
            View { phase: phase.into(), message: message.into(), running: false, task: None, environment: environment.into(), delivery, failure: None, progress: None, host_access: read_machine(data_dir).map(|machine| machine.host_access).unwrap_or(false) }
        },
        Err(message) => View { phase: "failed".into(), message, running: false, task: None, environment: "incomplete".into(), delivery: Value::Null, failure: None, progress: None, host_access: false },
    }
}

#[cfg(test)]
mod qmp_tests {
    use super::*;
    use std::sync::mpsc;

    #[cfg(windows)]
    #[test]
    fn windows_bootstrap_uses_short_file_argument_and_preserves_oem_string() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../evidence/smbios-tests/中文,space dir").join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&directory).unwrap();
        let content = "x".repeat(40000);
        let argument = smbios_credential(&directory, "test-unit", &content).unwrap();
        assert!(argument.starts_with("type=11,path="));
        assert!(argument.len() < 1024);
        assert!(argument.contains("中文,,space dir"));
        let value = fs::read_to_string(directory.join("test-unit.smbios")).unwrap();
        let encoded = value.strip_prefix("io.systemd.credential.binary:test-unit=").unwrap();
        assert_eq!(STANDARD.decode(encoded).unwrap(), content.as_bytes());
    }

    fn exchange(reply: &'static str) -> Result<Value, String> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (done, finished) = mpsc::channel::<()>();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["execute"], "query-status");
            stream.write_all(reply.as_bytes()).unwrap();
            let _ = finished.recv();
        });
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let result = qmp_command(&mut stream, "query-status", None);
        done.send(()).unwrap();
        peer.join().unwrap();
        result
    }

    #[test]
    fn untagged_qmp_error_is_reported_instead_of_waiting_for_timeout() {
        let error = exchange("{\"error\":{\"class\":\"GenericError\",\"desc\":\"JSON parse error from test peer\"}}\r\n").unwrap_err();
        assert!(error.contains("query-status") && error.contains("JSON parse error from test peer"), "{error}");
    }

    #[test]
    fn qmp_timeout_identifies_the_command_and_io_error() {
        let error = exchange("").unwrap_err();
        assert!(error.contains("QMP query-status 等待响应失败") && error.contains("："), "{error}");
    }

    #[test]
    fn qmp_events_do_not_replace_the_matching_command_response() {
        exchange("{\"event\":\"RESUME\"}\r\n{\"return\":{},\"id\":\"query-status\"}\r\n").unwrap();
    }

    #[test]
    fn existing_machine_retains_resources_and_access_when_switching_graphics() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evidence/2026-11-05-windows-egl-compat/machine-fixtures").join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(directory.join("vm")).unwrap();
        let original = json!({"resources":{"cpus":4,"memory_mib":6144,"disk_gib":16},"image_version":"existing-image","asset_root":"original-resources","configured":true,"host_access":true});
        fs::write(directory.join("vm/machine.json"), original.to_string()).unwrap();
        assert!(!read_machine(&directory).unwrap().software_rendering);
        set_software_rendering(&directory, true).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(directory.join("vm/machine.json")).unwrap()).unwrap();
        assert_eq!(saved["software_rendering"], true);
        for key in ["resources", "image_version", "asset_root", "configured", "host_access"] { assert_eq!(saved[key], original[key], "{key}"); }
        set_software_rendering(&directory, false).unwrap();
        assert!(!read_machine(&directory).unwrap().software_rendering);
    }
}
