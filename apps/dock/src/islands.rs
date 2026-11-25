//! Widget islands: two glass pills of their own at the bottom left and bottom
//! right of the display, apart from the dock, which keeps the apps in the
//! middle. They look like the dock (same glass, proportional corners) but are
//! taller (`ISLAND_SCALE` x the icon) so the widgets read well. They hide with
//! it and open each widget's panel above them.

use crate::gfx::Gfx;
use crate::scene::Theme;
use crate::widgets::{self, Placement, WidgetId};
use anyhow::Result;
use muelle_shell::system::Monitor;
use std::{cell::RefCell, sync::OnceLock};
use windows::{
    UI::Composition::{ContainerVisual, Desktop::DesktopWindowTarget},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute},
        System::{LibraryLoader::GetModuleHandleW, WinRT::Composition::ICompositorDesktopInterop},
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOPMOST, IDC_ARROW, IDC_HAND,
            LoadCursorW, RegisterClassW, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_SHOWWINDOW,
            SetCursor, SetWindowPos, WM_LBUTTONUP, WM_SETCURSOR, WNDCLASSW,
            WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
    },
    core::{BOOL, Interface, w},
};
use windows_numerics::{Vector2, Vector3};

/// Which side of the display an island sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

/// What the dock knows that the islands need: where and how to draw.
#[derive(Clone, Copy)]
pub struct Frame {
    pub monitor: Monitor,
    pub theme: Theme,
    /// Dock height, icon side, inner padding and distance from the screen
    /// edge, in physical pixels.
    pub height: f32,
    pub icon: f32,
    pub padding: f32,
    pub margin: f32,
    pub scale: f32,
}

/// Islands are 1.5 times the icon tall, so the widgets inside read well.
const ISLAND_SCALE: f32 = 1.5;

/// Height of a widget cell inside an island, in physical pixels.
fn cell_height(frame: &Frame) -> f32 {
    (frame.icon * ISLAND_SCALE).round()
}

/// Height of an island's glass pill: the dock's, grown by the extra cell
/// height. It stays at the same margin from the screen edge and grows up.
fn pill_height(frame: &Frame) -> f32 {
    (frame.height + frame.icon * (ISLAND_SCALE - 1.0)).round()
}

struct Island {
    side: Side,
    hwnd: HWND,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    /// Each widget and where it is, in window pixels: left, width.
    cells: Vec<(WidgetId, f32, f32)>,
    sprites: Vec<windows::UI::Composition::SpriteVisual>,
    left: i32,
    top: i32,
    width: i32,
}

#[derive(Default)]
struct State {
    gfx: Option<Gfx>,
    frame: Option<Frame>,
    islands: Vec<Island>,
    visible: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        visible: true,
        ..State::default()
    });
}

impl Drop for Island {
    fn drop(&mut self) {
        // SAFETY: Our own window, destroyed once.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn create_window() -> Result<HWND> {
    static CLASS: OnceLock<()> = OnceLock::new();
    // SAFETY: One class registration with a procedure that lives as long as
    // the process; the window is configured before it is shown.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("BarraIsland");
        CLASS.get_or_init(|| {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(island_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            });
        });
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("Barra"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        // Without this, the backdrop brush is black.
        let enable = BOOL::from(true);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            (&raw const enable).cast(),
            size_of::<BOOL>() as u32,
        );
        Ok(hwnd)
    }
}

fn build(gfx: &Gfx, frame: &Frame, side: Side, list: &[WidgetId], visible: bool) -> Result<Option<Island>> {
    if list.is_empty() {
        return Ok(None);
    }
    let readings = widgets::readings();
    let light = frame.theme.light;
    // Cells first: they decide the width.
    let mut cells = Vec::new();
    let gap = (frame.padding * 0.6).round();
    let mut x = frame.padding;
    let mut brushes = Vec::new();
    for id in list {
        let (brush, _) = widgets::render_cell(gfx, *id, &readings, cell_height(frame) as u32, light)?;
        let width = (cell_height(frame) * f32::from(widgets::dock_width(*id, widgets::dock_size(*id))) / 100.0).round();
        cells.push((*id, x, width));
        brushes.push(brush);
        x += width + gap;
    }
    let width = (x - gap + frame.padding).round();
    let height = pill_height(frame);
    let cell_h = cell_height(frame);
    let c = &gfx.compositor;
    let hwnd = create_window()?;
    // SAFETY: `hwnd` is the live window created above.
    let target = unsafe {
        c.cast::<ICompositorDesktopInterop>()?
            .CreateDesktopWindowTarget(hwnd, true)?
    };
    let root = c.CreateContainerVisual()?;
    root.SetSize(Vector2::new(width, height))?;
    target.SetRoot(&root)?;
    let radius = height * 0.36;
    let glass = frame.theme.surface(gfx, Vector2::new(width, height), radius, frame.scale)?;
    root.Children()?.InsertAtTop(&glass)?;
    let mut sprites = Vec::new();
    for ((_, left, w), brush) in cells.iter().zip(&brushes) {
        let sprite = c.CreateSpriteVisual()?;
        sprite.SetSize(Vector2::new(*w, cell_h))?;
        sprite.SetOffset(Vector3::new(*left, ((height - cell_h) / 2.0).round(), 0.0))?;
        sprite.SetBrush(brush)?;
        root.Children()?.InsertAtTop(&sprite)?;
        sprites.push(sprite);
    }
    let screen = frame.monitor.screen;
    let left = match side {
        Side::Left => screen.left as f32 + frame.margin,
        Side::Right => screen.right as f32 - frame.margin - width,
    } as i32;
    let top = (screen.bottom as f32 - frame.margin - height) as i32;
    let island = Island {
        side,
        hwnd,
        _target: target,
        root,
        cells,
        sprites,
        left,
        top,
        width: width as i32,
    };
    island.place(visible);
    Ok(Some(island))
}

