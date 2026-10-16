//! Live: what a Devin worker can do in a window that was opened again after the
//! app's quit cut it. It is opt-in (`cargo xtask live devin-resume`,
//! `npm run live:devin-resume`): it spends the machine's quota on windows of Devin,
//! on its cheap model, and needs Devin installed and logged in, so it is ignored
//! unless asked for and is no part of `check` or of CI. It runs wherever the pane
//! host does, on macOS and on Windows.
//!
//! From a field report of 2026-10-15 (a Windows machine, a Devin worker): after the
//! window of a task that was cut ("window closed") was opened again and the task
//! resumed, every write, shell and `cf ask` call of that window came back "Tool
//! execution was rejected: User skipped this tool call", and only reading, `grep` and
//! `find` worked. Those are Devin's own words, from its permission dialog, and the
//! window had the full-permission flag it has on a first launch. A hand probe on the
//! Mac (a session that wrote a file, ended at rest, resumed with the same flags, and
//! wrote another) did not show it. This check has what the probe had not:
//!
//! - the launch is the adapter's own, whole (`harness-ask devin_launch`: the owner's
//!   config with the hooks ConsensFlow adds, a `SessionStart` hook that gives the
//!   role, a question hook for a member, the first message in a prompt file, the
//!   wire log and role variables), and not the flags of a bare window;
//! - the window can be cut in the middle of a tool call, as the report's was;
//! - it is opened again on its conversation as the daemon opens it, with the note
//!   that its turn was cut where the case gives it one;
//! - what the daemon sends a window afterwards is sent as it sends it: a paste, then
//!   Enter once the window has drawn it, Enter once more after ten seconds if its
//!   record does not show the message and it has printed nothing, and the paste
//!   again after a minute;
//! - a stand-in `cf` is first on the windows' `PATH` and writes down each call, so
//!   that `cf ask` is a command the shell runs where there is no daemon for the real
//!   one to reach;
//! - the window's terminal, which marks what Devin is doing on its output, and its
//!   wire log, which says the mode its session is in, are read as the places Devin says
//!   what it waits for and what it was let do.
//!
//! Each case is a window of the check's own, opened as the daemon opens a member's,
//! in a folder of its own, on a task whose first step is a file written with the
//! shell (so that what the window may do before the cut is said). It is cut as the
//! app's quit cuts every window (`pane.kill`) and opened again on `--resume`. The
//! window is then asked, one message at a time, to write a file with the shell, to
//! make one with its file-writing tool, to change one with its editing tool, and to
//! run `cf ask`. Every case has a rig of its own, which lives no longer than nine
//! minutes: the rig keeps a process of the machine's that sleeps ten, and a rig that
//! outlives it is read as one that lost it. The long command runs a minute and ends by
//! itself ([`LONG`]): a window that goes on with its task and runs it again, at the start
//! of its next turn, is let run it to its end, and an ask waits for it. A check that
//! interrupted the turn for it cancelled the ask the turn was for, and a command that is
//! killed is run again, and again.
//!
//! Round 2 of the check found why (Devin 3000.11.92): Devin's `--resume` of a session
//! the quit cut in its first turn comes back in its normal permission mode, whatever
//! the flag says; the prompt file's calls are refused, in the report's words, and a
//! pasted turn's wait at a dialog. Since the fix the daemon puts a resumed window's
//! first message behind `/bypass`, and no window is ready for a delivery while its
//! wire says the normal mode: it is given `/bypass` first. `after-note` and `mid-tool`
//! have the product's way whole: the note is the engine's own (`engine-ask`), the
//! launch is the adapter's (which puts the note behind `/bypass`), and before the asks
//! the daemon's readiness is the product's own (`harness-ask devin_readiness`, the
//! adapter's window driven as the engine drives it, through the rig's pane host): what
//! the window is given first is what the product gives it, and the case gives it
//! nothing itself. The other cases are as they were, the evidence of what Devin does.
//!
//! The long command is found in the window's own tree: the processes that descend from
//! the window's program (the process the pane host said it opened, `pid`), by the process
//! that started each, the machine's own list of them read (`ps`, and Windows' process
//! table). Devin runs a command under its agent, a `devin` the window's started, with no
//! shell between, so the pane host's own look at a turn's work (`pane.background`, a
//! shell the harness started and what it started) sees none of it. Nothing outside the
//! window is looked at, whatever its command line says, and nothing is ended: what the
//! cut window left of the command runs out its minute. The check looked among all of the
//! machine's processes before, with `ps`: on Windows, which has none, it saw nothing and
//! failed every case at its setup while Devin ran the command (zeewin, 2026-10-16), and
//! it ended every process whose command line held the mark, the worker that ran it among
//! them, whose brief quoted it (2026-10-16).
//!
//! - `at-rest`                 cut at rest, after the shell wrote its file and the turn ended
//! - `mid-tool`                cut while its shell runs a long command (the report's shape),
//!   and readied as the daemon readies it before the asks
//! - `after-note`              as `mid-tool`, and opened again with the note that its turn
//!   was cut as the first message of its launch, in the prompt file: how the daemon gives
//!   a member the note when the human resumes the project (`cf-engine`'s `resume`), the
//!   adapter putting it behind `/bypass`; readied as the daemon readies it
//! - `after-note-pasted`       as `mid-tool`, and the note is pasted into the window once
//!   it has drawn, as the daemon pastes a message it gives an open window
//! - `mid-tool-interrupted`    as `mid-tool`, and the keys that interrupt a turn are
//!   pressed once it has drawn, as the app presses them (the adapter's own)
//! - `after-note-interrupted`  the note in the prompt file, and the keys that interrupt
//!   a turn after its turn
//! - `interrupted-then-note`   the keys that interrupt a turn, and then the note pasted
//! - `second-turn`             cut inside a command of its second turn, after a first
//!   that ended: what a window that had worked before the quit is
//! - `mid-tool-bypassed`       as `mid-tool`, and `/bypass`, the command that switches a
//!   window to full permission, is pasted once it has drawn
//! - `bypassed-then-note`      `/bypass`, and then the note pasted
//! - `after-note-bypass`       the note behind the check's own `/bypass`, in the prompt
//!   file (round 2's fix by hand; the adapter now puts its own before it too)
//! - `after-note-no-hooks`     as `after-note`, with none of the owner's hooks in the
//!   config of the launches, but the ones ConsensFlow adds
//! - `second-turn-note-bypass` as `second-turn`, and the note behind `/bypass` in the prompt
//!   file: what the prompt does to a window that is in full permission already
//!
//! ```text
//! cargo xtask live devin-resume                       the cases, one after the other
//! cargo xtask live devin-resume --case mid-tool       the ones named (--case a,b or --case a --case b)
//! cargo xtask live devin-resume --keep                leave the folders, the launches' files and wire logs
//! ```
//!
//! For each ask the run says, from the window's screen, its terminal's own marks
//! (`idle`, `working`, `blocked on permission`), Devin's record of the session (its
//! store) and the wire log of the launch: whether the call ran (what it left: a file, a
//! line of the stand-in `cf`, and for `cf` the record where another `cf` was found
//! first), the tools Devin's record says were called and what each came back with, every
//! word of a refusal or of a permission (the lines of Devin's program, the sentence of
//! the report, and the permission the wire says was asked and how it was answered) with
//! where and when it was seen and the last thing the check did to the window before it,
//! which is what answered a dialog where a key did, and the screen a turn that has not
//! ended holds the window still on, in whatever words it has them. A permission dialog is
//! told by its terminal's mark, and by the first of its options (`(Approve once)`) on the
//! screen, since its terminal marks it only at times. Before the asks, each case says
//! the mode the session is in, by the wire and by the label of full permission on the
//! screen, and a `/bypass` the window is given says what it did to its record and its
//! wire (a turn would be a prompt to its agent and an item of the record, which the
//! daemon's reader would read as an answer). A window held by a dialog nobody answered
//! has the app's interrupt pressed into it, as the human's Stop is, and is told what
//! that did to it: the asks go on
//! after it. A case that shows the skip makes plain which step did it. The run ends
//! with a table of what each case let the window do. A case is about twenty turns of
//! Devin's cheap model at the most.
//!
//! The run fails only when a case could not be run (a window that did not open, a first
//! turn that never was at work, no conversation recorded, a case that outlived its nine
//! minutes, a rig that did not end): what Devin does is the result, and it is said. What
//! the launch writes of the owner's Devin config (the copy with the hooks, each launch's
//! wire log) is in the folder of the case, under `.consensflow-candidate/live/`, and is
//! taken away with it unless the run is asked to `--keep` it. Whether the harness code
//! the launch is asked of, and the rest of the plumbing, work is held without Devin by
//! `cargo test -p cf-e2e --test rig live_member`, which runs the stand-in Devin.

