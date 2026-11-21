use std::collections::HashMap;

use crate::layout::Rect;

/// Position of notification banners on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BannerPosition {
    #[default]
    TopRight,
    BottomRight,
}

/// A single notification message. Content remains strictly in-memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationItem {
    pub id: u64,
    pub app_id: String,
    pub app_name: String,
    pub title: String,
    pub body: String,
    pub icon_path: Option<String>,
    pub target_hwnd: Option<isize>,
    pub duration_ms: u64,
    pub elapsed_ms: u64,
    pub paused: bool,
}

impl NotificationItem {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: u64,
        app_id: String,
        app_name: String,
        title: String,
        body: String,
        icon_path: Option<String>,
        target_hwnd: Option<isize>,
        duration_ms: u64,
    ) -> Self {
        Self {
            id,
            app_id,
            app_name,
            title,
            body,
            icon_path,
            target_hwnd,
            duration_ms: duration_ms.max(1000),
            elapsed_ms: 0,
            paused: false,
        }
    }
}

/// Maximum number of banners shown simultaneously.
pub const MAX_VISIBLE_BANNERS: usize = 3;

/// Manages active notifications, stacking, auto-dismiss timers, and dock badges.
#[derive(Debug, Default)]
pub struct NotificationQueue {
    next_id: u64,
    items: Vec<NotificationItem>,
    hovered_id: Option<u64>,
    badges: HashMap<String, usize>,
}

impl NotificationQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates an ID and enqueues a notification.
    /// If `dnd` is active or the app is `muted`, banners are suppressed,
    /// but the app badge count is still incremented (unless muted).
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        app_id: String,
        app_name: String,
        title: String,
        body: String,
        icon_path: Option<String>,
        target_hwnd: Option<isize>,
        duration_ms: u64,
        dnd: bool,
        muted: bool,
    ) -> Option<u64> {
        if muted {
            return None;
        }

        // Increment dock badge count for the source app
        *self.badges.entry(app_id.clone()).or_insert(0) += 1;

        if dnd {
            return None;
        }

        self.next_id = self.next_id.wrapping_add(1);
        let id = self.next_id;
        let item = NotificationItem::new(
            id,
            app_id,
            app_name,
            title,
            body,
            icon_path,
            target_hwnd,
            duration_ms,
        );
        self.items.push(item);
        Some(id)
    }

    /// Advances timers for active notifications and returns IDs of expired ones.
    pub fn tick(&mut self, delta_ms: u64) -> Vec<u64> {
        let mut expired = Vec::new();
        for item in &mut self.items {
            if !item.paused {
                item.elapsed_ms = item.elapsed_ms.saturating_add(delta_ms);
                if item.elapsed_ms >= item.duration_ms {
                    expired.push(item.id);
                }
            }
        }
        for id in &expired {
            self.dismiss(*id);
        }
        expired
    }

    /// Pauses timer while cursor is hovered over a banner.
    pub fn set_hovered(&mut self, id: Option<u64>) {
        self.hovered_id = id;
        for item in &mut self.items {
            item.paused = Some(item.id) == id;
        }
    }

    /// Dismisses a notification by ID.
    pub fn dismiss(&mut self, id: u64) -> Option<NotificationItem> {
        if let Some(pos) = self.items.iter().position(|it| it.id == id) {
            let item = self.items.remove(pos);
            if self.hovered_id == Some(id) {
                self.hovered_id = None;
            }
            Some(item)
        } else {
            None
        }
    }

    /// Dismisses all notifications originating from a specific app.
    pub fn dismiss_for_app(&mut self, app_id: &str) -> Vec<NotificationItem> {
        let mut removed = Vec::new();
        self.items.retain(|it| {
            if it.app_id == app_id {
                removed.push(it.clone());
                false
            } else {
                true
            }
        });
        if self
            .hovered_id
            .is_some_and(|h| removed.iter().any(|it| it.id == h))
        {
            self.hovered_id = None;
        }
        removed
    }

    /// Clears badge count when an app is focused or dismissed.
    pub fn clear_badge(&mut self, app_id: &str) {
        self.badges.remove(app_id);
    }

    /// Returns the badge count for an app.
    pub fn badge_count(&self, app_id: &str) -> usize {
        self.badges.get(app_id).copied().unwrap_or(0)
    }

    /// Returns all badge counts.
    pub fn all_badges(&self) -> &HashMap<String, usize> {
        &self.badges
    }

    /// Up to `MAX_VISIBLE_BANNERS` currently visible notifications.
    pub fn visible_items(&self) -> &[NotificationItem] {
        let count = self.items.len().min(MAX_VISIBLE_BANNERS);
        &self.items[..count]
    }

    /// Total number of active queued notifications.
    pub fn total_count(&self) -> usize {
        self.items.len()
    }

    /// Removes banners when Do Not Disturb is enabled, preserving unread badges.
    pub fn clear_banners(&mut self) {
        self.items.clear();
        self.hovered_id = None;
    }

    /// Clears all notifications and badges.
    pub fn clear_all(&mut self) {
        self.clear_banners();
        self.badges.clear();
    }
}

