//! The panel a widget opens when clicked: a frosted popup like the dock's
//! menus, laid out from a list of blocks (title, rows, bars, buttons, a grid
//! of Home Assistant buttons...). It closes when it loses the focus, when
//! Esc is pressed, or when its widget is clicked again. While open it
//! follows the readings, so a light switched on shows at once.

use super::{WidgetId, panel_content};
use crate::gfx::{Gfx, Rgba, Weight};
use crate::menu::{apply_popup_frame, draw_popup_background};
use anyhow::Result;
use std::{
    cell::RefCell,
    sync::OnceLock,
    time::{Duration, Instant},
};
use windows::{
    UI::Composition::{ContainerVisual, Desktop::DesktopWindowTarget},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, WinRT::Composition::ICompositorDesktopInterop},
        UI::{
            Input::KeyboardAndMouse::VK_ESCAPE,
            WindowsAndMessaging::{
                CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOPMOST,
                IDC_ARROW, IDC_HAND, LoadCursorW, RegisterClassW, SW_SHOW,
                SWP_NOACTIVATE, SetCursor, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow,
                WM_APP, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_TIMER,
                WM_RBUTTONUP,
                WM_SETCURSOR, WNDCLASSW, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
                WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::{Interface, w},
};
use windows_numerics::{Vector2, Vector3};

/// Logical width of every panel.
pub const WIDTH: f32 = 330.0;
const PAD: f32 = 16.0;
const CLOSE: u32 = WM_APP + 1;
/// Watches for a click outside the panel while it is open.
const OUTSIDE_TIMER: usize = 1;

/// Whether a mouse button went down outside the panel since the last check.
/// Menus that apps open from the panel (class `#32768`) count as inside.
fn clicked_outside(hwnd: HWND) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON},
        WindowsAndMessaging::{GA_ROOT, GetAncestor, GetClassNameW, GetCursorPos, WindowFromPoint},
    };
    static WAS_DOWN: AtomicBool = AtomicBool::new(true);
    // SAFETY: Read-only queries of input state and windows.
    unsafe {
        let down = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
            .iter()
            .any(|key| GetAsyncKeyState(i32::from(key.0)) as u16 & 0x8000 != 0);
        let pressed = down && !WAS_DOWN.swap(down, Ordering::Relaxed);
        if !down {
            WAS_DOWN.store(false, Ordering::Relaxed);
        }
        if !pressed {
            return false;
        }
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return false;
        }
        let under = WindowFromPoint(point);
        if under.is_invalid() {
            return true;
        }
        if GetAncestor(under, GA_ROOT) == hwnd {
            return false;
        }
        let mut class = [0u16; 16];
        let n = GetClassNameW(under, &mut class) as usize;
        String::from_utf16_lossy(&class[..n]) != "#32768"
    }
}

/// Where the panel goes from its anchor point.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Under the menu bar.
    Below,
    /// Over the dock.
    Above,
}

/// What a click on part of the panel does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelAction {
    Open(String),
    HomeToggle(String),
    RefreshIa,
    /// A tray icon: a left click here, a right click opens its menu.
    Tray(String),
}

/// A Home Assistant button in the panel grid.
pub struct Cell {
    pub title: String,
    pub detail: String,
    pub on: bool,
    pub action: Option<PanelAction>,
}

/// The parts a panel is made of, top to bottom.
pub enum Block {
    /// Bold title with a faded note on the right.
    Title(String, String),
    /// Small uppercase heading.
    Section(String),
    /// Wrapped text, faded or not.
    Text(String, bool),
    /// Big emoji and big text (weather, the time).
    Big(Option<String>, String),
    /// Label, a bar, the percent, and a faded note under it.
    Meter {
        label: String,
        percent: u8,
        color: Rgba,
        note: String,
    },
    /// A colored dot, a time and a title.
    Event(String, String),
    /// Numbered setup steps.
    Steps(Vec<String>),
    Grid(Vec<Cell>),
    /// Tray icons: picture and tooltip, five per row.
    Tray(Vec<super::TrayItem>),
    /// Pill buttons: label, action, primary (filled in the accent color).
    Buttons(Vec<(String, PanelAction, bool)>),
    Gap(f32),
}

const ACCENT: Rgba = [0.039, 0.518, 1.0, 1.0];
const RED: Rgba = [1.0, 0.231, 0.188, 1.0];

fn ink(dark: bool, alpha: f32) -> Rgba {
    if dark {
        [1.0, 1.0, 1.0, alpha * 0.94]
    } else {
        [0.0, 0.0, 0.0, alpha * 0.86]
    }
}

