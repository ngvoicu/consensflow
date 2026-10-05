//! The time the engine is made with in a test: the clock a test moves
//! (`clock` in `setup`), and timers that are the loop's, as JavaScript's
//! `setTimeout` was: a sleep ends once nothing else can move, the earliest
//! first, and the test's clock never sees it. Every sleep asked is written
//! down in the order asked ([`Recorder`]), under the seam `time`, which the
//! Node traces have none of.

use std::rc::Rc;
use std::time::Duration;

use cf_harness::contract::Work;
use cf_harness::seams::Time;
use cf_harness::testing::ManualTime;
use serde_json::json;

use super::recorder::Recorder;

/// The engine's time.
pub struct TestTime {
    clock: Rc<ManualTime>,
    /// The timers, on a clock of their own: when they fire it moves, the
    /// test's does not.
    timers: ManualTime,
    recorder: Recorder,
}

impl TestTime {
    pub fn new(clock: Rc<ManualTime>, recorder: Recorder) -> Self {
        Self {
            clock,
            timers: ManualTime::new(0),
            recorder,
        }
    }

    /// Ends the sleep due first: whether there was one.
    pub fn fire_next(&self) -> bool {
        self.timers.fire_next(i64::MAX)
    }
}

impl Time for TestTime {
    fn wall_ms(&self) -> i64 {
        self.clock.wall_ms()
    }

    fn sleep(&self, duration: Duration) -> Work<'_, ()> {
        self.recorder
            .call("time", Some("sleep"), json!([duration.as_millis()]));
        self.timers.sleep(duration)
    }
}

#[cfg(test)]
mod tests {
    use std::task::{Context, Waker};

    use super::*;

    #[test]
    fn a_sleep_ends_when_it_is_fired_in_the_order_asked_and_the_clock_never_sees_it() {
        let clock = Rc::new(ManualTime::new(1_000));
        let recorder = Recorder::default();
        let time = TestTime::new(Rc::clone(&clock), recorder.clone());
        let (mut long, mut short) = (
            time.sleep(Duration::from_millis(150)),
            time.sleep(Duration::from_millis(20)),
        );
        let mut context = Context::from_waker(Waker::noop());
        assert!(long.as_mut().poll(&mut context).is_pending());
        assert!(short.as_mut().poll(&mut context).is_pending());
        // The one due first goes first.
        assert!(time.fire_next());
        assert!(short.as_mut().poll(&mut context).is_ready());
        assert!(long.as_mut().poll(&mut context).is_pending());
        assert!(time.fire_next());
        assert!(long.as_mut().poll(&mut context).is_ready());
        assert!(!time.fire_next());
        assert_eq!(clock.wall_ms(), 1_000);
        assert_eq!(time.wall_ms(), 1_000);
        let asked: Vec<_> = recorder
            .calls("time", &["sleep"])
            .into_iter()
            .map(|(_, given)| given)
            .collect();
        assert_eq!(asked, [json!([150]), json!([20])]);
    }
}
