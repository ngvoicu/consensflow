//! Live: a stop, as the daemon presses it (the receipt and stop design, §1.3,
//! §5.6). It is opt-in (`cargo xtask live stops`, `npm run live:stops`): it spends
//! the machine's quota on a member of the harness each case runs, and needs it
//! installed and logged in, so it is ignored unless asked for and is no part of
//! `check` or of CI.
//!
//! A pause of a task asks a stop of its window; at a window at work the daemon
//! presses the harness's interrupt (Escape) into its pane, again three seconds
//! later, three rounds in all, and a window that reads at rest has paid the stop with
//! no key. Each case opens a real member on the native daemon, gives it a task,
//! pauses the task as the human's Pause does (`task.pause`), and shows what the
//! daemon pressed (every `pane.input` of an Escape it sent), what the command, the
//! hook and the turn did, and what the window's own status and record say, beside
//! the daemon's own reading of the window.
//!
//! Claude Code's cases:
//!
//! - `command`   Escape into Claude inside a long command: the command and the turn
//!   stop, and nothing more is pressed once it is at rest.
//! - `early`     Escape right after the task's words went in, before Claude's first
//!   output. Claude writes no record of such a stop and puts the words back in its
//!   input box, and out of the conversation it answers from; the daemon reads the
//!   window at rest by its own press, and the ledger keeps the brief for the window
//!   again. Then the resume of the task: its words must land as a message of their
//!   own, not after the old text, carry the brief, and the task must end with its
//!   result, which holds the brief's answer.
//! - `hook`      Escape at its question hook (the member's question waits on the
//!   board): what happens to the hook's wait and the turn; then the answer the chief
//!   gives late, and the resume that carries it.
//! - `escape`    The same hook, with no pause: one Escape into the pane, as the
//!   daemon presses it, and what that does to the hook, the turn and the question the
//!   board still holds.
//! - `slowhook`  Escape while a hook of the project's own, that ignores the signals
//!   Claude ends a hook with, outlives Claude's turn (`--hook` says which: `stop`,
//!   `pretool` or `prompt`): the stop the daemon could take for ignored. When Claude
//!   stopped, the resume of the task, and its result.
//!
//! - `tell`      The chief's `cf tell` into Claude inside a long command while two
//!   background agents it started run (T-604): one Escape stops the turn, the window
//!   stays open with its agents and nobody is told it did not stop, and the tell's
//!   words go into the same window once it rests, which answers them (T-551).
//!
//! Any harness (`--harness`, or its name):
//!
//! - `stuck`     A window that may not stop, pressed through every round and kept open
//!   for its command if it ignores them all, never closed (the human and the requester
//!   are told, T-1117): a command that
//!   ignores `INT`, `TERM` and `HUP` is run, then the task paused. When the harness
//!   stops anyway, it says so, and at which press.
//! - `midturn`   A window in the middle of a turn that runs no command (the member is
//!   writing a long reply), paused: the daemon's rounds of its harness's interrupt
//!   must end the turn within them, and the pause be paid. It says for each round the
//!   keys pressed and what the window showed after them (Devin's wire log too), where
//!   the turn goes on as where it stops. Written for the Devin window that did not
//!   stop for a pause on Windows (T-5, 2026-10-12, 3.0.1). A member that never gets to
//!   write (its window never shows its first message, and the daemon fails the task)
//!   is said with why: the task and its thread, what the daemon wrote and told, the
//!   lane, the window as it printed (the bytes too) and as its pane host draws it,
//!   Devin's own files, and the daemon's log.
//! - `midtool`   A window in the middle of a shell command (the member is asked to run a
//!   ping of the machine's own address, a reply a second for a minute, the platform's own
//!   flags), paused while the command runs: the same rounds must end the turn within them,
//!   and the pause be paid. It says what the window showed after each round, when the lane
//!   read idle, and whether the host's process tree (`pane.background`, on every platform)
//!   still has the command once the stop is paid; the stop is watched until the window has
//!   rested (`REST`) or `STUCK_WATCH` is up, so that when a turn that waits for its tool
//!   ends is told. Written for the Devin window of T-5 (3000.10.21, Windows), which was
//!   running a command.
//! - `midwsl`    `midtool` with the field's command of T-1117, run only when named and
//!   on Windows only: a sleep in WSL's Ubuntu through `wsl.exe` (a worker's database
//!   tests ran so when a pause closed its window, and ended them).
//! - `midbg`     `midtool` after a command the member started in the background (the
//!   field's `wsl` sleep on Windows, a long ping elsewhere), run only when named: the
//!   pause must leave that command running.
//!
//! ```text
//! cargo xtask live stops                                  every case, `stuck`, `midturn` and `midtool` on Devin
//! cargo xtask live stops --case command --case hook
//! cargo xtask live stops --case stuck codex
//! cargo xtask live stops --case midturn devin
//! cargo xtask live stops --case midtool devin
//! cargo xtask live stops --case midwsl claude                (Windows, with WSL's Ubuntu)
//! cargo xtask live stops --case slowhook --hook prompt
//! cargo xtask live stops --case early --transcripts /tmp/stops    each Claude transcript is copied there
//! ```
//!
//! The run fails when a case showed what the design says it must not.

