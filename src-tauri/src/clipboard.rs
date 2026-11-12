use crate::config::ClipboardConfig;
use arboard::Clipboard;
use std::{io::Write, net::TcpStream, process::Child, sync::{Arc, Mutex}, thread, time::Duration};

// One way by construction: this stream is only ever written to, and the guest
// opens its end read-only, so nothing in the VM can reach the host clipboard.
pub fn forward(mut stream: TcpStream, child: Arc<Mutex<Child>>, cfg: ClipboardConfig) {
    thread::spawn(move || {
        let Ok(mut clipboard) = Clipboard::new() else { return };
        let mut last = String::new();
        while matches!(child.lock().unwrap().try_wait(), Ok(None)) {
            if let Ok(text) = clipboard.get_text() {
                if text != last {
                    last = text;
                    if last.len() <= cfg.max_bytes {
                        let header = (last.len() as u32).to_be_bytes();
                        if stream.write_all(&header).and_then(|_| stream.write_all(last.as_bytes())).is_err() { return; }
                    }
                }
            }
            thread::sleep(Duration::from_millis(cfg.poll_milliseconds));
        }
    });
}
