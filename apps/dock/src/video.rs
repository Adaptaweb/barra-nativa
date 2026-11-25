//! Video wallpaper: an MP4 played behind the desktop icons on every display.
//!
//! The primary dock starts `muelle --video-wallpaper <file> <parent pid>`
//! while `video_wallpaper` is set. That process puts one window per display
//! in the desktop's wallpaper layer (the `WorkerW` that Explorer creates on
//! request, or Progman itself on Windows 11 24H2 and later) and plays the
//! video there with Media Foundation, muted and looping, cropped to fill the
//! display. It keeps playing behind maximized and full-screen windows; it
//! pauses only in energy saver. It exits with the dock, or when the setting
//! changes.

use anyhow::{Context, Result};
use muelle_config::Config;
use std::{
    cell::RefCell,
    os::windows::process::CommandExt,
    process::{Child, Command},
    sync::Mutex,
};
use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{CreateSolidBrush, HBRUSH},
        Media::MediaFoundation::{
            IMFPMediaPlayer, IMFPMediaPlayerCallback, IMFPMediaPlayerCallback_Impl,
            MFP_EVENT_HEADER, MFP_EVENT_TYPE_MEDIAITEM_SET, MFP_EVENT_TYPE_PLAYBACK_ENDED,
            MFP_OPTION_NONE, MFP_POSITIONTYPE_100NS, MFPCreateMediaPlayer, MFVideoNormalizedRect,
        },
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, StructuredStorage::PROPVARIANT},
            LibraryLoader::GetModuleHandleW,
            Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowExW, FindWindowW, GetMessageW, GetWindowRect, IsWindow, KillTimer,
            MSG, PostQuitMessage, RegisterClassW, SMTO_NORMAL, SWP_NOACTIVATE, SendMessageTimeoutW,
            SetParent, SetTimer, SetWindowPos, TranslateMessage, WM_DESTROY, WM_TIMER, WNDCLASSW,
            WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
        },
    },
    core::{BOOL, HSTRING, implement, w},
};

pub const FLAG: &str = "--video-wallpaper";

/// The running player process and the file it plays.
static PLAYER: Mutex<Option<(String, Child)>> = Mutex::new(None);

/// Starts, restarts or stops the player for the current settings. Only the
/// primary display's dock calls this; the player covers every display.
pub fn sync(config: &Config) {
    if muelle_shell::displays::is_secondary_process() {
        return;
    }
    let wanted = config
        .video_wallpaper
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty() && std::path::Path::new(path).is_file())
        .map(str::to_owned);
    let Ok(mut player) = PLAYER.lock() else {
        return;
    };
    if let Some((path, child)) = player.as_mut() {
        let alive = child.try_wait().ok().flatten().is_none();
        if alive && Some(&*path) == wanted.as_ref() {
            return;
        }
        let _ = child.kill();
        let _ = child.wait();
        *player = None;
    }
    let Some(path) = wanted else {
        return;
    };
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    match Command::new(exe)
        .args([FLAG, &path, &std::process::id().to_string()])
        .creation_flags(0x0800_0000)
        .spawn()
    {
        Ok(child) => *player = Some((path, child)),
        Err(error) => muelle_shell::log::line(&format!("fondo en video: {error}")),
    }
}

/// Stops the player (the dock is closing).
pub fn stop() {
    if let Ok(mut player) = PLAYER.lock()
        && let Some((_, mut child)) = player.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/* ------------------------------------------------------------ The player */

const CHECK_TIMER: usize = 1;
const CHECK_MS: u32 = 1_500;

struct Screen {
    window: HWND,
    player: Option<IMFPMediaPlayer>,
    playing: bool,
}

thread_local! {
    static SCREENS: RefCell<Vec<Screen>> = const { RefCell::new(Vec::new()) };
}

#[implement(IMFPMediaPlayerCallback)]
struct Events;

impl IMFPMediaPlayerCallback_Impl for Events_Impl {
    fn OnMediaPlayerEvent(&self, header: *const MFP_EVENT_HEADER) {
        // SAFETY: Media Foundation passes a valid header for the call.
        let Some(header) = (unsafe { header.as_ref() }) else {
            return;
        };
        let Some(player) = header.pMediaPlayer.as_ref() else {
            return;
        };
        if header.eEventType == MFP_EVENT_TYPE_MEDIAITEM_SET {
            fill(player);
            // SAFETY: Calls on the live player that raised the event.
            unsafe {
                let _ = player.SetMute(true);
                let _ = player.Play();
            }
        } else if header.eEventType == MFP_EVENT_TYPE_PLAYBACK_ENDED {
            // Loop: back to the start.
            let start = PROPVARIANT::from(0i64);
            // SAFETY: As above.
            unsafe {
                let _ = player.SetPosition(&MFP_POSITIONTYPE_100NS, &start);
                let _ = player.Play();
            }
        }
    }
}

/// Crops the video so it fills its display, like the "Fill" wallpaper fit.
fn fill(player: &IMFPMediaPlayer) {
    let mut native = Default::default();
    // SAFETY: Read-only query into a local.
    if unsafe { player.GetNativeVideoSize(Some(&mut native), None) }.is_err()
        || native.cx <= 0
        || native.cy <= 0
    {
        return;
    }
    let window = unsafe { player.GetVideoWindow() }.unwrap_or_default();
    let mut rect = RECT::default();
    // SAFETY: Read-only query on our own window.
    unsafe {
        let _ = GetWindowRect(window, &mut rect);
    }
    let (w, h) = ((rect.right - rect.left) as f32, (rect.bottom - rect.top) as f32);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let video = native.cx as f32 / native.cy as f32;
    let screen = w / h;
    let source = if video > screen {
        let keep = screen / video;
        MFVideoNormalizedRect { left: (1.0 - keep) / 2.0, top: 0.0, right: (1.0 + keep) / 2.0, bottom: 1.0 }
    } else {
        let keep = video / screen;
        MFVideoNormalizedRect { left: 0.0, top: (1.0 - keep) / 2.0, right: 1.0, bottom: (1.0 + keep) / 2.0 }
    };
    // SAFETY: Calls on the live player; MFVideoARMode_None stretches the
    // cropped part, which already has the display's proportions.
    unsafe {
        let _ = player.SetAspectRatioMode(0);
        let _ = player.SetVideoSourceRect(&source);
    }
}

/// The window the wallpaper layer lives in, with its top-left corner in
/// screen coordinates, and the window to stay below (the icons).
fn wallpaper_layer() -> Option<(HWND, Option<HWND>)> {
    // SAFETY: Window lookups and one message Explorer documents for
    // wallpaper hosts: it splits the desktop into icons and a layer behind.
    unsafe {
        let progman = FindWindowW(w!("Progman"), None).ok()?;
        let _ = SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(1), SMTO_NORMAL, 1000, None);
        // Windows 11 24H2 and later: the layer is a WorkerW child of Progman,
        // and the wallpaper window goes in Progman, below the icons.
        if FindWindowExW(Some(progman), None, w!("WorkerW"), None).is_ok() {
            let icons = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), None).ok();
            return Some((progman, icons));
        }
        // Earlier: the icons move to a top-level WorkerW; the next WorkerW
        // after it is the empty layer behind them.
        unsafe extern "system" fn find(hwnd: HWND, found: LPARAM) -> BOOL {
            // SAFETY: `found` points at the caller's local for the enumeration.
            unsafe {
                if FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None).is_ok() {
                    if let Ok(layer) = FindWindowExW(None, Some(hwnd), w!("WorkerW"), None) {
                        *(found.0 as *mut HWND) = layer;
                        return BOOL(0);
                    }
                }
            }
            BOOL(1)
        }
        let mut layer = HWND::default();
        let _ = EnumWindows(Some(find), LPARAM(&raw mut layer as isize));
        (!layer.is_invalid()).then_some((layer, None))
    }
}

