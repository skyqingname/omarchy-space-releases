mod browser;
mod clipboard;
mod config;
mod desktop;
mod download;
mod host;
mod hostaccess;
mod storage;
mod subscription;
mod terminal;
mod transcript;
mod vm;

use config::{AssetPack, ProductConfig, Resources, RuntimeProfile};
use serde::Serialize;
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Mutex}};
use tauri::{Manager, State};

#[derive(Clone, Serialize)]
pub struct View { phase: String, message: String, running: bool, task: Option<String>, environment: String, delivery: Value, failure: Option<Value>, progress: Option<f64>, host_access: bool }
pub struct AppState {
    config: ProductConfig, storage: Mutex<storage::Storage>, operation: Mutex<()>, asset_root: Mutex<PathBuf>,
    nodes: Mutex<Vec<subscription::Node>>, session: Mutex<Option<vm::Session>>, chat: Mutex<Option<terminal::Chat>>, browser: Mutex<Option<browser::Stream>>,
    view: Mutex<View>, busy: AtomicBool, closing: AtomicBool, run_id: AtomicU64,
}

impl AppState {
    fn data_dir(&self) -> PathBuf { self.storage.lock().unwrap().directory().to_path_buf() }

    fn require_idle(&self) -> Result<(), String> {
        if self.session.lock().unwrap().is_some() { return Err("请先关闭当前虚拟机".into()); }
        let view = self.view.lock().unwrap();
        if self.busy.load(Ordering::SeqCst) || view.task.is_some() || view.phase == "stopping" {
            return Err("当前任务尚未结束，请等待完成后再操作".into());
        }
        Ok(())
    }
}

fn host_snapshot(state: &AppState) -> Result<Value, String> {
    let (data_dir, default_dir, can_change) = {
        let storage = state.storage.lock().unwrap();
        (storage.directory().to_path_buf(), storage.default_directory().to_path_buf(), storage.can_change()?)
    };
    let root = state.asset_root.lock().unwrap().clone();
    let assets = match AssetPack::load(&root, &data_dir) {
        Ok(pack) => json!({"ready":true,"image_ready":pack.image_ready(),"download_bytes":pack.download_bytes(),"image_version":pack.manifest.image_version,"message":"运行组件已就绪"}),
        Err(message) => json!({"ready":false,"image_ready":false,"message":message}),
    };
    let supported = matches!((std::env::consts::OS,std::env::consts::ARCH), ("macos","aarch64") | ("windows","x86_64"));
    let hardware = host::inspect(&data_dir)?;
    let disk_limit_gib = hardware.free_disk_gib.min(state.config.limits.max_disk_gib);
    let runtime_profile = RuntimeProfile::load(&config::platform())?;
    let software_rendering = if data_dir.join("vm/machine.json").is_file() { vm::read_machine(&data_dir)?.software_rendering } else { false };
    Ok(json!({"platform":config::platform(),"supported":supported,"cpu_count":hardware.cpus,"hardware":hardware,
        "assets":assets,"asset_root":root,"data_directory":data_dir,"config":state.config,
        "runtime_profile":runtime_profile,
        "graphics":{"available":runtime_profile.software_graphics.is_some(),"software_rendering":software_rendering},
        "storage":{"default_directory":default_dir,"can_change":can_change,"disk_limit_gib":disk_limit_gib},
        "view":state.view.lock().unwrap().clone()}))
}

#[tauri::command]
fn set_software_rendering(enabled: bool, state: State<AppState>) -> Result<(), String> {
    let _operation = state.operation.lock().unwrap();
    state.require_idle()?;
    if RuntimeProfile::load(&config::platform())?.software_graphics.is_none() { return Err("当前平台不支持此显示切换".into()); }
    vm::set_software_rendering(&state.data_dir(), enabled)
}

#[tauri::command]
async fn inspect_host(state: State<'_, AppState>) -> Result<Value, String> {
    let _operation = state.operation.lock().unwrap();
    host_snapshot(&state)
}

#[tauri::command]
async fn select_storage_directory(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let _operation = state.operation.lock().unwrap();
    state.require_idle()?;
    // Query the chosen volume before changing the saved preference.
    host::free_disk_gib(Path::new(&path))?;
    state.storage.lock().unwrap().select(Path::new(&path))?;
    state.run_id.fetch_add(1, Ordering::SeqCst);
    let view = vm::initial_view(&state.data_dir());
    *state.view.lock().unwrap() = view;
    Ok(())
}

