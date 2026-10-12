//! The scenarios an eval plays: what the owner says to the chief, how the owner
//! answers what the chief asks, how long the run is quiet before it is over, and
//! what the numbers of the ledger must come to for the chief to have done well.
//!
//! A scenario is code, not data: its expectations are closures over the measure's
//! numbers ([`Metrics`]), each with the name a report says it by. A scenario has
//! a fixture, `evals/fixtures/<name>/`, the toy project the chief works on (`site`:
//! a small bilingual site about burnout at work, with a planted discrepancy: the
//! guides say "a scale from 1 to 5", the site shows a colour and a score).
//!
//! The prose of a scenario is the eval: a word changed in a prompt changes what
//! is measured. `tests/scenarios.rs` holds each prompt, answer and name to what
//! the JavaScript scenarios said, which `tests/fixtures/scenarios.json` keeps.

use std::time::Duration;

use regex::Regex;

use crate::measure::{Check, Metrics};

mod advice_trip;
mod chief_switch;
mod complex_launch;
mod control_trip;
mod conversation;
mod long_trip;
mod question_trip;
mod round_trip;
mod simple_fix;
mod six_decisions;
mod terminal_questions;

/// What the owner says after the first message, once the chief is done with the last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowUp {
    /// A message typed into the chief's terminal.
    Say(String),
    /// The chief is switched to another harness from its card (`--switch-to`);
    /// the new window takes the handoff first, then the owner goes on in it.
    Switch,
}

/// How the owner answers a question the chief leaves in its terminal: the answer
/// of the first whose pattern the question sentence matches.
#[derive(Debug, Clone)]
pub struct Answer {
    pub matches: Regex,
    pub text: String,
}

/// One thing a chief is held to: the name a report says it by, and whether the
/// numbers of the run say it held.
pub struct Expectation {
    pub name: &'static str,
    pub holds: Box<dyn Fn(&Metrics) -> bool + Send + Sync>,
}

impl std::fmt::Debug for Expectation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("Expectation")
            .field(&self.name)
            .finish()
    }
}

/// An expectation called `name` that holds where `holds` says so.
pub fn expect(
    name: &'static str,
    holds: impl Fn(&Metrics) -> bool + Send + Sync + 'static,
) -> Expectation {
    Expectation {
        name,
        holds: Box::new(holds),
    }
}

/// The expression `source`, which a scenario writes in its own source and which
/// the tests that build every scenario hold well formed: a mistake in it is the
/// source's and nothing a run could meet or mend.
#[allow(clippy::expect_used)]
pub(crate) fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("a pattern written in a scenario's own source")
}

/// An answer to the questions whose sentence `source` (a pattern) finds.
pub(crate) fn answer(source: &str, text: impl Into<String>) -> Answer {
    Answer {
        matches: pattern(source),
        text: text.into(),
    }
}

/// A scenario.
#[derive(Debug)]
pub struct Scenario {
    pub id: &'static str,
    pub title: &'static str,
    /// The toy project the chief works on: `evals/fixtures/<fixture>/`.
    pub fixture: &'static str,
    /// What the owner types into the chief's terminal first.
    pub prompt: String,
    /// What the owner says next, in turn.
    pub follow_ups: Vec<FollowUp>,
    /// The owner's answers, free text, by what the question is about (the specific
    /// subjects first: most questions mention publishing in passing).
    pub answers: Vec<Answer>,
    /// The answer to a question none of the answers is for.
    pub fallback: String,
    /// What the owner types into the chief's terminal when the chief stops there
    /// without asking anything and has put nothing on the board.
    pub nudge: Option<&'static str>,
    /// How long nothing must have moved, after the last activity, for the run to be over.
    pub quiet: Duration,
    pub expectations: Vec<Expectation>,
}

impl Scenario {
    /// Whether the owner switches the chief at some point of it.
    pub fn switches_the_chief(&self) -> bool {
        self.follow_ups.contains(&FollowUp::Switch)
    }

    /// Each of the expectations against the numbers: what held, what did not.
    pub fn verdict(&self, metrics: &Metrics) -> Vec<Check> {
        self.expectations
            .iter()
            .map(|expectation| Check {
                name: expectation.name.to_owned(),
                ok: (expectation.holds)(metrics),
            })
            .collect()
    }
}

/// The ids of the scenarios, in the order they are listed.
pub const IDS: [&str; 12] = [
    "advice-trip",
    "chief-switch",
    "complex-launch",
    "control-trip",
    "conversation",
    "long-trip",
    "question-trip",
    "question-trip-native",
    "round-trip",
    "simple-fix",
    "six-decisions",
    "terminal-questions",
];

/// The scenario called `id`, if there is one. The codeword of `chief-switch` is
/// random, and is another each time it is asked for.
pub fn by_id(id: &str) -> Option<Scenario> {
    Some(match id {
        "advice-trip" => advice_trip::scenario(),
        "chief-switch" => chief_switch::scenario(),
        "complex-launch" => complex_launch::scenario(),
        "control-trip" => control_trip::scenario(),
        "conversation" => conversation::scenario(),
        "long-trip" => long_trip::scenario(),
        "question-trip" => question_trip::scenario(),
        "question-trip-native" => question_trip::native(),
        "round-trip" => round_trip::scenario(),
        "simple-fix" => simple_fix::scenario(),
        "six-decisions" => six_decisions::scenario(),
        "terminal-questions" => terminal_questions::scenario(),
        _ => return None,
    })
}

/// The scenario of `chief-switch` that tells the first chief `codeword`.
pub fn chief_switch_with(codeword: &str) -> Scenario {
    chief_switch::with(codeword)
}

/// `words` as the sentence of a prompt: each of them one after the other, with a
/// space between (the prompts are written a line of the source to a line of the
/// text, and the owner types them as one).
pub(crate) fn join(words: &[&str]) -> String {
    words.join(" ")
}
