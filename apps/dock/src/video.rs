//! Video wallpaper: an MP4 played behind the desktop icons on every display.
//!
//! The primary dock starts `muelle --video-wallpaper <file> <parent pid>`
//! while `video_wallpaper` is set, and starts it again if it ever stops on
//! its own. That process puts one window per display in the desktop's
//! wallpaper layer (the `WorkerW` that Explorer creates on request, or
//! Progman itself on Windows 11 24H2 and later) and plays the video there
//! with the Media Foundation Media Engine on Direct3D 11: muted, looping,
//! cropped to fill each display. It keeps playing behind maximized and
//! full-screen windows and pauses only in energy saver. When Explorer
//! rebuilds the desktop layer, the windows are made again.

use anyhow::{Context, Result};
use muelle_config::Config;
use std::{
    os::windows::process::CommandExt,
    process::{Child, Command},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{COLORREF, HMODULE, HWND, LPARAM, LRESULT, RECT, WAIT_OBJECT_0, WPARAM},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device, ID3D11Multithread,
                ID3D11Texture2D,
            },
            Dxgi::{
                Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
                CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT,
                DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_DISCARD,
                DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory2, IDXGISwapChain1,
            },
            Gdi::{CreateSolidBrush, HBRUSH},
        },
        Media::MediaFoundation::{
            CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFDXGIDeviceManager, IMFMediaEngine,
            IMFMediaEngineClassFactory, IMFMediaEngineNotify, IMFMediaEngineNotify_Impl,
            MF_MEDIA_ENGINE_CALLBACK, MF_MEDIA_ENGINE_DXGI_MANAGER,
            MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, MF_VERSION, MFARGB, MFCreateAttributes,
            MFCreateDXGIDeviceManager, MFSTARTUP_FULL, MFShutdown, MFStartup,
            MFVideoNormalizedRect,
        },
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            LibraryLoader::GetModuleHandleW,
            Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows,
            FindWindowExW, FindWindowW, GetParent, GetWindowRect, IsWindow, MSG, PM_REMOVE,
            PeekMessageW, RegisterClassW, SMTO_NORMAL, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SendMessageTimeoutW, SetWindowPos, TranslateMessage, WNDCLASSW, WS_CHILD,
            WS_CLIPSIBLINGS, WS_VISIBLE,
        },
    },
    core::{BOOL, BSTR, Interface, implement, w},
};

pub const FLAG: &str = "--video-wallpaper";

/* -------------------------------------------------- Run by the dock */

/// The player process for one file. A watcher thread starts it again when it
/// stops on its own (a driver reset, Explorer restarting), a few times.
struct Player {
    path: String,
    child: Arc<Mutex<Option<Child>>>,
    stop: Arc<AtomicBool>,
}

static PLAYER: Mutex<Option<Player>> = Mutex::new(None);

fn spawn(path: &str) -> Option<Child> {
    let exe = std::env::current_exe().ok()?;
    Command::new(exe)
        .args([FLAG, path, &std::process::id().to_string()])
        .creation_flags(0x0800_0000)
        .spawn()
        .inspect_err(|error| muelle_shell::log::line(&format!("fondo en video: {error}")))
        .ok()
}

fn end(player: Player) {
    player.stop.store(true, Ordering::Release);
    if let Ok(mut child) = player.child.lock()
        && let Some(mut child) = child.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

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
    let Ok(mut slot) = PLAYER.lock() else {
        return;
    };
    if slot.as_ref().map(|p| &p.path) == wanted.as_ref() {
        return;
    }
    if let Some(old) = slot.take() {
        end(old);
    }
    let Some(path) = wanted else {
        return;
    };
    let child = Arc::new(Mutex::new(spawn(&path)));
    let stop = Arc::new(AtomicBool::new(false));
    let (watched, stopped, file) = (child.clone(), stop.clone(), path.clone());
    let _ = std::thread::Builder::new().name("barra-video".into()).spawn(move || {
        let mut restarts: Vec<Instant> = Vec::new();
        loop {
            std::thread::sleep(Duration::from_secs(2));
            if stopped.load(Ordering::Acquire) {
                return;
            }
            let Ok(mut child) = watched.lock() else { return };
            let running = child.as_mut().is_some_and(|c| c.try_wait().ok().flatten().is_none());
            if running {
                continue;
            }
            restarts.retain(|at| at.elapsed() < Duration::from_secs(600));
            if restarts.len() >= 5 {
                muelle_shell::log::line("fondo en video: se detuvo demasiadas veces; no se reinicia");
                return;
            }
            restarts.push(Instant::now());
            muelle_shell::log::line("fondo en video: el reproductor se detuvo; se inicia de nuevo");
            *child = spawn(&file);
        }
    });
    *slot = Some(Player { path, child, stop });
}

/// Stops the player (the dock is closing).
pub fn stop() {
    if let Ok(mut slot) = PLAYER.lock()
        && let Some(player) = slot.take()
    {
        end(player);
    }
}

