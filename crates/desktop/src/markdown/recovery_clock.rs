//! Debounce recovery writes without allowing continuous typing to starve them.
use std::time::{Duration, Instant};
#[derive(Default)]
pub struct RecoveryClock {
    first: Option<Instant>,
    last: Option<Instant>,
}
impl RecoveryClock {
    pub fn edit(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        let deadline =
            (self.first? + Duration::from_secs(2)).min(self.last? + Duration::from_millis(500));
        Some(deadline.saturating_duration_since(now))
    }
    pub fn clear(&mut self) {
        self.first = None;
        self.last = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quiet_input_waits_500ms_and_continuous_input_is_due_at_two_seconds() {
        let start = Instant::now();
        let mut clock = RecoveryClock::default();
        clock.edit(start);
        assert_eq!(clock.remaining(start), Some(Duration::from_millis(500)));
        clock.edit(start + Duration::from_millis(400));
        assert_eq!(
            clock.remaining(start + Duration::from_millis(500)),
            Some(Duration::from_millis(400))
        );
        for ms in [800, 1200, 1600, 1999] {
            clock.edit(start + Duration::from_millis(ms));
        }
        assert_eq!(
            clock.remaining(start + Duration::from_secs(2)),
            Some(Duration::ZERO)
        );
        clock.clear();
        assert_eq!(clock.remaining(start + Duration::from_secs(3)), None);
    }
}
