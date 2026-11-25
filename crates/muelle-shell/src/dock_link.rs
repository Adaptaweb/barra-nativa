//! How other Muelle processes talk to the running dock.

use windows::{
    Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::{FindWindowW, PostMessageW, RegisterWindowMessageW},
    },
    core::{PCWSTR, w},
};

pub const DOCK_WINDOW_CLASS: PCWSTR = w!("MuelleWindow");
/// Name of the registered window message meaning "the configuration file changed".
pub const CONFIG_CHANGED: PCWSTR = w!("Muelle.ConfigChanged");
/// `true` if a dock is running in this session.
#[must_use]
pub fn is_running() -> bool {
    // SAFETY: Read-only window lookup.
    unsafe { FindWindowW(DOCK_WINDOW_CLASS, None).is_ok() }
}

/// Tells the dock, if it is running, to reload its configuration. A window
/// message instead of a file watcher: the dock spends nothing while idle.
pub fn notify_config_changed() {
    // SAFETY: Looks up windows and posts a message without associated data.
    unsafe {
        let message = RegisterWindowMessageW(CONFIG_CHANGED);
        for dock in dock_windows() {
            let _ = PostMessageW(Some(dock), message, WPARAM(0), LPARAM(0));
        }
    }
}

/// The main window of every dock process: one per display.
#[must_use]
pub fn dock_windows() -> Vec<windows::Win32::Foundation::HWND> {
    let mut found = Vec::new();
    let mut previous = None;
    // SAFETY: Read-only window lookups.
    unsafe {
        while let Ok(hwnd) =
            windows::Win32::UI::WindowsAndMessaging::FindWindowExW(None, previous, DOCK_WINDOW_CLASS, None)
        {
            found.push(hwnd);
            previous = Some(hwnd);
        }
    }
    found
}
