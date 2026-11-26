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
        CompositionDrawingSurface, CompositionMaskBrush, CompositionRoundedRectangleGeometry,
        CompositionSpriteShape, CompositionStretch, CompositionVisualSurface, ShapeVisual, Visual,
        CompositionSurfaceBrush,
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

/// Keeps `hwnd` out of (`on`) or back in screen captures. With
/// `BARRA_LIQUID_SHOW` set, windows stay in captures (the glass then also
/// shows itself), so a screenshot can show what the glass looks like.
pub fn exclude(hwnd: HWND, on: bool) {
    let on = on && std::env::var_os("BARRA_LIQUID_SHOW").is_none();
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
    FIRST.store(true, Ordering::Release);
    muelle_shell::log::line(&format!("vidrio líquido: captura de {width}×{height} iniciada"));
    Ok(())
}

/// The next frame is the first of this capture (logged once).
static FIRST: AtomicBool = AtomicBool::new(false);

/// Stops the capture, if any.
pub fn stop() {
    if let Some(capture) = CAPTURE.try_with(|c| c.borrow_mut().take()).ok().flatten() {
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
        match copied {
            Err(error) => muelle_shell::log::line(&format!("vidrio líquido: {error:#}")),
            Ok(()) if FIRST.swap(false, Ordering::AcqRel) => {
                muelle_shell::log::line("vidrio líquido: primer fotograma copiado");
            }
            Ok(()) => {}
        }
        let _ = frame.Close();
    });
}

/// The lens, outermost ring first: where each ring starts and ends (logical
/// pixels in from the edge) and how far it moves what is behind it at the
/// edge. The shift shrinks inward, so what passes behind bends in a curve
/// near the edge and runs straight in the middle.
const RINGS: [(f32, f32, f32); 3] = [(0.0, 5.0, 18.0), (5.0, 10.0, 10.0), (10.0, 16.0, 4.0)];

/// Message a window gets when the capture has a new frame; its handler
/// calls [`frame`].
pub const FRAME_MESSAGE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x51;

thread_local! {
    /// The windows whose glass shows the capture, one entry per [`Layers`].
    /// The newest is told of new frames; with none left the capture stops.
    static USERS: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
}

/// One ring of the lens: a band along the glass's edge, cut out of the
/// magnified capture by a stroke drawn into a visual surface, so the band
/// follows the glass at any size without being painted again.
struct Ring {
    lens: CompositionSurfaceBrush,
    /// How far the ring moves what is behind it at the edge (physical pixels).
    shift: f32,
    /// Where the band starts and ends in from the edge (physical pixels).
    from: f32,
    to: f32,
    shape: ShapeVisual,
    stroke: CompositionSpriteShape,
    geometry: CompositionRoundedRectangleGeometry,
    surface: CompositionVisualSurface,
}

/// What a liquid glass shows over its tint: what is behind it, sharp, and
/// the lens rings along its edge.
pub struct Layers {
    /// Bottom to top: the sharp backdrop, then the rings (and a bezel's rim).
    pub visuals: Vec<Visual>,
    clear_brush: CompositionSurfaceBrush,
    rings: Vec<Ring>,
    /// A bezel's bright rim along the edge, and its outline.
    rim: Option<(ShapeVisual, CompositionRoundedRectangleGeometry, f32)>,
    /// The glass's corner radius in physical pixels.
    radius: f32,
    /// The window this glass is in, told of new frames.
    owner: isize,
}

impl Layers {
    /// Starts (or joins) the capture of display `screen` and makes the
    /// layers for a glass with corner `radius` in window `owner`, whose
    /// handler of [`FRAME_MESSAGE`] calls [`frame`].
    pub fn new(gfx: &Gfx, screen: RECT, owner: HWND, radius: f32, scale: f32) -> Result<Self> {
        Self::build(gfx, screen, owner, radius, scale, false)
    }

