use std::cell::RefCell;

use muelle_core::{
    layout::Rect,
    notifications::{
        BANNER_HEIGHT, BANNER_WIDTH, BannerPosition, NotificationQueue, banner_bounds,
        banner_close_button_bounds,
    },
};
use windows::{
    UI::Composition::{
        CompositionSurfaceBrush, ContainerVisual, Desktop::DesktopWindowTarget, SpriteVisual,
    },
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, WinRT::Composition::ICompositorDesktopInterop},
        UI::{
            Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowRect,
                HWND_TOPMOST, IDC_ARROW, KillTimer, LoadCursorW, MA_NOACTIVATE, RegisterClassW,
                SWP_NOACTIVATE, SWP_SHOWWINDOW, SetTimer, SetWindowPos, WM_DESTROY, WM_LBUTTONUP,
                WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_TIMER, WNDCLASSW,
                WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
                WS_POPUP,
            },
        },
    },
    core::{Interface, Result, w},
};
use windows_numerics::{Vector2, Vector3};

use crate::{gfx::Gfx, scene::Theme};

const WM_MOUSELEAVE: u32 = 0x02A3;
const TICK_TIMER: usize = 1;
const TICK_INTERVAL_MS: u32 = 100;

thread_local! {
    static BANNER_VIEW: RefCell<Option<NotificationBannerView>> = const { RefCell::new(None) };
}

pub enum BannerAction {
    Activate(Option<isize>, String),
    Dismiss(u64),
}

struct BannerCardVisual {
    id: u64,
    target_hwnd: Option<isize>,
    app_id: String,
    root: ContainerVisual,
    close_btn: SpriteVisual,
    close_normal: CompositionSurfaceBrush,
    close_hover: CompositionSurfaceBrush,
}

pub struct NotificationBannerView {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    cards: Vec<BannerCardVisual>,
    position: BannerPosition,
    work_area: Rect,
    scale: f32,
    theme: Theme,
    hovered_card: Option<usize>,
    hovered_close: Option<usize>,
}

impl NotificationBannerView {
    pub fn ensure_or_create(
        gfx: &Gfx,
        position: BannerPosition,
        work_area: Rect,
        scale: f32,
        theme: Theme,
    ) -> Result<()> {
        BANNER_VIEW.with(|cell| {
            let mut borrow = cell.borrow_mut();
            if borrow.is_none() {
                let view = Self::new(gfx, position, work_area, scale, theme)?;
                *borrow = Some(view);
            } else if let Some(view) = borrow.as_mut() {
                view.position = position;
                view.work_area = work_area;
                view.scale = scale;
                view.theme = theme;
            }
            Ok(())
        })
    }

    fn new(
        gfx: &Gfx,
        position: BannerPosition,
        work_area: Rect,
        scale: f32,
        theme: Theme,
    ) -> Result<Self> {
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = w!("MuelleNotificationBanner");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(banner_window_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class,
                ..Default::default()
            });

            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
                class,
                w!("Muelle Notification Banner"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };

        let target = unsafe {
            gfx.compositor
                .cast::<ICompositorDesktopInterop>()?
                .CreateDesktopWindowTarget(hwnd, true)?
        };
        let root = gfx.compositor.CreateContainerVisual()?;
        target.SetRoot(&root)?;

        unsafe {
            SetTimer(Some(hwnd), TICK_TIMER, TICK_INTERVAL_MS, None);
        }

