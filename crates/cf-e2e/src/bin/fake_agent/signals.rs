//! Waiting for the requests to end that a window or a job is held ready for:
//! the signals are blocked in the thread that starts the rest (and so in every
//! thread started after it), and one thread waits for them here.
//!
//! nix's `SigSet::wait` is not used. It unwraps the number of the signal that
//! `sigwait` hands back, and on macOS the call returned success with that
//! number untouched when the pane host asked a window to end: the thread
//! panicked, nothing was left waiting for the request, and every close of a
//! window waited out its whole grace. Here a return of `sigwait` is a request,
//! whichever number it came with: the set holds nothing else.

use nix::sys::signal::SigSet;

/// Waits until one of `signals`, which this thread blocks, is sent to the
/// program: whether the wait ended by one (a failed wait is not a request).
pub fn wait(signals: &SigSet) -> bool {
    let mut number = 0;
    // SAFETY: `sigwait` reads the set and writes one `int`, through the
    // pointer to a local; both are valid for the call.
    unsafe { nix::libc::sigwait(signals.as_ref(), &mut number) == 0 }
}

#[cfg(test)]
mod tests {
    use nix::sys::signal::{raise, Signal};

    use super::*;

    #[test]
    fn a_blocked_signal_that_is_sent_ends_the_wait_for_it() {
        let mut signals = SigSet::empty();
        signals.add(Signal::SIGUSR1);
        signals.thread_block().unwrap();
        raise(Signal::SIGUSR1).unwrap();
        assert!(wait(&signals));
    }
}
