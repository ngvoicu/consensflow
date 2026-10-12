//! Live: what a harness does when it is resumed on a conversation the app's quit
//! cut in the middle of a turn. It is opt-in (`cargo xtask live resume-cut`,
//! `npm run live:resume-cut`): it spends the machine's quota on a window of each
//! harness named, and needs each one installed and logged in, so it is ignored
//! unless asked for and is no part of `check` or of CI.
//!
//! The app's quit ends every window's process with its turn at work, and the daemon
//! that starts again opens the chief's window on the harness's own `resume` of the
//! conversation the quit cut: a record whose last turn has no end. The daemon reads
//! a window at rest from what the harness says of it and what its record says of the
//! turn, so what the resumed harness does with a turn it finds cut (go on with it,
//! close it itself, or wait at its prompt and leave the record as it was) is what
//! that reading has to agree with.
//!
//! Each case is a window of its own on a message that has the harness work, a kill of
//! its process with the turn at work (`pane.kill`, which the pane host does to each
//! window as the app quits), and then a window on `resume` of the conversation,
//! opened with the app's own arguments, and watched: the harness's own status where it
//! keeps one (Claude's), what it writes to its record, its screen, and how the
//! daemon's record reader reads the record. At the end a second message is pasted, to
//! show that the window takes it.
//!
//! - `command`  the message has the harness run a long command; the window is killed
//!   once the command runs: the record ends in the tool's call
//! - `early`    the window is killed as soon as its turn is at work and has said
//!   nothing (Claude's status reads busy, or the record holds the message and no more):
//!   the record ends in the user's message
//!
//! ```text
//! cargo xtask live resume-cut                              Claude, both cases
//! cargo xtask live resume-cut --case command
//! cargo xtask live resume-cut claude pi                    each of them, one after the other
//! cargo xtask live resume-cut codex devin pi opencode
//! cargo xtask live resume-cut --model haiku                another Claude model (the default is sonnet)
//! ```
//!
//! The other harnesses run on their cheap models ([`cf_e2e::live::agents`]), plain
//! windows in a folder of their own: what is read of them is their record and their
//! screen, which is what the daemon reads of a Codex, Pi or Devin window (OpenCode's
//! server says more, and Pi's extension). `--transcripts <folder>` copies each case's
//! Claude transcript there, as the cut left it and as it ended. Claude has no MCP
//! server and no browser. The run fails only when a case could not be run (a window
//! that did not open, or a turn that was never at work): what a harness does is the
//! result, and it is said.

// The parts are in `live_resume_cut/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "live_resume_cut/case.rs"]
mod case;
#[path = "live_resume_cut/read.rs"]
mod read;

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::machine::live_folder_in;
use cf_e2e::live::members::{named, rig_for, Account, LongCommand};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, agents, ask, installed, Harness, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows:
/// no window of this test runs it (each is opened on the real harness's own path),
/// but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The folder of the check, below the live folders: a folder of its own, so that what
/// Claude Code asks of a folder (its trust) is asked once and kept.
pub const FOLDER: &str = "resume-cut";

/// How long a turn may take to be at work, and the resumed window to be watched.
pub const WORK: Duration = Duration::from_secs(180);
pub const WATCH: Duration = Duration::from_secs(45);

/// The long command the `command` case has the harness run, which a killed window
/// leaves running.
pub const LONG: LongCommand = LongCommand::new("cf-resume-cut-long-command");

/// What is pasted into the resumed window, to show that it takes a message.
pub const SECOND: &str = "Reply with exactly one line: AFTER";

/// A case of the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Command,
    Early,
}

impl Case {
    const ALL: [Case; 2] = [Case::Command, Case::Early];

    pub const fn name(self) -> &'static str {
        match self {
            Case::Command => "command",
            Case::Early => "early",
        }
    }

    /// The message that has the harness work.
    pub fn message(self) -> String {
        match self {
            Case::Command => format!(
                "Run this exact command with your shell tool, wait for it to finish, and then reply with exactly one line: FINISHED\n{}",
                LONG.text()
            ),
            Case::Early => "Count from 1 to 40, one number to a line, and then reply with exactly one line: COUNTED".to_owned(),
        }
    }
}

/// What the command line asked.
struct Options {
    harnesses: Vec<Harness>,
    cases: Vec<Case>,
    /// The model Claude's windows run on, an alias or a name (`--model`).
    model: String,
    transcripts: Option<PathBuf>,
}

/// The harnesses and cases the words ask for, or Claude and both cases.
fn options(
    harnesses: &[String],
    cases: &[String],
    model: Option<String>,
    transcripts: Option<String>,
) -> Result<Options> {
    Ok(Options {
        harnesses: named(
            harnesses,
            &Harness::ALL,
            Harness::name,
            "harness",
            &[Harness::Claude],
        )?,
        cases: named(cases, &Case::ALL, Case::name, "case", &Case::ALL)?,
        model: model.unwrap_or_else(|| "sonnet".to_owned()),
        transcripts: transcripts.map(PathBuf::from),
    })
}

