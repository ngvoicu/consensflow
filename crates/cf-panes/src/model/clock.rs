//! The model's time: it moves when an actor rests, and not past the moment a key's echo is due
//! while that echo has not been read.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::arbiter::Clock;

use super::program::Pty;
use super::sched;

pub(crate) struct ModelClock {
    base: Instant,
    elapsed: Mutex<Duration>,
    pty: Mutex<Option<Arc<Pty>>>,
}

impl ModelClock {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            base: Instant::now(),
            elapsed: Mutex::new(Duration::ZERO),
            pty: Mutex::new(None),
        })
    }

    /// The pty whose unread echoes hold the time back.
    pub(crate) fn watching(&self, pty: &Arc<Pty>) {
        *self.pty.lock().unwrap() = Some(Arc::clone(pty));
    }

    pub(crate) fn elapsed(&self) -> Duration {
        *self.elapsed.lock().unwrap()
    }
}

impl Clock for ModelClock {
    fn now(&self) -> Instant {
        self.base + self.elapsed()
    }

    /// A point; then the time moves by `duration`, once no key's echo that is due before then is
    /// unread: the host reads it within a second of the key.
    fn rest(&self, duration: Duration) {
        sched::at("rest");
        let to = self.elapsed() + duration;
        let pty = self.pty.lock().unwrap().clone();
        if let Some(pty) = pty {
            sched::wait_until("an echo is due", || !pty.overdue(to));
        }
        let mut elapsed = self.elapsed.lock().unwrap();
        *elapsed = (*elapsed).max(to);
    }
}
