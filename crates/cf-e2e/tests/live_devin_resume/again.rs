//! A window that goes on with a task it was cut in runs the long command again, at the
//! start of its next turn. The command ends by itself in a minute ([`crate::LONG`]), so the
//! check lets it run to its end, and the turn goes on to what it was given: nothing of it
//! is ended and the turn is not interrupted. A check that interrupted the turn once the
//! command had run for ten seconds cancelled the ask the turn was for (after-note's shell
//! ask, 2026-10-16), and a command that is killed is run again, and again.

use std::time::{Duration, Instant};

use cf_e2e::live::secs;
use cf_e2e::Result;

use crate::drive::command_running;
use crate::scene::Scene;

/// The runs of the long command a window started again, as the check saw them.
#[derive(Debug, Default)]
pub struct Again {
    /// How many times it was seen to start the command.
    runs: u32,
    /// Whether the command ran at the last look.
    running: bool,
    /// How long it was seen running, all its runs together, and since when the run at
    /// the last look was.
    ran: Duration,
    since: Option<Instant>,
}

impl Again {
    /// Looks at the long command in the window's own tree, and notes what it saw.
    pub fn look(&mut self, scene: &Scene) -> Result<()> {
        self.saw(command_running(scene)?);
        Ok(())
    }

    /// Notes whether the command runs, as a look just now saw it.
    pub fn saw(&mut self, running: bool) {
        self.see(running, Instant::now());
    }

    /// Notes whether the command runs, as seen `at`: a run begins where it did not run at
    /// the look before.
    fn see(&mut self, running: bool, at: Instant) {
        match (self.running, running) {
            (false, true) => {
                self.runs += 1;
                self.since = Some(at);
            }
            (true, false) => {
                if let Some(since) = self.since.take() {
                    self.ran += at.duration_since(since);
                }
            }
            _ => {}
        }
        self.running = running;
    }

    /// How many times the window was seen to start the command.
    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// What it did, where it ran the command again.
    pub fn said(&self) -> Option<String> {
        (self.runs > 0).then(|| {
            format!(
                "it ran the long command again ({} run(s), seen for {} s), and was let run it to its end{}",
                self.runs,
                secs(self.ran),
                if self.running {
                    "; it still ran at the last look"
                } else {
                    ""
                }
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_that_ran_the_long_command_again_is_said_to_have_and_how_often_and_nothing_ends_it()
    {
        let mut again = Again::default();
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        again.see(false, at(0));
        assert_eq!(again.said(), None);
        // A run of 60 s, a look at rest, and a second run still going.
        again.see(true, at(1));
        again.see(true, at(30));
        again.see(false, at(61));
        again.see(false, at(62));
        assert_eq!(again.runs(), 1);
        assert_eq!(
            again.said().as_deref(),
            Some("it ran the long command again (1 run(s), seen for 60.0 s), and was let run it to its end")
        );
        again.see(true, at(70));
        assert_eq!(again.runs(), 2);
        assert_eq!(
            again.said().as_deref(),
            Some("it ran the long command again (2 run(s), seen for 60.0 s), and was let run it to its end; it still ran at the last look")
        );
    }
}