// The parts are in `live_stops/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "live_stops/cases.rs"]
mod cases;
#[path = "live_stops/hook.rs"]
mod hook;
#[path = "live_stops/midturn.rs"]
mod midturn;
#[path = "live_stops/options.rs"]
mod options;
#[path = "live_stops/resume.rs"]
mod resume;
#[path = "live_stops/slow.rs"]
mod slow;
#[path = "live_stops/stop.rs"]
mod stop;
#[path = "live_stops/stuck.rs"]
mod stuck;
#[path = "live_stops/tell.rs"]
mod tell;
#[path = "live_stops/watch.rs"]
mod watch;

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::members::{
    question_brief, session_of, stand_in_chief, Account, LongCommand, Members,
};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, Harness, Log};
use cf_e2e::{files, Result};

use midturn::tool::Command;
use options::{options, plan_of, runs_on, Case, Options};

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// How long a member may take to reach its command or ask, and a stop to take hold.
pub const WORK: Duration = Duration::from_secs(300);
pub const STOP: Duration = Duration::from_secs(40);
/// How long the words that resume a task may take to go into its window.
pub const RESUME: Duration = Duration::from_secs(90);
/// How long the task may then take to end with its result (a hook of the project's
/// may run 100 s of it).
pub const RESULT: Duration = Duration::from_secs(420);
/// How long the stop of a window that may not stop is watched.
pub const STUCK_WATCH: Duration = Duration::from_secs(150);
/// How long a window at rest is watched for a press it should not get.
pub const REST: Duration = Duration::from_secs(12);

/// The long command the `command` case has the member run.
pub const LONG: LongCommand = LongCommand::new("cf-stops-live-long-command");

/// The wall clock, in milliseconds since the epoch, as the status files and the
/// daemon's events are told by.
pub fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// The project of a case, as the case sees it: its member and what the check
/// says while it waits.
pub struct Ctx<'a> {
    pub members: &'a Members,
    pub log: &'a Log,
    pub agent: &'a str,
    /// What the member of a case that has it ask is told to ask its question with
    /// (its harness's own tool, named).
    pub question: &'a str,
    /// How long a window at rest is watched for a press it should not get
    /// ([`REST`] in a live run).
    pub rest: Duration,
}

/// Copies the transcript of each conversation the case opened for its member to
/// `folder`, as `<case's folder>-<session>.jsonl`, before the rig clears them.
fn keep_transcripts(members: &Members, agent: &str, folder: &Path) -> Result<Vec<PathBuf>> {
    let prefix = format!("p{}-{agent}", members.project_id());
    let label = members
        .folder()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut kept = Vec::new();
    let mut sessions: Vec<String> = Vec::new();
    for frame in members.rig().open_frames() {
        let ours = frame["id"]
            .as_str()
            .is_some_and(|id| id.starts_with(&prefix));
        let Some(session) = session_of(&frame).filter(|_| ours) else {
            continue;
        };
        if sessions.contains(&session) {
            continue;
        }
        if let Some(file) = live::claude::transcript_file(&members.claude_config(), &session) {
            let to = folder.join(format!("{label}-{session}.jsonl"));
            files::copy(&file, &to)?;
            kept.push(to);
        }
        sessions.push(session);
    }
    Ok(kept)
}

