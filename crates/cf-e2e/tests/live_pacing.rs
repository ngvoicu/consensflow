//! Live: paced delivery to a real chief (3.0.7 chief-meeting, CMT-09). It is opt-in (`cargo
//! xtask live pacing`, `npm run live:pacing`): it spends the machine's quota on a chief of each
//! harness named, a few minutes of a cheap model each, and needs each installed and logged in,
//! so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! A real harness is the chief of a project on the native daemon, at its own settle (three
//! seconds: no variable shortens it); two stand-ins are its workers. The check puts the chief in
//! a turn of a minute (a command it runs), and while it works the first worker's result comes,
//! then the second worker's question. Once the turn has ended, the daemon is to hand them over
//! one at a time, the question first though it came later, each after the window has rested
//! the settle since its last turn, each pasted once and on the first attempt, and the chief's
//! own record to hold each once.
//!
//! On Claude Code and Devin, the harnesses a message is pasted into, the human's typing is the
//! second case: words typed in the chief's window and not sent hold a third result; once they
//! are erased it goes in, the human's keys quiet the settle by then. And words the human sends
//! (typed, then Enter) while a fourth waits begin a turn of the chief's: the result goes in
//! after that turn, once, on its first attempt.
//!
//! What each chief is held to is read from the evidence: the daemon's event file (each
//! `delivery.begun` with the look that let it go, `delivery.confirmed`, `delivery.retried`),
//! the board, and the chief's record as the daemon reads it.
//!
//! ```text
//! cargo xtask live pacing                       Claude, the default
//! cargo xtask live pacing claude codex pi devin each, one after the other
//! cargo xtask live pacing -- --list             lists this test
//! ```

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::ask;
use cf_e2e::live::members::{find, member_as, stand_in_worker, Members, Plan, STAND_IN_WORKER};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, Harness, Log};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// The stand-in, the workers' harness.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The second worker.
const SECOND: &str = "worker-b";

/// The settle the daemon waits, its own default (`Limits::default`).
const SETTLE_MS: i64 = 3_000;

/// What the folder tells the chief.
const BRIEF: &str = "This folder is an automated ConsensFlow test. When a ConsensFlow message arrives, reply with exactly one line, SEEN, and run no tools: decide nothing, answer no question with cf, accept nothing. When the human asks you something in this window, do what it asks.\n";

/// The turn the chief is put in while the workers' messages come: a command of a minute.
const WORK: &str = "Use your shell tool to run exactly this command in the foreground, and wait for it to finish: for i in 1 2 3 4 5 6 7 8 9 10 11 12; do echo tick $i; sleep 5; done . Then reply with exactly one line: WAITED.";

/// How long a message has to go in, and how often the board is looked at.
const WITHIN: Duration = Duration::from_secs(360);
const LOOK: Duration = Duration::from_secs(1);

/// How long words typed and not sent are held to hold the message.
const HELD: Duration = Duration::from_secs(15);

/// The project: a real `harness` is the chief, and two stand-ins its workers.
fn plan(harness: Harness) -> Plan {
    let mut second = stand_in_worker();
    second["id"] = json!(SECOND);
    Plan::new(
        &format!("pacing-{}", harness.name()),
        member_as(harness, "chief"),
    )
    .with(stand_in_worker())
    .with(second)
    .told(BRIEF)
}

/// The chief's pane, as the pane host takes it: its newest generation.
fn chief_pane(members: &Members) -> Value {
    let id = format!("p{}-chief", members.project_id());
    let generation = members
        .rig()
        .open_frames()
        .into_iter()
        .rev()
        .find(|frame| frame["id"] == id.as_str())
        .and_then(|frame| frame["generation"].as_u64())
        .unwrap_or(1);
    json!({ "id": id, "generation": generation })
}

/// The chief's message of `kind` whose body holds `words`.
fn message_of(members: &Members, kind: &str, words: &str) -> Result<Option<Value>> {
    Ok(members.inbox(Some("chief"))?.into_iter().find(|message| {
        message["kind"] == kind
            && message["body"]
                .as_str()
                .is_some_and(|body| body.contains(words))
    }))
}

/// Waits for the chief's message of `kind` whose body holds `words` to be in `states`.
fn wait_for(
    members: &Members,
    log: &Log,
    what: &str,
    kind: &str,
    words: &str,
    states: &[&str],
) -> Result<Option<Value>> {
    let found = find(log, what, WITHIN, LOOK, || {
        Ok(message_of(members, kind, words)?
            .filter(|message| states.iter().any(|state| message["state"] == *state)))
    })?;
    if found.is_none() {
        // What the chief's inbox held instead, for the failure to say.
        let held: Vec<String> = members
            .inbox(Some("chief"))?
            .iter()
            .map(|message| {
                format!(
                    "m-{} {} {} from {}: {}",
                    message["id"],
                    message["kind"],
                    message["state"],
                    message["sender"],
                    live::cut(message["body"].as_str().unwrap_or_default(), 40)
                )
            })
            .collect();
        log.say(format!(
            "{what}: never; the chief's inbox: {held:?}; board: {}",
            members
                .board()
                .map(|board| live::cut(&board.to_string(), 600))
                .unwrap_or_default()
        ));
    }
    Ok(found)
}

