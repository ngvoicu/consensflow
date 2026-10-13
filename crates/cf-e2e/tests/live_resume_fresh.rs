//! Live: a project whose chief is closed and resumed, on every harness. It is
//! opt-in (`cargo xtask live resume-fresh`, `npm run live:resume-fresh`): it spends
//! the machine's quota on a chief of each harness named, twice, and needs each one
//! installed and logged in, so it is ignored unless asked for and is no part of
//! `check` or of CI.
//!
//! Found live on 2026-10-13 with a Codex chief: the project opened and the chief's
//! window came up idle; the human closed the project and pressed Resume, which
//! went open and, 0.6 s later, suspended again, and no window ever showed. Codex
//! writes a thread's record only once the thread has a turn, so `codex resume
//! <thread>` found none and ended at once, and a chief's own exit closes its
//! project. Whether a window is opened again on its conversation is now what its
//! harness's own record says of it: a conversation the harness holds nothing of is
//! opened afresh, as a first launch does, and one it holds is resumed, whether or
//! not the daemon, which looks at a window once a second, has copied the turn the
//! human typed before the Close.
//!
//! Each harness is the chief of a project of its own, one case after another, on
//! the native daemon and the real pane host, the chief's window waited for to read
//! idle. In the first nothing is said to it, and the project is closed and
//! resumed; what is judged is what the product showed ([`verdict`]): that the
//! chief's window opened again, that the project stayed open, and that a message
//! the human typed into the window then was answered. What is said besides, and
//! judged of no harness, is what the harness's record held before the Close and
//! whether the window opened on a conversation of its own. In the second a turn
//! is typed into the window that gives it a key, and the project is closed as soon
//! as the harness's record has the turn and its answer: what is judged
//! ([`continued`]) is that the window opened again on the conversation the turn was
//! saved in, that the project stayed open, and that the chief gives the key back
//! when it is asked what it was.
//!
//! ```text
//! cargo xtask live resume-fresh                    every harness as the chief, one after the other
//! cargo xtask live resume-fresh codex claude       the ones named
//! cargo xtask live resume-fresh -- --list          the check, listed
//! ```
//!
//! The run fails when a chief's window did not open again, when the project did
//! not stay open, when what the human typed was not answered, or when a chief
//! closed on after a typed turn was opened on another conversation than the turn's
//! or does not remember it.

// The verdict, and what is typed, are in `tests/live_resume_fresh/`: cargo looks for
// the modules of a test's root file beside it, where each would be a suite of its own.
#[path = "live_resume_fresh/typed.rs"]
mod typed;
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

use typed::{key_of, remembers, saved_turn, the_key, ASKED};
use verdict::{continued, verdict, Continued, Seen};

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

/// What came of the human's Close of the project and the Resume that followed, the
/// window opened again watched a while.
struct Came {
    /// The chief's windows the daemon opened.
    windows: Vec<Value>,
    /// The chief's lane at the end.
    lane: Value,
    /// Whether the window opened again had ended.
    ended: bool,
    /// Whether the project was open at the end.
    open: bool,
    /// The conversations the daemon bound to the chief's windows, oldest first.
    threads: Vec<String>,
    /// What the chief's screen showed at the end.
    screen: String,
    /// Whether a second window was opened.
    opened: bool,
    /// How long after the Resume the window read idle, in milliseconds.
    idle_after_ms: Option<i64>,
}

impl Came {
    /// Whether the window that came back is there to be typed to.
    fn is_there(&self) -> bool {
        self.opened && self.open && !self.ended && self.idle_after_ms.is_some()
    }
}

/// The human's Close of the project, the chief's window gone, and the Resume that
/// brings it back: none where the Close was refused or the window never went, which
/// the case says.
fn closed_and_resumed(members: &Members, case: &mut Account, log: &Log) -> Result<Option<Came>> {
    // The human's Close: the project is suspended and the chief's window goes.
    let closed = members.page("project.close", json!({}))?;
    if closed["ok"] != true {
        case.fail(format!("project.close was refused: {closed}"));
        return Ok(None);
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
        return Ok(None);
    }

    // The human's Resume: the window opens again, and is watched a while.
    members.resume()?;
    let resumed = Instant::now();
    let opened = find(
        log,
        "the chief's window to be opened again",
        OPENED,
        LOOK,
        || Ok((windows_of(members).len() >= 2).then_some(())),
    )?;
    let idle = find(log, "the chief's window to read idle", IDLE, LOOK, || {
        Ok((state_of(members)? == "idle").then_some(()))
    })?
    .map(|()| i64::try_from(resumed.elapsed().as_millis()).unwrap_or(i64::MAX));
    sleep(WATCH);
    let windows = windows_of(members);
    let lane = members.lane("chief")?;
    let ended =
        windows.get(1).is_some_and(|window| {
            members.rig().exits().iter().any(|exit| {
                exit["id"] == window["id"] && exit["generation"] == window["generation"]
            })
        });
    Ok(Some(Came {
        screen: members.tail(lane["pane"]["id"].as_str().unwrap_or_default()),
        open: members.is_open()?,
        threads: Trace::read(members.rig()).threads_bound(),
        opened: opened.is_some(),
        idle_after_ms: idle,
        windows,
        lane,
        ended,
    }))
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

    let Some(came) = closed_and_resumed(&members, &mut case, log)? else {
        case.closed(members.close());
        return Ok(case);
    };

    // What the human types into the window that came back.
    let token = token_of(harness);
    let mut answer = None;
    if came.is_there() {
        members.paste(&came.lane["pane"], &ask::window_text(&the_ask(&token))?)?;
        answer = match came.threads.last() {
            Some(thread) => find(log, "the chief to answer", ANSWERED, LOOK, || {
                Ok(answered(&members, harness, thread, &token))
            })?,
            None => None,
        };
    }
    verdict(
        &Seen {
            windows: came.windows.len(),
            ended: came.ended,
            open: came.open,
            state: state_in(&came.lane),
            threads: came.threads,
            held,
            token,
            answer,
            idle_after_ms: came.idle_after_ms,
            screen: came.screen,
        },
        &mut case,
    );
    case.closed(members.close());
    Ok(case)
}