/// The agent a harness's windows run: Claude's model is the one named, the others'
/// their cheap model.
fn agent_of(harness: Harness, model: &str) -> Value {
    if harness == Harness::Claude {
        json!({ "kind": harness.kind(), "model": model })
    } else {
        agents::agent(harness)
    }
}

/// The model a harness runs on, for the line that names it.
fn model_of(harness: Harness, model: &str) -> String {
    agent_of(harness, model)["model"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of each harness named; cargo xtask live resume-cut (npm run live:resume-cut)"]
fn a_window_resumed_on_a_conversation_the_quit_cut_is_watched_and_takes_a_second_message() -> Result
{
    let log = Log::new("live resume-cut");
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let options = options(
        &live::list("harness"),
        &live::list("case"),
        live::option("model"),
        live::option("transcripts"),
    )?;
    let installs: Vec<Installed> = options
        .harnesses
        .iter()
        .map(|harness| installed(*harness))
        .collect::<Result<_>>()?;
    for install in &installs {
        log.say(format!(
            "{} is {} ({}); its login is read from {}",
            install.harness().name(),
            install.command().display(),
            install.version(),
            install.home().display()
        ));
    }
    let shims = tempfile::tempdir().map_err(|source| Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })?;
    let mut rig = Rig::start(rig_for(&installs, shims.path(), Path::new(FAKE_AGENT))?)?;
    if live::keep_requested() {
        log.say(format!(
            "{} is set: the home stays at {}",
            live::KEEP,
            rig.keep_home().display()
        ));
    }
    if let Some(home) = rig.var("HOME") {
        // What Claude kept of the folder, which its memory is read by the next run, goes with it.
        rig.forget_claude_memory_of(&live_folder_in(Path::new(home), FOLDER)?);
    }
    let mut checks = Vec::new();
    for install in &installs {
        let harness = install.harness();
        for case in options.cases.clone() {
            let run = case::Run {
                rig: &rig,
                install,
                agent: agent_of(harness, &options.model),
                transcripts: options.transcripts.as_deref(),
                log: &log,
            };
            let account = case::run(&run, case).unwrap_or_else(|failed| {
                let mut account = Account::new();
                account.fail(failed);
                account
            });
            log.block(
                &format!(
                    "== {} {}{}",
                    harness.name(),
                    case.name(),
                    if account.held() { "" } else { " (not run)" }
                ),
                &account.text(),
            );
            checks.push(Check::new(
                harness.name(),
                case.name(),
                account.held(),
                format!(
                    "{}, model {}",
                    install.version(),
                    model_of(harness, &options.model)
                ),
            ));
        }
    }
    rig.close()?;
    log.block("what each case came to", &report::table(&checks));
    report::verdict(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn claude_and_both_cases_are_run_unless_others_are_named_and_the_model_is_sonnet() {
        let all = options(&[], &[], None, None).unwrap();
        assert_eq!(all.harnesses, [Harness::Claude]);
        assert_eq!(all.cases, [Case::Command, Case::Early]);
        assert_eq!(all.model, "sonnet");
        assert!(all.transcripts.is_none());
        let some = options(
            &words(&["pi", "codex"]),
            &words(&["early"]),
            Some("haiku".to_owned()),
            Some("/tmp/cut".to_owned()),
        )
        .unwrap();
        assert_eq!(some.harnesses, [Harness::Pi, Harness::Codex]);
        assert_eq!(some.cases, [Case::Early]);
        assert_eq!(some.model, "haiku");
        assert_eq!(some.transcripts, Some(PathBuf::from("/tmp/cut")));
    }

    #[test]
    fn a_harness_or_a_case_that_is_none_is_refused_with_its_name() {
        let failed = |result: Result<Options>| result.err().map(|failed| failed.to_string());
        assert_eq!(
            failed(options(&words(&["kimi"]), &[], None, None)).as_deref(),
            Some("no such harness: kimi")
        );
        assert_eq!(
            failed(options(&[], &words(&["late"]), None, None)).as_deref(),
            Some("no such case: late")
        );
    }

    #[test]
    fn the_command_case_has_the_harness_run_a_long_command_and_the_early_case_a_count() {
        let command = Case::Command.message();
        assert!(command.starts_with("Run this exact command with your shell tool, wait for it to finish, and then reply with exactly one line: FINISHED\n"));
        assert!(command.ends_with("cf-resume-cut-long-command"));
        assert_eq!(
            Case::Early.message(),
            "Count from 1 to 40, one number to a line, and then reply with exactly one line: COUNTED"
        );
        assert_eq!(SECOND, "Reply with exactly one line: AFTER");
    }

    #[test]
    fn claude_runs_on_the_model_named_and_the_others_on_their_cheap_one() {
        assert_eq!(
            agent_of(Harness::Claude, "haiku"),
            json!({ "kind": "claude-code", "model": "haiku" })
        );
        assert_eq!(model_of(Harness::Claude, "sonnet"), "sonnet");
        assert_eq!(
            model_of(Harness::Pi, "sonnet"),
            "openrouter/meta/muse-spark-1.3"
        );
        assert_eq!(agent_of(Harness::Devin, "x")["effort"], "medium");
    }
}
