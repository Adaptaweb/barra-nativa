//! Widget islands: two glass pills of their own at the bottom left and bottom
//! right of the display, apart from the dock, which keeps the apps in the
//! middle. They look like the dock (same glass, proportional corners) but are
//! taller (`island_scale` x the icon, 1.2 by default) so the widgets read well. They hide with
//! it and open each widget's panel above them.

use crate::gfx::Gfx;
use crate::scene::Theme;
use crate::widgets::{self, CellAction, Placement, WidgetId};
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
            KillTimer, SetCursor, SetTimer, SetWindowPos, WM_LBUTTONUP, WM_SETCURSOR, WM_TIMER,
            WNDCLASSW,
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
    /// Island height in times the icon, from the settings (1.0 to 1.6).
    pub island_scale: f32,
}

/// How many times the icon an island is tall, kept within the allowed range.
fn island_scale(frame: &Frame) -> f32 {
    frame.island_scale.clamp(1.0, 1.6)
}

/// Height of a widget cell inside an island, in physical pixels.
fn cell_height(frame: &Frame) -> f32 {
    (frame.icon * island_scale(frame)).round()
}

/// Height of an island's glass pill: the dock's, grown by the extra cell
/// height. It stays at the same margin from the screen edge and grows up.
fn pill_height(frame: &Frame) -> f32 {
    (frame.height + frame.icon * (island_scale(frame) - 1.0)).round()
}

/// Timer that repaints the moving cells: music while its title or artist
/// scrolls, and the battery's water.
const MARQUEE_TIMER: usize = 0x4D51;
/// Marquee repaint period (about 30 frames per second).
const MARQUEE_MS: u32 = 33;

