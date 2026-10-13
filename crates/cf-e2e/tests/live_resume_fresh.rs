//! Live: a project whose chief never had a turn, closed and resumed, on every
//! harness. It is opt-in (`cargo xtask live resume-fresh`, `npm run
//! live:resume-fresh`): it spends the machine's quota on a chief of each harness
//! named, and needs each one installed and logged in, so it is ignored unless
//! asked for and is no part of `check` or of CI.
//!
//! Found live on 2026-10-13 with a Codex chief: the project opened and the chief's
//! window came up idle; the human closed the project and pressed Resume, which
//! went open and, 0.6 s later, suspended again, and no window ever showed. Codex
//! writes a thread's record only once the thread has a turn, so `codex resume
//! <thread>` found none and ended at once, and a chief's own exit closes its
//! project. A conversation that never had a turn is not resumed now, whatever
//! its harness keeps of it: the window opens afresh, as a first launch does.
//!
//! Each harness is the chief of a project of its own, one after another, on the
//! native daemon and the real pane host. The chief's window is waited for to read
//! idle, nothing is said to it, and the project is closed and resumed. What is
//! judged is what the product showed ([`verdict`]): that the chief's window
//! opened again, that the project stayed open, and that a message the human
//! typed into the window then was answered. What is said besides, and judged of
//! no harness, is what the harness's record held before the Close and whether
//! the window opened on a conversation of its own.
//!
//! ```text
//! cargo xtask live resume-fresh                    every harness as the chief, one after the other
//! cargo xtask live resume-fresh codex claude       the ones named
//! cargo xtask live resume-fresh -- --list          the check, listed
//! ```
//!
//! The run fails when a chief's window did not open again, when the project did
//! not stay open, or when what the human typed was not answered.

// The verdict is in `tests/live_resume_fresh/`: cargo looks for the modules of a
// test's root file beside it, where each would be a suite of its own.
#[path = "live_resume_fresh/verdict.rs"]
mod verdict;

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{find, member_as, named, Account, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::trace::Trace;
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::Result;
use serde_json::{json, Value};

use verdict::{verdict, Seen};

/// The stand-in for the window of the project's members: the project has none, and
/// its chief is a real harness, but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// How long the chief's window has to close once the project is closed, to be
/// opened again once it is resumed, to read idle, and to answer.
const CLOSED: Duration = Duration::from_secs(60);
const OPENED: Duration = Duration::from_secs(120);
const IDLE: Duration = Duration::from_secs(180);
const ANSWERED: Duration = Duration::from_secs(180);

/// How long the project is watched after the chief reads idle: the window that
/// came back to nothing ended 0.6 s after the Resume.
const WATCH: Duration = Duration::from_secs(5);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// What the check opens: a project whose chief is the real `chief`, on its harness's
/// cheap model, and whose staff is empty.
fn plan(chief: Harness) -> Plan {
    Plan::new("resume-fresh", member_as(chief, "chief"))
}

/// What the human types into the chief's window: a word to answer with, which no
/// harness's own screen holds.
fn the_ask(token: &str) -> String {
    format!("Reply with exactly one line: {token}")
}

/// The word the human asks `harness`'s chief to answer with.
fn token_of(harness: Harness) -> String {
    format!("FRESH_{}", harness.name().to_ascii_uppercase())
}

/// The chief's windows the daemon opened so far, oldest first.
fn windows_of(members: &Members) -> Vec<Value> {
    let id = format!("p{}-chief", members.project_id());
    members
        .rig()
        .open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == id.as_str())
        .collect()
}

/// What the chief's lane reads, or `no lane`.
fn state_of(members: &Members) -> Result<String> {
    Ok(members.lane("chief")?["activity"]["state"]
        .as_str()
        .unwrap_or("no lane")
        .to_owned())
}

/// The record the harness keeps of `thread`, as the daemon reads it.
fn record_of(members: &Members, harness: Harness, thread: &str) -> Option<ask::Record> {
    ask::records(harness.kind(), thread, members.rig().environment())
        .ok()
        .flatten()
}

/// The line of the record of `thread` in which the chief says `token`, once it does.
fn answered(members: &Members, harness: Harness, thread: &str, token: &str) -> Option<String> {
    record_of(members, harness, thread)?
        .items
        .into_iter()
        .find(|item| item.role == "assistant" && item.text.contains(token))
        .map(|item| item.text)
}

