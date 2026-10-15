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
//!   one to reach.
//!
//! Each case is a window of the check's own, opened as the daemon opens a member's,
//! in a folder of its own, on a task whose first step is a file written with the
//! shell (so that what the window may do before the cut is said). It is cut as the
//! app's quit cuts every window (`pane.kill`) and opened again on `--resume`. The
//! window is then asked, one message at a time, to write a file with the shell, to
//! make one with its file-writing tool, to change one with its editing tool, and to
//! run `cf ask`.
//!
//! - `at-rest`            cut at rest, after the shell wrote its file and the turn ended
//! - `mid-tool`           cut while its shell runs a long command (the report's shape)
//! - `after-note`         as `mid-tool`, and opened again with the note that its turn
//!   was cut as the first message of its launch, in the prompt file: how the daemon gives
//!   a member the note when the human resumes the project (`cf-engine`'s `resume`)
//! - `after-note-pasted`  as `mid-tool`, and the note is pasted into the window once it
//!   has drawn, as the daemon pastes a message it gives an open window
//!
//! ```text
//! cargo xtask live devin-resume                       the four cases, one after the other
//! cargo xtask live devin-resume --case mid-tool       the ones named (--case a,b or --case a --case b)
//! cargo xtask live devin-resume --keep                leave the folders, the launches' files and wire logs
//! ```
//!
//! For each ask the run says, from the window's screen, Devin's own record of the
//! session (its store) and the wire log of the launch: whether the call ran (what it
//! left: a file, a line of the stand-in `cf`, and for `cf` the record where another
//! `cf` was found first), the tools Devin's record says were called and what each came
//! back with, every word of a refusal or of a permission (the lines of Devin's program,
//! the sentence of the report, and the permission the wire says was asked and how it
//! was answered) with where and when it was seen and the last thing the check did to
//! the window before it, which is what answered a dialog where a key did, and the
//! screen a turn that has not ended holds the window still on, in whatever words it
//! has them (the check does not guess a dialog's). A case that shows the skip makes
//! plain which step did it. The run ends with a table of what each case let the window
//! do. A run is about twenty turns of Devin's cheap model, and takes at the most what
//! the limits of its cases allow ([`WORK`], [`SETTLE`] twice, [`WATCH`], and three
//! minutes for each ask).
//!
//! The run fails only when a case could not be run (a window that did not open, a first
//! turn that never was at work, no conversation recorded): what Devin does is the
//! result, and it is said. What the launch writes of the owner's Devin config (the
//! copy with the hooks, each launch's wire log) is in the folder of the case, under
//! `.consensflow-candidate/live/`, and is taken away with it unless the run is asked
//! to `--keep` it. Whether the harness code the launch is asked of, and the rest of
//! the plumbing, work is held without Devin by `cargo test -p cf-e2e --test rig
//! live_member`, which runs the stand-in Devin.

// The parts are in `live_devin_resume/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "live_devin_resume/asks.rs"]
mod asks;
#[path = "live_devin_resume/case.rs"]
mod case;
#[path = "live_devin_resume/drive.rs"]
mod drive;
#[path = "live_devin_resume/outcome.rs"]
mod outcome;
#[path = "live_devin_resume/scene.rs"]
mod scene;
#[path = "live_devin_resume/tools.rs"]
mod tools;
#[path = "live_devin_resume/trail.rs"]
mod trail;
#[path = "live_devin_resume/watch.rs"]
mod watch;
#[path = "live_devin_resume/words.rs"]
mod words;

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::calls;
use cf_e2e::live::members::{named, LongCommand};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, agents, ask, Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};

use case::Case;
use drive::Run;
use outcome::{matrix_row, summary};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows: no
/// window of this test runs it, but a rig has one. And the stand-in for the `cf` of a
/// member's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");
const FAKE_CF: &str = env!("CARGO_BIN_EXE_fake-cf");

/// The folders of the cases, below the live folders, each `devin-resume-<case>`.
pub const FOLDER: &str = "devin-resume";

/// The long command a case cuts a window inside of, which a killed window leaves
/// running.
pub const LONG: LongCommand = LongCommand::new("cf-devin-resume-long-command");

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
    let agent = agents::agent(Harness::Devin);
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
    let mut rig = Rig::start(install.rig(FAKE_AGENT).first_on_path(stand_in.path()))?;
    if live::keep_requested() {
        log.say(format!(
            "{} is set: the home stays at {}, and the folders of the cases where they are",
            live::KEEP,
            rig.keep_home().display()
        ));
    }
    let (mut checks, mut matrix) = (Vec::new(), Vec::new());
    for case in options.cases {
        let run = Run {
            rig: &rig,
            install: &install,
            agent: agent.clone(),
            calls: stand_in.path(),
            log: &log,
        };
        let done = drive::run(&run, case);
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
        matrix.push(matrix_row(case, &done.outcomes));
    }
    // What the run found is said whether or not the rig ends well: a run of this length
    // is not to lose its table to a process the rig could not end.
    let closed = rig.close();
    log.block("what the window was let do, by case", &matrix.join("\n"));
    report::conclude(&log, "what each case came to", &checks).and(closed)
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
        assert_eq!(some.cases, [Case::MidTool, Case::AtRest]);
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
}