/// Standard layout parameters for notification cards.
pub const BANNER_WIDTH: f32 = 340.0;
pub const BANNER_HEIGHT: f32 = 72.0;
pub const BANNER_GAP: f32 = 10.0;
pub const BANNER_MARGIN: f32 = 16.0;

/// Calculates the screen bounds for a visible banner at `index` (0-based, top-to-bottom or bottom-to-top).
pub fn banner_bounds(index: usize, position: BannerPosition, work_area: Rect, scale: f32) -> Rect {
    let width = (BANNER_WIDTH * scale).round();
    let height = (BANNER_HEIGHT * scale).round();
    let gap = (BANNER_GAP * scale).round();
    let margin = (BANNER_MARGIN * scale).round();

    let x = work_area.x + work_area.width - width - margin;
    let y = match position {
        BannerPosition::TopRight => work_area.y + margin + (index as f32) * (height + gap),
        BannerPosition::BottomRight => {
            work_area.y + work_area.height - margin - height - (index as f32) * (height + gap)
        }
    };

    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Close button ("✕") hit area on a notification banner (top-right corner of the banner).
pub fn banner_close_button_bounds(banner_rect: Rect, scale: f32) -> Rect {
    let size = (20.0 * scale).round().max(20.0);
    let padding = (8.0 * scale).round();
    Rect {
        x: banner_rect.width - size - padding,
        y: padding,
        width: size,
        height: size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabling_dnd_drops_old_banners_but_preserves_unread_badges() {
        let mut queue = NotificationQueue::default();
        queue.push(
            "app".into(),
            "App".into(),
            "Title".into(),
            "Body".into(),
            None,
            None,
            5000,
            false,
            false,
        );
        queue.clear_banners();
        assert!(queue.visible_items().is_empty());
        assert_eq!(queue.badge_count("app"), 1);
        queue.push(
            "app".into(),
            "App".into(),
            "Silent".into(),
            "Body".into(),
            None,
            None,
            5000,
            true,
            false,
        );
        assert!(queue.tick(6000).is_empty());
        assert!(queue.visible_items().is_empty());
        assert_eq!(queue.badge_count("app"), 2);
    }

    #[test]
    fn test_push_and_expiration() {
        let mut queue = NotificationQueue::new();
        let Some(id1) = queue.push(
            "slack".into(),
            "Slack".into(),
            "New message".into(),
            "Hello!".into(),
            None,
            None,
            5000,
            false,
            false,
        ) else {
            panic!("should push notification");
        };

        assert_eq!(queue.visible_items().len(), 1);
        assert_eq!(queue.badge_count("slack"), 1);

        // Advance 3000ms -> not expired
        let expired = queue.tick(3000);
        assert!(expired.is_empty());
        assert_eq!(queue.visible_items().len(), 1);

        // Advance another 2000ms -> expired
        let expired = queue.tick(2000);
        assert_eq!(expired, vec![id1]);
        assert_eq!(queue.visible_items().len(), 0);
        // Badge stays until focused/cleared
        assert_eq!(queue.badge_count("slack"), 1);
    }

    #[test]
    fn test_hover_pauses_timer() {
        let mut queue = NotificationQueue::new();
        let Some(id) = queue.push(
            "teams".into(),
            "Teams".into(),
            "Meeting".into(),
            "Starting now".into(),
            None,
            None,
            5000,
            false,
            false,
        ) else {
            panic!("should push notification");
        };

        // Hover pauses
        queue.set_hovered(Some(id));
        let expired = queue.tick(6000);
        assert!(expired.is_empty());
        assert_eq!(queue.visible_items().len(), 1);

        // Hover end resumes
        queue.set_hovered(None);
        let expired = queue.tick(5000);
        assert_eq!(expired, vec![id]);
        assert_eq!(queue.visible_items().len(), 0);
    }

    #[test]
    fn test_dnd_and_mute() {
        let mut queue = NotificationQueue::new();

        // Muted app: completely ignored, no badge, no banner
        let id_muted = queue.push(
            "noisy".into(),
            "Noisy".into(),
            "Spam".into(),
            "Buy now".into(),
            None,
            None,
            5000,
            false,
            true,
        );
        assert!(id_muted.is_none());
        assert_eq!(queue.badge_count("noisy"), 0);

        // DND mode: banner suppressed, badge still increments
        let id_dnd = queue.push(
            "mail".into(),
            "Mail".into(),
            "Urgent".into(),
            "Invoice".into(),
            None,
            None,
            5000,
            true, // dnd
            false,
        );
        assert!(id_dnd.is_none());
        assert_eq!(queue.badge_count("mail"), 1);
        assert_eq!(queue.visible_items().len(), 0);
    }

    #[test]
    fn test_banner_bounds_layout() {
        let work_area = Rect {
            x: 0.0,
            y: 32.0, // under menu bar
            width: 1920.0,
            height: 1048.0,
        };

        // Top right
        let b0 = banner_bounds(0, BannerPosition::TopRight, work_area, 1.0);
        let b1 = banner_bounds(1, BannerPosition::TopRight, work_area, 1.0);
        assert_eq!(b0.x, 1920.0 - BANNER_WIDTH - BANNER_MARGIN);
        assert_eq!(b0.y, 32.0 + BANNER_MARGIN);
        assert_eq!(b1.y, b0.y + BANNER_HEIGHT + BANNER_GAP);

        // Bottom right
        let br0 = banner_bounds(0, BannerPosition::BottomRight, work_area, 1.0);
        let br1 = banner_bounds(1, BannerPosition::BottomRight, work_area, 1.0);
        assert_eq!(br0.x, 1920.0 - BANNER_WIDTH - BANNER_MARGIN);
        assert_eq!(br0.y, 32.0 + 1048.0 - BANNER_MARGIN - BANNER_HEIGHT);
        assert_eq!(br1.y, br0.y - BANNER_HEIGHT - BANNER_GAP);
    }

    #[test]
    fn test_burst_of_notifications() {
        let mut queue = NotificationQueue::new();
        let count = 25;
        let mut ids = Vec::new();

        for i in 0..count {
            let app = if i % 2 == 0 { "app_a" } else { "app_b" };
            let Some(id) = queue.push(
                app.into(),
                format!("App {app}"),
                format!("Title {i}"),
                format!("Body {i}"),
                None,
                None,
                5000,
                false,
                false,
            ) else {
                panic!("should push notification");
            };
            ids.push(id);
        }

        // Only MAX_VISIBLE_BANNERS (3) are visible at a time
        assert_eq!(queue.visible_items().len(), MAX_VISIBLE_BANNERS);
        assert_eq!(queue.visible_items()[0].id, ids[0]);
        assert_eq!(queue.visible_items()[1].id, ids[1]);
        assert_eq!(queue.visible_items()[2].id, ids[2]);

        // Badge counts accumulate correctly across all 25 notifications
        assert_eq!(queue.badge_count("app_a"), 13);
        assert_eq!(queue.badge_count("app_b"), 12);

        // Dismissing the first 3 reveals the next 3
        for id in &ids[..3] {
            queue.dismiss(*id);
        }
        assert_eq!(queue.visible_items().len(), MAX_VISIBLE_BANNERS);
        assert_eq!(queue.visible_items()[0].id, ids[3]);
        assert_eq!(queue.visible_items()[1].id, ids[4]);
        assert_eq!(queue.visible_items()[2].id, ids[5]);
    }

    #[test]
    fn test_app_exit_notification_dismissal() {
        let mut queue = NotificationQueue::new();
        queue.push(
            "app_x".into(),
            "App X".into(),
            "T1".into(),
            "B1".into(),
            None,
            None,
            5000,
            false,
            false,
        );
        queue.push(
            "app_y".into(),
            "App Y".into(),
            "T2".into(),
            "B2".into(),
            None,
            None,
            5000,
            false,
            false,
        );
        queue.push(
            "app_x".into(),
            "App X".into(),
            "T3".into(),
            "B3".into(),
            None,
            None,
            5000,
            false,
            false,
        );

        assert_eq!(queue.badge_count("app_x"), 2);
        assert_eq!(queue.badge_count("app_y"), 1);

        // App X exits: remove all notifications for app_x and clear its badge
        let removed = queue.dismiss_for_app("app_x");
        assert_eq!(removed.len(), 2);
        queue.clear_badge("app_x");

        assert_eq!(queue.badge_count("app_x"), 0);
        assert_eq!(queue.badge_count("app_y"), 1);
        assert_eq!(queue.visible_items().len(), 1);
        assert_eq!(queue.visible_items()[0].app_id, "app_y");
    }

    #[test]
    fn test_banner_close_button_geometry() {
        let banner = Rect {
            x: 100.0,
            y: 50.0,
            width: BANNER_WIDTH,
            height: BANNER_HEIGHT,
        };
        let close_1x = banner_close_button_bounds(banner, 1.0);
        assert_eq!(close_1x.width, 20.0);
        assert_eq!(close_1x.height, 20.0);
        assert_eq!(close_1x.x, BANNER_WIDTH - 20.0 - 8.0);
        assert_eq!(close_1x.y, 8.0);

        let close_2x = banner_close_button_bounds(banner, 2.0);
        assert_eq!(close_2x.width, 40.0);
        assert_eq!(close_2x.height, 40.0);
        assert_eq!(close_2x.x, BANNER_WIDTH - 40.0 - 16.0);
        assert_eq!(close_2x.y, 16.0);
    }
}