    /// Only the lens and a bright rim, thinner, for drawing over a panel's
    /// own content (bubbles, the weather and music cards): the glass shows as
    /// a bezel around what the panel draws, which stays readable.
    pub fn bezel(gfx: &Gfx, screen: RECT, owner: HWND, radius: f32, scale: f32) -> Result<Self> {
        Self::build(gfx, screen, owner, radius, scale, true)
    }

    fn build(gfx: &Gfx, screen: RECT, owner: HWND, radius: f32, scale: f32, bezel: bool) -> Result<Self> {
        start(gfx, screen, owner, FRAME_MESSAGE)?;
        USERS.with(|users| users.borrow_mut().push(owner.0 as isize));
        let c = &gfx.compositor;
        // From here on, `Drop` gives the capture back if anything fails.
        let mut layers = Self {
            visuals: Vec::new(),
            clear_brush: c.CreateSurfaceBrush()?,
            rings: Vec::new(),
            rim: None,
            radius,
            owner: owner.0 as isize,
        };
        let Some(surface) = CAPTURE.with(|c| c.borrow().as_ref().map(|c| c.surface.clone())) else {
            anyhow::bail!("sin captura");
        };
        let brush = || -> Result<CompositionSurfaceBrush> {
            let brush = c.CreateSurfaceBrushWithSurface(&surface)?;
            brush.SetStretch(CompositionStretch::None)?;
            brush.SetHorizontalAlignmentRatio(0.0)?;
            brush.SetVerticalAlignmentRatio(0.0)?;
            Ok(brush)
        };
        let sprite = |brush: &windows::UI::Composition::CompositionBrush| -> Result<Visual> {
            let sprite = c.CreateSpriteVisual()?;
            sprite.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
            sprite.SetBrush(brush)?;
            Ok(sprite.cast()?)
        };
        layers.clear_brush = brush()?;
        if !bezel {
            layers.visuals.push(sprite(&layers.clear_brush.cast()?)?);
        }
        // A bezel is narrower and bends less: it frames content.
        let band = if bezel { 0.6 } else { 1.0 };
        let white = c.CreateColorBrushWithColor(windows::UI::Color { A: 255, R: 255, G: 255, B: 255 })?;
        for (from, to, shift) in RINGS {
            let (from, to) = ((from * band * scale).round(), (to * band * scale).round());
            let shift = shift * band;
            let geometry = c.CreateRoundedRectangleGeometry()?;
            let stroke = c.CreateSpriteShapeWithGeometry(&geometry)?;
            stroke.SetStrokeBrush(&white)?;
            stroke.SetStrokeThickness(to - from)?;
            let shape = c.CreateShapeVisual()?;
            shape.Shapes()?.Append(&stroke)?;
            let surface = c.CreateVisualSurface()?;
            surface.SetSourceVisual(&shape)?;
            let picture = c.CreateSurfaceBrushWithSurface(&surface)?;
            picture.SetStretch(CompositionStretch::None)?;
            picture.SetHorizontalAlignmentRatio(0.0)?;
            picture.SetVerticalAlignmentRatio(0.0)?;
            let lens = brush()?;
            let mask: CompositionMaskBrush = c.CreateMaskBrush()?;
            mask.SetSource(&lens)?;
            mask.SetMask(&picture)?;
            layers.visuals.push(sprite(&mask.cast()?)?);
            layers.rings.push(Ring { lens, shift: shift * scale, from, to, shape, stroke, geometry, surface });
        }
        if bezel {
            // Light catching the edge: bright at the top left and bottom
            // right corners, faint along the sides.
            let gradient = c.CreateLinearGradientBrush()?;
            gradient.SetStartPoint(Vector2 { X: 0.0, Y: 0.0 })?;
            gradient.SetEndPoint(Vector2 { X: 1.0, Y: 1.0 })?;
            let stops = gradient.ColorStops()?;
            for (at, alpha) in [(0.0, 200), (0.35, 40), (0.65, 30), (1.0, 150)] {
                stops.Append(&c.CreateColorGradientStopWithOffsetAndColor(at, windows::UI::Color { A: alpha, R: 255, G: 255, B: 255 })?)?;
            }
            let width = scale.max(1.0);
            let geometry = c.CreateRoundedRectangleGeometry()?;
            let line = c.CreateSpriteShapeWithGeometry(&geometry)?;
            line.SetStrokeBrush(&gradient)?;
            line.SetStrokeThickness(width)?;
            let shape = c.CreateShapeVisual()?;
            shape.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
            shape.Shapes()?.Append(&line)?;
            layers.visuals.push(shape.cast()?);
            layers.rim = Some((shape, geometry, width));
        }
        Ok(layers)
    }