/// Runs the case on the harness: what it came to.
fn run(case: Case, harness: Harness, options: &Options, log: &Log) -> Result<Account> {
    let log = log.scoped(format!("{} {}", harness.name(), case.name()));
    let plan = plan_of(case, harness, options);
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), &log)?;
    let agent = live::agents::id(harness);
    members.add(&agent, &[])?;
    let mut account = Account::new();
    if let Some(trust) = members.trust() {
        account.say(format!("trust: {trust}"));
    }
    let question = question_brief(Harness::Claude).unwrap_or_default();
    let ctx = Ctx {
        members: &members,
        log: &log,
        agent: &agent,
        question: &question,
        rest: REST,
    };
    let body = match case {
        Case::Command => cases::command(&ctx, &mut account),
        Case::Early => cases::early(&ctx, &mut account),
        Case::Hook => hook::hook_case(&ctx, &mut account),
        Case::Escape => hook::escape_case(&ctx, &mut account),
        Case::SlowHook => slow::run(&ctx, &mut account, &options.hook),
        Case::Stuck => stuck::run(&ctx, &mut account, harness.name()),
        Case::MidTurn => midturn::run(&ctx, &mut account, harness),
        Case::MidTool => midturn::tool::run(&ctx, &mut account, harness, Command::here()),
        Case::MidWsl => midturn::tool::run(&ctx, &mut account, harness, Command::wsl()),
        Case::MidBg => midturn::tool::run(&ctx, &mut account, harness, Command::after_background()),
        Case::Tell => tell::tell(&ctx, &mut account),
    };
    if let Some(folder) = &options.transcripts {
        for file in keep_transcripts(&members, &agent, folder)? {
            log.say(format!("kept {}", file.display()));
        }
    }
    let closed = members.close();
    body?;
    account.closed(closed);
    Ok(account)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a member of the harness each case runs; cargo xtask live stops (npm run live:stops)"]
fn a_stop_the_daemon_presses_is_paid_at_rest_and_never_said_to_be_ignored_where_the_window_stopped(
) -> Result {
    let log = Log::begin("live stops")?;
    let options = options(
        &live::list("case"),
        live::option("hook").as_deref(),
        &live::list("harness"),
        live::option("transcripts"),
    )?;
    let mut checks = Vec::new();
    let mut tally = Tally::default();
    for case in options.cases.clone() {
        for harness in runs_on(case, &options.harnesses) {
            let before = live::version_of(harness);
            let account = run(case, harness, &options, &log).unwrap_or_else(|failed| {
                let mut account = Account::new();
                account.fail(failed);
                account
            });
            let after = live::version_of(harness);
            let version = if after == before {
                before
            } else {
                format!("{before}, then {after}")
            };
            log.block(
                &format!(
                    "== {} {version}, {} (native daemon): {}",
                    harness.name(),
                    case.name(),
                    status(&account)
                ),
                &account.text(),
            );
            tally.add(harness.name(), case.name(), &account);
            checks.push(Check::new(
                harness.name(),
                &if account.is_unmet() && account.held() {
                    format!("{} (unmet)", case.name())
                } else {
                    case.name().to_owned()
                },
                account.held(),
                if account.is_unmet() {
                    format!(
                        "{version}: {}",
                        account
                            .lines()
                            .iter()
                            .find(|line| line.starts_with("UNMET"))
                            .map_or("UNMET", String::as_str)
                    )
                } else {
                    version
                },
            ));
        }
    }
    log.say(tally.summary());
    report::conclude(&log, "what each case came to", &checks)?;
    tally.tried()
}

/// How a case's heading says it came out: `unmet` for a case whose precondition was not met
/// (never `ok`, which says a Stop was tried and held), else `ok` or `FAILED`.
fn status(account: &Account) -> &'static str {
    if !account.held() {
        "FAILED"
    } else if account.is_unmet() {
        "unmet"
    } else {
        "ok"
    }
}

/// How the cases of a run came out, by name: held, failed, and unmet (their member declined
/// the workload, so no Stop was tried: neither a pass nor a failure).
#[derive(Debug, Default)]
struct Tally {
    held: Vec<String>,
    failed: Vec<String>,
    unmet: Vec<String>,
}

impl Tally {
    fn add(&mut self, harness: &str, case: &str, account: &Account) {
        let name = format!("{harness} {case}");
        if account.is_unmet() && account.held() {
            self.unmet.push(name);
        } else if account.held() {
            self.held.push(name);
        } else {
            self.failed.push(name);
        }
    }

    /// The run's own line: the counts, and each unmet case and failed one by name.
    fn summary(&self) -> String {
        let named = |names: &[String]| {
            if names.is_empty() {
                String::new()
            } else {
                format!(" ({})", names.join(", "))
            }
        };
        format!(
            "{} held, {} failed{}, {} unmet{}",
            self.held.len(),
            self.failed.len(),
            named(&self.failed),
            self.unmet.len(),
            named(&self.unmet)
        )
    }

