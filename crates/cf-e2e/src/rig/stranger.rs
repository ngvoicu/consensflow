//! A process that is none of any window's, kept beside the rig, as the human's own
//! programs are beside the pane host: a program of the machine's, in a group of its own
//! (Unix's; Windows has none, and its pane host ends a window's own by the window's
//! job). The pane host ends only what is a window's own, whatever the daemon asked of
//! it and whatever its tree took for the turn's work, so a rig that ends with the
//! stranger still running has shown it did. A rig that finds it ended by something
//! else fails ([`Rig::close`](super::Rig::close)): in every case that ends a window,
//! which is what a wrong selection of what to end would take the human's work with.
//!
//! It reads its input to the end, and the rig holds its input open, so it lives as
//! long as the rig does, however long that is: a rig of the long-run suite lives for
//! hours, and a stranger that slept ten minutes ran out by itself under a live suite
//! and was read as ended with the windows (2026-10-15). A test that ends without
//! closing its rig takes the stranger with it, its input closing with the test's
//! process. Its own end, which only its input's end brings, is told from an end it
//! was given by whether the rig ended its input: any end before that was given,
//! whatever code it left with (a job's end gives the code its caller names, 0 too:
//! Astra on the B1 fix), and after it, a signal on Unix, and on Windows, where there
//! is none, any code but the 0 it leaves with (`taskkill /F` gives 1).

use std::time::Duration;

use super::Rig;
use crate::process::{Run, Spawned};
use crate::Result;

/// The program it runs, which reads its input to its end and then leaves.
#[cfg(unix)]
fn program() -> std::ffi::OsString {
    "/bin/cat".into()
}