    /// Aligns the backdrop with a glass of `size` whose top left is at
    /// (`x`, `y`) in screen pixels. Cheap: only transforms and the rings'
    /// shapes change, so it can run on every frame of an animation.
    pub fn place(&self, (x, y): (f32, f32), size: Vector2) -> Result<()> {
        let screen = CAPTURE.with(|c| c.borrow().as_ref().map(|c| c.screen));
        let Some(screen) = screen else {
            return Ok(());
        };
        let (ox, oy) = (x - screen.left as f32, y - screen.top as f32);
        self.clear_brush.SetTransformMatrix(Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: -ox, M32: -oy })?;
        let radius = self.radius.min(size.X / 2.0).min(size.Y / 2.0);
        // Magnified about the glass's centre, enough on each axis that the
        // edge shows what lies `shift` pixels further in: a point q of the
        // capture lands at c + s * (q - o - c), with s = 1 / (1 - 2 shift / side).
        let (cx, cy) = (size.X / 2.0, size.Y / 2.0);
        for ring in &self.rings {
            let sx = 1.0 / (1.0 - (2.0 * ring.shift / size.X.max(1.0)).min(0.6));
            let sy = 1.0 / (1.0 - (2.0 * ring.shift / size.Y.max(1.0)).min(0.6));
            ring.lens.SetTransformMatrix(Matrix3x2 {
                M11: sx,
                M12: 0.0,
                M21: 0.0,
                M22: sy,
                M31: cx - sx * (ox + cx),
                M32: cy - sy * (oy + cy),
            })?;
            // The band: a stroke along the outline, `mid` in from the edge.
            let to = ring.to.min(size.Y.min(size.X) / 2.0);
            let width = (to - ring.from).max(0.0);
            let mid = (ring.from + to) / 2.0;
            ring.stroke.SetStrokeThickness(width)?;
            ring.geometry.SetOffset(Vector2 { X: mid, Y: mid })?;
            ring.geometry.SetSize(Vector2 { X: (size.X - mid * 2.0).max(0.0), Y: (size.Y - mid * 2.0).max(0.0) })?;
            let corner = (radius - mid).max(0.0);
            ring.geometry.SetCornerRadius(Vector2 { X: corner, Y: corner })?;
            ring.shape.SetSize(size)?;
            ring.surface.SetSourceSize(size)?;
        }
        if let Some((_, geometry, width)) = &self.rim {
            let half = width / 2.0;
            geometry.SetOffset(Vector2 { X: half, Y: half })?;
            geometry.SetSize(Vector2 { X: (size.X - width).max(0.0), Y: (size.Y - width).max(0.0) })?;
            let corner = (radius - half).max(0.0);
            geometry.SetCornerRadius(Vector2 { X: corner, Y: corner })?;
        }
        Ok(())
    }
}

impl Drop for Layers {
    /// Gives the capture back; the last glass to go stops it.
    fn drop(&mut self) {
        let Ok(left) = USERS.try_with(|users| {
            let mut users = users.borrow_mut();
            if let Some(at) = users.iter().position(|&owner| owner == self.owner) {
                users.remove(at);
            }
            users.last().copied()
        }) else {
            return;
        };
        match left {
            Some(owner) => NOTIFY.store(owner, Ordering::Release),
            None => stop(),
        }
    }
}