/// `harness` as the chief: the project open and the chief's window idle, a turn typed
/// into it and saved by the harness, the project closed on at once and resumed, and
/// the chief asked for what the turn told it. The daemon looks at a window once a
/// second, so a turn may be in the harness's record and in no copy of the daemon's
/// when the Close comes; the window must be opened on its conversation all the same.
fn drive_typed(members: Members, harness: Harness, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    case.say(format!("trust: {}", members.trust().unwrap_or("none")));
    let Some(thread) = Trace::read(members.rig()).threads_bound().first().cloned() else {
        case.fail("the daemon bound no conversation to the chief's window");
        case.closed(members.close());
        return Ok(case);
    };
    let key = key_of(harness);

    // What the human types into the window, and the harness saves of it.
    let lane = members.lane("chief")?;
    members.paste(&lane["pane"], &ask::window_text(&the_key(&key))?)?;
    let saved = find(log, "the harness to save the turn", ANSWERED, LOOK, || {
        Ok(record_of(&members, harness, &thread)
            .is_some_and(|record| saved_turn(&record, &key))
            .then_some(()))
    })?;
    if saved.is_none() {
        case.fail(format!(
            "the harness saved no turn that gave {key} in {} s: {}",
            ANSWERED.as_secs(),
            members.tail(lane["pane"]["id"].as_str().unwrap_or_default())
        ));
        case.closed(members.close());
        return Ok(case);
    }

    // The human closes the project at once, with no wait for a look of the daemon's.
    let Some(came) = closed_and_resumed(&members, &mut case, log)? else {
        case.closed(members.close());
        return Ok(case);
    };

    // What the human asks the window that came back: what the turn told it.
    let mut remembered = None;
    if came.is_there() {
        members.paste(&came.lane["pane"], &ask::window_text(ASKED)?)?;
        remembered = find(
            log,
            "the chief to give the key back",
            ANSWERED,
            LOOK,
            || Ok(record_of(&members, harness, &thread).and_then(|record| remembers(&record, &key))),
        )?;
    }
    let argv = came.windows.get(1).map_or_else(String::new, |window| {
        window["argv"]
            .as_array()
            .map(|argv| argv.iter().map(live::text).collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    });
    continued(
        &Continued {
            windows: came.windows.len(),
            ended: came.ended,
            open: came.open,
            state: state_in(&came.lane),
            threads: came.threads,
            argv,
            key,
            remembered,
            screen: came.screen,
        },
        &mut case,
    );
    case.closed(members.close());
    Ok(case)
}

/// What the chief's lane reads, as `lane` has it.
fn state_in(lane: &Value) -> String {
    lane["activity"]["state"]
        .as_str()
        .unwrap_or("no lane")
        .to_owned()
}

/// What a case does with one harness's chief: a case, as the harness is in it.
type Case = fn(Members, Harness, &Log) -> Result<Account>;

/// The cases, and what each holds of a chief.
const CASES: [(&str, Case); 2] = [
    (
        "a chief that never had a turn comes back on Resume, and answers",
        drive,
    ),
    (
        "a chief closed on right after a typed turn is resumed on its conversation, and remembers it",
        drive_typed,
    ),
];

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of each harness named; cargo xtask live resume-fresh (npm run live:resume-fresh)"]
fn a_chief_comes_back_on_the_humans_resume_afresh_if_it_never_had_a_turn_and_on_its_conversation_if_it_did(
) -> Result {
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
        let version = live::version_of(chief);
        for (what, case) in CASES {
            let account = Members::open(&plan(chief), Path::new(FAKE_AGENT), &scoped)
                .and_then(|members| case(members, chief, &scoped))
                .unwrap_or_else(|failed| {
                    let mut account = Account::new();
                    account.fail(failed);
                    account
                });
            log.block(
                &format!("== {} chief, {what}: {version}", chief.name()),
                &account.text(),
            );
            checks.push(Check::new(
                &format!("{} chief", chief.name()),
                what,
                account.held(),
                version.clone(),
            ));
        }
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