/* -------------------------------------------------------- The player */

#[implement(IMFMediaEngineNotify)]
struct Events;

impl IMFMediaEngineNotify_Impl for Events_Impl {
    fn EventNotify(&self, _event: u32, _param1: usize, _param2: u32) -> windows::core::Result<()> {
        Ok(())
    }
}

struct Screen {
    window: HWND,
    width: u32,
    height: u32,
    engine: IMFMediaEngine,
    chain: IDXGISwapChain1,
    last: i64,
}

impl Drop for Screen {
    fn drop(&mut self) {
        // SAFETY: Our own engine and window, released once.
        unsafe {
            let _ = self.engine.Shutdown();
            if IsWindow(Some(self.window)).as_bool() {
                let _ = DestroyWindow(self.window);
            }
        }
    }
}

/// The window the wallpaper layer lives in and the window to stay below
/// (the icons), when that layer is Progman itself.
fn wallpaper_layer() -> Option<(HWND, Option<HWND>)> {
    // SAFETY: Window lookups and one message Explorer documents for
    // wallpaper hosts: it splits the desktop into icons and a layer behind.
    // Sent only when the windows are made: sent again, Explorer redraws the
    // wallpaper over them.
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
                if FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None).is_ok()
                    && let Ok(layer) = FindWindowExW(None, Some(hwnd), w!("WorkerW"), None)
                {
                    *(found.0 as *mut HWND) = layer;
                    return BOOL(0);
                }
            }
            BOOL(1)
        }
        let mut layer = HWND::default();
        let _ = EnumWindows(Some(find), LPARAM(&raw mut layer as isize));
        (!layer.is_invalid()).then_some((layer, None))
    }
}

/// Before 24H2 the windows live in a top-level WorkerW, not in Progman.
fn below_none() -> bool {
    // SAFETY: A read-only lookup.
    unsafe {
        let Ok(progman) = FindWindowW(w!("Progman"), None) else { return false };
        FindWindowExW(Some(progman), None, w!("WorkerW"), None).is_err()
    }
}

unsafe extern "system" fn layer_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: Default handling for our own window.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

struct Device {
    d3d: ID3D11Device,
    manager: IMFDXGIDeviceManager,
    factory: IDXGIFactory2,
    engines: IMFMediaEngineClassFactory,
}

fn device() -> Result<Device> {
    // SAFETY: Plain creation calls; every output is checked.
    unsafe {
        let mut d3d = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d),
            None,
            None,
        )?;
        let d3d: ID3D11Device = d3d.context("Direct3D no devolvió un dispositivo")?;
        // The Media Engine decodes on its own threads with this device.
        if let Ok(multi) = d3d.cast::<ID3D11Multithread>() {
            let _ = multi.SetMultithreadProtected(true);
        }
        let mut token = 0;
        let mut manager = None;
        MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
        let manager = manager.context("sin administrador DXGI")?;
        manager.ResetDevice(&d3d, token)?;
        let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        let engines: IMFMediaEngineClassFactory =
            CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)?;
        Ok(Device { d3d, manager, factory, engines })
    }
}

fn build(device: &Device, path: &str) -> Result<Vec<Screen>> {
    let (layer, below) = wallpaper_layer().context("no se encontró la capa del fondo del escritorio")?;
    // SAFETY: Windows and COM objects created here are owned by the screens.
    unsafe {
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
        let source = BSTR::from(path);
        let mut screens = Vec::new();
        for rect in muelle_shell::system::monitor_rects() {
            let (width, height) = ((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32);
            let window = CreateWindowExW(
                Default::default(),
                class,
                w!("Barra"),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                rect.left - origin.left,
                rect.top - origin.top,
                width as i32,
                height as i32,
                Some(layer),
                None,
                Some(instance.into()),
                None,
            )?;
            if let Some(icons) = below {
                // Just below the icons, above the static wallpaper.
                let _ = SetWindowPos(window, Some(icons), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
            }
            let chain = device.factory.CreateSwapChainForHwnd(
                &device.d3d,
                window,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: width,
                    Height: height,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                    ..Default::default()
                },
                None,
                None,
            )?;
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 3)?;
            let attributes = attributes.context("sin atributos")?;
            let events: IMFMediaEngineNotify = Events.into();
            attributes.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &events)?;
            attributes.SetUnknown(&MF_MEDIA_ENGINE_DXGI_MANAGER, &device.manager)?;
            attributes.SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32)?;
            let engine = device.engines.CreateInstance(0, &attributes)?;
            engine.SetMuted(true)?;
            engine.SetLoop(true)?;
            engine.SetAutoPlay(true)?;
            engine.SetSource(&source)?;
            screens.push(Screen { window, width, height, engine, chain, last: -1 });
        }
        Ok(screens)
    }
}

