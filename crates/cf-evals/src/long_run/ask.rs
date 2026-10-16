//! What the owner of a long run says, as a [`Scenario`] the evals' scripted owner
//! plays (`run::owner`): the ask typed into the chief's terminal first, the answer
//! to the one question the ask tells the chief to put to the owner, a follow-up once
//! the work is done, and a nudge for a chief that stops without doing anything. A
//! long run has no expectations of its own: its verdict is the watch's.
//!
//! The project is `evals/fixtures/long-run/` (ledgerline, a small bookkeeping
//! library with a report site and a dev server), and the ask is the work of its
//! `docs/requirements.md`: three phases, planned as a spec, with work long on purpose
//! that each harness does its own way (a dev server Claude Code starts with
//! `run_in_background`, a long build Devin moves to the background, a full test suite
//! that takes minutes, a reading of every file that grows a window's context). The
//! tasks reach a harness by its tier (`staff::tier_of`).
//!
//! The stand-in agent reads none of that: [`Ask::StandIn`] says the same shapes in
//! its words (`DISPATCH` lines it runs as `cf task add`, a `JOB` that runs while its
//! turn works, a `BACKGROUND --wake` that wakes it when the job ends), for the dry run.

use std::time::Duration;

use crate::scenarios::{answer, FollowUp, Scenario};
use crate::{Error, Result};

/// Which owner's words a run is played with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// The project's ask, for real chiefs.
    Real,
    /// The same shapes in the stand-in agent's words, for the dry run.
    StandIn,
}

impl Ask {
    /// The ask a command line names.
    pub fn of(word: &str) -> Result<Self> {
        match word {
            "real" => Ok(Self::Real),
            "stand-in" => Ok(Self::StandIn),
            other => Err(Error::Usage(format!(
                "--ask is real or stand-in, not {other}"
            ))),
        }
    }

    /// Its name in a report.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::StandIn => "stand-in",
        }
    }

    /// The scenario the owner plays: `quiet` is how long nothing moves before the
    /// run is over.
    pub fn scenario(self, quiet: Duration) -> Scenario {
        match self {
            Self::Real => real(quiet),
            Self::StandIn => stand_in(quiet),
        }
    }
}

/// What a fixture's work asks of the staff: the ask a real chief is typed.
pub const PROMPT: &str = "\
This is ledgerline, a small bookkeeping library with a report site and a dev server (README.md). \
I want the work in docs/requirements.md done, all three phases, by the staff: plan it as a spec \
with phases first, then put the work on the board. Do not do the work yourself.

Before you plan, ask me one thing here in this terminal: which currency the reports default to. \
Wait for my answer.

Some of the work is long on purpose, and must stay so:
- npm test and npm run build each take minutes. After each phase a member runs both in full and \
reports the numbers: give that at tier standard.
- One task starts the dev server (npm run dev, which never exits) and checks every page of the \
built site against it with curl: give it at tier complex, and say in it that the server is \
started with the Bash tool's run_in_background and left running while the pages are checked.
- The full build of phase 3 is one long command run in the foreground: give it at tier light.
- One task reads all of docs/ and src/ and writes docs/review.md, what each module does and what \
is risky in it: give it at tier standard.
- Ask advice once (cf task add --advice) on how budget alerts should round, and have each phase \
reviewed (cf task add --review) before you accept it.
Spread the rest of the work across the tiers complex, standard and light.

When every task is accepted, tell me in a note (cf note --human) what was done, with the test and \
build numbers.";

/// The owner's answer to the question the ask tells the chief to ask.
pub const CURRENCY: &str = "EUR, with two decimals and a comma between thousands: 1,234.50 EUR.";

/// The follow-up, once the three phases are done.
pub const FOLLOW_UP: &str = "\
One more thing, now that the three phases are done: add a CSV export of the budget alerts \
(npm run export -- --alerts), with a test, and run the full test suite and the build again.";

fn real(quiet: Duration) -> Scenario {
    Scenario {
        id: "long-run",
        title: "A real project worked for hours by a chief and a staff of every harness",
        fixture: "long-run",
        prompt: PROMPT.to_owned(),
        follow_ups: vec![FollowUp::Say(FOLLOW_UP.to_owned())],
        answers: vec![answer(r"(?i)currenc|EUR|USD|RON", CURRENCY)],
        fallback: "Your call: go on with what you think is best.".to_owned(),
        nudge: Some("Go on: plan the phases and put the work on the board."),
        quiet,
        expectations: Vec::new(),
    }
}