/// `harness` as the chief: the project open and the chief's window idle, nothing
/// said to it, the project closed and resumed, and a word typed to the chief.
fn drive(members: Members, harness: Harness, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    case.say(format!("trust: {}", members.trust().unwrap_or("none")));
    let first = Trace::read(members.rig()).threads_bound().first().cloned();
    let held = first
        .as_deref()
        .and_then(|thread| record_of(&members, harness, thread))
        .map(|record| record.items.len());

    // The human's Close: the project is suspended and the chief's window goes.
    let closed = members.page("project.close", json!({}))?;
    if closed["ok"] != true {
        case.fail(format!("project.close was refused: {closed}"));
        case.closed(members.close());
        return Ok(case);
    }
    let gone = find(log, "the chief's window to close", CLOSED, LOOK, || {
        Ok(members.lane("chief")?["pane"].is_null().then_some(()))
    })?;
    if gone.is_none() {
        case.fail(format!(
            "the chief's window did not close in {} s: {}",
            CLOSED.as_secs(),
            members.lane("chief")?["activity"]
        ));
        case.closed(members.close());
        return Ok(case);
    }

    // The human's Resume: the window opens again, and is watched a while.
    members.resume()?;
    let resumed = Instant::now();
    let opened = find(
        log,
        "the chief's window to be opened again",
        OPENED,
        LOOK,
        || Ok((windows_of(&members).len() >= 2).then_some(())),
    )?;
    let idle = find(log, "the chief's window to read idle", IDLE, LOOK, || {
        Ok((state_of(&members)? == "idle").then_some(()))
    })?
    .map(|()| i64::try_from(resumed.elapsed().as_millis()).unwrap_or(i64::MAX));
    sleep(WATCH);
    let windows = windows_of(&members);
    let lane = members.lane("chief")?;
    let ended =
        windows.get(1).is_some_and(|window| {
            members.rig().exits().iter().any(|exit| {
                exit["id"] == window["id"] && exit["generation"] == window["generation"]
            })
        });
    let open = members.is_open()?;
    let threads = Trace::read(members.rig()).threads_bound();
    let screen = members.tail(lane["pane"]["id"].as_str().unwrap_or_default());

    // What the human types into the window that came back.
    let token = token_of(harness);
    let mut answer = None;
    if opened.is_some() && open && !ended && idle.is_some() {
        members.paste(&lane["pane"], &ask::window_text(&the_ask(&token))?)?;
        answer = match threads.last() {
            Some(thread) => find(log, "the chief to answer", ANSWERED, LOOK, || {
                Ok(answered(&members, harness, thread, &token))
            })?,
            None => None,
        };
    }
    verdict(
        &Seen {
            windows: windows.len(),
            ended,
            open,
            state: lane["activity"]["state"]
                .as_str()
                .unwrap_or("no lane")
                .to_owned(),
            threads,
            held,
            token,
            answer,
            idle_after_ms: idle,
            screen,
        },
        &mut case,
    );
    case.closed(members.close());
    Ok(case)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of each harness named; cargo xtask live resume-fresh (npm run live:resume-fresh)"]
fn a_chief_that_never_had_a_turn_comes_back_on_the_humans_resume_and_answers() -> Result {
    let log = Log::new("live resume-fresh");
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let chiefs = named(
        &live::list("harness"),
        &Harness::ALL,
        Harness::name,
        "harness",
        &Harness::ALL,
    )?;
    let mut checks = Vec::new();
    for chief in chiefs {
        let scoped = log.scoped(format!("{} chief", chief.name()));
        let case = Members::open(&plan(chief), Path::new(FAKE_AGENT), &scoped)
            .and_then(|members| drive(members, chief, &scoped))
            .unwrap_or_else(|failed| {
                let mut case = Account::new();
                case.fail(failed);
                case
            });
        let version = live::version_of(chief);
        log.block(
            &format!("== {} chief: {version}", chief.name()),
            &case.text(),
        );
        checks.push(Check::new(
            &format!("{} chief", chief.name()),
            "a chief that never had a turn comes back on Resume, and answers",
            case.held(),
            version,
        ));
    }
    report::conclude(&log, "what each case came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_harness_is_asked_for_a_word_of_its_own_in_one_line() {
        let tokens: Vec<String> = Harness::ALL.into_iter().map(token_of).collect();
        assert_eq!(
            tokens,
            [
                "FRESH_CLAUDE",
                "FRESH_OPENCODE",
                "FRESH_PI",
                "FRESH_DEVIN",
                "FRESH_CODEX"
            ]
        );
        assert_eq!(the_ask("FRESH_PI"), "Reply with exactly one line: FRESH_PI");
    }

    #[test]
    fn the_chief_is_a_real_harness_on_its_cheap_model_with_no_staff_and_every_harness_is_run_by_default(
    ) {
        for harness in Harness::ALL {
            let plan = plan(harness);
            assert_eq!(plan.folder, "resume-fresh");
            assert_eq!(plan.chief, "chief");
            let (cheap, _) = live::agents::cheap(harness);
            assert_eq!(plan.roster.len(), 1, "{harness:?}");
            assert_eq!(plan.roster[0]["model"], cheap, "{harness:?}");
            assert_eq!(plan.roster[0]["kind"], harness.kind());
            assert_eq!(plan.harnesses(), [harness]);
        }
        let all = named(&[], &Harness::ALL, Harness::name, "harness", &Harness::ALL).unwrap();
        assert_eq!(all, Harness::ALL);
        let failed = named(
            &["kimi".to_owned()],
            &Harness::ALL,
            Harness::name,
            "harness",
            &Harness::ALL,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(failed, "no such harness: kimi");
    }
}