struct Hit {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    action: PanelAction,
}

struct Panel {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    gfx: Gfx,
    id: WidgetId,
    anchor: POINT,
    placement: Placement,
    scale: f32,
    dark: bool,
    hits: Vec<Hit>,
}

thread_local! {
    static OPEN: RefCell<Option<Panel>> = const { RefCell::new(None) };
    /// The widget whose panel just closed, and when: a click on that widget
    /// closes the panel through the focus change, and must not reopen it.
    static CLOSED: RefCell<Option<(WidgetId, Instant)>> = const { RefCell::new(None) };
}

/// Opens the widget's panel at `anchor`, or closes it if it is open.
pub fn toggle(gfx: &Gfx, id: WidgetId, anchor: POINT, placement: Placement) {

    let was_open = OPEN.with(|open| {
        open.try_borrow()
            .is_ok_and(|panel| panel.as_ref().is_some_and(|p| p.id == id))
    });
    close();
    let just_closed = CLOSED.with(|closed| {
        closed
            .borrow()
            .is_some_and(|(last, at)| last == id && at.elapsed() < Duration::from_millis(350))
    });
    if was_open || just_closed {
        return;
    }
    match open(gfx, id, anchor, placement) {
        Ok(panel) => OPEN.with(|open| {
            if let Ok(mut slot) = open.try_borrow_mut() {
                *slot = Some(panel);
            }
        }),
        Err(error) => eprintln!("muelle: panel de {id:?}: {error:#}"),
    }
}

/// Repaints the open panel with the latest readings.
pub fn refresh() {
    OPEN.with(|open| {
        if let Ok(mut slot) = open.try_borrow_mut()
            && let Some(panel) = slot.as_mut()
            && let Err(error) = panel.paint()
        {
            eprintln!("muelle: panel: {error:#}");
        }
    });
}

fn close() {
    let panel = OPEN.with(|open| open.try_borrow_mut().ok().and_then(|mut slot| slot.take()));
    if let Some(panel) = panel {
        CLOSED.with(|closed| *closed.borrow_mut() = Some((panel.id, Instant::now())));
        // SAFETY: Our own window, destroyed once.
        unsafe {
            let _ = DestroyWindow(panel.hwnd);
        }
    }
}

fn create_window() -> Result<HWND> {
    static CLASS: OnceLock<()> = OnceLock::new();
    // SAFETY: The class is registered once with a procedure that lives for
    // the whole process; the window is configured before it is shown.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("BarraPanel");
        CLASS.get_or_init(|| {
            RegisterClassW(&WNDCLASSW {
                style: CS_DROPSHADOW,
                lpfnWndProc: Some(panel_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            });
        });
        Ok(CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
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
        )?)
    }
}

fn open(gfx: &Gfx, id: WidgetId, anchor: POINT, placement: Placement) -> Result<Panel> {
    let monitor = muelle_shell::system::monitor_at(anchor)
        .or_else(muelle_shell::system::primary_monitor);
    let scale = monitor.as_ref().map_or(1.0, |m| m.scale);
    let dark = !crate::appearance::current().light;
    let hwnd = create_window()?;
    apply_popup_frame(hwnd, dark);
    // SAFETY: `hwnd` is the live window created above.
    let target = unsafe {
        gfx.compositor
            .cast::<ICompositorDesktopInterop>()?
            .CreateDesktopWindowTarget(hwnd, true)?
    };
    let root = gfx.compositor.CreateContainerVisual()?;
    target.SetRoot(&root)?;
    let mut panel = Panel {
        hwnd,
        _target: target,
        root,
        gfx: gfx.clone(),
        id,
        anchor,
        placement,
        scale,
        dark,
        hits: Vec::new(),
    };
    panel.paint()?;
    // SAFETY: Showing our own window and giving it the focus, so losing the
    // focus (a click elsewhere) closes it.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        SetTimer(Some(hwnd), OUTSIDE_TIMER, 50, None);
    }
    Ok(panel)
}

