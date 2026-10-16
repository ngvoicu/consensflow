//! What makes a catch of a run that failed with a plant: what one run came to
//! (`Judged`), and how its command does without the plant (`CleanRuns`, which
//! remembers how often a command passed, and one that failed twice).
//!
//! Where the run's report names the test the plant was meant for among those that
//! failed, the plant is caught once its command passes without it, once, whatever
//! else broke beside it (an error outside every test is said with the catch); where
//! it names only other tests, or a meant name that stands for more than one test of
//! the report, it is MISSED-BY-MEANT. Where it names no test (a command that says no
//! more than its status, a test file that failed whole), nothing can be attributed:
//! the run is run so again and must fail again, and the command must pass twice
//! without the plant. A run whose reporter wrote no report it could read catches
//! nothing. A command that fails without the plant is always run again once before
//! it is taken to fail; one that passes once and fails once is unstable.

use std::collections::HashMap;

use super::super::report::{self, Verdict};
use super::super::runs::{Execution, Ran, Runner};
use super::super::Error;

/// What one run with a plant in the sources came to.
pub(super) enum Judged {
    Passed(Ran),
    /// It failed, the meant test among the tests that failed (`caught`); `beside` is
    /// what else broke, outside every test.
    Meant {
        ran: Ran,
        caught: Vec<String>,
        beside: Option<String>,
    },
    /// It failed naming tests of its own, none of them the meant one beyond doubt;
    /// `doubt` is why the meant name is not taken where it is among them.
    Others {
        ran: Ran,
        caught: Vec<String>,
        doubt: Option<String>,
    },
    /// It failed, and names no test of its own that did: the run is what caught it.
    Unnamed(Ran, Vec<String>),
    /// Whatever else ends the run: it was ended (out of time, or for writing more
    /// than a run may), it did not build, no test of it could see the plant.
    Not(Verdict, Ran),
}

impl Judged {
    /// What `ran` came to, for a plant meant for the test `meant`.
    pub(super) fn of(ran: Ran, meant: &str) -> Self {
        if ran.hung || ran.overflowed {
            return Self::Not(Verdict::Hung, ran);
        }
        if !ran.compiled() {
            return Self::Not(Verdict::DoesNotCompile, ran);
        }
        let caught = ran.caught();
        let broken = ran.broken();
        if !caught.is_empty() && ran.names_tests() && !meant.is_empty() {
            match meant_in(&ran, meant) {
                Some(Ok(())) => {
                    return Self::Meant {
                        ran,
                        caught,
                        beside: broken,
                    }
                }
                doubt if broken.is_none() => {
                    return Self::Others {
                        ran,
                        caught,
                        doubt: doubt.and_then(Result::err),
                    }
                }
                _ => {}
            }
        }
        if broken.is_some() {
            Self::Not(Verdict::Broken, ran)
        } else if caught.is_empty() {
            Self::Passed(ran)
        } else {
            Self::Unnamed(ran, caught)
        }
    }

    /// Whether the run failed, the plant seen by a test or by the run itself.
    pub(super) fn failed(&self) -> bool {
        matches!(
            self,
            Self::Meant { .. } | Self::Others { .. } | Self::Unnamed(..)
        )
    }

    /// What it came to, in words that follow "failed with the plant, then".
    pub(super) fn then(&self) -> &'static str {
        match self {
            Self::Passed(_) => "passed",
            Self::Meant { .. } | Self::Others { .. } | Self::Unnamed(..) => "failed",
            Self::Not(Verdict::DoesNotCompile, _) => "did not build",
            Self::Not(Verdict::Broken, _) => "ran no test that could see it",
            Self::Not(_, ran) if ran.overflowed => "wrote more than a run may",
            Self::Not(..) => "did not end",
        }
    }
}

/// Whether the meant test is among those `ran` says failed: none where it is not;
/// else, where the meant name stands for more than one test of the report (two
/// tests of one name in two places, which the name cannot tell apart), why it is
/// not taken for the one that failed.
fn meant_in(ran: &Ran, meant: &str) -> Option<Result<(), String>> {
    let named: Vec<_> = ran
        .tests()
        .iter()
        .filter(|test| report::is_meant(&test.name, meant))
        .collect();
    if !named.iter().any(|test| test.failed) {
        return None;
    }
    let mut places: Vec<(&str, &str)> = Vec::new();
    for test in &named {
        if !places.contains(&(test.name.as_str(), test.at.as_str())) {
            places.push((test.name.as_str(), test.at.as_str()));
        }
    }
    if places.len() == 1 {
        return Some(Ok(()));
    }
    let each: Vec<String> = places
        .iter()
        .map(|(name, at)| format!("{name} ({at})"))
        .collect();
    Some(Err(format!(
        "the meant name {meant} stands for {} tests of {}, so none of them is taken for it: {}",
        places.len(),
        ran.command.join(" "),
        each.join(", ")
    )))
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
/// failed, where its reporter wrote no report it could read, or that it could not be
/// started. `None` is a stop.
pub(super) fn once(
    command: &[String],
    runner: &mut dyn Runner,
) -> Result<Option<Result<(), String>>, Error> {
    Ok(Some(match runner.run(command) {
        Ok(Execution::Stopped) => return Ok(None),
        Ok(Execution::Ended(Ran {
            report: Some(Err(why)),
            ..
        })) => Err(format!("no report of its tests could be read: {why}")),
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