#[tauri::command]
async fn select_asset_pack(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let _operation = state.operation.lock().unwrap();
    state.require_idle()?;
    let root = PathBuf::from(path);
    AssetPack::load(&root, &state.data_dir())?;
    *state.asset_root.lock().unwrap() = root;
    Ok(())
}
#[tauri::command]
fn download_image(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    let _operation = state.operation.lock().unwrap();
    state.require_idle()?;
    let pack = AssetPack::load(&state.asset_root.lock().unwrap(), &state.data_dir())?;
    vm::begin_download(app, pack);
    Ok(())
}
#[tauri::command]
async fn set_host_access(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _operation = state.operation.lock().unwrap();
        if state.busy.load(Ordering::SeqCst) || state.view.lock().unwrap().task.is_some() {
            return Err("请等待当前任务结束后再更改本机访问权限".into());
        }
        if vm::lifecycle(&state.data_dir())?.0 != "ready" { return Err("请先完成环境配置".into()); }
        let home = app.path().home_dir().map_err(|e| e.to_string())?;
        if enabled {
            hostaccess::ensure_service(&state.config.host_access, &state.data_dir().join("host-ssh-setup"))?;
            let delivery = state.view.lock().unwrap().delivery.clone();
            if let Some(key) = delivery["host_public_key"].as_str() { hostaccess::authorize(&home, key, &state.config.host_access)?; }
        } else { hostaccess::revoke(&home, &state.config.host_access)?; }
        vm::set_host_access(&app, enabled)
    }).await.map_err(|error| format!("本机访问设置任务异常结束：{error}"))?
}
#[tauri::command]
async fn import_subscription(input: String, state: State<'_, AppState>) -> Result<subscription::ImportResult, String> {
    let cfg = state.config.subscription.clone();
    let (nodes, result) = tauri::async_runtime::spawn_blocking(move || subscription::import(&input, &cfg)).await.map_err(|_| "订阅读取任务异常结束")??;
    *state.nodes.lock().unwrap() = nodes;
    Ok(result)
}
#[tauri::command]
fn current_state(state: State<AppState>) -> View { state.view.lock().unwrap().clone() }

#[tauri::command]
async fn chat_open(generation: u64, session: Option<String>, fresh: bool, cols: u16, rows: u16, app: tauri::AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || terminal::open(&app, generation, session, fresh, cols, rows)).await.map_err(|error| format!("对话连接任务异常结束：{error}"))?
}
#[tauri::command]
async fn chat_sessions(app: tauri::AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || terminal::sessions(&app.state::<AppState>())).await.map_err(|error| format!("历史对话任务异常结束：{error}"))?
}
#[tauri::command]
async fn chat_open_link(address: String, app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || terminal::open_link(&app.state::<AppState>(), &address)).await.map_err(|error| format!("链接任务异常结束：{error}"))?
}
#[tauri::command]
fn chat_close(state: State<AppState>) { terminal::close(&state) }
#[tauri::command]
fn chat_send(text: String, app: tauri::AppHandle) -> Result<(), String> { terminal::send(&app, &text) }
#[tauri::command]
fn terminal_input(data: String, state: State<AppState>) -> Result<(), String> { terminal::input(&state, &data) }
#[tauri::command]
fn terminal_resize(cols: u16, rows: u16, state: State<AppState>) -> Result<(), String> { terminal::resize(&state, cols, rows) }
#[tauri::command]
async fn browser_open(generation: u64, size: serde_json::Value, frames: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>, app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || browser::open(&app, generation, size, frames)).await.map_err(|error| format!("浏览器连接任务异常结束：{error}"))?
}
#[tauri::command]
fn browser_send(request: tauri::ipc::Request<'_>, state: State<AppState>) -> Result<(), String> {
    match request.body() {
        tauri::ipc::InvokeBody::Raw(data) => browser::send(&state, data),
        tauri::ipc::InvokeBody::Json(_) => Err("浏览器输入应为二进制数据".into()),
    }
}
#[tauri::command]
fn browser_resize(size: serde_json::Value, state: State<AppState>) -> Result<(), String> { browser::resize(&state, size) }
#[tauri::command]
fn browser_close(state: State<AppState>) { browser::close(&state) }
#[tauri::command]
fn show_desktop(state: State<AppState>) -> Result<(), String> { vm::show_desktop(&state) }