// The parts are in `live_devin_resume/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "live_devin_resume/again.rs"]
mod again;
#[path = "live_devin_resume/asks.rs"]
mod asks;
#[path = "live_devin_resume/back.rs"]
mod back;
#[path = "live_devin_resume/case.rs"]
mod case;
#[path = "live_devin_resume/drive.rs"]
mod drive;
#[path = "live_devin_resume/life.rs"]
mod life;
#[path = "live_devin_resume/outcome.rs"]
mod outcome;
#[path = "live_devin_resume/scene.rs"]
mod scene;
#[path = "live_devin_resume/states.rs"]
mod states;
#[path = "live_devin_resume/tools.rs"]
mod tools;
#[path = "live_devin_resume/trail.rs"]
mod trail;
#[path = "live_devin_resume/turn.rs"]
mod turn;
#[path = "live_devin_resume/watch.rs"]
mod watch;
#[path = "live_devin_resume/wire.rs"]
mod wire;
#[path = "live_devin_resume/words.rs"]
mod words;

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::calls;
use cf_e2e::live::members::{named, Account, LongCommand};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, agents, ask, Harness, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};

use case::Case;
use drive::{Done, Run};
use life::{Life, LIFE};
use outcome::{matrix_row, summary};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows: no
/// window of this test runs it, but a rig has one. And the stand-in for the `cf` of a
/// member's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");
const FAKE_CF: &str = env!("CARGO_BIN_EXE_fake-cf");

