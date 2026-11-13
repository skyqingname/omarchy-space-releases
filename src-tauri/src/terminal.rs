// The chat is a view over Claude Code running in a real terminal inside the
// VM. One ssh connection runs guest/transcript_follow.py, which writes the
// settings Claude is launched with and then reports Claude's own transcript,
// hooks and status line; those become the chat. The terminal itself is a
// second ssh connection in a local pty.
use crate::{config::{self, RuntimeProfile, TerminalConfig}, transcript::{self, Limits}, AppState};
use base64::{engine::general_purpose::STANDARD, Engine};
use portable_pty::{native_pty_system, Child as PtyChild, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Map, Value};
use std::{fs::{self, File}, io::{BufRead, BufReader, Read, Write}, path::PathBuf, process::{Child, ChildStdin, ChildStdout, Command, Stdio}, sync::{mpsc, Arc, Mutex}, thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager};

pub struct Chat {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    claude: Box<dyn PtyChild + Send + Sync>,
    follower: Child,
}

fn key_path(state: &AppState) -> PathBuf {
    // The default directory sits in the user's profile, whose permissions
    // OpenSSH accepts for a private key on every platform.
    state.storage.lock().unwrap().default_directory().join("terminal").join(&state.config.terminal.key_name)
}

#[cfg(windows)]
pub fn windowless(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
}
#[cfg(not(windows))]
pub fn windowless(command: &mut Command) -> &mut Command { command }

pub fn public_key(state: &AppState) -> Result<String, String> {
    let key = key_path(state);
    if !key.is_file() {
        fs::create_dir_all(key.parent().unwrap()).map_err(|e| e.to_string())?;
        let tools = RuntimeProfile::load(&config::platform())?.terminal;
        let output = windowless(Command::new(&tools.ssh_keygen).args(["-q", "-t", "ed25519", "-N", "", "-C", &state.config.terminal.key_comment, "-f"]).arg(&key))
            .stdin(Stdio::null()).output().map_err(|error| format!("无法运行 {}：{error}", tools.ssh_keygen))?;
        if !output.status.success() {
            return Err(format!("终端密钥生成失败：{}", String::from_utf8_lossy(&output.stderr).trim()));
        }
    }
    fs::read_to_string(format!("{}.pub", key.display())).map(|key| key.trim().to_owned()).map_err(|e| format!("终端公钥读取失败：{e}"))
}

pub fn quote(text: &str) -> String { format!("'{}'", text.replace('\'', "'\\''")) }

// Options for one connection to the VM account; `extra` goes before the destination.
pub fn ssh_arguments(state: &AppState, port: u16, user: &str, extra: &[String]) -> Vec<String> {
    let cfg = &state.config.terminal;
    let known_hosts = state.data_dir().join("vm").join(&cfg.known_hosts_name);
    let mut arguments = cfg.ssh_options.clone();
    arguments.extend(["-i".into(), key_path(state).to_string_lossy().into_owned(), "-p".into(), port.to_string(),
        "-o".into(), format!("UserKnownHostsFile=\"{}\"", known_hosts.display()),
        "-o".into(), format!("HostKeyAlias={}", cfg.host_key_alias)]);
    arguments.extend_from_slice(extra);
    arguments.push(format!("{user}@{}", cfg.forward_address));
    arguments
}

// The forwarded SSH port and the account the guest reported for this start.
pub fn connection(state: &AppState) -> Result<(u16, String), String> {
    let port = state.session.lock().unwrap().as_ref().ok_or("虚拟机当前没有运行")?.ssh_port;
    let view = state.view.lock().unwrap();
    if view.environment != "ready" || view.task.is_some() { return Err("环境启动完成后才能连接".into()); }
    let user = view.delivery["terminal_user"].as_str().ok_or("这次启动没有收到终端账号，请重启环境")?;
    Ok((port, user.to_owned()))
}

// The desktop's Wayland session is shared with what ssh starts, which lets
// Claude's sign-in and the chat's links open in the VM browser.
const WAYLAND: &str = "for socket in \"$XDG_RUNTIME_DIR\"/wayland-[0-9]; do [ -S \"$socket\" ] && export WAYLAND_DISPLAY=\"${socket##*/}\"; done";

