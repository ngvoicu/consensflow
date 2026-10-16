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

/// What it runs: a sleep far longer than any case, and than any long run of the evals
/// (`crates/cf-evals`, hours on one rig): a day. Ten minutes, as it was, ended by itself
/// in the long run's dry run, and its close then read as a window's having ended it.
/// Until the stranger that lives as long as the rig (`cat` on a pipe the rig holds,
/// `wt/devin-resume`) is merged, which takes this away: a run past a day is not held.
pub(super) const SLEEP: &str = "exec sleep 86400";

impl Rig {
    /// The process id of the process that is none of any window's, which the rig keeps
    /// beside the pane host: for the case that ends it, to see the rig say so.
    pub fn stranger_pid(&self) -> u32 {
        self.stranger.0.id()
    }
}

/// The stranger a rig keeps. It is ended with the rig, if nothing ended it first,
/// through the handle the rig holds of it (its `Child`, when it is dropped): never by
/// its number.
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
    use crate::process::{group_of, is_alive};
    use crate::rig::wait;

    #[test]
    fn a_stranger_runs_in_a_group_of_its_own_until_something_ends_it_and_then_says_so() {
        let mut stranger = Stranger::start().unwrap();
        let id = stranger.0.id();
        assert!(stranger.runs());
        assert!(is_alive(id));
        assert_eq!(group_of(id), Some(id), "it is none of the tests' group");
        assert_ne!(group_of(id), group_of(std::process::id()));

        // Ended through the handle held of it, as the rig's drop ends it.
        stranger.0.kill();

        assert!(
            wait::until(Duration::from_secs(5), || !stranger.runs()),
            "it was ended, and says so"
        );
    }

    #[test]
    fn a_stranger_dropped_is_ended_through_its_handle() {
        let stranger = Stranger::start().unwrap();
        let id = stranger.0.id();
        assert!(is_alive(id));
        drop(stranger);
        assert!(!is_alive(id), "the rig's own child goes with it");
    }

    #[test]
    fn a_stranger_outlives_the_longest_run_a_rig_is_kept_for() {
        let seconds: u64 = SLEEP
            .strip_prefix("exec sleep ")
            .and_then(|seconds| seconds.parse().ok())
            .unwrap();
        // The long run's budget is hours; a stranger that ends by itself before the
        // rig does fails the rig's close as if a window had ended it.
        assert!(seconds >= 24 * 3600, "{SLEEP}");
    }
}