/// The program it runs, which reads its input to its end and then leaves: Windows'
/// own `sort`, where the system is.
#[cfg(windows)]
fn program() -> std::ffi::OsString {
    let system = crate::process::own_var("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    std::path::Path::new(&system)
        .join("System32")
        .join("sort.exe")
        .into_os_string()
}

impl Rig {
    /// The process id of the process that is none of any window's, which the rig keeps
    /// beside the pane host: for the case that ends it, to see the rig say so.
    pub fn stranger_pid(&self) -> u32 {
        self.stranger.spawned.id()
    }

    /// Ends the input of the process that is none of any window's, which reads to the
    /// end and leaves, of itself, and waits up to `within` for it to be gone: whether
    /// it is. For the case that holds the rig to telling that end from one it was given.
    pub fn stranger_leaves(&mut self, within: Duration) -> Result<bool> {
        self.stranger.release();
        Ok(self.stranger.spawned.wait(within)?.is_some())
    }
}

/// The stranger a rig keeps, and whether the rig ended its input. It is ended with
/// the rig, if nothing ended it first.
#[derive(Debug)]
pub(super) struct Stranger {
    spawned: Spawned,
    released: bool,
}

impl Stranger {
    /// Starts it, in a group of its own, its input a pipe the rig holds open.
    pub(super) fn start() -> Result<Self> {
        Run::new(program()).own_group().spawn().map(|spawned| Self {
            spawned,
            released: false,
        })
    }

    /// Ends its input: it reads to the end, and leaves of itself.
    fn release(&mut self) {
        self.released = true;
        self.spawned.end_input();
    }

    /// How something else ended it, where something did: none while it runs, and none
    /// where it left of itself.
    pub(super) fn ended_by_another(&mut self) -> Option<String> {
        let status = self.spawned.status()?;
        #[cfg(unix)]
        let signal = std::os::unix::process::ExitStatusExt::signal(&status);
        #[cfg(windows)]
        let signal = None;
        given(status.code(), signal, self.released)
    }
}

/// How a stranger that exited with `code`, or was ended by `signal`, was ended by
/// something else, `released` where the rig had ended its input: by any end before
/// that, whatever its code; after it, by the signal on Unix, and on Windows by any
/// code but the 0 it leaves with. None for an end of its own.
fn given(code: Option<i32>, signal: Option<i32>, released: bool) -> Option<String> {
    if let Some(signal) = signal {
        return Some(format!("by the signal {signal}"));
    }
    let with = code.map_or_else(|| "no code".to_owned(), |code| format!("the code {code}"));
    if !released {
        return Some(format!("before its input ended, with {with}"));
    }
    if cfg!(unix) || code == Some(0) {
        return None;
    }
    Some(format!("with {with}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{is_alive, Signal};
    use crate::rig::wait;

    #[test]
    fn an_end_of_its_own_is_told_from_an_end_it_was_given() {
        // Its input ended, it leaves of itself with 0.
        assert_eq!(
            given(None, Some(9), true).as_deref(),
            Some("by the signal 9")
        );
        assert_eq!(given(Some(0), None, true), None);
        if cfg!(unix) {
            // On Unix only a signal is given: a code is its own, whatever it is.
            assert_eq!(given(Some(1), None, true), None);
        } else {
            // Windows has no signal, and an end given is a code other than its own 0.
            assert_eq!(
                given(Some(1), None, true).as_deref(),
                Some("with the code 1")
            );
            assert_eq!(given(None, None, true).as_deref(), Some("with no code"));
        }
    }

    #[test]
    fn any_end_before_its_input_ended_was_given_whatever_its_code() {
        // A job ended with the code its caller names, 0 too, was read as an end of its
        // own on Windows (Astra on the B1 fix).
        for (code, said) in [
            (Some(0), "before its input ended, with the code 0"),
            (Some(1), "before its input ended, with the code 1"),
            (None, "before its input ended, with no code"),
        ] {
            assert_eq!(given(code, None, false).as_deref(), Some(said));
        }
        assert_eq!(
            given(None, Some(9), false).as_deref(),
            Some("by the signal 9")
        );
    }

    #[test]
    fn a_stranger_runs_until_something_ends_it_and_then_says_how() {
        let mut stranger = Stranger::start().unwrap();
        let id = stranger.spawned.id();
        assert!(is_alive(id));
        assert_eq!(stranger.ended_by_another(), None, "it runs");
        #[cfg(unix)]
        {
            use crate::process::group_of;
            assert_eq!(group_of(id), Some(id), "it is none of the tests' group");
            assert_ne!(group_of(id), group_of(std::process::id()));
        }

        stranger.spawned.signal(Signal::Kill);

        let mut said = None;
        assert!(
            wait::until(Duration::from_secs(10), || {
                said = stranger.ended_by_another();
                said.is_some()
            }),
            "it was ended, and says so"
        );
    }

    /// A rig dropped (a test that failed, or one that never asked) takes its stranger with it:
    /// the handle ends it and waits for it, so nothing of it is left, not even an exit no one
    /// collected. Its input closes with the handle too, and it would leave of itself, but as a
    /// process no one waited for, which the system still answers for.
    #[test]
    fn a_stranger_dropped_is_ended_through_its_handle() {
        let stranger = Stranger::start().unwrap();
        let id = stranger.spawned.id();
        assert!(is_alive(id));
        drop(stranger);
        assert!(
            wait::until(Duration::from_secs(5), || !is_alive(id)),
            "the stranger {id} is still there"
        );
    }

    #[test]
    fn a_stranger_whose_input_ends_leaves_of_itself_and_is_not_said_to_have_been_ended() {
        let mut stranger = Stranger::start().unwrap();
        let id = stranger.spawned.id();
        // A long while is no matter: it waits on its input, not on a clock.
        std::thread::sleep(Duration::from_millis(300));
        assert!(is_alive(id));
        stranger.release();
        assert!(
            wait::until(Duration::from_secs(10), || stranger.spawned.has_exited()),
            "it left once its input ended"
        );
        assert_eq!(stranger.ended_by_another(), None);
    }
}