// These strings are executed by SSH inside the guest, never by a host shell.
fn guest_login_command(command: &str) -> String {
    format!("exec bash -lc {}", quote(command))
}

// Runs in the guest account's login shell, so Claude sees the same environment
// as its status follower.
fn claude_command(cfg: &TerminalConfig, claude_path: &str, session: &str, settings: &str) -> String {
    let mut arguments: Vec<String> = cfg.claude_arguments.iter().map(|argument| quote(argument)).collect();
    arguments.extend(["--settings".into(), quote(settings)]);
    let arguments = arguments.join(" ");
    let directory = quote(&cfg.working_directory);
    let projects = quote(&cfg.claude_projects_path);
    let inner = format!(
        "mkdir -p \"$HOME\"/{directory} && cd \"$HOME\"/{directory} && {WAYLAND}; \
if [ -f \"$HOME\"/{projects}/\"$(pwd -P | sed 's/[^A-Za-z0-9]/-/g')\"/{session}.jsonl ]; \
then exec \"$HOME\"/{claude} {arguments} --resume {session}; \
else exec \"$HOME\"/{claude} {arguments} --session-id {session}; fi",
        claude = quote(claude_path));
    guest_login_command(&inner)
}

// The guest programs take the terminal parameters, and the session they are
// about, as one JSON argument.
fn guest_argument(cfg: &TerminalConfig, session: Option<&str>) -> String {
    let mut value = serde_json::to_value(cfg).unwrap();
    value["session"] = json!(session);
    quote(&value.to_string())
}

fn follower_command(cfg: &TerminalConfig, session: &str) -> String {
    // Keep login-profile output in the diagnostic stream, not in the JSON
    // handshake. Only the Python follower gets the saved protocol stdout.
    let command = format!("exec python3 -u - {} >&3 3>&-", guest_argument(cfg, Some(session)));
    format!("{} 3>&1 1>&2", guest_login_command(&command))
}

fn follow(app: AppHandle, generation: u64, mut reader: BufReader<ChildStdout>, record_path: PathBuf) {
    let cfg = &app.state::<AppState>().config.terminal;
    let limits = Limits { output_chars: cfg.tool_output_max_chars, summary_chars: cfg.tool_summary_max_chars };
    let mut items: Vec<Value> = Vec::new();
    let mut notes = Map::new();
    let mut line = String::new();
    while reader.read_line(&mut line).map(|read| read > 0).unwrap_or(false) {
        if let Ok(record) = serde_json::from_str::<Value>(&line) {
            if let Some(session) = record["gotocs_session"].as_str() {
                // Resume the session the user is in now, including one begun by /clear.
                let _ = fs::write(&record_path, json!({"session_id":session}).to_string());
                items.clear();
                notes.remove("live");
                notes.insert("reset".into(), json!(true));
                notes.insert("session_id".into(), json!(session));
            } else if record.get("gotocs_live").is_some() {
                notes.insert("live".into(), json!(items.len()));
            } else if let Some(status) = record.get("gotocs_status") {
                notes.insert("status".into(), status.clone());
            } else if let Some(meter) = record.get("gotocs_meter") {
                notes.insert("meter".into(), meter.clone());
            } else if let Some(hook) = record.get("gotocs_hook") {
                // Hooks keep their place among the records: what Claude is doing follows from their order.
                items.push(json!({"kind":"hook","event":hook["hook_event_name"],"notification":hook["notification_type"],"tool":hook["tool_name"]}));
            } else {
                items.extend(transcript::chat_items(&record, &limits));
            }
        }
        line.clear();
        // Emit whatever has arrived once the pipe is drained, so a long
        // history reaches the window in a few events instead of thousands.
        if reader.buffer().is_empty() {
            let mut event = std::mem::take(&mut notes);
            event.insert("generation".into(), json!(generation));
            event.insert("items".into(), Value::Array(std::mem::take(&mut items)));
            let _ = app.emit("chat", Value::Object(event));
        }
    }
}

