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
//! [`Ask::StandInFast`] says them with their seconds cut to a twentieth or so: the project of
//! a run that finishes its work long before its schedule of disruptions is played, as the
//! 2-hour run on Windows did (2026-11-05: its project ended at 53 minutes, with the chief
//! switch, the Devin cut and the suspension still to play).
//!
//! **The extra work** ([`Ask::extra_work`]) is a short fixed list of pieces the owner gives, one
//! at a time and from the first again when they are given, to a project that went quiet while a
//! disruption is due and waits for its shape (staff at work, a job running, a Devin worker
//! mid-tool): the check the ask already has the staff do, the full test suite and the build,
//! with the dev server's pages. A shape cannot come of a project with nothing left to do, and
//! the run is neither over nor passed before its schedule is played (`run::owner`).

use std::time::Duration;

use crate::run::owner::Piece;
use crate::scenarios::{answer, FollowUp, Scenario};
use crate::{Error, Result};

/// Which owner's words a run is played with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// The project's ask, for real chiefs.
    Real,
    /// The same shapes in the stand-in agent's words, for the dry run.
    StandIn,
    /// The stand-in's words with their seconds cut: a project that ends long before its
    /// schedule is played.
    StandInFast,
}

impl Ask {
    /// The ask a command line names.
    pub fn of(word: &str) -> Result<Self> {
        match word {
            "real" => Ok(Self::Real),
            "stand-in" => Ok(Self::StandIn),
            "stand-in-fast" => Ok(Self::StandInFast),
            other => Err(Error::Usage(format!(
                "--ask is real, stand-in or stand-in-fast, not {other}"
            ))),
        }
    }

    /// Its name in a report.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::StandIn => "stand-in",
            Self::StandInFast => "stand-in-fast",
        }
    }

    /// The scenario the owner plays: `quiet` is how long nothing moves before the
    /// run is over.
    pub fn scenario(self, quiet: Duration) -> Scenario {
        match self {
            Self::Real => real(quiet),
            Self::StandIn => stand_in(quiet, SLOW),
            Self::StandInFast => stand_in(quiet, FAST),
        }
    }

    /// The pieces of extra work the owner may give a project that went quiet before its
    /// schedule of disruptions was played, in the order they are given.
    pub fn extra_work(self) -> Vec<Piece> {
        match self {
            Self::Real => real_extra_work(),
            Self::StandIn | Self::StandInFast => stand_in_extra_work(),
        }
    }
}

/// The project the long run works on: `evals/fixtures/long-run/`.
pub const FIXTURE: &str = "long-run";

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

/// How long a piece of the real extra work takes, with the chief's report after it: the
/// owner gives one only where the budget has that much left (the end, 9, is held to it). The
/// follow-up above took 9 minutes of the 2-hour run on Windows and was still going 9 minutes
/// after it was given in the 1-hour diagnostic.
pub const REAL_PIECE_TAKES: Duration = Duration::from_secs(25 * 60);

/// The check the extra work asks again, with the tiers of the ask: the full suite, the full
/// build in the foreground, the dev server and its pages.
const CHECK: &str = "\
put three tasks on the board: the full test suite (npm test) in full at tier standard; the \
full build (npm run build), one long command run in the foreground, at tier light; and the dev \
server (npm run dev, started with the Bash tool's run_in_background and left running while the \
pages are checked) with every page of the built site checked against it with curl, at tier \
complex. Each reports its numbers. When all three are accepted, tell me in a note \
(cf note --human) what each one ran and the numbers.";

/// The real ask's extra work: the check again, twice over, as the owner would ask it of a
/// staff that has finished.
fn real_extra_work() -> Vec<Piece> {
    [
        "Before we close this, check the whole project once more, in full: ",
        "Once more, the same check, to see that the numbers hold from one run to the next: ",
    ]
    .iter()
    .map(|opening| Piece {
        say: format!("{opening}{CHECK}"),
        takes: REAL_PIECE_TAKES,
    })
    .collect()
}

