//! The sharp backdrop of the liquid glass themes.
//!
//! The glass Windows gives a window (`HostBackdropBrush`) is always blurred,
//! so the liquid themes see through to the desktop themselves: the display is
//! captured with Windows.Graphics.Capture, the dock's own windows are kept
//! out of that capture (`WDA_EXCLUDEFROMCAPTURE`, which also keeps them out of
//! screenshots while a liquid theme is on), and each frame is copied, on the
//! GPU, into one drawing surface the size of the display. Every liquid glass
//! shows that surface twice through brushes: as it is, aligned with what is
//! behind it, and magnified a little in a band along its edge, so what passes
//! behind bends there like light through a lens.
//!
//! Frames come only when the screen changes. Their arrival only posts a
//! message to an island window; the copy happens on the UI thread.

use crate::gfx::Gfx;
use anyhow::Result;
use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
};
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    UI::Composition::{
        CompositionDrawingSurface, CompositionMaskBrush, CompositionNineGridBrush, CompositionStretch,
        CompositionSurfaceBrush, SpriteVisual,
    },
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        Graphics::{
            Dxgi::{IDXGIDevice, IDXGISurface},
            Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint},
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
        },
        UI::WindowsAndMessaging::{PostMessageW, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE},
    },
    core::Interface,
};
use windows_numerics::{Matrix3x2, Vector2};

/// How much the lens band magnifies what is behind it.
const LENS: f32 = 1.12;
/// Width of the lens band along the edge, in physical pixels per unit of scale.
const BAND: f32 = 9.0;

struct Capture {
    screen: RECT,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    _item: GraphicsCaptureItem,
    surface: CompositionDrawingSurface,
}

thread_local! {
    static CAPTURE: RefCell<Option<Capture>> = const { RefCell::new(None) };
}

/// A frame is waiting and its message is already posted.
static PENDING: AtomicBool = AtomicBool::new(false);
/// The window told of new frames, and with which message: the newest
/// island, since an island window can be rebuilt while the capture runs.
static NOTIFY: AtomicIsize = AtomicIsize::new(0);
static MESSAGE: AtomicU32 = AtomicU32::new(0);

/// Keeps `hwnd` out of (`on`) or back in screen captures.
pub fn exclude(hwnd: HWND, on: bool) {
    // SAFETY: Our own live window.
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE });
    }
}

/// Starts capturing display `screen` (no-op if already capturing it); each
/// new frame posts `message` to `notify`, whose handler calls [`frame`].
pub fn start(gfx: &Gfx, screen: RECT, notify: HWND, message: u32) -> Result<()> {
    NOTIFY.store(notify.0 as isize, Ordering::Release);
    MESSAGE.store(message, Ordering::Release);
    let running = CAPTURE.with(|c| c.borrow().as_ref().is_some_and(|c| c.screen == screen));
    if running {
        return Ok(());
    }
    stop();
    let (width, height) = (screen.right - screen.left, screen.bottom - screen.top);
    // SAFETY: Standard Windows.Graphics.Capture setup on our own device.
    let capture = unsafe {
        let centre = POINT { x: screen.left + width / 2, y: screen.top + height / 2 };
        let monitor = MonitorFromPoint(centre, MONITOR_DEFAULTTONEAREST);
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = interop.CreateForMonitor(monitor)?;
        let dxgi: IDXGIDevice = gfx.d3d().cast()?;
        let device: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?;
        let size = SizeInt32 { Width: width, Height: height };
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
        pool.FrameArrived(&TypedEventHandler::new(move |_, _| {
            if !PENDING.swap(true, Ordering::AcqRel) {
                let target = HWND(NOTIFY.load(Ordering::Acquire) as *mut _);
                let _ = PostMessageW(Some(target), MESSAGE.load(Ordering::Acquire), WPARAM(0), LPARAM(0));
            }
            Ok(())
        }))?;
        let session = pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsCursorCaptureEnabled(false);
        // Windows 11 draws a yellow border around a capture unless told not to.
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;
        Capture {
            screen,
            pool,
            session,
            _item: item,
            surface: gfx.blank_surface(width as u32, height as u32)?,
        }
    };
    CAPTURE.with(|c| *c.borrow_mut() = Some(capture));
    Ok(())
}

/// Stops the capture, if any.
pub fn stop() {
    if let Some(capture) = CAPTURE.with(|c| c.borrow_mut().take()) {
        let _ = capture.session.Close();
        let _ = capture.pool.Close();
    }
    PENDING.store(false, Ordering::Release);
}

