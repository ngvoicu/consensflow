//! The Escape that stops a turn at work. It is read from the terminal by one
//! thread and waited for by the one that takes the turn, whose wait it ends.
//! An Escape pressed into a window at rest is nothing: it stops no turn, and
//! none that begins after it.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

/// Whether a turn is at work, and whether it was stopped.
#[derive(Default)]
struct State {
    working: bool,
    pressed: bool,
}

/// The Escape of the turn at work.
#[derive(Default)]
pub struct Stop {
    state: Mutex<State>,
    pressed: Condvar,
}

impl Stop {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A turn begins: no Escape from before it was for it.
    pub fn begin(&self) {
        *self.state() = State {
            working: true,
            pressed: false,
        };
    }

    /// The turn is over.
    pub fn end(&self) {
        *self.state() = State::default();
    }

    /// Escape was pressed: it stops the turn at work.
    pub fn press(&self) {
        let mut state = self.state();
        if state.working {
            state.pressed = true;
            self.pressed.notify_all();
        }
    }

    /// Works for up to `how_long`: whether the turn was stopped before it
    /// was over, which ends it at once.
    pub fn work(&self, how_long: Duration) -> bool {
        let (state, _) = self
            .pressed
            .wait_timeout_while(self.state(), how_long, |state| !state.pressed)
            .unwrap_or_else(PoisonError::into_inner);
        state.pressed
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    use super::*;

    #[test]
    fn work_ends_when_the_turn_is_stopped_and_not_before() {
        let stop = Arc::new(Stop::default());
        stop.begin();
        let started = Instant::now();
        let presser = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                stop.press();
            })
        };
        assert!(stop.work(Duration::from_secs(30)));
        assert!(started.elapsed() >= Duration::from_millis(100));
        assert!(started.elapsed() < Duration::from_secs(20));
        presser.join().unwrap();
    }

    #[test]
    fn a_turn_nobody_stops_works_its_whole_time() {
        let stop = Stop::default();
        stop.begin();
        let started = Instant::now();
        assert!(!stop.work(Duration::from_millis(200)));
        assert!(started.elapsed() >= Duration::from_millis(200));
    }

    #[test]
    fn an_escape_into_a_window_at_rest_stops_no_turn_that_begins_after_it() {
        let stop = Stop::default();
        stop.press();
        stop.begin();
        assert!(!stop.work(Duration::from_millis(50)));
        // Nor one that ended: its press goes with it.
        stop.press();
        stop.end();
        stop.begin();
        assert!(!stop.work(Duration::from_millis(50)));
    }
}
