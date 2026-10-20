//! How long a case has to live. Each case has a rig of its own, and the rig keeps a
//! process of the machine's beside its pane host, which sleeps ten minutes (`rig`'s
//! stranger: a rig that ends once it is gone says the pane host took it with the
//! windows). A case that is longer than that is no case: every wait of the check is
//! cut to what is left of the case's time, and a case that has none left says so
//! where it stopped.

use std::time::{Duration, Instant};

/// How long a case may take, from its rig's start to the end of its last step: a
/// minute short of the ten the stranger sleeps, for what the rig does around it.
pub const LIFE: Duration = Duration::from_secs(9 * 60);

/// When a case began, and how long it has.
#[derive(Debug, Clone, Copy)]
pub struct Life {
    began: Instant,
    span: Duration,
}

impl Life {
    /// A case that begins now and may take `span`.
    pub fn of(span: Duration) -> Self {
        Self {
            began: Instant::now(),
            span,
        }
    }

    /// What is left of the case's time.
    pub fn left(&self) -> Duration {
        self.span.saturating_sub(self.began.elapsed())
    }

    /// Whether the case has no time left.
    pub fn over(&self) -> bool {
        self.left().is_zero()
    }

    /// `wait`, or what is left of the case's time where that is less.
    pub fn cap(&self, wait: Duration) -> Duration {
        wait.min(self.left())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wait_is_cut_to_what_is_left_of_the_cases_time() {
        let life = Life::of(Duration::from_secs(100));
        assert!(!life.over());
        assert_eq!(life.cap(Duration::from_secs(5)), Duration::from_secs(5));
        let capped = life.cap(Duration::from_secs(500));
        assert!(capped <= Duration::from_secs(100) && capped > Duration::from_secs(90));
        let spent = Life::of(Duration::ZERO);
        assert!(spent.over());
        assert_eq!(spent.cap(Duration::from_secs(5)), Duration::ZERO);
        assert_eq!(spent.left(), Duration::ZERO);
    }

    #[test]
    fn a_case_has_a_minute_less_than_the_stranger_sleeps() {
        assert_eq!(LIFE, Duration::from_secs(540));
    }
}