/// The milliseconds of an event's `at`.
fn at_ms(event: &Value) -> Option<i64> {
    event["at"]
        .as_str()?
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|at| at.as_millisecond())
}

/// The daemon's events of `kind` for the message `id`.
fn of<'a>(events: &'a [Value], kind: &str, id: i64) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
        .collect()
}

/// How many of the chief's user items hold the header of the message `id`.
fn in_record(items: &[ask::Item], id: i64) -> usize {
    let marker = format!("[ConsensFlow m-{id} ");
    items
        .iter()
        .filter(|item| item.role == "user" && item.text.contains(&marker))
        .count()
}

/// The checks of one message `id` the chief was sent, named `what`, against the daemon's
/// events and the chief's record: delivered, pasted once, on its first attempt, in the record
/// once, and let go by a look that read the window at rest the whole settle.
fn delivered_once(
    who: &str,
    what: &str,
    message: &Value,
    events: &[Value],
    items: &[ask::Item],
) -> Vec<Check> {
    let id = message["id"].as_i64().unwrap_or_default();
    let begun = of(events, "delivery.begun", id);
    let retries = of(events, "delivery.retried", id);
    let retried = retries.len();
    let rested = begun
        .first()
        .and_then(|begun| begun["data"]["look"]["restMs"].as_i64());
    vec![
        Check::new(
            who,
            &format!("{what} is delivered on its first attempt"),
            message["state"] == "delivered" && message["attempts"] == 1,
            format!(
                "m-{id} is {}, after {} attempt(s): {}",
                message["state"], message["attempts"], message["reason"]
            ),
        ),
        Check::new(
            who,
            &format!("{what} is pasted once"),
            begun.len() == 1 && retried == 0,
            format!(
                "{} begun, {retried} retried{}",
                begun.len(),
                retries
                    .iter()
                    .map(|retry| format!("; retried: {}", retry["data"]))
                    .collect::<String>()
            ),
        ),
        Check::new(
            who,
            &format!("{what} is in the chief's record once"),
            in_record(items, id) == 1,
            format!("{} user item(s) hold its header", in_record(items, id)),
        ),
        Check::new(
            who,
            &format!("{what} went in after the window rested the settle"),
            rested.is_some_and(|rested| rested >= SETTLE_MS),
            format!(
                "the look that let it go: {}",
                begun.first().map_or_else(
                    || "none".to_owned(),
                    |begun| begun["data"]["look"].to_string()
                )
            ),
        ),
    ]
}

/// The checks of the two messages that waited behind the chief's turn: the question first,
/// though it came later, and the second a settle after the first was confirmed.
fn in_order(who: &str, question: &Value, result: &Value, events: &[Value]) -> Vec<Check> {
    let id = |message: &Value| message["id"].as_i64().unwrap_or_default();
    let begun_at = |message: &Value| {
        of(events, "delivery.begun", id(message))
            .first()
            .and_then(|event| at_ms(event))
    };
    let confirmed_at = |message: &Value| {
        of(events, "delivery.confirmed", id(message))
            .first()
            .and_then(|event| at_ms(event))
    };
    let (asked, told) = (begun_at(question), begun_at(result));
    let gap = confirmed_at(question)
        .zip(told)
        .map(|(confirmed, told)| told - confirmed);
    vec![
        Check::new(
            who,
            "the question went in first, though it came after the result",
            id(question) > id(result) && asked.zip(told).is_some_and(|(asked, told)| asked < told),
            format!(
                "question m-{} begun at {asked:?}, result m-{} begun at {told:?}",
                id(question),
                id(result)
            ),
        ),
        Check::new(
            who,
            "the result went in a settle or more after the question was confirmed",
            gap.is_some_and(|gap| gap >= SETTLE_MS),
            format!("{gap:?} ms between them"),
        ),
    ]
}

/// The native session of the conversation `handle` has newest, as the daemon's events say it
/// was bound: a harness given no session id on its command line (Codex, Devin) is known by it
/// alone.
fn session_in(events: &[Value], handle: &str) -> Option<String> {
    let started = events.iter().rev().find(|event| {
        event["kind"] == "conversation.started" && event["data"]["participant"] == handle
    })?;
    let conversation = &started["data"]["conversation"];
    let bound = events.iter().rev().find(|event| {
        event["kind"] == "conversation.bound" && event["data"]["conversation"] == *conversation
    })?;
    bound["data"]["nativeSession"].as_str().map(str::to_owned)
}