#[tauri::command]
async fn start_install(node_index: usize, resources: Resources, app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _operation = state.operation.lock().unwrap();
    state.require_idle()?;
    let data_dir = state.data_dir();
    let limits = &state.config.limits;
    let hardware = host::inspect(&data_dir)?;
    if !hardware.virtualization_ready { return Err(hardware.prerequisite); }
    if !data_dir.join("vm/disk.img").exists() && hardware.free_disk_gib < limits.min_free_disk_gib { return Err(format!("所选存放位置需要至少 {} GB 可用空间", limits.min_free_disk_gib)); }
    let pack = AssetPack::load(&state.asset_root.lock().unwrap(), &data_dir)?;
    if !data_dir.join("vm/disk.img").exists() && !pack.image_ready() { return Err("请先在所选位置下载 Omarchy，或选择完整离线资源包".into()); }
    let resources = if data_dir.join("vm/machine.json").exists() {
        vm::read_machine(&data_dir)?.resources
    } else {
        RuntimeProfile::load(&config::platform())?.validate_disk(resources.disk_gib, hardware.free_disk_gib, limits)?;
        if resources.cpus < limits.min_cpus || resources.cpus as usize > hardware.cpus || resources.memory_mib < limits.min_memory_mib || resources.memory_mib + limits.host_memory_mib > hardware.memory_mib || resources.memory_mib > limits.max_memory_mib { return Err("所选资源不符合当前镜像和电脑的运行要求".into()); }
        resources
    };
    if state.busy.swap(true, Ordering::SeqCst) { return Err("安装任务正在进行".into()); }
    vm::begin(app, pack, data_dir, resources, Some(node_index));
    Ok(())
}
#[tauri::command]
fn start_vm(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    let _operation = state.operation.lock().unwrap();
    state.require_idle()?;
    let data_dir = state.data_dir();
    if !matches!(vm::lifecycle(&data_dir)?.0, "ready" | "restart_required") { return Err("环境尚未配置完成，请选择节点后继续配置".into()); }
    let machine = vm::read_machine(&data_dir)?;
    let pack = AssetPack::load(&state.asset_root.lock().unwrap(), &data_dir)?;
    if state.busy.swap(true, Ordering::SeqCst) { return Err("安装任务正在进行".into()); }
    vm::begin(app, pack, data_dir, machine.resources, None);
    Ok(())
}
#[tauri::command]
async fn stop_vm(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || vm::shutdown(&app)).await.map_err(|_| "关机任务异常结束")?
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let default_directory = app.path().app_local_data_dir()?;
            fs::create_dir_all(&default_directory)?;
            let storage = storage::Storage::load(default_directory).map_err(std::io::Error::other)?;
            let data_dir = storage.directory().to_path_buf();
            // Tauri canonicalizes this path. Legacy Windows tools such as the
            // bundled zstd CRT cannot reliably consume its verbatim prefix.
            let asset_root = dunce::simplified(&app.path().resource_dir()?).join("resources/runtime-pack");
            let state = AppState { config: ProductConfig::load(), view: Mutex::new(vm::initial_view(&data_dir)), storage: Mutex::new(storage), operation: Mutex::new(()), asset_root: Mutex::new(asset_root), nodes: Mutex::new(Vec::new()), session: Mutex::new(None), chat: Mutex::new(None), browser: Mutex::new(None), busy: AtomicBool::new(false), closing: AtomicBool::new(false), run_id: AtomicU64::new(0) };
            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.session.lock().unwrap().is_some() || state.busy.load(Ordering::SeqCst) {
                    api.prevent_close();
                    if state.session.lock().unwrap().is_some() && !state.closing.swap(true, Ordering::SeqCst) {
                        let handle = window.app_handle().clone();
                        std::thread::spawn(move || match vm::shutdown(&handle) {
                            Ok(()) => handle.exit(0),
                            Err(_) => handle.state::<AppState>().closing.store(false, Ordering::SeqCst),
                        });
                    } else if state.session.lock().unwrap().is_none() {
                        vm::hold_close(window.app_handle());
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![inspect_host,select_storage_directory,select_asset_pack,download_image,set_host_access,set_software_rendering,import_subscription,current_state,start_install,start_vm,stop_vm,chat_open,chat_sessions,chat_open_link,chat_close,chat_send,terminal_input,terminal_resize,show_desktop,browser_open,browser_send,browser_resize,browser_close])
        .build(tauri::generate_context!()).expect("could not start desktop application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                let state = app.state::<AppState>();
                if state.session.lock().unwrap().is_some() {
                    api.prevent_exit();
                    if state.closing.swap(true, Ordering::SeqCst) { return; }
                    let handle = app.clone();
                    std::thread::spawn(move || match vm::shutdown(&handle) {
                        Ok(()) => handle.exit(0),
                        Err(_) => handle.state::<AppState>().closing.store(false, Ordering::SeqCst),
                    });
                } else if state.busy.load(Ordering::SeqCst) { api.prevent_exit(); vm::hold_close(app); }
            }
        });
}
