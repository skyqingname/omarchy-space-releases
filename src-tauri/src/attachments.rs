//! Explicit host-to-guest file copies over the existing authenticated SSH route.
use crate::{config::{self, RuntimeProfile}, terminal, AppState};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashMap, fs::File, io::{Read, Write}, path::Path,
    process::{Child, Command, Stdio}, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}},
    thread, time::{Duration, Instant}};
use tauri::{AppHandle, Manager, ipc::Channel};

type Transfers = Mutex<HashMap<String, Arc<AtomicBool>>>;
static TRANSFERS: OnceLock<Transfers> = OnceLock::new();
fn transfers() -> &'static Transfers { TRANSFERS.get_or_init(Default::default) }

#[derive(Clone, Serialize, Deserialize)]
pub struct Attachment { pub id: String, pub name: String, pub size: u64, pub path: String, pub sha256: String }
#[derive(Clone, Serialize)]
pub struct Progress { id: String, sent: u64, total: u64 }

pub fn cancel(id: &str) {
    if let Some(cancelled) = transfers().lock().unwrap().get(id) { cancelled.store(true, Ordering::SeqCst); }
}

// All exit paths kill/reap SSH and retire its cancellation handle. The watchdog
// also interrupts a blocked stdin write after timeout, cancellation or VM restart.
struct Transfer {
    id: String, child: Arc<Mutex<Child>>, finished: Arc<AtomicBool>,
    watchdog: Option<thread::JoinHandle<()>>,
}
impl Drop for Transfer {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::SeqCst);
        if let Some(watchdog) = self.watchdog.take() { let _ = watchdog.join(); }
        let mut child = self.child.lock().unwrap();
        let _ = child.kill();
        let _ = child.wait();
        transfers().lock().unwrap().remove(&self.id);
    }
}

fn filename(path: &Path) -> Result<String, String> {
    let name = path.file_name().and_then(|name| name.to_str()).ok_or("文件名不是有效的 Unicode")?;
    if name.is_empty() || name == "." || name == ".." || name == ".partial" || name.len() > 240 || name.chars().any(|c| c.is_control() || c == '/' || c == '\\') {
        return Err("文件名过长或含不支持的字符".into());
    }
    Ok(name.to_owned())
}

fn read_reply(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    (&mut pipe).take(16 * 1024).read_to_end(&mut bytes)?;
    std::io::copy(&mut pipe, &mut std::io::sink())?;
    Ok(bytes)
}

