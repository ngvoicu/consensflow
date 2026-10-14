//! A process that is none of any window's, kept beside the rig on Unix, as the
//! human's own programs are beside the pane host: a program of the machine's, asleep,
//! in a group of its own. The pane host ends only what is a window's own, whatever the
//! daemon asked of it and whatever its tree took for the turn's work, so a rig that ends
//! with the stranger still running has shown it did. A rig that finds it gone fails
//! ([`Rig::close`](super::Rig::close)): in every case that ends a window, which is what
//! a wrong selection of what to end would take the human's work with.

use super::Rig;
use crate::process::{Run, Spawned};
use crate::Result;

/// What it runs: a sleep far longer than any case.
const SLEEP: &str = "exec sleep 600";

impl Rig {
    /// The process id of the process that is none of any window's, which the rig keeps
    /// beside the pane host: for the case that ends it, to see the rig say so.
    pub fn stranger_pid(&self) -> u32 {
        self.stranger.0.id()
    }
}

/// The stranger a rig keeps. It is ended with the rig, if nothing ended it first.
#[derive(Debug)]
pub(super) struct Stranger(Spawned);

impl Stranger {
    /// Starts it, in a group of its own.
    pub(super) fn start() -> Result<Self> {
        Run::new("/bin/sh")
            .args(["-c", SLEEP])
            .own_group()
            .closed_input()
            .spawn()
            .map(Self)
    }

    /// Whether it runs still: it has not been ended by anything.
    pub(super) fn runs(&mut self) -> bool {
        !self.0.has_exited()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::process::{group_of, is_alive, Signal};
    use crate::rig::wait;

    #[test]
    fn a_stranger_runs_in_a_group_of_its_own_until_something_ends_it_and_then_says_so() {
        let mut stranger = Stranger::start().unwrap();
        let id = stranger.0.id();
        assert!(stranger.runs());
        assert!(is_alive(id));
        assert_eq!(group_of(id), Some(id), "it is none of the tests' group");
        assert_ne!(group_of(id), group_of(std::process::id()));

        stranger.0.signal(Signal::Kill);

        assert!(
            wait::until(Duration::from_secs(5), || !stranger.runs()),
            "it was ended, and says so"
        );
    }
}
