// The browser stays in the VM. guest/browser_stream.py puts its windows on a
// headless screen and serves that screen with wayvnc on the guest loopback;
// this SSH connection forwards it to a host loopback port, and the VNC bytes
// travel between that port and the page's noVNC over a Tauri binary channel.
use crate::{config::{self, RuntimeProfile}, terminal, AppState};
use serde_json::{json, Value};
use std::{fs::File, io::{BufRead, BufReader, Read, Write}, net::{TcpListener, TcpStream}, process::{Child, ChildStdin, Command, Stdio}, sync::mpsc, thread, time::Duration};
use tauri::{ipc::{Channel, InvokeResponseBody}, AppHandle, Emitter, Manager};

pub struct Stream { child: Child, sizes: ChildStdin, vnc: TcpStream }

pub fn close(state: &AppState) {
    if let Some(mut stream) = state.browser.lock().unwrap().take() {
        let _ = stream.vnc.shutdown(std::net::Shutdown::Both);
        let _ = stream.child.kill();
        let _ = stream.child.wait();
    }
}

pub fn open(app: &AppHandle, generation: u64, size: Value, frames: Channel<InvokeResponseBody>) -> Result<(), String> {
    let state = app.state::<AppState>();
    close(&state);
    let (port, user) = terminal::connection(&state)?;
    let cfg = state.config.browser.clone();
    let address = state.config.terminal.forward_address.clone();
    let local = TcpListener::bind((address.as_str(), 0)).and_then(|listener| listener.local_addr()).map_err(|e| e.to_string())?.port();
    let forward = format!("{address}:{local}:127.0.0.1:{}", cfg.guest_port);
    let ssh = terminal::ssh_arguments(&state, port, &user, &["-o".into(), "ExitOnForwardFailure=yes".into(), "-L".into(), forward]);
    let settings = json!({"output":cfg.output,"port":cfg.guest_port,"window_class":cfg.window_class,"launch_command":cfg.launch_command,
        "refresh_rate":cfg.refresh_rate,"max_fps":cfg.max_fps,"poll_seconds":cfg.poll_seconds,"relaunch_seconds":cfg.relaunch_seconds,
        "ready_timeout_seconds":cfg.ready_timeout_seconds,"width":size["width"],"height":size["height"],"scale":size["scale"]});
    let remote = format!("exec python3 -u -c {} {}", terminal::quote(include_str!("../../guest/browser_stream.py")), terminal::quote(&settings.to_string()));
    let tools = RuntimeProfile::load(&config::platform())?.terminal;
    let log = File::create(state.data_dir().join("vm").join(&cfg.log_name)).map_err(|e| e.to_string())?;
    let mut child = terminal::windowless(Command::new(&tools.ssh).args(&ssh).arg(remote))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(log).spawn().map_err(|e| format!("无法连接虚拟机浏览器：{e}"))?;
    let sizes = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (ready, waiting) = mpsc::channel();
    let events = app.clone();
    thread::spawn(move || {
        let mut lines = BufReader::new(stdout).lines();
        if let Some(Ok(line)) = lines.next() { let _ = ready.send(line); }
        // Later lines report the area Omarchy's bar keeps on the screen; the page crops it.
        for line in lines.map_while(Result::ok) {
            if let Ok(layout) = serde_json::from_str::<Value>(&line) {
                let _ = events.emit("browser-layout", json!({"generation":generation,"reserved":layout["reserved"]}));
            }
        }
        let _ = events.emit("browser-exit", json!({"generation":generation}));
    });
    let reply = waiting.recv_timeout(Duration::from_secs(cfg.ready_timeout_seconds));
    let vnc = match reply {
        Ok(line) if serde_json::from_str::<Value>(&line).map(|value| value["ready"] == true).unwrap_or(false) =>
            TcpStream::connect((address.as_str(), local)).map_err(|e| format!("虚拟机浏览器画面连接失败：{e}")),
        _ => Err(format!("虚拟机浏览器没有就绪，详情见数据目录 vm/{}", cfg.log_name)),
    };
    let vnc = match vnc {
        Ok(vnc) => vnc,
        Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error); },
    };
    let mut reader = vnc.try_clone().map_err(|e| e.to_string())?;
    let events = app.clone();
    thread::spawn(move || {
        let mut buffer = vec![0u8; cfg.read_buffer_bytes];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 || frames.send(InvokeResponseBody::Raw(buffer[..read].to_vec())).is_err() { break; }
        }
        let _ = events.emit("browser-exit", json!({"generation":generation}));
    });
    *state.browser.lock().unwrap() = Some(Stream { child, sizes, vnc });
    Ok(())
}

pub fn send(state: &AppState, data: &[u8]) -> Result<(), String> {
    let mut stream = state.browser.lock().unwrap();
    let stream = stream.as_mut().ok_or("浏览器尚未连接")?;
    stream.vnc.write_all(data).map_err(|e| format!("浏览器输入发送失败：{e}"))
}

pub fn resize(state: &AppState, size: Value) -> Result<(), String> {
    let mut stream = state.browser.lock().unwrap();
    let stream = stream.as_mut().ok_or("浏览器尚未连接")?;
    writeln!(stream.sizes, "{size}").map_err(|e| format!("浏览器尺寸更新失败：{e}"))
}