pub fn upload(app: &AppHandle, id: String, path: String, progress: Channel<Progress>) -> Result<Attachment, String> {
    let id = uuid::Uuid::parse_str(&id).map_err(|_| "上传编号不正确")?.to_string();
    let state = app.state::<AppState>();
    let (port, user) = terminal::connection(&state)?;
    let cfg = &state.config.attachments;
    let source = Path::new(&path);
    if !source.is_absolute() { return Err("请选择本机文件的完整路径".into()); }
    let name = filename(source)?;
    let mut file = File::open(source).map_err(|e| format!("无法读取所选文件：{e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() { return Err("请选择文件；文件夹请先压缩后再上传".into()); }
    let size = metadata.len();
    if size > cfg.max_file_bytes { return Err(format!("单个文件不能超过 {} MiB", cfg.max_file_bytes / 1048576)); }
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut active = transfers().lock().unwrap();
        if !active.is_empty() { return Err("已有文件正在上传，请等待完成".into()); }
        active.insert(id.clone(), cancelled.clone());
    }
    let parameters = json!({"id":id, "name":name, "size":size, "max_bytes":cfg.max_file_bytes,
        "directory":format!("{}/{}", state.config.terminal.working_directory, cfg.directory)});
    let remote = format!("exec python3 -c {} {}", terminal::quote(include_str!("../../guest/file_receive.py")), terminal::quote(&parameters.to_string()));
    let tools = match RuntimeProfile::load(&config::platform()) {
        Ok(profile) => profile.terminal,
        Err(error) => { transfers().lock().unwrap().remove(&id); return Err(error); },
    };
    let child = terminal::windowless(Command::new(&tools.ssh)
        .args(terminal::ssh_arguments(&state, port, &user, &[])).arg(remote))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => { transfers().lock().unwrap().remove(&id); return Err(format!("无法连接虚拟机：{error}")); },
    };
    let mut input = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let child = Arc::new(Mutex::new(child));
    let finished = Arc::new(AtomicBool::new(false));
    let mut transfer = Transfer { id: id.clone(), child: child.clone(), finished: finished.clone(), watchdog: None };
    let run_id = state.run_id.load(Ordering::SeqCst);
    let timeout = Duration::from_secs(cfg.timeout_seconds);
    let handle = app.clone();
    let aborted = cancelled.clone();
    transfer.watchdog = Some(thread::spawn(move || {
        let start = Instant::now();
        while !finished.load(Ordering::SeqCst) {
            if cancelled.load(Ordering::SeqCst) || start.elapsed() >= timeout
                || handle.state::<AppState>().run_id.load(Ordering::SeqCst) != run_id {
                cancelled.store(true, Ordering::SeqCst);
                let _ = child.lock().unwrap().kill();
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }));
    let reply = thread::spawn(move || read_reply(stdout));
    let errors = thread::spawn(move || read_reply(stderr));
    let result = (|| {
        let mut buffer = vec![0u8; cfg.chunk_bytes];
        if buffer.is_empty() { return Err("上传缓冲区配置不正确".to_owned()); }
        let mut sent = 0;
        let mut reported = Instant::now();
        progress.send(Progress { id: id.clone(), sent, total: size }).map_err(|e| e.to_string())?;
        loop {
            let read = file.read(&mut buffer).map_err(|e| format!("文件读取失败：{e}"))?;
            if read == 0 { break; }
            if sent + read as u64 > size { return Err("文件在上传时发生变化，请重新上传".into()); }
            input.write_all(&buffer[..read]).map_err(|_| "文件传输中断、超时或已取消，请重试".to_owned())?;
            sent += read as u64;
            if reported.elapsed() >= Duration::from_millis(100) || sent == size {
                progress.send(Progress { id: id.clone(), sent, total: size }).map_err(|e| e.to_string())?;
                reported = Instant::now();
            }
        }
        if sent != size { return Err("文件在上传时发生变化，请重新上传".into()); }
        Ok(())
    })();
    drop(input);
    if result.is_err() { let _ = transfer.child.lock().unwrap().kill(); }
    let exit = loop {
        let status = transfer.child.lock().unwrap().try_wait();
        match status {
            Ok(Some(exit)) => break Ok(exit),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => break Err(error.to_string()),
        }
    };
    // On OS wait errors, terminate before joining pipe readers.
    if exit.is_err() { let _ = transfer.child.lock().unwrap().kill(); }
    let output = reply.join().map_err(|_| "上传回执读取失败")?.map_err(|e| e.to_string())?;
    let error_output = errors.join().map_err(|_| "上传错误读取失败")?.map_err(|e| e.to_string())?;
    result?;
    if aborted.load(Ordering::SeqCst) { return Err("上传已取消、超时或虚拟机已重新启动，请重试".into()); }
    if !exit?.success() {
        let reason = serde_json::from_slice::<serde_json::Value>(&output).ok()
            .and_then(|v| v["error"].as_str().map(str::to_owned))
            .unwrap_or_else(|| String::from_utf8_lossy(&error_output).trim().to_owned());
        return Err(format!("上传失败：{reason}"));
    }
    let receipt: Attachment = serde_json::from_slice(&output).map_err(|_| "虚拟机没有返回有效上传回执")?;
    if receipt.id != id || receipt.name != name || receipt.size != size {
        return Err("虚拟机上传回执与所选文件不一致".into());
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_keep_unicode_and_reject_control_characters() {
        assert_eq!(filename(Path::new("报告 final's.docx")).unwrap(), "报告 final's.docx");
        assert!(filename(Path::new("bad\nname.pdf")).is_err());
        assert!(filename(Path::new(&format!("{}.pdf", "长".repeat(100)))).is_err());
        assert!(filename(Path::new(".partial")).is_err());
    }
}