/// The folders of the cases, below the live folders, each `devin-resume-<case>`.
pub const FOLDER: &str = "devin-resume";

/// The long command a case cuts a window inside of, found by its mark in the window's
/// own tree alone. It runs a minute and ends by itself: long enough for a window to be
/// cut while it runs (`mid-tool`: seen running, and cut two seconds later), and short
/// enough that a window opened again that runs it again at the start of its next turn
/// ends it, and goes on to what the turn was for in the time of an ask ([`asks::ASKED`]).
/// The check ends none of it: a check that interrupted the turn for it cancelled the ask
/// (after-note's shell ask, 2026-10-16).
pub const LONG: LongCommand = LongCommand::lasting("cf-devin-resume-long-command", 60);

/// How long a first turn may take to be at work (or over), and a window that is
/// opened again to go on with what it is given and to come to rest; and how long a
/// window opened again with nothing to do is watched for what it does by itself.
pub const WORK: Duration = Duration::from_secs(180);
pub const SETTLE: Duration = Duration::from_secs(240);
pub const WATCH: Duration = Duration::from_secs(45);

/// What the command line asked.
struct Options {
    cases: Vec<Case>,
}

/// The cases the words ask for, or all of them: Devin is the only harness this runs.
fn options(harnesses: &[String], cases: &[String]) -> Result<Options> {
    if let Some(other) = harnesses
        .iter()
        .find(|word| word.as_str() != Harness::Devin.name())
    {
        return Err(Error::Daemon(format!(
            "this check runs Devin alone, not {other}"
        )));
    }
    Ok(Options {
        cases: named(cases, &Case::ALL, Case::name, "case", &Case::ALL)?,
    })
}

/// What a run has for every case of it.
struct Setup<'a> {
    log: &'a Log,
    install: &'a Installed,
    agent: serde_json::Value,
    interrupt: ask::Interrupt,
    /// The folder of the stand-in `cf`.
    stand_in: &'a Path,
}

/// Runs `case` in a rig of its own, which is started for it and ended with it: what
/// came of the rig's end is a check of the case.
fn lived(setup: &Setup, case: Case) -> Done {
    let started = Rig::start(setup.install.rig(FAKE_AGENT).first_on_path(setup.stand_in));
    let mut rig = match started {
        Ok(rig) => rig,
        Err(failed) => {
            let mut account = Account::new();
            account.fail(format!("the rig did not start: {failed}"));
            return Done {
                account,
                ..Done::new()
            };
        }
    };
    if live::keep_requested() {
        setup.log.say(format!(
            "{} is set: the home stays at {}, and the folders of the cases where they are",
            live::KEEP,
            rig.keep_home().display()
        ));
    }
    let run = Run {
        rig: &rig,
        install: setup.install,
        agent: setup.agent.clone(),
        calls: setup.stand_in,
        log: setup.log,
        life: Life::of(LIFE),
        interrupt: setup.interrupt,
    };
    let mut done = drive::run(&run, case);
    done.account.closed(rig.close());
    done
}

