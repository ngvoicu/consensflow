//! What makes a catch of a run that failed with a plant: what one run came to
//! (`Judged`), and how its command does without the plant (`CleanRuns`, which
//! remembers how often a command passed, and one that failed twice).
//!
//! Where the run's report names the test the plant was meant for among those that
//! failed, the plant is caught once its command passes without it, once; where it
//! names only other tests, it is MISSED-BY-MEANT. Where it names no test (a command
//! that says no more than its status, a test binary that died, a run with no
//! report), nothing can be attributed: the run is run so again and must fail again,
//! and the command must pass twice without the plant. A command that fails without
//! the plant is always run again once before it is taken to fail; one that passes
//! once and fails once is unstable.

use std::collections::HashMap;

use super::super::report::{self, Verdict};
use super::super::runs::{Execution, Ran, Runner};
use super::super::Error;

/// What one run with a plant in the sources came to.
pub(super) enum Judged {
    Passed(Ran),
    Caught(Ran, Vec<String>),
    /// Whatever else ends the trial: it was ended (out of time, or for writing more
    /// than a run may), it did not build, no test of it could see the plant.
    Not(Verdict, Ran),
}

impl Judged {
    pub(super) fn of(ran: Ran) -> Self {
        if ran.hung || ran.overflowed {
            return Self::Not(Verdict::Hung, ran);
        }
        if !ran.compiled() {
            return Self::Not(Verdict::DoesNotCompile, ran);
        }
        if ran.broken().is_some() {
            return Self::Not(Verdict::Broken, ran);
        }
        let caught = ran.caught();
        if caught.is_empty() {
            Self::Passed(ran)
        } else {
            Self::Caught(ran, caught)
        }
    }

    /// What it came to, in words that follow "failed with the plant, then".
    pub(super) fn then(&self) -> &'static str {
        match self {
            Self::Passed(_) => "passed",
            Self::Caught(..) => "failed",
            Self::Not(Verdict::DoesNotCompile, _) => "did not build",
            Self::Not(Verdict::Broken, _) => "ran no test that could see it",
            Self::Not(_, ran) if ran.overflowed => "wrote more than a run may",
            Self::Not(..) => "did not end",
        }
    }
}

/// How a command does on the sources as they are.
#[derive(Clone)]
pub(super) enum Clean {
    Passes,
    /// It failed twice: what it said the last.
    Fails(String),
    /// It passed once and failed once.
    Unstable(String),
}

/// What is known of a command without a plant.
enum Known {
    /// It passed this many times, and never failed.
    Passed(usize),
    /// It failed twice: what is said of it.
    Fails(String),
}

/// What each command that decided a plant did without one, by its words and the state
/// of the checkout it ran in (`Runner::state`), as far as this run of the plants has
/// run it: how often it passed, or that it failed twice. One that passed once and
/// failed once is forgotten, and run again for the next plant; all of it is forgotten
/// where the runs change what a command runs on beyond the sources (`forget`).
#[derive(Default)]
pub(super) struct CleanRuns(HashMap<(Vec<String>, String), Known>);

impl CleanRuns {
    /// What `command` said where it failed twice without a plant in `state`, if it did.
    pub(super) fn fails(&self, command: &[String], state: &str) -> Option<String> {
        match self.0.get(&(command.to_vec(), state.to_owned())) {
            Some(Known::Fails(said)) => Some(said.clone()),
            _ => None,
        }
    }

    /// Forgets how every command did: a plant's `prepare`, or a build again of the
    /// native `cf`, changed what they run on.
    pub(super) fn forget(&mut self) {
        self.0.clear();
    }

    /// How `command` does on the checkout as it is: it passes once it has passed
    /// `passes` times in this state, those of earlier plants counted; a failure is
    /// run again once. `None` is a stop.
    pub(super) fn measure(
        &mut self,
        command: &[String],
        passes: usize,
        runner: &mut dyn Runner,
    ) -> Result<Option<Clean>, Error> {
        let key = (command.to_vec(), runner.state());
        let mut passed = match self.0.get(&key) {
            Some(Known::Fails(said)) => return Ok(Some(Clean::Fails(said.clone()))),
            Some(Known::Passed(passed)) => *passed,
            None => 0,
        };
        let words = command.join(" ");
        while passed < passes {
            let Some(first) = once(command, runner)? else {
                return Ok(None);
            };
            let Err(said) = first else {
                passed += 1;
                continue;
            };
            if passed > 0 {
                self.0.remove(&key);
                return Ok(Some(Clean::Unstable(format!(
                    "{words} passed without the plant, then failed:\n{said}"
                ))));
            }
            let Some(again) = once(command, runner)? else {
                return Ok(None);
            };
            return Ok(Some(match again {
                Ok(()) => Clean::Unstable(format!(
                    "{words} failed without the plant, then passed:\n{said}"
                )),
                Err(said) => {
                    let fails = format!(
                        "{words} fails without the plant too, twice, so it tells nothing of it:\n{said}"
                    );
                    self.0.insert(key, Known::Fails(fails.clone()));
                    Clean::Fails(fails)
                }
            }));
        }
        self.0.insert(key, Known::Passed(passed));
        Ok(Some(Clean::Passes))
    }
}

/// One run of `command` on the sources as they are: `Err` is what it said where it
/// failed, or that it could not be started. `None` is a stop.
pub(super) fn once(
    command: &[String],
    runner: &mut dyn Runner,
) -> Result<Option<Result<(), String>>, Error> {
    Ok(Some(match runner.run(command) {
        Ok(Execution::Stopped) => return Ok(None),
        Ok(Execution::Ended(ran)) if ran.code == Some(0) => Ok(()),
        Ok(Execution::Ended(ran)) => Err(report::tail(&ran.output, 1500).to_owned()),
        Err(Error::Process(failure)) => Err(unstarted(command, &failure)),
        Err(other) => return Err(other),
    }))
}

/// What is said of a command that could not be started.
pub(super) fn unstarted(command: &[String], failure: &crate::process::Failure) -> String {
    format!("{} could not be started: {failure}", command.join(" "))
}
