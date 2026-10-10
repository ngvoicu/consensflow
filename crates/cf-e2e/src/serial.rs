//! One daemon at a time. The cases that start a daemon, and the pane host and
//! the windows' programs with it, are timed (a task must finish in a minute, a
//! daemon must stop in ten seconds) and share the machine's processors with
//! every program they start. Cargo runs the cases of a test file at once, so
//! they take turns here instead: the JavaScript suites ran one at a time
//! (`--test-concurrency=1`) for the same reason.
//!
//! A case holds its turn as long as it holds its home ([`crate::daemon::Home`],
//! [`crate::rig::Rig`]), which is as long as anything of its is running.

use std::sync::{Mutex, MutexGuard, PoisonError};

static TURN: Mutex<()> = Mutex::new(());

/// The turn to run a daemon in, waited for. A case that failed while it held it
/// leaves the next its turn all the same.
pub fn turn() -> MutexGuard<'static, ()> {
    TURN.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn two_cases_never_hold_the_turn_at_once_and_a_failed_one_leaves_it() {
        let inside = Arc::new(AtomicUsize::new(0));
        let overlapped = Arc::new(AtomicUsize::new(0));
        let cases: Vec<_> = (0..4)
            .map(|case| {
                let (inside, overlapped) = (Arc::clone(&inside), Arc::clone(&overlapped));
                thread::spawn(move || {
                    let _turn = turn();
                    if inside.fetch_add(1, Ordering::SeqCst) != 0 {
                        overlapped.fetch_add(1, Ordering::SeqCst);
                    }
                    thread::sleep(Duration::from_millis(20));
                    inside.fetch_sub(1, Ordering::SeqCst);
                    // The last one fails with the turn in hand.
                    assert_ne!(case, 3, "a case that fails");
                })
            })
            .collect();
        let failed = cases
            .into_iter()
            .map(|case| case.join())
            .filter(Result::is_err)
            .count();
        assert_eq!(failed, 1);
        assert_eq!(overlapped.load(Ordering::SeqCst), 0);
        // A case after them all still gets its turn.
        drop(turn());
    }
}