struct Island {
    side: Side,
    hwnd: HWND,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    /// The glass pill under the cells, resized in place when the width changes.
    glass: crate::scene::GlassPill,
    /// What the window was built for: reusable only while these still match.
    monitor: Monitor,
    /// Height of the glass pill.
    height: f32,
    /// The whole look the glass was made with: any change in it builds new glass.
    theme: Theme,
    scale: f32,
    /// Each widget and where it is, in window pixels: left, width.
    cells: Vec<(WidgetId, f32, f32)>,
    /// Per cell, the parts a click acts on: x from, x to (in the cell), what.
    zones: Vec<widgets::Zones>,
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

/// What `layout_cells` produces: cells, click zones, brushes and total width.
type Laid = (
    Vec<(WidgetId, f32, f32)>,
    Vec<widgets::Zones>,
    Vec<windows::UI::Composition::CompositionSurfaceBrush>,
    f32,
);

/// Renders every cell and lays them out left to right: they decide the width.
fn layout_cells(gfx: &Gfx, frame: &Frame, list: &[WidgetId]) -> Result<Laid> {
    let readings = widgets::readings();
    let light = frame.theme.light;
    let mut cells = Vec::new();
    let mut zones = Vec::new();
    let gap = (frame.padding * 0.6).round();
    let mut x = frame.padding;
    let mut brushes = Vec::new();
    for id in list {
        widgets::set_marquee(true);
        let rendered = widgets::render_cell(gfx, *id, &readings, cell_height(frame) as u32, light);
        widgets::set_marquee(false);
        let (brush, _, cell_zones) = rendered?;
        let width = (cell_height(frame) * f32::from(widgets::dock_width(*id, widgets::dock_size(*id))) / 100.0).round();
        cells.push((*id, x, width));
        zones.push(cell_zones);
        brushes.push(brush);
        x += width + gap;
    }
    let width = (x - gap + frame.padding).round();
    Ok((cells, zones, brushes, width))
}

/// Creates one sprite per cell and inserts them on top of the glass.
fn make_sprites(
    gfx: &Gfx,
    root: &ContainerVisual,
    frame: &Frame,
    cells: &[(WidgetId, f32, f32)],
    brushes: &[windows::UI::Composition::CompositionSurfaceBrush],
) -> Result<Vec<windows::UI::Composition::SpriteVisual>> {
    let cell_h = cell_height(frame);
    let height = pill_height(frame);
    let mut sprites = Vec::new();
    for ((_, left, w), brush) in cells.iter().zip(brushes) {
        let sprite = gfx.compositor.CreateSpriteVisual()?;
        sprite.SetSize(Vector2::new(*w, cell_h))?;
        sprite.SetOffset(Vector3::new(*left, ((height - cell_h) / 2.0).round(), 0.0))?;
        sprite.SetBrush(brush)?;
        root.Children()?.InsertAtTop(&sprite)?;
        sprites.push(sprite);
    }
    Ok(sprites)
}

/// Window left edge for an island of `width` on `side`.
fn island_left(frame: &Frame, side: Side, width: f32) -> i32 {
    let screen = frame.monitor.screen;
    let left = match side {
        Side::Left => screen.left as f32 + frame.margin,
        Side::Right => screen.right as f32 - frame.margin - width,
    };
    left as i32
}

fn build(gfx: &Gfx, frame: &Frame, side: Side, list: &[WidgetId], visible: bool) -> Result<Option<Island>> {
    if list.is_empty() {
        return Ok(None);
    }
    let (cells, zones, brushes, width) = layout_cells(gfx, frame, list)?;
    let height = pill_height(frame);
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
    let glass = frame.theme.pill(gfx, Vector2::new(width, height), radius, frame.scale)?;
    root.Children()?.InsertAtTop(glass.visual())?;
    let sprites = make_sprites(gfx, &root, frame, &cells, &brushes)?;
    let left = island_left(frame, side, width);
    let top = (frame.monitor.screen.bottom as f32 - frame.margin - height) as i32;
    let island = Island {
        side,
        hwnd,
        _target: target,
        root,
        glass,
        monitor: frame.monitor,
        height,
        theme: frame.theme,
        scale: frame.scale,
        cells,
        zones,
        sprites,
        left,
        top,
        width: width as i32,
    };
    island.place(visible);
    Ok(Some(island))
}

/// Lays an island's cells out again in place, reusing its window, target,
/// root and glass so nothing flickers.
fn relayout(island: &mut Island, gfx: &Gfx, frame: &Frame, list: &[WidgetId], visible: bool) -> Result<()> {
    let (cells, zones, brushes, width) = layout_cells(gfx, frame, list)?;
    for sprite in &island.sprites {
        island.root.Children()?.Remove(sprite)?;
    }
    island.sprites.clear();
    let size = Vector2::new(width, island.height);
    island.root.SetSize(size)?;
    island.glass.resize(size)?;
    island.sprites = make_sprites(gfx, &island.root, frame, &cells, &brushes)?;
    island.cells = cells;
    island.zones = zones;
    island.left = island_left(frame, island.side, width);
    island.top = (frame.monitor.screen.bottom as f32 - frame.margin - island.height) as i32;
    island.width = width as i32;
    island.place(visible);
    Ok(())
}

/// Runs the timer only while the island has a cell that moves.
fn sync_marquee(island: &Island) {
    let readings = widgets::readings();
    let wanted = island.cells.iter().any(|(id, _, _)| widgets::animates(*id, &readings));
    // SAFETY: Our own live window; the timer is replaced or killed by id.
    unsafe {
        if wanted {
            SetTimer(Some(island.hwnd), MARQUEE_TIMER, MARQUEE_MS, None);
        } else {
            let _ = KillTimer(Some(island.hwnd), MARQUEE_TIMER);
        }
    }
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

    /// What a click at `x` does of its own, if it is on a button of a cell.
    fn action(&self, x: f32) -> Option<CellAction> {
        let index = self
            .cells
            .iter()
            .position(|(_, left, width)| x >= *left && x < left + width)?;
        let inside = x - self.cells[index].1;
        self.zones
            .get(index)?
            .iter()
            .find(|(from, to, _)| inside >= *from && inside < *to)
            .map(|(_, _, action)| *action)
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
                && f.island_scale == frame.island_scale
                && f.theme == frame.theme
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
    let visible = state.visible;
    let mut old = std::mem::take(&mut state.islands);
    for side in [Side::Left, Side::Right] {
        let list: Vec<WidgetId> = widgets::islands(side == Side::Left)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let existing = old.iter().position(|i| i.side == side).map(|at| old.remove(at));
        if list.is_empty() {
            continue;
        }
        if let Some(mut island) = existing {
            let reusable = island.monitor == frame.monitor
                && island.height == pill_height(&frame)
                && island.theme == frame.theme
                && island.scale == frame.scale;
            if reusable {
                match relayout(&mut island, &gfx, &frame, &list, visible) {
                    Ok(()) => {
                        sync_marquee(&island);
                        state.islands.push(island);
                        continue;
                    }
                    Err(error) => eprintln!("muelle: isla de widgets: {error:#}"),
                }
            }
            // Dropping the old island destroys its window before the new one.
        }
        match build(&gfx, &frame, side, &list, visible) {
            Ok(Some(island)) => {
                sync_marquee(&island);
                state.islands.push(island);
            }
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
        'all: for island in &mut state.islands {
            for (((id, _, width), sprite), zones) in island
                .cells
                .iter()
                .zip(&island.sprites)
                .zip(island.zones.iter_mut())
            {
                widgets::set_marquee(true);
                let rendered =
                    widgets::render_cell(&gfx, *id, &readings, cell_height(&frame) as u32, frame.theme.light);
                widgets::set_marquee(false);
                let Ok((brush, _, cell_zones)) = rendered else {
                    continue;
                };
                let now = (cell_height(&frame) * f32::from(widgets::dock_width(*id, widgets::dock_size(*id))) / 100.0).round();
                if (now - width).abs() > 0.5 {
                    resized = true;
                    break 'all;
                }
                *zones = cell_zones;
                let _ = sprite.SetBrush(&brush);
            }
        }
        if resized {
            rebuild(&mut state);
        }
        for island in &state.islands {
            sync_marquee(island);
        }
    });
}

/// The timer fired: repaint only the moving cells (and rebuild if their
/// width changed).
fn repaint_music() {
    STATE.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        let (Some(gfx), Some(frame)) = (state.gfx.clone(), state.frame) else {
            return;
        };
        let readings = widgets::readings();
        let mut resized = false;
        'all: for island in &mut state.islands {
            for (((id, _, width), sprite), zones) in island
                .cells
                .iter()
                .zip(&island.sprites)
                .zip(island.zones.iter_mut())
            {
                if !matches!(id, WidgetId::Musica | WidgetId::Bateria) {
                    continue;
                }
                widgets::set_marquee(true);
                let rendered =
                    widgets::render_cell(&gfx, *id, &readings, cell_height(&frame) as u32, frame.theme.light);
                widgets::set_marquee(false);
                let Ok((brush, _, cell_zones)) = rendered else {
                    continue;
                };
                let now = (cell_height(&frame) * f32::from(widgets::dock_width(*id, widgets::dock_size(*id))) / 100.0).round();
                if (now - width).abs() > 0.5 {
                    resized = true;
                    break 'all;
                }
                *zones = cell_zones;
                let _ = sprite.SetBrush(&brush);
            }
        }
        if resized {
            rebuild(&mut state);
        }
        for island in &state.islands {
            sync_marquee(island);
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
            // A button inside a cell acts on its own and does not open the panel.
            let action = STATE.with(|state| {
                state
                    .try_borrow()
                    .ok()
                    .and_then(|state| state.islands.iter().find(|i| i.hwnd == hwnd)?.action(x))
            });
            if let Some(action) = action {
                widgets::run_action(action);
                return LRESULT(0);
            }
            if let Some((gfx, id, center, top, _)) = found() {
                widgets::toggle_panel(&gfx, id, POINT { x: center, y: top }, Placement::Above);
            }
            return LRESULT(0);
        }
        WM_TIMER if w.0 == MARQUEE_TIMER => {
            repaint_music();
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