    /// A run in which no case reached its Stop tried nothing, whatever it says of why: it
    /// fails, so that a release's check cannot read green from a run that tested nothing.
    fn tried(&self) -> Result {
        if self.held.is_empty() && self.failed.is_empty() {
            return Err(cf_e2e::Error::Daemon(format!(
                "nothing was tried: no case reached its Stop ({})",
                self.summary()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use cf_e2e::live::members::{find, stand_in_worker, Plan};

    use super::*;
    use crate::resume::{resume_after, Expect};
    use crate::stop::{Shape, Stop};

    /// A stop, and the resume that carries the task on, against the stand-in: it is the
    /// member, in a turn of two minutes, and the daemon is the real one. The stand-in stops
    /// at the Escape the daemon presses, as Claude does, and keeps the status and the
    /// records Claude keeps, so what the case reads of the window is what it reads in a live
    /// run: what this proves is that the case reads what the daemon and the window write.
    #[test]
    fn a_stand_in_paused_in_its_turn_is_a_stop_the_daemon_pays_at_rest_and_the_resume_carries_on() {
        let log = Log::new("live stops test");
        let plan = Plan::new("stops-test", stand_in_chief()).with(stand_in_worker());
        let mut members = Members::open(&plan, Path::new(FAKE_AGENT), &log).unwrap();
        members.add("worker", &[]).unwrap();
        let number = members
            .give("worker", "SLEEP 120 Reply with exactly: STOP_TEST")
            .unwrap();
        // The window is at work: the board says so, and the turn has been going a moment.
        let at_work = find(
            &log,
            "the member's window at work",
            Duration::from_secs(60),
            Duration::from_millis(100),
            || {
                let window = members.window_of("worker")?;
                Ok(window.filter(|window| window.activity["state"] == "working"))
            },
        )
        .unwrap();
        assert!(at_work.is_some());
        std::thread::sleep(Duration::from_secs(2));

        let mut case = Account::new();
        let watched = Stop {
            members: &members,
            log: &log,
            agent: "worker",
            number,
            shape: Shape {
                command: false,
                recordless: false,
            },
            long: &LONG,
            rest: Duration::from_secs(2),
        }
        .run(&mut case)
        .unwrap();
        assert!(case.held(), "{}", case.text());
        // The checks of a stop are all in what it said, with what each was made of.
        let said = case.text();
        for name in [
            "the daemon pressed Escape",
            "the turn stopped",
            "the record says it was interrupted",
            "the daemon read the window at rest, and paid the stop there",
            "nothing is pressed once it has stopped",
            "it is not said to have ignored the stop",
            "the task is paused",
        ] {
            assert!(said.contains(&format!("ok   {name}: ")), "{name}: {said}");
        }
        assert!(said.contains("task.pause made T-1 paused"), "{said}");

        // The resume: its words go into the window, land whole, and the task ends with a result.
        resume_after(
            &members,
            &mut case,
            &log,
            ("worker", number),
            &watched,
            Expect::default(),
        )
        .unwrap();
        assert!(case.held(), "{}", case.text());
        assert!(
            case.text()
                .contains("ok   the task ended with its result: T-1 is "),
            "{}",
            case.text()
        );
        drop(watched);
        members.close().unwrap();
    }

    #[test]
    fn the_heading_of_an_unmet_case_says_unmet_and_never_ok() {
        let mut unmet = Account::new();
        unmet.check("the foreground held", true, "x");
        unmet.unmet("no job at the pause");
        assert_eq!(status(&unmet), "unmet");
        assert_eq!(status(&Account::new()), "ok");
        let mut failed = Account::new();
        failed.unmet("declined");
        failed.fail("and a failure beside it");
        assert_eq!(status(&failed), "FAILED");
    }

    #[test]
    fn a_run_says_its_unmet_cases_by_name_apart_and_fails_where_none_reached_its_stop() {
        let held = Account::new();
        let mut unmet = Account::new();
        unmet.unmet(
            "the workload never began (the model declined): precondition unmet, no Stop was tried",
        );
        let mut failed = Account::new();
        failed.fail("the turn was over before the pause");
        let mut tally = Tally::default();
        tally.add("devin", "midturn", &unmet);
        tally.add("codex", "midtool", &held);
        tally.add("pi", "midbg", &failed);
        assert_eq!(
            tally.summary(),
            "1 held, 1 failed (pi midbg), 1 unmet (devin midturn)"
        );
        tally.tried().unwrap();
        // Nothing reached its Stop: not green.
        let mut none = Tally::default();
        none.add("devin", "midturn", &unmet);
        none.add("pi", "early", &unmet);
        let said = none.tried().unwrap_err().to_string();
        assert!(said.contains("nothing was tried"), "{said}");
        assert!(said.contains("2 unmet (devin midturn, pi early)"), "{said}");
        assert!(Tally::default().tried().is_err());
    }
}