fn real(quiet: Duration) -> Scenario {
    Scenario {
        id: "long-run",
        title: "A real project worked for hours by a chief and a staff of every harness",
        fixture: FIXTURE,
        uncommitted: None,
        prompt: PROMPT.to_owned(),
        follow_ups: vec![FollowUp::Say(FOLLOW_UP.to_owned())],
        answers: vec![answer(r"(?i)currenc|EUR|USD|RON", CURRENCY)],
        fallback: "Your call: go on with what you think is best.".to_owned(),
        nudge: Some("Go on: plan the phases and put the work on the board."),
        quiet,
        stand_in_reviews: Vec::new(),
        stand_in_work: Vec::new(),
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

/// The same five shapes with their seconds cut: each job and turn lasts under a minute, so the
/// whole project is done in a few minutes, long before a schedule with an interval of minutes
/// has played its seven disruptions.
const FAST_SHAPES: [&str; 5] = [
    "DISPATCH --tier complex JOB 60 SLEEP 40 Reply with exactly: BUILT",
    "DISPATCH --tier complex BACKGROUND --wake 8 Reply with exactly: DEV-SERVER-UP",
    "DISPATCH --tier light SLEEP 40 Reply with exactly: DEVIN-BUILD-DONE",
    "DISPATCH --tier light BACKGROUND 20 Reply with exactly: DEVIN-MOVED-IT",
    "DISPATCH --tier complex JOB 60 SLEEP 40 Reply with exactly: TESTS-PASS",
];

/// How long the stand-in's work takes, in seconds: the shapes it begins with, the job and the
/// turn of the long work queued after them, and the turns of the two follow-ups.
#[derive(Clone, Copy)]
struct Pace {
    shapes: [&'static str; 5],
    job: u32,
    sleep: u32,
    export: u32,
    last: u32,
}

/// The pace of the dry run.
const SLOW: Pace = Pace {
    shapes: STAND_IN_SHAPES,
    job: 1200,
    sleep: 900,
    export: 400,
    last: 300,
};

/// The pace of a project that is done long before its schedule is.
const FAST: Pace = Pace {
    shapes: FAST_SHAPES,
    job: 60,
    sleep: 40,
    export: 30,
    last: 25,
};

/// How many tasks of long work each tier has queued after the shapes. A stand-in
/// answers at once what a cut window is told on its return, so every disruption
/// that cuts a turn ends its task: the work queued behind the two workers of each
/// tier is what keeps a worker at work for the disruption after it.
const STAND_IN_MORE: usize = 6;

/// The stand-in chief's work: the shapes, then long work of each tier queued.
fn stand_in_tasks(pace: Pace) -> String {
    let (job, sleep) = (pace.job, pace.sleep);
    let more = (1..=STAND_IN_MORE).flat_map(|at| {
        [
            format!(
                "DISPATCH --tier complex JOB {job} SLEEP {sleep} Reply with exactly: PHASE-{at}"
            ),
            format!("DISPATCH --tier light SLEEP {sleep} Reply with exactly: DEVIN-PHASE-{at}"),
        ]
    });
    pace.shapes
        .iter()
        .map(|line| (*line).to_owned())
        .chain(more)
        .collect::<Vec<_>>()
        .join("\n")
}

fn stand_in(quiet: Duration, pace: Pace) -> Scenario {
    let more = |words: &str, seconds: u32| {
        FollowUp::Say(format!(
            "DISPATCH --tier complex JOB {} SLEEP {seconds} Reply with exactly: {words}\n\
             DISPATCH --tier light SLEEP {seconds} Reply with exactly: DEVIN-{words}",
            pace.job
        ))
    };
    Scenario {
        id: "long-run",
        title: "The long run's shapes, in the stand-in agent's words",
        fixture: FIXTURE,
        uncommitted: None,
        prompt: "Reply with exactly: Currency?".to_owned(),
        follow_ups: vec![
            more("CSV-EXPORT", pace.export),
            more("LAST-CHECK", pace.last),
            FollowUp::Say("CF note --human :: All done: BUILT, TESTS-PASS, CSV-EXPORT.".to_owned()),
        ],
        answers: vec![answer(r"(?i)currency", stand_in_tasks(pace))],
        fallback: stand_in_tasks(pace),
        nudge: None,
        quiet,
        stand_in_reviews: Vec::new(),
        stand_in_work: Vec::new(),
        expectations: Vec::new(),
    }
}

/// How long a piece of the stand-in's extra work takes, with the chief's accepting after it.
pub const STAND_IN_PIECE_TAKES: Duration = Duration::from_secs(4 * 60);

/// The stand-in's extra work: the shapes of one complex and one light task, two minutes of work
/// each, a job running beside the complex one's turn (a tool call at work, the shape the
/// switch and the Devin cut are for).
fn stand_in_extra_work() -> Vec<Piece> {
    (1..=2)
        .map(|at| Piece {
            say: format!(
                "DISPATCH --tier complex JOB 150 SLEEP 120 Reply with exactly: RERUN-{at}\n\
                 DISPATCH --tier light SLEEP 120 Reply with exactly: DEVIN-RERUN-{at}"
            ),
            takes: STAND_IN_PIECE_TAKES,
        })
        .collect()
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
    }

    #[test]
    fn both_asks_work_the_long_run_s_project_with_none_of_the_owner_s_work_uncommitted() {
        for ask in [Ask::Real, Ask::StandIn] {
            let scenario = ask.scenario(Duration::from_secs(600));
            assert_eq!(scenario.fixture, "long-run", "{}", ask.name());
            assert_eq!(scenario.uncommitted, None, "{}", ask.name());
        }
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
            "--ask is real, stand-in or stand-in-fast, not fake"
        );
    }
}
