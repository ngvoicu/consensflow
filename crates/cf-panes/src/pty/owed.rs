//! The exits a table owes the daemon: one for each window whose output a thread is
//! reading, so that it may tell `pane.exit` when the output ends. A host that is shutting
//! down waits for them, as long as it can afford, before it closes its own output.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How many windows' exits are yet to be told, and whoever waits to know that none is.
#[derive(Default)]
pub(super) struct ExitsOwed {
    owed: Mutex<usize>,
    paid: Condvar,
}

/// One window's exit, owed from the moment its output is streamed until the thread that
/// reads it has told it, or has ended without: paid when this is dropped.
pub struct ExitOwed(Arc<ExitsOwed>);

impl ExitsOwed {
    /// Owes the exit of a window whose output is about to be read.
    pub(super) fn owe(self: &Arc<Self>) -> ExitOwed {
        *self.owed.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        ExitOwed(Arc::clone(self))
    }

    /// Whether no exit is owed, waited for until `within`.
    pub(super) fn paid_within(&self, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        let mut owed = self.owed.lock().unwrap_or_else(PoisonError::into_inner);
        while *owed != 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            owed = self
                .paid
                .wait_timeout(owed, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

impl Drop for ExitOwed {
    fn drop(&mut self) {
        let mut owed = self.0.owed.lock().unwrap_or_else(PoisonError::into_inner);
        *owed -= 1;
        if *owed == 0 {
            self.0.paid.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn nothing_owed_is_paid_at_once() {
        let exits = Arc::new(ExitsOwed::default());
        assert!(exits.paid_within(Duration::ZERO));
    }

    #[test]
    fn an_exit_owed_is_waited_for_only_as_long_as_it_is_given() {
        let exits = Arc::new(ExitsOwed::default());
        let _owed = exits.owe();
        let began = Instant::now();
        assert!(!exits.paid_within(Duration::from_millis(100)));
        assert!(began.elapsed() >= Duration::from_millis(100));
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "waited {:?}",
            began.elapsed()
        );
    }

    #[test]
    fn the_wait_ends_when_the_last_exit_owed_is_paid() {
        let exits = Arc::new(ExitsOwed::default());
        let (first, second) = (exits.owe(), exits.owe());
        let paying = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            drop(first);
            thread::sleep(Duration::from_millis(50));
            drop(second);
        });
        assert!(exits.paid_within(Duration::from_secs(10)));
        paying.join().expect("the payer ends");
    }
}