/// The chief's record, as the daemon reads it.
fn record_of(members: &Members, harness: Harness) -> Result<Vec<ask::Item>> {
    let pane = format!("p{}-chief", members.project_id());
    let session = members
        .rig()
        .open_frames()
        .into_iter()
        .rev()
        .find(|frame| frame["id"] == pane.as_str())
        .and_then(|frame| cf_e2e::live::members::session_of(&frame))
        .or_else(|| session_in(&members.events(), "chief"));
    let Some(session) = session else {
        return Ok(Vec::new());
    };
    Ok(
        ask::records(harness.kind(), &session, members.rig().environment())?
            .map(|record| record.items)
            .unwrap_or_default(),
    )
}

/// The human types `words` into the chief's window and sends them: Enter is a key of its
/// own, pressed a moment after the words, as a person presses it (a line and its Enter in
/// one burst is a paste to Devin, which keeps the line in its box).
fn says(members: &Members, words: &str) -> Result<()> {
    let pane = chief_pane(members);
    members.rig().types(&pane, words.as_bytes())?;
    std::thread::sleep(Duration::from_millis(700));
    members.rig().types(&pane, b"\r")
}

/// The human's typing (Claude Code and Devin): words typed and not sent hold the next result;
/// once erased, it goes in a settle after the last key. Then words the human sends while
/// another waits begin a turn, and the result goes in after it, once.
fn typing(members: &Members, log: &Log, who: &str, harness: Harness) -> Result<Vec<Check>> {
    let pane = chief_pane(members);
    let words = "half a thought";
    members.rig().types(&pane, words.as_bytes())?;
    members.give(STAND_IN_WORKER, "Reply with exactly: TWO_DONE")?;
    let waiting = wait_for(
        members,
        log,
        "the third result to queue",
        "result",
        "TWO_DONE",
        &["queued", "delivering", "delivered"],
    )?;
    std::thread::sleep(HELD);
    let held = message_of(members, "result", "TWO_DONE")?;
    let still = held
        .as_ref()
        .is_some_and(|message| message["state"] == "queued");
    members
        .rig()
        .types(&pane, &vec![0x7f; words.chars().count()])?;
    let third = wait_for(
        members,
        log,
        "the third result to go in",
        "result",
        "TWO_DONE",
        &["delivered", "failed"],
    )?;
    let mut checks = vec![Check::new(
        who,
        "words typed and not sent hold the result",
        waiting.is_some() && still,
        format!(
            "after {} s: {:?}",
            HELD.as_secs(),
            held.map(|held| held["state"].clone())
        ),
    )];
    let events = members.events();
    let items = record_of(members, harness)?;
    if let Some(third) = &third {
        checks.extend(delivered_once(
            who,
            "the result held for the typing",
            third,
            &events,
            &items,
        ));
        let quiet = of(
            &events,
            "delivery.begun",
            third["id"].as_i64().unwrap_or_default(),
        )
        .first()
        .and_then(|begun| begun["data"]["look"]["typedQuietMs"].as_i64());
        checks.push(Check::new(
            who,
            "it went in once the human's keys had been quiet the settle",
            quiet.is_some_and(|quiet| quiet >= SETTLE_MS),
            format!("typedQuietMs {quiet:?}"),
        ));
    }
    // The human sends words while a result waits: they begin a turn, and the result goes after.
    if !members.chief_is_idle(log)? {
        return Ok(checks);
    }
    says(members, "Reply with exactly one line: HUMAN")?;
    members.give(STAND_IN_WORKER, "Reply with exactly: THREE_DONE")?;
    let fourth = find(log, "the fourth result to go in", WITHIN, LOOK, || {
        Ok(members.inbox(Some("chief"))?.into_iter().find(|message| {
            message["kind"] == "result"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.contains("THREE_DONE"))
                && (message["state"] == "delivered" || message["state"] == "failed")
        }))
    })?;
    let events = members.events();
    let items = record_of(members, harness)?;
    if let Some(fourth) = &fourth {
        checks.extend(delivered_once(
            who,
            "the result behind the human's words",
            fourth,
            &events,
            &items,
        ));
    } else {
        checks.push(Check::failed(
            who,
            "the result behind the human's words settled",
            "it never did",
        ));
    }
    Ok(checks)
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
    // The chief works: what the workers send waits behind its turn. The human asks for the
    // work as a human does, in words and then Enter, its own key.
    says(&members, WORK)?;
    find(
        log,
        "the chief to be at work",
        Duration::from_secs(60),
        LOOK,
        || Ok((members.lane("chief")?["activity"]["state"] == "working").then_some(())),
    )?;
    members.give(STAND_IN_WORKER, "Reply with exactly: ONE_DONE")?;
    wait_for(
        &members,
        log,
        "the result to wait for the chief",
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
        "the question to wait for the chief",
        "question",
        "output format",
        &["queued", "delivering", "delivered"],
    )?;
    let result = wait_for(
        &members,
        log,
        "the result to go in",
        "result",
        "ONE_DONE",
        &["delivered", "failed"],
    )?;
    let question = wait_for(
        &members,
        log,
        "the question to go in",
        "question",
        "output format",
        &["delivered", "failed"],
    )?;
    members.chief_is_idle(log)?;
    let events = members.events();
    let items = record_of(&members, harness)?;
    let mut checks = Vec::new();
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
            "the question and the result settled",
            format!("question {question:?}, result {result:?}"),
        )),
    }
    if matches!(harness, Harness::Claude | Harness::Devin) {
        checks.extend(typing(&members, log, &who, harness)?);
    }
    members.close()?;
    Ok(checks)
}