        Ok(Self {
            hwnd,
            _target: target,
            root,
            cards: Vec::new(),
            position,
            work_area,
            scale,
            theme,
            hovered_card: None,
            hovered_close: None,
        })
    }

    pub fn sync_queue(gfx: &Gfx, queue: &NotificationQueue) -> Result<()> {
        BANNER_VIEW.with(|cell| {
            let mut borrow = cell.borrow_mut();
            if let Some(view) = borrow.as_mut() {
                view.rebuild(gfx, queue)?;
            }
            Ok(())
        })
    }

    pub fn hide() {
        BANNER_VIEW.with(|cell| {
            let mut borrow = cell.borrow_mut();
            if let Some(view) = borrow.take() {
                unsafe {
                    KillTimer(Some(view.hwnd), TICK_TIMER).ok();
                    DestroyWindow(view.hwnd).ok();
                }
            }
        });
    }

    fn rebuild(&mut self, gfx: &Gfx, queue: &NotificationQueue) -> Result<()> {
        let items = queue.visible_items();
        if items.is_empty() {
            self.root.Children()?.RemoveAll()?;
            self.cards.clear();
            unsafe {
                SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE).ok();
            }
            return Ok(());
        }

        let first_rect = banner_bounds(0, self.position, self.work_area, self.scale);
        let last_rect = banner_bounds(items.len() - 1, self.position, self.work_area, self.scale);

        let min_x = first_rect.x.min(last_rect.x) as i32;
        let min_y = first_rect.y.min(last_rect.y) as i32;
        let max_x = (first_rect.x + first_rect.width).max(last_rect.x + last_rect.width) as i32;
        let max_y = (first_rect.y + first_rect.height).max(last_rect.y + last_rect.height) as i32;

        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                min_x,
                min_y,
                max_x - min_x,
                max_y - min_y,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
        }

        self.root.Children()?.RemoveAll()?;
        self.cards.clear();

        let c = &gfx.compositor;

        for (index, item) in items.iter().enumerate() {
            let rect = banner_bounds(index, self.position, self.work_area, self.scale);
            let local_x = rect.x - (min_x as f32);
            let local_y = rect.y - (min_y as f32);

            let card = c.CreateContainerVisual()?;
            card.SetOffset(Vector3 {
                X: local_x,
                Y: local_y,
                Z: 0.0,
            })?;
            card.SetSize(Vector2 {
                X: rect.width,
                Y: rect.height,
            })?;

            // Render glass card brush with title and body
            let banner_brush = gfx
                .notification_banner_brush(
                    &item.title,
                    &item.body,
                    rect.width,
                    rect.height,
                    self.scale,
                    self.theme.light,
                )
                .map_err(|e| {
                    windows::core::Error::new(windows::core::HRESULT(-1), e.to_string())
                })?;

            let bg_visual = c.CreateSpriteVisual()?;
            bg_visual.SetSize(Vector2 {
                X: rect.width,
                Y: rect.height,
            })?;
            bg_visual.SetBrush(&banner_brush)?;
            card.Children()?.InsertAtBottom(&bg_visual)?;

            // Close button ("✕") in top-right corner
            let btn_bounds = banner_close_button_bounds(rect, self.scale);
            let btn_side = btn_bounds.width as u32;
            let close_normal = gfx
                .close_button_brush(btn_side, false, self.theme.light)
                .map_err(|e| {
                    windows::core::Error::new(windows::core::HRESULT(-1), e.to_string())
                })?;
            let close_hover = gfx
                .close_button_brush(btn_side, true, self.theme.light)
                .map_err(|e| {
                    windows::core::Error::new(windows::core::HRESULT(-1), e.to_string())
                })?;

            let close_btn = c.CreateSpriteVisual()?;
            close_btn.SetBrush(&close_normal)?;
            close_btn.SetSize(Vector2 {
                X: btn_bounds.width,
                Y: btn_bounds.height,
            })?;
            close_btn.SetOffset(Vector3 {
                X: btn_bounds.x,
                Y: btn_bounds.y,
                Z: 0.0,
            })?;
            card.Children()?.InsertAtTop(&close_btn)?;

            self.root.Children()?.InsertAtTop(&card)?;

            self.cards.push(BannerCardVisual {
                id: item.id,
                target_hwnd: item.target_hwnd,
                app_id: item.app_id.clone(),
                root: card,
                close_btn,
                close_normal,
                close_hover,
            });
        }

        Ok(())
    }

    fn hit_test(&self, point: POINT) -> (Option<usize>, Option<usize>) {
        let width = BANNER_WIDTH * self.scale;
        let height = BANNER_HEIGHT * self.scale;

        for (index, card) in self.cards.iter().enumerate() {
            let offset = card.root.Offset().unwrap_or(Vector3::default());
            let card_rect = Rect {
                x: offset.X,
                y: offset.Y,
                width,
                height,
            };

            let pt_x = point.x as f32;
            let pt_y = point.y as f32;

            if pt_x >= card_rect.x
                && pt_x <= card_rect.x + card_rect.width
                && pt_y >= card_rect.y
                && pt_y <= card_rect.y + card_rect.height
            {
                let btn_rect = banner_close_button_bounds(card_rect, self.scale);
                let btn_local_x = pt_x - card_rect.x;
                let btn_local_y = pt_y - card_rect.y;

                if btn_local_x >= btn_rect.x
                    && btn_local_x <= btn_rect.x + btn_rect.width
                    && btn_local_y >= btn_rect.y
                    && btn_local_y <= btn_rect.y + btn_rect.height
                {
                    return (Some(index), Some(index));
                }
                return (Some(index), None);
            }
        }
        (None, None)
    }

    fn on_mouse_move(&mut self, point: POINT) -> Option<u64> {
        let (card_idx, close_idx) = self.hit_test(point);

        if self.hovered_close != close_idx {
            if let Some(card) = self.hovered_close.and_then(|prev| self.cards.get(prev)) {
                let _ = card.close_btn.SetBrush(&card.close_normal);
            }
            if let Some(card) = close_idx.and_then(|curr| self.cards.get(curr)) {
                let _ = card.close_btn.SetBrush(&card.close_hover);
            }
            self.hovered_close = close_idx;
        }

        self.hovered_card = card_idx;
        card_idx.and_then(|idx| self.cards.get(idx).map(|c| c.id))
    }

    fn on_click(&mut self, point: POINT) -> Option<BannerAction> {
        let (card_idx, close_idx) = self.hit_test(point);
        if let Some(card) = close_idx.and_then(|close| self.cards.get(close)) {
            Some(BannerAction::Dismiss(card.id))
        } else {
            card_idx
                .and_then(|card| self.cards.get(card))
                .map(|card_visual| {
                    BannerAction::Activate(card_visual.target_hwnd, card_visual.app_id.clone())
                })
        }
    }
}

