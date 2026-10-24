//! 悬停跟踪:谁正被悬停、从什么时候起(驱动悬停高亮与 2 秒标题气泡)。
//!
//! 与 `AgentPanel` 里的同款逻辑同一口径:**进入同一个槽位不重置计时**(控件树变化会让鼠标区域
//! 重发 enter),**离开只清"正是当前槽位"**(事件顺序可能是新槽位的 enter 先于旧槽位的 leave)。

use std::time::{Duration, Instant};

/// 悬停多久后弹出标题全称气泡。
pub const TOOLTIP_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
pub struct HoverTracker<K: Copy + PartialEq> {
    current: Option<(K, Instant)>,
}

impl<K: Copy + PartialEq> Default for HoverTracker<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Copy + PartialEq> HoverTracker<K> {
    pub const fn new() -> Self {
        Self { current: None }
    }

    /// 进入 / 离开某个槽位。
    pub fn on_hover(&mut self, key: K, entered: bool, now: Instant) {
        if entered {
            if self.current.map(|(current, _)| current) != Some(key) {
                self.current = Some((key, now));
            }
        } else if self.current.is_some_and(|(current, _)| current == key) {
            self.current = None;
        }
    }

    pub fn clear(&mut self) {
        self.current = None;
    }

    pub fn current(&self) -> Option<(K, Instant)> {
        self.current
    }

    /// 阶跃高亮:进入即满、离开即空。
    pub fn progress(&self, key: K) -> f32 {
        if self.current.is_some_and(|(current, _)| current == key) {
            1.0
        } else {
            0.0
        }
    }

    pub fn tooltip_ready_at(&self, key: K, now: Instant) -> bool {
        self.current.is_some_and(|(current, entered)| {
            current == key && now.saturating_duration_since(entered) >= TOOLTIP_DELAY
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_a_slot_highlights_only_that_slot() {
        let mut h = HoverTracker::new();
        h.on_hover(1, true, Instant::now());
        assert_eq!(h.progress(1), 1.0);
        assert_eq!(h.progress(2), 0.0);
    }

    #[test]
    fn leaving_the_current_slot_clears_it() {
        let mut h = HoverTracker::new();
        h.on_hover(1, true, Instant::now());
        h.on_hover(1, false, Instant::now());
        assert_eq!(h.progress(1), 0.0);
        assert!(h.current().is_none());
    }

    #[test]
    fn leaving_a_stale_slot_does_not_clear_the_new_one() {
        // 事件顺序可能是新槽位的 enter 先于旧槽位的 leave。
        let mut h = HoverTracker::new();
        let t = Instant::now();
        h.on_hover(1, true, t);
        h.on_hover(2, true, t);
        h.on_hover(1, false, t);
        assert_eq!(h.progress(2), 1.0);
    }

    #[test]
    fn re_entering_the_same_slot_does_not_restart_the_delay() {
        let mut h = HoverTracker::new();
        let start = Instant::now();
        h.on_hover(1, true, start);
        h.on_hover(1, true, start + Duration::from_millis(1500));
        assert!(h.tooltip_ready_at(1, start + TOOLTIP_DELAY));
    }

    #[test]
    fn the_tooltip_waits_for_the_full_delay_and_only_for_its_own_slot() {
        let mut h = HoverTracker::new();
        let start = Instant::now();
        h.on_hover(1, true, start);
        assert!(!h.tooltip_ready_at(1, start + Duration::from_millis(1999)));
        assert!(h.tooltip_ready_at(1, start + TOOLTIP_DELAY));
        assert!(!h.tooltip_ready_at(2, start + TOOLTIP_DELAY));
    }

    #[test]
    fn clear_drops_the_hover() {
        let mut h = HoverTracker::new();
        h.on_hover(1, true, Instant::now());
        h.clear();
        assert_eq!(h.progress(1), 0.0);
    }
}
