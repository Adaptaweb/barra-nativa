//! Notification area (tray) icons: the apps running in the background.
//!
//! Windows hands tray icons only to the window of class `Shell_TrayWnd` that
//! apps find first. Like other taskbar replacements, this creates its own
//! always-on-top window of that class: every `Shell_NotifyIcon` call arrives
//! here as `WM_COPYDATA`, a copy of the icon is kept, and the message is
//! forwarded to Explorer's taskbar so it keeps working exactly as before. At
//! start it broadcasts "TaskbarCreated" so running apps register their icons
//! again. Clicks are sent to the owning app as the Windows taskbar does.

use std::{
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicIsize, Ordering},
        mpsc::{Sender, channel},
    },
    thread,
    time::Duration,
};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::{DataExchange::COPYDATASTRUCT, LibraryLoader::GetModuleHandleW},
        UI::WindowsAndMessaging::{
            AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DispatchMessageW,
            EnumWindows, GetClassNameW, GetCursorPos, GetMessageW, GetWindowThreadProcessId,
            HICON, HWND_BROADCAST, HWND_TOPMOST, IsWindow, MSG, PostMessageW, RegisterClassW,
            RegisterWindowMessageW, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SendMessageTimeoutW, SetWindowPos, TranslateMessage, WM_COPYDATA, WM_LBUTTONDBLCLK,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_USER, WNDCLASSW,
            WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
    },
    core::{BOOL, GUID, w},
};

/// `NOTIFYICONDATA` as `Shell_NotifyIcon` sends it: 32-bit handles even on
/// 64-bit Windows.
#[repr(C)]
#[derive(Clone, Copy)]
struct NotifyIconData32 {
    cb_size: u32,
    hwnd: u32,
    uid: u32,
    flags: u32,
    callback: u32,
    hicon: u32,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    version: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid: GUID,
    balloon_icon: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ShellTrayData {
    signature: u32,
    message: u32,
    nid: NotifyIconData32,
}

const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
const NIF_MESSAGE: u32 = 0x1;
const NIF_ICON: u32 = 0x2;
const NIF_TIP: u32 = 0x4;
const NIF_STATE: u32 = 0x8;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x1;
const NIN_SELECT: u32 = WM_USER;
const WM_CONTEXTMENU: u32 = 0x007B;

#[derive(Clone)]
struct Entry {
    hwnd: u32,
    uid: u32,
    guid: Option<GUID>,
    callback: u32,
    hicon: u32,
    tip: String,
    version: u32,
    hidden: bool,
}

impl Entry {
    fn id(&self) -> String {
        match self.guid {
            Some(g) => format!("{g:?}"),
            None => format!("{}:{}", self.hwnd, self.uid),
        }
    }
}

/// A visible tray icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayIcon {
    pub id: String,
    /// Its tooltip, usually the app name and a status.
    pub tip: String,
    /// The icon handle; changes when the app changes its icon.
    pub hicon: isize,
}

static ENTRIES: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static CHANGED: OnceLock<Sender<()>> = OnceLock::new();
static OWN: AtomicIsize = AtomicIsize::new(0);

fn text(buffer: &[u16]) -> String {
    let n = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..n])
}

