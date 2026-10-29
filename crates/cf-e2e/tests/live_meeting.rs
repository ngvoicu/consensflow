//! Live: a meeting with a real chief (3.0.7 chief-meeting, CMT-17). It is opt-in (`cargo
//! xtask live meeting`, `npm run live:meeting`): it spends the machine's quota on a chief of
//! each harness named, a few minutes of a cheap model each, and needs each installed and logged
//! in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! A real harness is the chief of a project on the native daemon; two stand-ins are its
//! workers. The human starts a meeting through the page's operation and talks with the chief,
//! a turn and then another, while the first worker's result comes and then the second worker's
//! question. Nothing of either goes into the chief's window while the meeting is on: no
//! `delivery.begun`, and no header of theirs in the chief's record, whatever the chief's
//! turns. The human stops the meeting, and the daemon hands them over one at a time, the
//! question first though it came later, each after the window has rested the settle, each
//! pasted once and on the first attempt, and the chief's record holds each once.
//!
//! ```text
//! cargo xtask live meeting                       Claude, the default
//! cargo xtask live meeting claude codex pi       each, one after the other
//! ```

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::members::{member_as, stand_in_worker, Members, Plan, STAND_IN_WORKER};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

#[path = "live_pacing/evidence.rs"]
mod evidence;

use evidence::{
    chiefs, delivered_once, in_order, in_record, message_of, of, record_of, says, wait_for,
};

/// The stand-in, the workers' harness.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The second worker.
const SECOND: &str = "worker-b";

/// What the folder tells the chief.
const BRIEF: &str = "This folder is an automated ConsensFlow test. When a ConsensFlow message arrives, reply with exactly one line, SEEN, and run no tools: decide nothing, answer no question with cf, accept nothing. When the human asks you something in this window, do what it asks.\n";

/// How long the meeting holds what came, after the last of it came and the chief rested.
const HELD: Duration = Duration::from_secs(20);

/// The project: a real `harness` is the chief, and two stand-ins its workers.
fn plan(harness: Harness) -> Plan {
    let mut second = stand_in_worker();
    second["id"] = json!(SECOND);
    Plan::new(
        &format!("meeting-{}", harness.name()),
        member_as(harness, "chief"),
    )
    .with(stand_in_worker())
    .with(second)
    .told(BRIEF)
}

/// The human starts (`on`) or stops the meeting, as the page does.
fn meeting(members: &Members, on: bool) -> Result<Value> {
    let said = members.page("project.meeting", json!({ "on": on }))?;
    if said["ok"] != true {
        return Err(Error::Daemon(format!("project.meeting {on}: {said}")));
    }
    Ok(said)
}

/// One chief's run.
fn run(log: &Log, harness: Harness) -> Result<Vec<Check>> {
    let who = format!("{} chief", harness.name());
    let mut members = Members::open(&plan(harness), Path::new(FAKE_AGENT), log)?;
    members.add(STAND_IN_WORKER, &[])?;
    members.add(SECOND, &[])?;
    if !members.chief_is_idle(log)? {
        let checks = vec![Check::failed(
            &who,
            "the chief's window goes idle",
            members.chief_window(),
        )];
        members.close()?;
        return Ok(checks);
    }
    let started = meeting(&members, true)?;
    let mut checks = vec![Check::new(
        &who,
        "the meeting is on",
        started["project"]["meeting"].is_string(),
        started["project"].to_string(),
    )];
    // The human talks with the chief while the workers' words come.
    says(
        &members,
        "We are in a meeting. Reply with exactly one line: LISTENING",
    )?;
    members.give(STAND_IN_WORKER, "Reply with exactly: ONE_DONE")?;
    wait_for(
        &members,
        log,
        "the result to queue",
        "result",
        "ONE_DONE",
        &["queued", "delivering", "delivered"],
    )?;
    members.give(
        SECOND,
        "CF ask :: Which output format do you want, JSON or text?\nReply with exactly: ASKED",
    )?;
    wait_for(
        &members,
        log,
        "the question to queue",
        "question",
        "output format",
        &["queued", "delivering", "delivered"],
    )?;
    members.chief_is_idle(log)?;
    says(
        &members,
        "Still in the meeting. Reply with exactly one line: STILL",
    )?;
    std::thread::sleep(HELD);
    members.chief_is_idle(log)?;
    let events = members.events();
    let items = record_of(&members, harness)?;
    for (what, kind, words) in [
        ("the result", "result", "ONE_DONE"),
        ("the question", "question", "output format"),
    ] {
        let held = message_of(&members, kind, words)?;
        let id = held
            .as_ref()
            .and_then(|held| held["id"].as_i64())
            .unwrap_or_default();
        checks.push(Check::new(
            &who,
            &format!("{what} waits, queued, for the meeting's end"),
            held.as_ref().is_some_and(|held| held["state"] == "queued")
                && of(&events, "delivery.begun", id).is_empty()
                && in_record(&items, id) == 0,
            format!(
                "{:?}; {} begun; {} item(s) of the record hold it",
                held.map(|held| held["state"].clone()),
                of(&events, "delivery.begun", id).len(),
                in_record(&items, id)
            ),
        ));
    }
    let listed = members.board()?["held"].as_array().map_or(0, Vec::len);
    checks.push(Check::new(
        &who,
        "the board lists both as held",
        listed == 2,
        format!("{listed} held"),
    ));

    meeting(&members, false)?;
    let question = wait_for(
        &members,
        log,
        "the question to go in",
        "question",
        "output format",
        &["delivered", "failed"],
    )?;
    let result = wait_for(
        &members,
        log,
        "the result to go in",
        "result",
        "ONE_DONE",
        &["delivered", "failed"],
    )?;
    members.chief_is_idle(log)?;
    let events = members.events();
    let items = record_of(&members, harness)?;
    match (&question, &result) {
        (Some(question), Some(result)) => {
            checks.extend(delivered_once(
                &who,
                "the question",
                question,
                &events,
                &items,
            ));
            checks.extend(delivered_once(&who, "the result", result, &events, &items));
            checks.extend(in_order(&who, question, result, &events));
        }
        _ => checks.push(Check::failed(
            &who,
            "the question and the result settled after the Stop",
            format!("question {question:?}, result {result:?}"),
        )),
    }
    members.close()?;
    Ok(checks)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of each harness named; cargo xtask live meeting"]
fn a_real_chief_in_a_meeting_is_given_nothing_and_after_the_stop_one_message_at_a_time() -> Result {
    let log = Log::begin("live meeting")?;
    ask::prepare()?;
    let mut checks = Vec::new();
    for harness in chiefs(&live::list("harness"))? {
        log.found(harness)?;
        checks.extend(
            run(&log, harness)
                .map_err(|error| Error::Daemon(format!("{}: {error}", harness.name())))?,
        );
    }
    report::conclude(&log, "what each check came to", &checks)
}
