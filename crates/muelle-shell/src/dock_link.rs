//! How other Muelle processes talk to the running dock.

use windows::{
    Win32::{
        Foundation::{LPARAM, WPARAM},
        System::DataExchange::COPYDATASTRUCT,
        UI::WindowsAndMessaging::{
            FindWindowW, PostMessageW, RegisterWindowMessageW, SMTO_ABORTIFHUNG,
            SendMessageTimeoutW, WM_COPYDATA,
        },
    },
    core::{PCWSTR, w},
};

pub const DOCK_WINDOW_CLASS: PCWSTR = w!("MuelleWindow");
/// Name of the registered window message meaning "the configuration file changed".
pub const CONFIG_CHANGED: PCWSTR = w!("Muelle.ConfigChanged");
/// Tag for notification payloads sent via WM_COPYDATA.
pub const NOTIFY_DATA_TAG: usize = 0x4D55_4E46; // "MUNF"

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPayload {
    pub app_id: String,
    pub app_name: String,
    pub title: String,
    pub body: String,
    pub icon_path: Option<String>,
    pub duration_secs: u32,
}

impl NotificationPayload {
    pub fn encode(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.app_id.replace('\t', " "),
            self.app_name.replace('\t', " "),
            self.title.replace('\t', " "),
            self.body.replace('\t', " "),
            self.icon_path.as_deref().unwrap_or("").replace('\t', " "),
            self.duration_secs
        )
    }

    pub fn decode(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('\t').collect();
        if parts.len() < 6 {
            return None;
        }
        Some(Self {
            app_id: parts[0].to_string(),
            app_name: parts[1].to_string(),
            title: parts[2].to_string(),
            body: parts[3].to_string(),
            icon_path: if parts[4].is_empty() {
                None
            } else {
                Some(parts[4].to_string())
            },
            duration_secs: parts[5].parse().unwrap_or(5),
        })
    }
}

/// `true` if a dock is running in this session.
#[must_use]
pub fn is_running() -> bool {
    // SAFETY: Read-only window lookup.
    unsafe { FindWindowW(DOCK_WINDOW_CLASS, None).is_ok() }
}

/// Tells the dock, if it is running, to reload its configuration. A window
/// message instead of a file watcher: the dock spends nothing while idle.
pub fn notify_config_changed() {
    // SAFETY: Looks up a window and posts a message without associated data.
    unsafe {
        if let Ok(dock) = FindWindowW(DOCK_WINDOW_CLASS, None) {
            let message = RegisterWindowMessageW(CONFIG_CHANGED);
            let _ = PostMessageW(Some(dock), message, WPARAM(0), LPARAM(0));
        }
    }
}

/// Sends a notification payload to the running dock process via WM_COPYDATA.
/// Returns `true` if delivered successfully.
pub fn post_notification(payload: &NotificationPayload) -> bool {
    let raw = payload.encode();
    let bytes = raw.as_bytes();

    unsafe {
        let dock = match FindWindowW(DOCK_WINDOW_CLASS, None) {
            Ok(hwnd) => hwnd,
            Err(_) => return false,
        };

        let cds = COPYDATASTRUCT {
            dwData: NOTIFY_DATA_TAG,
            cbData: bytes.len() as u32,
            lpData: bytes.as_ptr() as *mut _,
        };

        let mut result = 0;
        let ok = SendMessageTimeoutW(
            dock,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&cds as *const _ as isize),
            SMTO_ABORTIFHUNG,
            2000,
            Some(&mut result),
        );
        ok.0 != 0
    }
}