/// The shapes the stand-in chief puts on the board first at the owner's answer: a
/// build whose job runs while its turn works (a tool call at work), a dev server
/// whose job wakes its worker when it ends, Devin's long turn and the command it
/// moves to the background, and a slow test run.
const STAND_IN_SHAPES: [&str; 5] = [
    "DISPATCH --tier complex JOB 1200 SLEEP 900 Reply with exactly: BUILT",
    "DISPATCH --tier complex BACKGROUND --wake 90 Reply with exactly: DEV-SERVER-UP",
    "DISPATCH --tier light SLEEP 900 Reply with exactly: DEVIN-BUILD-DONE",
    "DISPATCH --tier light BACKGROUND 300 Reply with exactly: DEVIN-MOVED-IT",
    "DISPATCH --tier complex JOB 1200 SLEEP 900 Reply with exactly: TESTS-PASS",
];

/// How many tasks of long work each tier has queued after the shapes. A stand-in
/// answers at once what a cut window is told on its return, so every disruption
/// that cuts a turn ends its task: the work queued behind the two workers of each
/// tier is what keeps a worker at work for the disruption after it.
const STAND_IN_MORE: usize = 6;

/// The stand-in chief's work: the shapes, then long work of each tier queued.
fn stand_in_tasks() -> String {
    let more = (1..=STAND_IN_MORE).flat_map(|at| {
        [
            format!("DISPATCH --tier complex JOB 1200 SLEEP 900 Reply with exactly: PHASE-{at}"),
            format!("DISPATCH --tier light SLEEP 900 Reply with exactly: DEVIN-PHASE-{at}"),
        ]
    });
    STAND_IN_SHAPES
        .iter()
        .map(|line| (*line).to_owned())
        .chain(more)
        .collect::<Vec<_>>()
        .join("\n")
}

fn stand_in(quiet: Duration) -> Scenario {
    let more = |words: &str, seconds: u32| {
        FollowUp::Say(format!(
            "DISPATCH --tier complex JOB 1200 SLEEP {seconds} Reply with exactly: {words}\n\
             DISPATCH --tier light SLEEP {seconds} Reply with exactly: DEVIN-{words}"
        ))
    };
    Scenario {
        id: "long-run",
        title: "The long run's shapes, in the stand-in agent's words",
        fixture: "long-run",
        prompt: "Reply with exactly: Currency?".to_owned(),
        follow_ups: vec![
            more("CSV-EXPORT", 400),
            more("LAST-CHECK", 300),
            FollowUp::Say("CF note --human :: All done: BUILT, TESTS-PASS, CSV-EXPORT.".to_owned()),
        ],
        answers: vec![answer(r"(?i)currency", stand_in_tasks())],
        fallback: stand_in_tasks(),
        nudge: None,
        quiet,
        expectations: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::terminal_answer;

    #[test]
    fn the_real_ask_names_every_shape_the_run_is_for_and_the_owner_answers_its_question() {
        let scenario = Ask::Real.scenario(Duration::from_secs(600));
        for shape in [
            "docs/requirements.md",
            "spec",
            "which currency",
            "npm test and npm run build each take minutes",
            "run_in_background",
            "tier complex",
            "tier light",
            "tier standard",
            "docs/review.md",
            "--advice",
            "--review",
            "cf note --human",
        ] {
            assert!(scenario.prompt.contains(shape), "{shape}");
        }
        assert_eq!(
            terminal_answer(&scenario, "Which currency should the reports default to?"),
            CURRENCY
        );
        assert_eq!(scenario.follow_ups, [FollowUp::Say(FOLLOW_UP.to_owned())]);
        assert!(scenario.expectations.is_empty());
        assert_eq!(scenario.fixture, "long-run");
    }

    #[test]
    fn the_stand_in_ask_asks_its_question_and_the_answer_puts_the_shapes_and_more_work_on_the_board(
    ) {
        let scenario = Ask::StandIn.scenario(Duration::from_secs(120));
        let answered = terminal_answer(&scenario, "Currency?");
        let lines: Vec<&str> = answered.lines().collect();
        assert_eq!(lines.len(), 5 + 2 * STAND_IN_MORE);
        assert!(lines
            .iter()
            .all(|line| line.starts_with("DISPATCH --tier ")));
        assert_eq!(lines[..5], STAND_IN_SHAPES);
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("--tier light"))
                .count(),
            2 + STAND_IN_MORE
        );
        assert!(answered.contains("BACKGROUND --wake"));
        assert!(answered.contains("--tier light") && answered.contains("--tier complex"));
        assert_eq!(scenario.follow_ups.len(), 3);
        assert!(
            matches!(&scenario.follow_ups[2], FollowUp::Say(said) if said.starts_with("CF note --human :: "))
        );
    }

    #[test]
    fn an_ask_is_named_on_the_command_line() {
        assert_eq!(Ask::of("real").unwrap(), Ask::Real);
        assert_eq!(Ask::of("stand-in").unwrap().name(), "stand-in");
        assert_eq!(
            Ask::of("fake").unwrap_err().to_string(),
            "--ask is real or stand-in, not fake"
        );
    }
}
