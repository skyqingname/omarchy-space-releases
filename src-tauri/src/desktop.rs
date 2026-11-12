// The VM desktop is QEMU's own window. Daily use happens in the chat, so a
// normal start keeps that window hidden until the user asks for the desktop.

#[cfg(target_os = "macos")]
pub fn hide(pid: u32) {
    use objc2_app_kit::NSRunningApplication;
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32) {
        if app.isFinishedLaunching() && !app.isHidden() { app.hide(); }
    }
}

#[cfg(target_os = "macos")]
pub fn show(pid: u32) -> Result<(), String> {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32).ok_or("虚拟机桌面窗口尚未出现")?;
    app.unhide();
    #[allow(deprecated)]
    app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows | NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    Ok(())
}

#[cfg(windows)]
mod windows {
    use std::sync::Mutex;
    use windows_sys::Win32::{Foundation::{HWND, LPARAM}, UI::WindowsAndMessaging::{EnumWindows, GetWindow, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow, GW_OWNER, SW_HIDE, SW_SHOW}};

    // Windows hidden here are the ones shown again; QEMU's own invisible
    // helper windows must stay as they are.
    pub static HIDDEN: Mutex<Vec<isize>> = Mutex::new(Vec::new());

    struct Search { pid: u32, windows: Vec<isize> }

    unsafe extern "system" fn collect(window: HWND, search: LPARAM) -> i32 {
        let search = &mut *(search as *mut Search);
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        if pid == search.pid && IsWindowVisible(window) != 0 && GetWindow(window, GW_OWNER).is_null() {
            search.windows.push(window as isize);
        }
        1
    }

    pub fn visible(pid: u32) -> Vec<isize> {
        let mut search = Search { pid, windows: Vec::new() };
        unsafe { EnumWindows(Some(collect), &mut search as *mut Search as LPARAM); }
        search.windows
    }

    pub fn hide(window: isize) { unsafe { ShowWindow(window as HWND, SW_HIDE); } }
    pub fn show(window: isize) { unsafe { ShowWindow(window as HWND, SW_SHOW); SetForegroundWindow(window as HWND); } }
}

#[cfg(windows)]
pub fn hide(pid: u32) {
    for window in windows::visible(pid) {
        windows::hide(window);
        windows::HIDDEN.lock().unwrap().push(window);
    }
}

#[cfg(windows)]
pub fn show(pid: u32) -> Result<(), String> {
    let hidden: Vec<isize> = windows::HIDDEN.lock().unwrap().drain(..).collect();
    let targets = [hidden, windows::visible(pid)].concat();
    if targets.is_empty() { return Err("虚拟机桌面窗口尚未出现".into()); }
    for window in targets { windows::show(window); }
    Ok(())
}