pub fn handle_mouse_move(point: POINT) -> Option<u64> {
    BANNER_VIEW.with(|cell| {
        let mut borrow = cell.borrow_mut();
        borrow.as_mut().and_then(|v| v.on_mouse_move(point))
    })
}

pub fn handle_click(point: POINT) -> Option<BannerAction> {
    BANNER_VIEW.with(|cell| {
        let mut borrow = cell.borrow_mut();
        borrow.as_mut().and_then(|v| v.on_click(point))
    })
}

unsafe extern "system" fn banner_window_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            unsafe {
                let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                });
            }
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            };
            let mut rect = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut rect);
            };
            let local_pt = POINT {
                x: pt.x - rect.left,
                y: pt.y - rect.top,
            };
            handle_mouse_move(local_pt);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            handle_mouse_move(POINT { x: -999, y: -999 });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            };
            let mut rect = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut rect);
            };
            let local_pt = POINT {
                x: pt.x - rect.left,
                y: pt.y - rect.top,
            };
            if let Some(action) = handle_click(local_pt) {
                match action {
                    BannerAction::Dismiss(id) => {
                        crate::app::with_app("dismiss banner", |app| {
                            app.dismiss_notification(id);
                            Ok(())
                        });
                    }
                    BannerAction::Activate(target_hwnd, app_id) => {
                        crate::app::with_app_then("activate notification", |app| {
                            let outside = if let Some(h) = target_hwnd {
                                app.activating(vec![h])
                            } else {
                                None
                            };
                            app.clear_notification_badge(&app_id);
                            Ok(outside)
                        });
                    }
                }
            }
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            };
            let mut rect = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut rect);
            };
            let local_pt = POINT {
                x: pt.x - rect.left,
                y: pt.y - rect.top,
            };
            if let Some(action) = handle_click(local_pt) {
                let id = match action {
                    BannerAction::Dismiss(id) => id,
                    BannerAction::Activate(_, _) => 0,
                };
                if id != 0 {
                    crate::app::with_app("middle click dismiss banner", |app| {
                        app.dismiss_notification(id);
                        Ok(())
                    });
                }
            }
            LRESULT(0)
        }
        WM_TIMER if w.0 == TICK_TIMER => {
            crate::app::with_app("tick notifications", |app| {
                app.tick_notifications(TICK_INTERVAL_MS as u64);
                Ok(())
            });
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}
