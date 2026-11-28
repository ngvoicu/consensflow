//! The model's time: it moves when an actor rests, and only then. No echo and no read is owed
//! within any time: a window may stall for as long as the schedule has it.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::arbiter::Clock;

use super::sched;

pub(crate) struct ModelClock {
    base: Instant,
    elapsed: Mutex<Duration>,
}

impl ModelClock {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            base: Instant::now(),
            elapsed: Mutex::new(Duration::ZERO),
        })
    }

    pub(crate) fn elapsed(&self) -> Duration {
        *self.elapsed.lock().unwrap()
    }
}

impl Clock for ModelClock {
    fn now(&self) -> Instant {
        self.base + self.elapsed()
    }

    /// A point; then the time moves by `duration`.
    fn rest(&self, duration: Duration) {
        sched::at("rest");
        sched::resting();
        let to = self.elapsed() + duration;
        let mut elapsed = self.elapsed.lock().unwrap();
        *elapsed = (*elapsed).max(to);
    }
}