// Sends the follower its program and waits for the settings file Claude is
// launched with.
fn prepare(mut script: ChildStdin, records: &mut BufReader<ChildStdout>) -> Result<String, String> {
    script.write_all(include_bytes!("../../guest/transcript_follow.py")).map_err(|e| e.to_string())?;
    drop(script);
    let mut ready = String::new();
    records.read_line(&mut ready).map_err(|e| e.to_string())?;
    let ready: Value = serde_json::from_str(&ready).map_err(|_| "没有收到就绪回执")?;
    Ok(ready["gotocs_ready"].as_str().ok_or("没有收到就绪回执")?.to_owned())
}

pub fn close(state: &AppState) {
    if let Some(mut chat) = state.chat.lock().unwrap().take() {
        let _ = chat.claude.kill();
        let _ = chat.claude.wait();
        let _ = chat.follower.kill();
        let _ = chat.follower.wait();
    }
}

type Terminal = (Box<dyn MasterPty + Send>, Box<dyn PtyChild + Send + Sync>, Box<dyn Read + Send>, Box<dyn Write + Send>);

// A local pty running `command`, with its two ends.
fn launch(command: CommandBuilder, cols: u16, rows: u16) -> Result<Terminal, String> {
    let pty = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| format!("无法打开本机终端：{e}"))?;
    let claude = pty.slave.spawn_command(command).map_err(|e| format!("无法连接虚拟机终端：{e}"))?;
    drop(pty.slave);
    let output = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pty.master.take_writer().map_err(|e| e.to_string())?;
    Ok((pty.master, claude, output, writer))
}

// `session` continues that one; otherwise the last one, or a new one when `fresh`.
pub fn open(app: &AppHandle, generation: u64, session: Option<String>, fresh: bool, cols: u16, rows: u16) -> Result<String, String> {
    let state = app.state::<AppState>();
    close(&state);
    let (port, user) = connection(&state)?;
    let cfg = state.config.terminal.clone();
    let vm_dir = state.data_dir().join("vm");
    let record_path = vm_dir.join(&cfg.session_record_name);
    let session = match session {
        // The id becomes part of a shell command in the VM.
        Some(session) => uuid::Uuid::parse_str(&session).map_err(|_| "对话编号不正确")?.to_string(),
        None if fresh || !record_path.is_file() => uuid::Uuid::new_v4().to_string(),
        None => {
            let record: Value = serde_json::from_slice(&fs::read(&record_path).map_err(|e| e.to_string())?).map_err(|_| "对话记录格式不正确")?;
            record["session_id"].as_str().ok_or("对话记录格式不正确")?.to_owned()
        },
    };
    fs::write(&record_path, json!({"session_id":session}).to_string()).map_err(|e| e.to_string())?;
    let tools = RuntimeProfile::load(&config::platform())?.terminal;
    let ssh = ssh_arguments(&state, port, &user, &[]);
    let claude_path = state.config.guest["claude_path"].as_str().ok_or("缺少 Claude 安装路径配置")?;

    let log = File::create(vm_dir.join(&cfg.follow_log_name)).map_err(|e| e.to_string())?;
    let mut follower = windowless(Command::new(&tools.ssh).args(&ssh).arg(follower_command(&cfg, &session)))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(log).spawn().map_err(|e| format!("无法连接对话记录：{e}"))?;
    let mut records = BufReader::new(follower.stdout.take().unwrap());
    let settings = match prepare(follower.stdin.take().unwrap(), &mut records) {
        Ok(settings) => settings,
        Err(error) => {
            let _ = follower.kill();
            let _ = follower.wait();
            return Err(format!("对话记录程序没有就绪（{error}），详情见数据目录 vm/{}", cfg.follow_log_name));
        },
    };

    let mut command = CommandBuilder::new(&tools.ssh);
    command.args(&ssh);
    command.arg("-tt");
    command.arg(claude_command(&cfg, claude_path, &session, &settings));
    command.env("TERM", &cfg.term);
    let (master, claude, mut output, writer) = match launch(command, cols, rows) {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = follower.kill();
            let _ = follower.wait();
            return Err(error);
        },
    };

    let (chunks, arrived) = mpsc::channel::<Vec<u8>>();
    let buffer_bytes = cfg.read_buffer_bytes;
    thread::spawn(move || {
        let mut buffer = vec![0u8; buffer_bytes];
        while let Ok(read) = output.read(&mut buffer) {
            if read == 0 || chunks.send(buffer[..read].to_vec()).is_err() { break; }
        }
    });
    let events = app.clone();
    let (gather, limit) = (Duration::from_millis(cfg.output_gather_milliseconds), cfg.output_gather_bytes);
    thread::spawn(move || {
        // A TUI redraws its screen in many small writes. Those that arrive
        // together reach the window as one event and one render.
        while let Ok(mut data) = arrived.recv() {
            while data.len() < limit {
                match arrived.recv_timeout(gather) { Ok(more) => data.extend(more), Err(_) => break }
            }
            let _ = events.emit("terminal", json!({"generation":generation,"data":STANDARD.encode(&data)}));
        }
        let _ = events.emit("chat-exit", json!({"generation":generation}));
    });
    let events = app.clone();
    thread::spawn(move || follow(events, generation, records, record_path));
    *state.chat.lock().unwrap() = Some(Chat { writer: Arc::new(Mutex::new(writer)), master, claude, follower });
    Ok(session)
}