/// The part of the video that fills a `width`×`height` display.
fn crop(video: (u32, u32), width: u32, height: u32) -> MFVideoNormalizedRect {
    let (vw, vh) = (video.0.max(1) as f32, video.1.max(1) as f32);
    let (video, screen) = (vw / vh, width.max(1) as f32 / height.max(1) as f32);
    if video > screen {
        let keep = screen / video;
        MFVideoNormalizedRect { left: (1.0 - keep) / 2.0, top: 0.0, right: (1.0 + keep) / 2.0, bottom: 1.0 }
    } else {
        let keep = video / screen;
        MFVideoNormalizedRect { left: 0.0, top: (1.0 - keep) / 2.0, right: 1.0, bottom: (1.0 + keep) / 2.0 }
    }
}

/// Draws each display's newest frame. Returns false when a window is gone
/// (Explorer rebuilt the desktop layer) and everything must be made again.
fn draw(screens: &mut [Screen], layer_ok: bool) -> bool {
    let mut presented = false;
    for screen in screens.iter_mut() {
        // SAFETY: Calls on our own live objects.
        unsafe {
            if !layer_ok || !IsWindow(Some(screen.window)).as_bool() || GetParent(screen.window).is_err() {
                return false;
            }
            let Ok(time) = screen.engine.OnVideoStreamTick() else {
                continue;
            };
            if time < 0 || time == screen.last {
                continue;
            }
            screen.last = time;
            let (mut vw, mut vh) = (0, 0);
            if screen.engine.GetNativeVideoSize(Some(&mut vw), Some(&mut vh)).is_err() || vw == 0 {
                continue;
            }
            let Ok(target) = screen.chain.GetBuffer::<ID3D11Texture2D>(0) else {
                continue;
            };
            let source = crop((vw, vh), screen.width, screen.height);
            let whole = RECT { left: 0, top: 0, right: screen.width as i32, bottom: screen.height as i32 };
            let black = MFARGB { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbAlpha: 255 };
            if screen.engine.TransferVideoFrame(&target, Some(&source), &whole, Some(&black)).is_ok() {
                // Only the first display waits for the screen refresh.
                let _ = screen.chain.Present(u32::from(!presented), DXGI_PRESENT(0));
                presented = true;
            }
        }
    }
    if !presented {
        std::thread::sleep(Duration::from_millis(8));
    }
    true
}

/// Body of `muelle --video-wallpaper <file> <parent pid>`.
pub fn run(path: &str, parent: u32) -> Result<()> {
    // SAFETY: COM and Media Foundation for this process; the parent handle
    // is only waited on.
    let parent = unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        OpenProcess(PROCESS_SYNCHRONIZE, false, parent).context("dock no encontrado")?
    };
    let device = device()?;
    muelle_shell::log::line(&format!("fondo en video: {path}"));
    let mut screens = build(&device, path)?;
    let mut playing = true;
    let mut checked = Instant::now();
    loop {
        // SAFETY: A standard non-blocking message pump for our windows.
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let mut layer_ok = true;
        if checked.elapsed() >= Duration::from_millis(1500) {
            checked = Instant::now();
            // SAFETY: A zero wait on the parent's process handle.
            if unsafe { WaitForSingleObject(parent, 0) } == WAIT_OBJECT_0 {
                break;
            }
            // SAFETY: A read-only lookup: Explorer restarted if Progman is new.
            layer_ok = unsafe { FindWindowW(w!("Progman"), None) }.is_ok_and(|progman| {
                screens.first().is_none_or(|s| unsafe { GetParent(s.window) }.is_ok_and(|p| p == progman) || below_none())
            });
            let play = !muelle_shell::system::energy_saver_on();
            if play != playing {
                for screen in &screens {
                    // SAFETY: Calls on our live engines.
                    unsafe {
                        let _ = if play { screen.engine.Play() } else { screen.engine.Pause() };
                    }
                }
                playing = play;
            }
        }
        if !draw(&mut screens, layer_ok) {
            muelle_shell::log::line("fondo en video: la capa del escritorio cambió; se vuelve a crear");
            screens.clear();
            std::thread::sleep(Duration::from_millis(500));
            screens = build(&device, path)?;
        }
        if !playing {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    screens.clear();
    // SAFETY: Pairs with MFStartup.
    unsafe {
        let _ = MFShutdown();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wider_video_is_cropped_at_the_sides_and_a_taller_one_at_the_top() {
        let side = crop((3440, 1440), 1920, 1080);
        assert!(side.left > 0.0 && side.top == 0.0 && (side.left + side.right - 1.0).abs() < 1e-5);
        let top = crop((1080, 1920), 1920, 1080);
        assert!(top.top > 0.0 && top.left == 0.0);
        let same = crop((1920, 1080), 1920, 1080);
        assert!(same.left.abs() < 1e-5 && same.top.abs() < 1e-5);
    }
}