/// The harnesses named, Claude where none is.
fn chiefs(named: &[String]) -> Result<Vec<Harness>> {
    if named.is_empty() {
        return Ok(vec![Harness::Claude]);
    }
    named
        .iter()
        .map(|word| {
            Harness::from_name(word).ok_or_else(|| {
                Error::Daemon(format!(
                    "no chief is called {word}: {}",
                    Harness::ALL.map(Harness::name).join(", ")
                ))
            })
        })
        .collect()
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of each harness named; cargo xtask live pacing"]
fn a_real_chief_is_given_one_message_at_a_time_each_once_after_it_rested() -> Result {
    let log = Log::begin("live pacing")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn event(at: &str, kind: &str, data: Value) -> Value {
        json!({ "at": at, "kind": kind, "data": data })
    }

    fn message(id: i64, attempts: i64) -> Value {
        json!({ "id": id, "state": "delivered", "attempts": attempts, "reason": null })
    }

    fn item(text: &str) -> ask::Item {
        ask::Item {
            id: String::new(),
            role: "user".to_owned(),
            text: text.to_owned(),
            complete: true,
        }
    }

    fn held(checks: &[Check]) -> Vec<bool> {
        checks.iter().map(|check| check.ok).collect()
    }

    #[test]
    fn a_message_pasted_once_after_the_settle_holds_and_one_pasted_again_does_not() {
        let events = [
            event(
                "2026-10-27T12:00:00.000Z",
                "delivery.begun",
                json!({ "message": 4, "look": { "restMs": 3_200 } }),
            ),
            event(
                "2026-10-27T12:01:00.000Z",
                "delivery.begun",
                json!({ "message": 5, "look": { "restMs": 900 } }),
            ),
            event(
                "2026-10-27T12:02:00.000Z",
                "delivery.retried",
                json!({ "message": 5 }),
            ),
            event(
                "2026-10-27T12:03:00.000Z",
                "delivery.begun",
                json!({ "message": 5, "look": { "restMs": 3_000 } }),
            ),
        ];
        let items = [
            item("[ConsensFlow m-4 · note]\nOne"),
            item("[ConsensFlow m-5 · note]\nTwo"),
            item("[ConsensFlow m-5 · note]\nTwo"),
        ];
        assert_eq!(
            held(&delivered_once("c", "m-4", &message(4, 1), &events, &items)),
            [true, true, true, true]
        );
        assert_eq!(
            held(&delivered_once("c", "m-5", &message(5, 2), &events, &items)),
            [false, false, false, false]
        );
    }

    #[test]
    fn the_question_first_and_a_settle_after_it_hold_and_the_oldest_first_does_not() {
        let question = message(6, 1);
        let result = message(5, 1);
        let paced = [
            event(
                "2026-10-27T12:00:00.000Z",
                "delivery.begun",
                json!({ "message": 6 }),
            ),
            event(
                "2026-10-27T12:00:01.000Z",
                "delivery.confirmed",
                json!({ "message": 6 }),
            ),
            event(
                "2026-10-27T12:00:20.000Z",
                "delivery.begun",
                json!({ "message": 5 }),
            ),
        ];
        assert_eq!(
            held(&in_order("c", &question, &result, &paced)),
            [true, true]
        );
        let oldest_first = [
            event(
                "2026-10-27T12:00:00.000Z",
                "delivery.begun",
                json!({ "message": 5 }),
            ),
            event(
                "2026-10-27T12:00:01.000Z",
                "delivery.begun",
                json!({ "message": 6 }),
            ),
            event(
                "2026-10-27T12:00:02.000Z",
                "delivery.confirmed",
                json!({ "message": 6 }),
            ),
        ];
        assert_eq!(
            held(&in_order("c", &question, &result, &oldest_first)),
            [false, false]
        );
    }
}
