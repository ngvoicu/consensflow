//! What the live checks of a chief's deliveries read, shared by `live_pacing` and
//! `live_meeting`: the chief's messages on the board, the daemon's events of each, the chief's
//! own record, and the checks of a message pasted once and of two in order. The human's words
//! typed into the chief's window, and the harnesses named.

use std::time::Duration;

use cf_e2e::live::ask;
use cf_e2e::live::members::{find, Members};
use cf_e2e::live::report::Check;
use cf_e2e::live::{self, Harness, Log};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// The settle the daemon waits, its own default (`Limits::default`).
pub(crate) const SETTLE_MS: i64 = 3_000;

/// How long a message has to go in, and how often the board is looked at.
pub(crate) const WITHIN: Duration = Duration::from_secs(360);
pub(crate) const LOOK: Duration = Duration::from_secs(1);

/// The chief's pane, as the pane host takes it: its newest generation.
pub(crate) fn chief_pane(members: &Members) -> Value {
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
pub(crate) fn message_of(members: &Members, kind: &str, words: &str) -> Result<Option<Value>> {
    Ok(members.inbox(Some("chief"))?.into_iter().find(|message| {
        message["kind"] == kind
            && message["body"]
                .as_str()
                .is_some_and(|body| body.contains(words))
    }))
}

/// Waits for the chief's message of `kind` whose body holds `words` to be in `states`.
pub(crate) fn wait_for(
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
pub(crate) fn at_ms(event: &Value) -> Option<i64> {
    event["at"]
        .as_str()?
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|at| at.as_millisecond())
}

/// The daemon's events of `kind` for the message `id`.
pub(crate) fn of<'a>(events: &'a [Value], kind: &str, id: i64) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
        .collect()
}

/// How many of the chief's user items hold the header of the message `id`.
pub(crate) fn in_record(items: &[ask::Item], id: i64) -> usize {
    let marker = format!("[ConsensFlow m-{id} ");
    items
        .iter()
        .filter(|item| item.role == "user" && item.text.contains(&marker))
        .count()
}

/// The checks of one message `id` the chief was sent, named `what`, against the daemon's
/// events and the chief's record: delivered, pasted once, on its first attempt, in the record
/// once, and let go by a look that read the window at rest the whole settle.
pub(crate) fn delivered_once(
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
pub(crate) fn in_order(
    who: &str,
    question: &Value,
    result: &Value,
    events: &[Value],
) -> Vec<Check> {
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
pub(crate) fn session_in(events: &[Value], handle: &str) -> Option<String> {
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
pub(crate) fn record_of(members: &Members, harness: Harness) -> Result<Vec<ask::Item>> {
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
pub(crate) fn says(members: &Members, words: &str) -> Result<()> {
    let pane = chief_pane(members);
    members.rig().types(&pane, words.as_bytes())?;
    std::thread::sleep(Duration::from_millis(700));
    members.rig().types(&pane, b"\r")
}

/// The harnesses named, Claude where none is.
pub(crate) fn chiefs(named: &[String]) -> Result<Vec<Harness>> {
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