fn check(parent: windows::Win32::Foundation::HANDLE) {
    // SAFETY: A zero wait on the parent's process handle.
    if unsafe { WaitForSingleObject(parent, 0) } == windows::Win32::Foundation::WAIT_OBJECT_0 {
        // SAFETY: Ends this thread's message loop.
        unsafe { PostQuitMessage(0) };
        return;
    }
    let saver = muelle_shell::system::energy_saver_on();
    SCREENS.with(|screens| {
        for screen in screens.borrow_mut().iter_mut() {
            let play = !saver;
            if play != screen.playing {
                if let Some(player) = &screen.player {
                    // SAFETY: Calls on our live player.
                    unsafe {
                        let _ = if play { player.Play() } else { player.Pause() };
                    }
                }
                screen.playing = play;
            }
        }
    });
}

unsafe extern "system" fn layer_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_DESTROY {
        return LRESULT(0);
    }
    // SAFETY: Default handling for our own window.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

/// Body of `muelle --video-wallpaper <file> <parent pid>`.
pub fn run(path: &str, parent: u32) -> Result<()> {
    // SAFETY: COM for this thread (MFPlay calls back on it); the parent
    // handle is only waited on; windows and players are ours.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let parent = OpenProcess(PROCESS_SYNCHRONIZE, false, parent).context("dock no encontrado")?;
        let (layer, below) = wallpaper_layer().context("no se encontró la capa del fondo del escritorio")?;
        let instance = GetModuleHandleW(None)?;
        let class = w!("BarraVideoWallpaper");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(layer_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            hbrBackground: HBRUSH(CreateSolidBrush(COLORREF(0)).0),
            ..Default::default()
        });
        let mut origin = RECT::default();
        let _ = GetWindowRect(layer, &mut origin);
        let callback: IMFPMediaPlayerCallback = Events.into();
        let file = HSTRING::from(path);
        let mut screens = Vec::new();
        for rect in muelle_shell::system::monitor_rects() {
            let window = CreateWindowExW(
                Default::default(),
                class,
                w!("Barra"),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                rect.left - origin.left,
                rect.top - origin.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                Some(layer),
                None,
                Some(instance.into()),
                None,
            )?;
            let _ = SetParent(window, Some(layer));
            if let Some(icons) = below {
                // Just below the icons, above the static wallpaper.
                let _ = SetWindowPos(window, Some(icons), 0, 0, 0, 0, SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE);
            }
            let mut player = None;
            if let Err(error) = MFPCreateMediaPlayer(&file, false, MFP_OPTION_NONE, &callback, Some(window), Some(&mut player)) {
                muelle_shell::log::line(&format!("fondo en video: {error}"));
            }
            screens.push(Screen { window, player, playing: true });
        }
        SCREENS.with(|slot| *slot.borrow_mut() = screens);
        muelle_shell::log::line(&format!("fondo en video: {path}"));
        let timer_window = SCREENS.with(|s| s.borrow().first().map(|s| s.window)).unwrap_or_default();
        SetTimer(Some(timer_window), CHECK_TIMER, CHECK_MS, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER && msg.wParam.0 == CHECK_TIMER {
                check(parent);
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = KillTimer(Some(timer_window), CHECK_TIMER);
        SCREENS.with(|slot| {
            for screen in slot.borrow_mut().drain(..) {
                if let Some(player) = screen.player {
                    let _ = player.Shutdown();
                }
                if IsWindow(Some(screen.window)).as_bool() {
                    let _ = DestroyWindow(screen.window);
                }
            }
        });
    }
    Ok(())
}