impl Panel {
    fn paint(&mut self) -> Result<()> {
        let blocks = panel_content::blocks(self.id, &super::readings());
        let px = |v: f32| (v * self.scale).round();
        let width = px(WIDTH);
        let inner = width - px(PAD) * 2.0;
        let height = measure(&self.gfx, &blocks, self.scale, inner)? + px(PAD) * 2.0;
        let mut hits = Vec::new();
        let content = self.gfx.paint((width, height), self.scale, |pen| {
            draw(pen, &self.gfx, &blocks, self.scale, self.dark, inner, &mut hits)
        })?;
        let c = &self.gfx.compositor;
        let children = self.root.Children()?;
        children.RemoveAll()?;
        self.root.SetSize(Vector2::new(width, height))?;
        let glass = muelle_shell::system::transparency_enabled();
        draw_popup_background(&children, self.dark, glass, width, height)?;
        let sprite = c.CreateSpriteVisual()?;
        sprite.SetSize(Vector2::new(content.width, content.height))?;
        sprite.SetOffset(Vector3::new(0.0, 0.0, 0.0))?;
        sprite.SetBrush(&content.brush)?;
        children.InsertAtTop(&sprite)?;
        self.hits = hits;
        self.place(width as i32, height as i32);
        Ok(())
    }

    fn place(&self, width: i32, height: i32) {
        let monitor = muelle_shell::system::monitor_at(self.anchor)
            .or_else(muelle_shell::system::primary_monitor);
        let gap = (8.0 * self.scale) as i32;
        let mut x = self.anchor.x - width / 2;
        let mut y = match self.placement {
            Placement::Below => self.anchor.y + gap,
            Placement::Above => self.anchor.y - gap - height,
        };
        if let Some(m) = monitor {
            x = x.clamp(m.work_left + gap, (m.work_right - gap - width).max(m.work_left));
            y = y.clamp(m.screen.top + gap, (m.work_bottom - gap - height).max(m.screen.top));
        }
        // SAFETY: Moving our own window.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<PanelAction> {
        self.hits
            .iter()
            .find(|h| x >= h.left && x < h.right && y >= h.top && y < h.bottom)
            .map(|h| h.action.clone())
    }
}

fn line_height(gfx: &Gfx, size: f32, scale: f32) -> Result<f32> {
    Ok(gfx.measure("Ág", size, scale, Weight::Normal)?.1)
}

fn block_height(gfx: &Gfx, block: &Block, scale: f32, inner: f32) -> Result<f32> {
    let px = |v: f32| (v * scale).round();
    Ok(match block {
        Block::Title(..) => line_height(gfx, 15.0, scale)? + px(10.0),
        Block::Section(_) => line_height(gfx, 9.0, scale)? + px(6.0),
        Block::Text(text, _) => gfx.measure_paragraph(text, 11.5, scale, inner)? + px(8.0),
        Block::Big(..) => line_height(gfx, 30.0, scale)? + px(6.0),
        Block::Meter { .. } => line_height(gfx, 12.0, scale)? * 2.0 + px(12.0),
        Block::Event(..) => line_height(gfx, 12.0, scale)? + px(8.0),
        Block::Steps(steps) => {
            let mut h = 0.0;
            for step in steps {
                h += gfx.measure_paragraph(step, 11.5, scale, inner - px(22.0))? + px(6.0);
            }
            h + px(4.0)
        }
        Block::Grid(cells) => {
            let rows = cells.len().div_ceil(2) as f32;
            rows * px(52.0) + (rows - 1.0).max(0.0) * px(8.0) + px(8.0)
        }
        Block::Buttons(_) => px(30.0) + px(8.0),
        Block::Tray(items) => {
            let rows = items.len().div_ceil(5).max(1) as f32;
            rows * px(64.0) + px(4.0)
        }
        Block::Gap(v) => px(*v),
    })
}

fn measure(gfx: &Gfx, blocks: &[Block], scale: f32, inner: f32) -> Result<f32> {
    let mut height = 0.0;
    for block in blocks {
        height += block_height(gfx, block, scale, inner)?;
    }
    Ok(height)
}