// The workspace's sessions, newest first, for the chat's history.
pub fn sessions(state: &AppState) -> Result<Value, String> {
    let (port, user) = connection(state)?;
    let tools = RuntimeProfile::load(&config::platform())?.terminal;
    let remote = format!("exec python3 - {}", guest_argument(&state.config.terminal, None));
    let mut child = windowless(Command::new(&tools.ssh).args(ssh_arguments(state, port, &user, &[])).arg(remote))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("无法连接虚拟机：{e}"))?;
    child.stdin.take().unwrap().write_all(include_bytes!("../../guest/session_list.py")).map_err(|e| format!("历史对话程序发送失败：{e}"))?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() { return Err(format!("历史对话读取失败：{}", String::from_utf8_lossy(&output.stderr).trim())); }
    serde_json::from_slice(&output.stdout).map_err(|_| "历史对话格式不正确".into())
}

// A link in the conversation opens in the VM's browser, behind the chosen node.
pub fn open_link(state: &AppState, address: &str) -> Result<(), String> {
    let cfg = &state.config.terminal;
    let link = url::Url::parse(address).map_err(|_| "链接地址不正确")?;
    if !cfg.link_schemes.iter().any(|scheme| scheme == link.scheme()) { return Err(format!("不支持打开 {} 链接", link.scheme())); }
    let (port, user) = connection(state)?;
    let tools = RuntimeProfile::load(&config::platform())?.terminal;
    // Detached, so the connection ends even when this starts the browser.
    let inner = format!("{WAYLAND}; exec setsid -f {} {} >/dev/null 2>&1", quote(&cfg.link_command), quote(link.as_str()));
    let output = windowless(Command::new(&tools.ssh).args(ssh_arguments(state, port, &user, &[])).arg(format!("exec bash -lc {}", quote(&inner))))
        .stdin(Stdio::null()).output().map_err(|e| format!("无法连接虚拟机：{e}"))?;
    if !output.status.success() { return Err(format!("链接打开失败：{}", String::from_utf8_lossy(&output.stderr).trim())); }
    Ok(())
}

fn write(state: &AppState, data: &[u8]) -> Result<(), String> {
    let chat = state.chat.lock().unwrap();
    let chat = chat.as_ref().ok_or("对话尚未连接")?;
    let mut writer = chat.writer.lock().unwrap();
    writer.write_all(data).and_then(|_| writer.flush()).map_err(|e| format!("终端写入失败：{e}"))
}

pub fn input(state: &AppState, data: &str) -> Result<(), String> { write(state, data.as_bytes()) }

// Agent TUIs read a carriage return inside the same write as pasted text, so
// the body goes in as a bracketed paste and Enter follows on its own.
pub fn send(app: &AppHandle, text: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let body = text.replace('\u{1b}', "");
    write(&state, format!("\u{1b}[200~{body}\u{1b}[201~").as_bytes())?;
    let delay = Duration::from_millis(state.config.terminal.submit_delay_milliseconds);
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(delay);
        let _ = write(&app.state::<AppState>(), b"\r");
    });
    Ok(())
}

pub fn resize(state: &AppState, cols: u16, rows: u16) -> Result<(), String> {
    let chat = state.chat.lock().unwrap();
    let chat = chat.as_ref().ok_or("对话尚未连接")?;
    chat.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())
}

#[cfg(all(test, unix))]
mod tests;