impl Island {
    fn place(&self, visible: bool) {
        let height = self.root.Size().map_or(0.0, |s| s.Y) as i32;
        // SAFETY: Moving and showing our own window without activating it.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                self.left,
                self.top,
                self.width,
                height,
                SWP_NOACTIVATE | if visible { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW },
            );
        }
    }

    fn hit(&self, x: f32) -> Option<(WidgetId, f32)> {
        self.cells
            .iter()
            .find(|(_, left, width)| x >= *left && x < left + width)
            .map(|(id, left, width)| (*id, left + width / 2.0))
    }
}

/// Lays the islands out again for the dock's current look and place.
pub fn update(gfx: &Gfx, frame: Frame) {
    STATE.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        // The dock is laid out again for many reasons (an app opened): the
        // islands only when their place or look changed.
        let same = state.frame.is_some_and(|f| {
            f.monitor == frame.monitor
                && f.height == frame.height
                && f.icon == frame.icon
                && f.margin == frame.margin
                && f.theme.light == frame.theme.light
        });
        state.gfx = Some(gfx.clone());
        state.frame = Some(frame);
        if !same || state.islands.is_empty() {
            rebuild(&mut state);
        }
    });
}

/// The settings or a widget's width changed: draw the islands again.
pub fn refresh() {
    STATE.with(|state| {
        if let Ok(mut state) = state.try_borrow_mut() {
            rebuild(&mut state);
        }
    });
}

fn rebuild(state: &mut State) {
    let (Some(gfx), Some(frame)) = (state.gfx.clone(), state.frame) else {
        return;
    };
    state.islands.clear();
    for side in [Side::Left, Side::Right] {
        let list: Vec<WidgetId> = widgets::islands(side == Side::Left)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        match build(&gfx, &frame, side, &list, state.visible) {
            Ok(Some(island)) => state.islands.push(island),
            Ok(None) => {}
            Err(error) => eprintln!("muelle: isla de widgets: {error:#}"),
        }
    }
}

/// A reading changed: repaint the cells in place, or lay the islands out
/// again if a cell's width changed.
pub fn repaint() {
    STATE.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        let (Some(gfx), Some(frame)) = (state.gfx.clone(), state.frame) else {
            return;
        };
        let readings = widgets::readings();
        let mut resized = false;
        'all: for island in &state.islands {
            for ((id, _, width), sprite) in island.cells.iter().zip(&island.sprites) {
                let Ok((brush, _)) =
                    widgets::render_cell(&gfx, *id, &readings, cell_height(&frame) as u32, frame.theme.light)
                else {
                    continue;
                };
                let now = (cell_height(&frame) * f32::from(widgets::dock_width(*id, widgets::dock_size(*id))) / 100.0).round();
                if (now - width).abs() > 0.5 {
                    resized = true;
                    break 'all;
                }
                let _ = sprite.SetBrush(&brush);
            }
        }
        if resized {
            rebuild(&mut state);
        }
    });
}

/// Shows or hides the islands with the dock (full screen, auto-hide).
pub fn set_visible(visible: bool) {
    STATE.with(|state| {
        if let Ok(mut state) = state.try_borrow_mut()
            && state.visible != visible
        {
            state.visible = visible;
            for island in &state.islands {
                island.place(visible);
            }
        }
    });
}

unsafe extern "system" fn island_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let x = f32::from((l.0 & 0xFFFF) as i16);
    let found = || {
        STATE.with(|state| {
            state.try_borrow().ok().and_then(|state| {
                let island = state.islands.iter().find(|i| i.hwnd == hwnd)?;
                let (id, center) = island.hit(x)?;
                Some((state.gfx.clone()?, id, island.left + center as i32, island.top, island.side))
            })
        })
    };
    match msg {
        WM_LBUTTONUP => {
            if let Some((gfx, id, center, top, _)) = found() {
                widgets::toggle_panel(&gfx, id, POINT { x: center, y: top }, Placement::Above);
            }
            return LRESULT(0);
        }
        WM_SETCURSOR => {
            // SAFETY: Stock cursors belong to Windows.
            unsafe {
                if let Ok(cursor) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(Some(cursor));
                }
            }
            return LRESULT(1);
        }
        _ => {}
    }
    // SAFETY: Default handling for our own window.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}