/// Explorer's own tray window, to forward everything to.
fn explorer_tray() -> Option<HWND> {
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` points at the caller's Option for the whole enumeration.
        let found = unsafe { &mut *(found.0 as *mut Option<HWND>) };
        let mut class = [0u16; 32];
        // SAFETY: Read-only queries on a window being enumerated.
        let n = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
        if String::from_utf16_lossy(&class[..n]) == "Shell_TrayWnd" && !is_own(hwnd) {
            *found = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut found: Option<HWND> = None;
    // SAFETY: The callback only writes through the pointer it is given.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&raw mut found as isize));
    }
    found
}

fn forward(msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(target) = explorer_tray() else {
        return LRESULT(0);
    };
    let mut result: usize = 0;
    // SAFETY: Forwards the message unchanged; COPYDATA is marshalled by Windows.
    unsafe {
        let _ = SendMessageTimeoutW(target, msg, w, l, SMTO_ABORTIFHUNG, 2000, Some(&mut result));
    }
    LRESULT(result as isize)
}

fn same(entry: &Entry, n: &NotifyIconData32) -> bool {
    if n.flags & NIF_GUID != 0 {
        return entry.guid == Some(n.guid);
    }
    entry.guid.is_none() && entry.hwnd == n.hwnd && entry.uid == n.uid
}

/// Applies one `Shell_NotifyIcon` call to the copy of the tray.
fn apply(data: &ShellTrayData) {
    let n = &data.nid;
    let Ok(mut list) = ENTRIES.lock() else {
        return;
    };
    match data.message {
        NIM_ADD | NIM_MODIFY => {
            let index = match list.iter().position(|e| same(e, n)) {
                Some(index) => index,
                None if data.message == NIM_MODIFY => return,
                None => {
                    list.push(Entry {
                        hwnd: n.hwnd,
                        uid: n.uid,
                        guid: (n.flags & NIF_GUID != 0).then_some(n.guid),
                        callback: 0,
                        hicon: 0,
                        tip: String::new(),
                        version: 0,
                        hidden: false,
                    });
                    list.len() - 1
                }
            };
            let entry = &mut list[index];
            entry.hwnd = n.hwnd;
            if n.flags & NIF_MESSAGE != 0 {
                entry.callback = n.callback;
            }
            if n.flags & NIF_ICON != 0 {
                entry.hicon = n.hicon;
            }
            if n.flags & NIF_TIP != 0 {
                entry.tip = text(&n.tip);
            }
            if n.flags & NIF_STATE != 0 && n.state_mask & NIS_HIDDEN != 0 {
                entry.hidden = n.state & NIS_HIDDEN != 0;
            }
        }
        NIM_DELETE => list.retain(|e| !same(e, n)),
        NIM_SETVERSION => {
            if let Some(entry) = list.iter_mut().find(|e| same(e, n)) {
                entry.version = n.version;
            }
        }
        _ => {}
    }
}

unsafe extern "system" fn tray_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA {
        // SAFETY: For WM_COPYDATA, lParam points at a COPYDATASTRUCT valid
        // during the call; its payload is checked for size before reading.
        unsafe {
            let copy = &*(l.0 as *const COPYDATASTRUCT);
            if copy.dwData == 1
                && copy.cbData as usize >= size_of::<ShellTrayData>()
                && !copy.lpData.is_null()
            {
                let data = std::ptr::read_unaligned(copy.lpData as *const ShellTrayData);
                apply(&data);
                if let Some(tx) = CHANGED.get() {
                    let _ = tx.send(());
                }
            }
        }
        // Explorer's taskbar gets everything too (icons, AppBar messages...).
        return forward(msg, w, l);
    }
    if msg >= WM_USER {
        return forward(msg, w, l);
    }
    // SAFETY: Default handling for our own window.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

/// The visible icons, in the order apps added them.
pub fn icons() -> Vec<TrayIcon> {
    let Ok(mut list) = ENTRIES.lock() else {
        return Vec::new();
    };
    // Icons of apps that closed without removing them.
    // SAFETY: IsWindow only reads.
    list.retain(|e| unsafe { IsWindow(Some(HWND(e.hwnd as isize as _))) }.as_bool());
    list.iter()
        .filter(|e| !e.hidden && e.hicon != 0)
        .map(|e| TrayIcon {
            id: e.id(),
            tip: e.tip.clone(),
            hicon: e.hicon as isize,
        })
        .collect()
}

/// The icon's picture.
pub fn image(icon: &TrayIcon) -> Option<crate::icons::IconImage> {
    crate::icons::icon_image(HICON(icon.hicon as _))
}

/// Starts the tray window; `changed` runs (on a worker thread) shortly after
/// the icons change, and every 10 seconds to drop icons of closed apps.
pub fn start(changed: fn()) {
    if OWN.load(Ordering::Acquire) != 0 {
        return;
    }
    let (tx, rx) = channel::<()>();
    let _ = CHANGED.set(tx);
    // Apps update tray icons in bursts (animated icons too): debounce.
    thread::spawn(move || {
        loop {
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(()) => {
                    thread::sleep(Duration::from_millis(150));
                    while rx.try_recv().is_ok() {}
                    changed();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => changed(),
                Err(_) => return,
            }
        }
    });
    thread::spawn(|| {
        // SAFETY: Class registration and a message loop for a window that
        // lives as long as this thread.
        unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(tray_proc),
                hInstance: instance.into(),
                lpszClassName: w!("Shell_TrayWnd"),
                ..Default::default()
            });
            let Ok(hwnd) = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                w!("Shell_TrayWnd"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            ) else {
                return;
            };
            OWN.store(hwnd.0 as isize, Ordering::Release);
            let raised = hwnd.0 as isize;
            // Stay first among the Shell_TrayWnd windows apps look for.
            thread::spawn(move || {
                loop {
                    let _ = SetWindowPos(
                        HWND(raised as _),
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                    thread::sleep(Duration::from_secs(3));
                }
            });
            // Every running app registers its icon again, now with us too.
            let created = RegisterWindowMessageW(w!("TaskbarCreated"));
            let _ = PostMessageW(Some(HWND_BROADCAST), created, WPARAM(0), LPARAM(0));
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
}

/// Whether `hwnd` is this process's own tray window, which must never be
/// taken for Explorer's.
pub fn is_own(hwnd: HWND) -> bool {
    let own = OWN.load(Ordering::Acquire);
    own != 0 && own == hwnd.0 as isize
}

/// Which mouse action a click on a tray icon is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Left,
    Right,
    Double,
}

/// Sends a click to the icon's app, as the Windows taskbar does.
pub fn click(id: &str, button: Click) {
    let Some(entry) = ENTRIES
        .lock()
        .ok()
        .and_then(|list| list.iter().find(|e| e.id() == id).cloned())
    else {
        return;
    };
    if entry.callback == 0 {
        return;
    }
    let target = HWND(entry.hwnd as isize as _);
    // SAFETY: Posts plain messages to the icon's window, as Explorer does.
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(target, Some(&mut pid));
        // Let the app bring its own window or menu to the front.
        let _ = AllowSetForegroundWindow(pid);
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let messages: &[u32] = match button {
            Click::Right => &[WM_RBUTTONDOWN, WM_RBUTTONUP, WM_CONTEXTMENU],
            Click::Double => &[WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK, WM_LBUTTONUP],
            Click::Left => &[WM_LBUTTONDOWN, WM_LBUTTONUP, NIN_SELECT],
        };
        for &message in messages {
            // Version 4 icons get the cursor in wParam and (event, id) in
            // lParam; older ones get the id in wParam and the message in lParam.
            let (wp, lp) = if entry.version >= 4 {
                let xy = ((point.y as u16 as u32) << 16) | (point.x as u16 as u32);
                (
                    WPARAM(xy as usize),
                    LPARAM((((entry.uid & 0xFFFF) << 16) | (message & 0xFFFF)) as isize),
                )
            } else {
                if message == WM_CONTEXTMENU || message == NIN_SELECT {
                    continue;
                }
                (WPARAM(entry.uid as usize), LPARAM(message as isize))
            };
            let _ = PostMessageW(Some(target), entry.callback, wp, lp);
        }
    }
    raise_popups(pid_of(target));
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0;
    // SAFETY: Read-only query.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// The dock and its panels stay on top of everything, so a menu or flyout an
/// app opens from its tray icon would show behind them and could not be
/// clicked. For a moment after the click, every menu (`#32768`) and every
/// captionless popup of that app is lifted above them.
fn raise_popups(pid: u32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, GetWindowLongPtrW, IsWindowVisible, WS_CAPTION, WS_POPUP,
    };
    unsafe extern "system" fn each(hwnd: HWND, pid: LPARAM) -> BOOL {
        // SAFETY: Read-only queries and a z-order change without activation.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut class = [0u16; 16];
            let n = GetClassNameW(hwnd, &mut class) as usize;
            let menu = String::from_utf16_lossy(&class[..n]) == "#32768";
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let popup = style & WS_POPUP.0 != 0 && style & WS_CAPTION.0 != WS_CAPTION.0;
            if menu || (popup && pid_of(hwnd) == pid.0 as u32 && !is_own(hwnd)) {
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
        BOOL(1)
    }
    thread::spawn(move || {
        for _ in 0..80 {
            thread::sleep(Duration::from_millis(30));
            // SAFETY: The callback only reads windows and reorders them.
            unsafe {
                let _ = EnumWindows(Some(each), LPARAM(pid as isize));
            }
        }
    });
}