#[test]
#[ignore = "opt-in: spends the machine's quota on windows of Devin; cargo xtask live devin-resume (npm run live:devin-resume)"]
fn a_member_window_opened_again_after_a_cut_is_asked_to_write_edit_and_run_cf_and_what_devin_does_is_said(
) -> Result {
    let log = Log::new("live devin-resume");
    // A word that asks for what this does not run is refused before anything is built.
    let options = options(&live::list("harness"), &live::list("case"))?;
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let install = log.found(Harness::Devin)?;
    let interrupt = ask::interrupt(Harness::Devin.kind())?;
    // The model as Devin is given it: its level is part of its id (`swe-2-medium`).
    let (family, level) = agents::cheap(Harness::Devin);
    let model = level.map_or_else(|| family.to_owned(), |level| format!("{family}-{level}"));
    // The stand-in `cf` is first on the windows' PATH: the machine may have the real one
    // beside its harnesses.
    let stand_in = tempfile::tempdir().map_err(|source| Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })?;
    let cf = calls::install(stand_in.path(), Path::new(FAKE_CF))?;
    log.say(format!(
        "the windows' cf is the stand-in at {}",
        cf.display()
    ));
    let setup = Setup {
        log: &log,
        install: &install,
        agent: agents::agent(Harness::Devin),
        interrupt,
        stand_in: stand_in.path(),
    };
    let (mut checks, mut matrix) = (Vec::new(), Vec::new());
    for case in options.cases {
        let done = lived(&setup, case);
        let held = done.account.held();
        log.block(
            &format!(
                "== devin {}{}",
                case.name(),
                if held { "" } else { " (not run)" }
            ),
            &done.account.text(),
        );
        checks.push(Check::new(
            "devin",
            case.name(),
            held,
            format!(
                "{}, model {model}; {}",
                install.version(),
                summary(&done.outcomes)
            ),
        ));
        matrix.push(matrix_row(case, &done.mode, &done.outcomes));
    }
    log.block("what the window was let do, by case", &matrix.join("\n"));
    report::conclude(&log, "what each case came to", &checks)
}

#[cfg(test)]
mod tests {
    use cf_e2e::process::Run as Program;

    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn every_case_is_run_unless_some_are_named_and_devin_is_the_only_harness() {
        let all = options(&[], &[]).unwrap();
        assert_eq!(all.cases, Case::ALL);
        let some = options(&words(&["devin"]), &words(&["mid-tool", "at-rest"])).unwrap();
        assert_eq!(some.cases, [Case::ALL[1], Case::ALL[0]]);
        let failed = |harnesses: &[&str], cases: &[&str]| {
            options(&words(harnesses), &words(cases))
                .err()
                .map(|failed| failed.to_string())
        };
        assert_eq!(
            failed(&["claude"], &[]).as_deref(),
            Some("this check runs Devin alone, not claude")
        );
        assert_eq!(
            failed(&[], &["late"]).as_deref(),
            Some("no such case: late")
        );
    }

    #[test]
    fn the_stand_in_cf_is_found_as_cf_writes_down_each_call_and_says_it_is_a_stand_in() {
        let folder = tempfile::tempdir().unwrap();
        let cf = calls::install(folder.path(), Path::new(FAKE_CF)).unwrap();
        // As a window's shell runs it: by the name `cf`, with the words of the call.
        let asked = Program::new(&cf).args(["ask", "which-ab12"]).run().unwrap();
        assert_eq!(asked.code, Some(0));
        assert_eq!(asked.stdout.trim_end(), "stand-in cf: ask which-ab12");
        // The question hook the member's config names is answered with nothing.
        let hook = Program::new(&cf).args(["hook", "devin"]).run().unwrap();
        assert_eq!((hook.code, hook.stdout.as_str()), (Some(0), ""));
        let written: Vec<String> = calls::calls(folder.path())
            .into_iter()
            .map(|call| call.words)
            .collect();
        assert_eq!(written, ["ask which-ab12", "hook devin"]);
    }

    #[test]
    fn the_long_command_the_cases_are_cut_in_has_a_mark_of_its_own() {
        assert!(LONG.text().ends_with("cf-devin-resume-long-command"));
        assert_eq!(FOLDER, "devin-resume");
    }

    #[test]
    fn the_long_command_runs_long_enough_to_be_cut_in_and_ends_in_the_time_of_an_ask() {
        // A window is cut two seconds after the command is seen running; one opened again
        // that runs it again at the start of an ask's turn ends it, and has a minute left
        // for the ask, whose turn the check no longer interrupts for it.
        assert_eq!(LONG.seconds(), 60);
        assert!(LONG
            .text()
            .starts_with("node -e \"setTimeout(() => {}, 60000)\" "));
        assert!(LONG.seconds() + 60 <= asks::ASKED.as_secs());
    }
}