/// Copies the newest captured frame into the shared surface.
pub fn frame(gfx: &Gfx) {
    PENDING.store(false, Ordering::Release);
    CAPTURE.with(|c| {
        let c = c.borrow();
        let Some(capture) = c.as_ref() else {
            return;
        };
        // Only the newest frame matters.
        let mut newest = None;
        while let Ok(frame) = capture.pool.TryGetNextFrame() {
            if let Some(old) = newest.replace(frame) {
                let _ = old.Close();
            }
        }
        let Some(frame) = newest else {
            return;
        };
        let copied = (|| -> Result<()> {
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            // SAFETY: The frame's surface stays alive until the frame is closed below.
            let dxgi: IDXGISurface = unsafe { access.GetInterface()? };
            gfx.redraw_surface(&capture.surface, |dc| {
                crate::gfx::draw_dxgi(dc, &dxgi)
            })
        })();
        if let Err(error) = copied {
            muelle_shell::log::line(&format!("vidrio líquido: {error:#}"));
        }
        let _ = frame.Close();
    });
}

/// The two layers a liquid glass shows over its tint: what is behind it,
/// sharp, and the lens band along its edge.
pub struct Layers {
    pub clear: SpriteVisual,
    pub lens: SpriteVisual,
    clear_brush: CompositionSurfaceBrush,
    lens_brush: CompositionSurfaceBrush,
}

impl Layers {
    /// Layers for a glass with corner `radius`, or `None` without a capture.
    pub fn new(gfx: &Gfx, radius: f32, scale: f32) -> Result<Option<Self>> {
        let Some(surface) = CAPTURE.with(|c| c.borrow().as_ref().map(|c| c.surface.clone())) else {
            return Ok(None);
        };
        let c = &gfx.compositor;
        let brush = || -> Result<CompositionSurfaceBrush> {
            let brush = c.CreateSurfaceBrushWithSurface(&surface)?;
            brush.SetStretch(CompositionStretch::None)?;
            brush.SetHorizontalAlignmentRatio(0.0)?;
            brush.SetVerticalAlignmentRatio(0.0)?;
            Ok(brush)
        };
        let clear_brush = brush()?;
        let lens_brush = brush()?;
        let clear = c.CreateSpriteVisual()?;
        clear.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        clear.SetBrush(&clear_brush)?;
        // The band: a ring drawn once and stretched by a nine-grid, so its
        // width stays the same whatever the glass's size.
        let band = (BAND * scale).round().max(4.0);
        let side = (radius * 2.0 + 2.0).ceil();
        let ring = gfx.paint((side, side), scale, |pen| {
            pen.stroke_round_rect((band / 2.0, band / 2.0), (side - band, side - band), (radius - band / 2.0).max(0.0), band, [1.0, 1.0, 1.0, 1.0])
        })?;
        let nine: CompositionNineGridBrush = c.CreateNineGridBrush()?;
        nine.SetSource(&ring.brush)?;
        nine.SetInsets(radius.max(band) + 1.0)?;
        let mask: CompositionMaskBrush = c.CreateMaskBrush()?;
        mask.SetSource(&lens_brush)?;
        mask.SetMask(&nine)?;
        let lens = c.CreateSpriteVisual()?;
        lens.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        lens.SetBrush(&mask)?;
        Ok(Some(Self { clear, lens, clear_brush, lens_brush }))
    }

    /// Aligns the backdrop with a glass of `size` whose top left is at
    /// (`x`, `y`) in screen pixels.
    pub fn place(&self, (x, y): (f32, f32), size: Vector2) -> Result<()> {
        let screen = CAPTURE.with(|c| c.borrow().as_ref().map(|c| c.screen));
        let Some(screen) = screen else {
            return Ok(());
        };
        let (ox, oy) = (x - screen.left as f32, y - screen.top as f32);
        self.clear_brush.SetTransformMatrix(Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: -ox, M32: -oy })?;
        // Magnified about the glass's centre: a point q of the capture lands
        // at c + LENS * (q - o - c).
        let (cx, cy) = (size.X / 2.0, size.Y / 2.0);
        self.lens_brush.SetTransformMatrix(Matrix3x2 {
            M11: LENS,
            M12: 0.0,
            M21: 0.0,
            M22: LENS,
            M31: cx - LENS * (ox + cx),
            M32: cy - LENS * (oy + cy),
        })?;
        Ok(())
    }
}