#[expect(clippy::too_many_lines, reason = "One arm per kind of block")]
fn draw(
    pen: &crate::gfx::Pen<'_>,
    gfx: &Gfx,
    blocks: &[Block],
    scale: f32,
    dark: bool,
    inner: f32,
    hits: &mut Vec<Hit>,
) -> Result<()> {
    let px = |v: f32| (v * scale).round();
    let left = px(PAD);
    let right = left + inner;
    let mut y = px(PAD);
    for block in blocks {
        let height = block_height(gfx, block, scale, inner)?;
        match block {
            Block::Title(title, note) => {
                pen.text(title, (left, y), 15.0, Weight::Bold, ink(dark, 1.0))?;
                if !note.is_empty() {
                    let (nw, nh) = gfx.measure(note, 10.5, scale, Weight::Normal)?;
                    let th = line_height(gfx, 15.0, scale)?;
                    pen.text(note, (right - nw, y + (th - nh) / 2.0), 10.5, Weight::Normal, ink(dark, 0.55))?;
                }
            }
            Block::Section(text) => {
                pen.text(&text.to_uppercase(), (left, y), 9.0, Weight::Bold, ink(dark, 0.5))?;
            }
            Block::Text(text, faded) => {
                pen.paragraph(text, (left, y), inner, 11.5, ink(dark, if *faded { 0.6 } else { 0.9 }))?;
            }
            Block::Big(emoji, text) => {
                let mut x = left;
                if let Some(e) = emoji {
                    x += pen.emoji(e, (x, y), 24.0)? + px(10.0);
                }
                pen.text(text, (x, y), 30.0, Weight::Normal, ink(dark, 1.0))?;
            }
            Block::Meter { label, percent, color, note } => {
                let lh = line_height(gfx, 12.0, scale)?;
                pen.text(label, (left, y), 12.0, Weight::Medium, ink(dark, 0.9))?;
                let pct = format!("{percent}%");
                let (pw, _) = gfx.measure(&pct, 12.0, scale, Weight::Bold)?;
                pen.text(&pct, (right - pw, y), 12.0, Weight::Bold, *color)?;
                let bar_y = y + lh + px(3.0);
                let bar_h = px(6.0);
                pen.round_rect((left, bar_y), (inner, bar_h), bar_h / 2.0, ink(dark, 0.15))?;
                let filled = inner * f32::from((*percent).min(100)) / 100.0;
                if filled > 0.5 {
                    pen.round_rect((left, bar_y), (filled.max(bar_h), bar_h), bar_h / 2.0, *color)?;
                }
                if !note.is_empty() {
                    pen.text(note, (left, bar_y + bar_h + px(2.0)), 10.0, Weight::Normal, ink(dark, 0.55))?;
                }
            }
            Block::Event(time, title) => {
                let lh = line_height(gfx, 12.0, scale)?;
                let dot = px(7.0);
                pen.round_rect((left, y + (lh - dot) / 2.0), (dot, dot), dot / 2.0, RED)?;
                let x = left + dot + px(8.0);
                let tw = pen.text(time, (x, y), 12.0, Weight::Medium, ink(dark, 0.7))?;
                pen.text(title, (x + tw.max(px(78.0)) + px(6.0), y), 12.0, Weight::Normal, ink(dark, 1.0))?;
            }
            Block::Steps(steps) => {
                let mut sy = y;
                for (index, step) in steps.iter().enumerate() {
                    let badge = px(16.0);
                    pen.round_rect((left, sy), (badge, badge), badge / 2.0, ACCENT)?;
                    let n = (index + 1).to_string();
                    let (nw, nh) = gfx.measure(&n, 9.0, scale, Weight::Bold)?;
                    pen.text(&n, (left + (badge - nw) / 2.0, sy + (badge - nh) / 2.0), 9.0, Weight::Bold, [1.0; 4])?;
                    let h = pen.paragraph(step, (left + px(22.0), sy), inner - px(22.0), 11.5, ink(dark, 0.9))?;
                    sy += h + px(6.0);
                }
            }
            Block::Grid(cells) => {
                let gap = px(8.0);
                let cell_w = (inner - gap) / 2.0;
                let cell_h = px(52.0);
                for (index, cell) in cells.iter().enumerate() {
                    let cx = left + (index % 2) as f32 * (cell_w + gap);
                    let cy = y + (index / 2) as f32 * (cell_h + gap);
                    let back: Rgba = if cell.on {
                        [0.99, 0.78, 0.25, 1.0]
                    } else {
                        ink(dark, 0.1)
                    };
                    pen.round_rect((cx, cy), (cell_w, cell_h), px(12.0), back)?;
                    let text: Rgba = if cell.on { [0.15, 0.1, 0.0, 1.0] } else { ink(dark, 1.0) };
                    pen.text(&shorten(&cell.title, 18), (cx + px(10.0), cy + px(8.0)), 11.5, Weight::Medium, text)?;
                    pen.text(&cell.detail, (cx + px(10.0), cy + px(28.0)), 10.0, Weight::Normal, [text[0], text[1], text[2], 0.65])?;
                    if let Some(action) = &cell.action {
                        hits.push(Hit {
                            left: cx,
                            top: cy,
                            right: cx + cell_w,
                            bottom: cy + cell_h,
                            action: action.clone(),
                        });
                    }
                }
            }
            Block::Buttons(buttons) => {
                let mut x = left;
                let h = px(28.0);
                for (label, action, primary) in buttons {
                    let (tw, th) = gfx.measure(label, 11.5, scale, Weight::Medium)?;
                    let w = tw + px(24.0);
                    let back = if *primary { ACCENT } else { ink(dark, 0.12) };
                    pen.round_rect((x, y), (w, h), h / 2.0, back)?;
                    let color = if *primary { [1.0; 4] } else { ink(dark, 1.0) };
                    pen.text(label, (x + px(12.0), y + (h - th) / 2.0), 11.5, Weight::Medium, color)?;
                    hits.push(Hit {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                        action: action.clone(),
                    });
                    x += w + px(8.0);
                }
            }
            Block::Tray(items) => {
                let cell_w = inner / 5.0;
                let cell_h = px(64.0);
                let side = px(24.0);
                for (index, item) in items.iter().enumerate() {
                    let cx = left + (index % 5) as f32 * cell_w;
                    let cy = y + (index / 5) as f32 * cell_h;
                    pen.round_rect((cx + px(2.0), cy), (cell_w - px(4.0), cell_h - px(6.0)), px(10.0), ink(dark, 0.07))?;
                    if let Some(image) = &item.image {
                        pen.image(image, (cx + (cell_w - side) / 2.0, cy + px(8.0)), (side, side))?;
                    }
                    let name = item.icon.tip.lines().next().unwrap_or_default();
                    let name = shorten(name, 10);
                    let (nw, _) = gfx.measure(&name, 9.0, scale, Weight::Normal)?;
                    pen.text(&name, (cx + (cell_w - nw) / 2.0, cy + px(38.0)), 9.0, Weight::Normal, ink(dark, 0.75))?;
                    hits.push(Hit {
                        left: cx,
                        top: cy,
                        right: cx + cell_w,
                        bottom: cy + cell_h,
                        action: PanelAction::Tray(item.icon.id.clone()),
                    });
                }
            }
            Block::Gap(_) => {}
        }
        y += height;
    }
    Ok(())
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

fn perform(action: PanelAction) {
    match action {
        PanelAction::Open(target) => {
            if let Err(error) = muelle_shell::launcher::open(&target) {
                eprintln!("muelle: panel: {error}");
            }
        }
        PanelAction::HomeToggle(id) => super::casa::toggle(id),
        PanelAction::RefreshIa => super::ia::wake(),
        PanelAction::Tray(id) => muelle_shell::tray::click(&id, muelle_shell::tray::Click::Left),
    }
}

unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let x = f32::from((l.0 & 0xFFFF) as i16);
    let y = f32::from(((l.0 >> 16) & 0xFFFF) as i16);
    match msg {
        // The panel stays while you use it (an app opened from it may take
        // the focus); a click anywhere else closes it.
        WM_TIMER if w.0 == OUTSIDE_TIMER => {
            if clicked_outside(hwnd) {
                close();
            }
        }
        CLOSE => close(),
        WM_KEYDOWN if w.0 == usize::from(VK_ESCAPE.0) => close(),
        WM_LBUTTONUP => {
            let action = OPEN.with(|open| {
                open.try_borrow()
                    .ok()
                    .and_then(|panel| panel.as_ref().and_then(|p| p.hit(x, y)))
            });
            if let Some(action) = action {
                perform(action);
            }
            return LRESULT(0);
        }
        WM_RBUTTONUP => {
            let action = OPEN.with(|open| {
                open.try_borrow()
                    .ok()
                    .and_then(|panel| panel.as_ref().and_then(|p| p.hit(x, y)))
            });
            if let Some(PanelAction::Tray(id)) = action {
                muelle_shell::tray::click(&id, muelle_shell::tray::Click::Right);
            }
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {}
        WM_SETCURSOR => {
            let mut point = POINT::default();
            // SAFETY: Reading the cursor into a local, converting with our window.
            let over = unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point);
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut point);
                OPEN.with(|open| {
                    open.try_borrow().ok().is_some_and(|panel| {
                        panel
                            .as_ref()
                            .and_then(|p| p.hit(point.x as f32, point.y as f32))
                            .is_some()
                    })
                })
            };
            // SAFETY: Stock cursors are owned by Windows.
            unsafe {
                if let Ok(cursor) = LoadCursorW(None, if over { IDC_HAND } else { IDC_ARROW }) {
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
