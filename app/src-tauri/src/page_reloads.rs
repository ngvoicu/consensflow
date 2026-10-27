//! How often a page whose process failed is loaded again (page_crash.rs, Windows). A page
//! that fails as it loads would otherwise be loaded again as fast as it fails, for as long
//! as the app runs, each time a line in the app's log (T-479). So the loads are kept apart
//! ([`APART`]), and once [`AT_MOST`] have been asked for within [`WITHIN`] the page is left
//! as it is, said once: the app has to be started again.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// The least time between two loads of the page.
pub(crate) const APART: Duration = Duration::from_secs(5);
/// How many loads are let within [`WITHIN`]: one more failure there is given up on.
pub(crate) const AT_MOST: usize = 5;
pub(crate) const WITHIN: Duration = Duration::from_secs(60);

/// What is done with a failure of the page's process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reload {
    /// Loaded again at once.
    Now,
    /// Loaded again once this has passed, [`APART`] after the load before.
    After(Duration),
    /// Not loaded: a load is already due, which the failure was before.
    Due,
    /// Not loaded, and none will be again: too many loads within [`WITHIN`].
    GivenUp,
    /// Not loaded: given up on before.
    GaveUp,
}

/// The loads asked for so far, the last ones first to go.
#[derive(Debug, Default)]
pub(crate) struct Reloads {
    /// When each load within [`WITHIN`] of the last failure is, or was, made.
    at: VecDeque<Instant>,
    given_up: bool,
}

impl Reloads {
    /// What to do with a failure of the page's process at `now`.
    pub(crate) fn failed(&mut self, now: Instant) -> Reload {
        if self.given_up {
            return Reload::GaveUp;
        }
        if self.at.back().is_some_and(|last| *last > now) {
            return Reload::Due;
        }
        while self
            .at
            .front()
            .is_some_and(|first| now.saturating_duration_since(*first) >= WITHIN)
        {
            self.at.pop_front();
        }
        if self.at.len() >= AT_MOST {
            self.given_up = true;
            return Reload::GivenUp;
        }
        let at = self.at.back().map_or(now, |last| now.max(*last + APART));
        self.at.push_back(at);
        match at.saturating_duration_since(now) {
            Duration::ZERO => Reload::Now,
            wait => Reload::After(wait),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_that_fails_now_and_then_is_loaded_again_each_time_at_once() {
        let mut reloads = Reloads::default();
        let start = Instant::now();
        for minute in 0..20 {
            assert_eq!(
                reloads.failed(start + Duration::from_secs(minute * 60)),
                Reload::Now,
                "{minute}"
            );
        }
        // Two failures more than five seconds apart are each loaded again at once.
        let later = start + Duration::from_secs(3600);
        assert_eq!(reloads.failed(later), Reload::Now);
        assert_eq!(reloads.failed(later + APART), Reload::Now);
    }

    #[test]
    fn a_page_that_fails_as_it_loads_is_loaded_five_s_apart_and_given_up_on_after_five_in_a_minute()
    {
        let mut reloads = Reloads::default();
        let mut now = Instant::now();
        // The first failure is loaded again at once.
        assert_eq!(reloads.failed(now), Reload::Now);
        // The page fails again a second after each load: the next load waits out the five
        // seconds, and a failure while one is due loads nothing more.
        for _ in 1..AT_MOST {
            now += Duration::from_secs(1);
            assert_eq!(reloads.failed(now), Reload::After(Duration::from_secs(4)));
            assert_eq!(reloads.failed(now + Duration::from_millis(10)), Reload::Due);
            now += Duration::from_secs(4);
        }
        // Five loads in 21 s: the sixth failure is given up on, once, and so is every one after.
        now += Duration::from_secs(1);
        assert_eq!(reloads.failed(now), Reload::GivenUp);
        assert_eq!(reloads.failed(now), Reload::GaveUp);
        assert_eq!(
            reloads.failed(now + Duration::from_secs(3600)),
            Reload::GaveUp
        );
    }

    #[test]
    fn the_loads_of_more_than_a_minute_ago_are_not_counted() {
        let mut reloads = Reloads::default();
        let start = Instant::now();
        for second in [0, 5, 10, 15] {
            assert_eq!(
                reloads.failed(start + Duration::from_secs(second)),
                Reload::Now
            );
        }
        // A fifth within the minute is still let; a sixth a minute after the first is too.
        assert_eq!(reloads.failed(start + Duration::from_secs(20)), Reload::Now);
        assert_eq!(reloads.failed(start + Duration::from_secs(60)), Reload::Now);
        // And one more five seconds on, the second of the minute having gone too.
        assert_eq!(reloads.failed(start + Duration::from_secs(65)), Reload::Now);
        assert_eq!(reloads.failed(start + Duration::from_secs(70)), Reload::Now);
        // Then five within the last minute (15, 20, 60, 65 and 70): the next is given up on.
        assert_eq!(
            reloads.failed(start + Duration::from_secs(71)),
            Reload::GivenUp
        );
    }
}
