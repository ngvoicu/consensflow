//! What a case ends with, and what the rig can tell of it from the inside.
//!
//! A case that fails by a check fails by a panic, and a rig dropped in it knows
//! (`std::thread::panicking()`). A case that fails by an error it returns, as most
//! of them do (the wait that timed out), drops its rig on the way out with no panic
//! in flight: the test harness turns the returned value into a failure only after
//! the case's variables are gone. And a rig a helper owns is dropped when the helper
//! returns its error, before the case's own `?` has even seen it.
//!
//! So a case that returns [`Outcome`] says that it failed at the one moment it can,
//! when its `?` turns the error into a [`Failed`], and a rig dropped unclosed does
//! not decide whether its report is a failed case's: it writes it while it is up
//! and hands it to [`leave`], which delivers it at once where the case has failed
//! since the rig started, and holds it on the thread otherwise. A [`Failed`] made
//! later on the thread, or a panic, delivers what is held; a thread that ends
//! without either was a case that passed, and what it held goes unsaid.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::sync::Once;
use std::thread::{self, ThreadId};

thread_local! {
    /// How many errors the cases of this thread have turned into a [`Failed`].
    static FAILURES: Cell<usize> = const { Cell::new(0) };
    /// What the rigs dropped on this thread have to say, held until their case is
    /// known to have failed.
    static HELD: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}

/// Puts the panic hook that delivers what is held in front of the one there is.
static HOOK: Once = Once::new();

/// What a case ends with: nothing, or why it failed without a check that did not
/// hold (a check that does not hold is a panic).
pub type Outcome = Result<(), Failed>;

/// Why a case could not go on: any error, with its `Debug` as the test harness prints
/// it (`cf_e2e::Error` says what it says on the lines it says it). A case returns
/// one with `?`, which is how the rigs it started learn that it failed.
///
/// It is not itself an `Error`: that is what lets any error become one.
pub struct Failed(Box<dyn std::error::Error>);

impl<E: Into<Box<dyn std::error::Error>>> From<E> for Failed {
    fn from(error: E) -> Self {
        FAILURES.with(|failures| failures.set(failures.get() + 1));
        deliver_held();
        Self(error.into())
    }
}

impl fmt::Debug for Failed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, formatter)
    }
}

/// Where a rig started in the failures of its thread: it has seen its case fail if
/// the thread has turned an error into a [`Failed`] since.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Verdict {
    thread: ThreadId,
    failures: usize,
}

impl Verdict {
    /// The failures of this thread now.
    pub(crate) fn start() -> Self {
        Self {
            thread: thread::current().id(),
            failures: FAILURES.with(Cell::get),
        }
    }

    /// Whether the case has failed since [`Verdict::start`]: it has turned an error
    /// into a [`Failed`] on this thread, or this is not the thread the verdict was
    /// started on, which sees none of it and is taken for a failure, so that no report
    /// of one is lost.
    pub(crate) fn failed(&self) -> bool {
        thread::current().id() != self.thread || FAILURES.with(Cell::get) > self.failures
    }
}

/// What a rig dropped unclosed leaves to say, once `verdict` says whether its case has
/// failed: `say` runs at once if it has, and otherwise when a [`Failed`] is made on this
/// thread or a panic begins on it, whichever comes first, and not at all if the thread
/// ends without.
pub(crate) fn leave(verdict: Verdict, say: impl FnOnce() + 'static) {
    if verdict.failed() {
        say();
        return;
    }
    HOOK.call_once(|| {
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            deliver_held();
            before(panic);
        }));
    });
    HELD.with(|held| held.borrow_mut().push(Box::new(say)));
}

/// Says what the rigs dropped on this thread are holding.
fn deliver_held() {
    let held = HELD
        .try_with(|held| std::mem::take(&mut *held.borrow_mut()))
        .unwrap_or_default();
    for say in held {
        say();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A `say` that counts how many times it ran.
    fn counting(said: &Arc<AtomicUsize>) -> impl FnOnce() + 'static {
        let said = Arc::clone(said);
        move || {
            said.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_case_has_failed_once_an_error_it_returned_became_a_failed_and_not_before() {
        fn step(error: bool) -> Outcome {
            if error {
                Err(std::io::Error::other("the step"))?;
            }
            Ok(())
        }
        let verdict = Verdict::start();
        assert!(step(false).is_ok());
        assert!(!verdict.failed());
        assert!(step(true).is_err());
        assert!(verdict.failed());
        // One started after it has seen the failure behind it, and none of its own.
        assert!(!Verdict::start().failed());
    }

    #[test]
    fn a_verdict_read_on_another_thread_than_its_start_is_taken_for_a_failure() {
        let verdict = Verdict::start();
        assert!(thread::spawn(move || verdict.failed()).join().unwrap());
    }

    #[test]
    fn a_failed_says_what_its_error_says_as_the_error_does() {
        let failed: Failed = crate::Error::Timeout("the daemon\nnever did it".to_owned()).into();
        assert_eq!(
            format!("Error: {failed:?}"),
            "Error: the daemon\nnever did it"
        );
        let from_text: Failed = "no frame".into();
        assert_eq!(format!("{from_text:?}"), "\"no frame\"");
    }

    #[test]
    fn what_a_rig_leaves_is_said_at_once_for_a_case_that_failed_and_held_for_one_that_has_not() {
        let said = Arc::new(AtomicUsize::new(0));
        let before = Verdict::start();
        let _ = Failed::from("the case failed");
        leave(before, counting(&said));
        assert_eq!(said.load(Ordering::SeqCst), 1, "said at once");
        let after = Verdict::start();
        leave(after, counting(&said));
        assert_eq!(
            said.load(Ordering::SeqCst),
            1,
            "held for a case that has not"
        );
        let _ = Failed::from("and now it has");
        assert_eq!(said.load(Ordering::SeqCst), 2, "said once the case failed");
        let _ = Failed::from("and again");
        assert_eq!(said.load(Ordering::SeqCst), 2, "said once");
    }

    #[test]
    fn what_a_rig_leaves_is_said_when_a_check_fails_and_not_said_when_the_thread_ends_without() {
        let said = Arc::new(AtomicUsize::new(0));
        let for_the_panic = Arc::clone(&said);
        let failed_by_a_check = thread::spawn(move || {
            leave(Verdict::start(), counting(&for_the_panic));
            let _ = std::panic::catch_unwind(|| panic!("a check that does not hold"));
        });
        failed_by_a_check.join().unwrap();
        assert_eq!(said.load(Ordering::SeqCst), 1, "said as the check failed");
        let for_the_pass = Arc::clone(&said);
        thread::spawn(move || leave(Verdict::start(), counting(&for_the_pass)))
            .join()
            .unwrap();
        assert_eq!(
            said.load(Ordering::SeqCst),
            1,
            "a case that passed says nothing"
        );
    }
}
