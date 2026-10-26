//! The daemon's run: a real Claude Code as the chief of a project on the native daemon, the
//! stand-in as its image designer, whose result starts with the absolute path of each picture it
//! saved, as the designer's role asks. Claude makes attachments of those paths, and the daemon
//! has to find the result's header behind their placeholders to confirm the paste. The run holds
//! the board and the chief's own record to one thing: the result was pasted once, confirmed, and
//! is in the record once, with its pictures.

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::ask::{self, Item};
use cf_e2e::live::members::{
    find, member_as, session_of, stand_in_worker, Members, Plan, STAND_IN_WORKER,
};
use cf_e2e::live::report::Check;
use cf_e2e::live::{cut, Harness, Log};
use cf_e2e::Result;
use serde_json::Value;

/// Who the checks are of.
const WHO: &str = "claude chief";

/// The pictures the designer saves, and the words after their paths: its one line on what
/// they show.
const PICTURES: &str = "desk-illustration.png,hero.png";
const WORDS: &str = "Warm editorial desk scene, and its hero.";

/// How long the designer has to answer and its result to settle: delivered, or failed after
/// the daemon's three attempts of a minute each (`Limits::default`), which is what the field
/// bug came to.
const SETTLE: Duration = Duration::from_secs(420);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_secs(1);

/// What the folder tells the chief: to look at a result and say so in a line, and run nothing.
const BRIEF: &str = "This folder is an automated ConsensFlow test. When a ConsensFlow result arrives, reply with exactly one line: SEEN, and run no tools. When any other message arrives, do what it asks and reply with one line.\n";

/// The project: a real Claude Code is the chief, and the stand-in is the designer.
pub fn plan() -> Plan {
    Plan::new("image-paste-daemon", member_as(Harness::Claude, "chief"))
        .with(stand_in_worker())
        .with_file("CLAUDE.md", BRIEF)
}

/// The result the chief was sent, as its inbox holds it: the first of kind result.
fn result_of(members: &Members) -> Result<Option<Value>> {
    Ok(members
        .inbox(Some("chief"))?
        .into_iter()
        .find(|message| message["kind"] == "result"))
}

/// How many events of `kind` the daemon wrote of the message `id`.
fn events_of(events: &[Value], kind: &str, id: i64) -> usize {
    events
        .iter()
        .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
        .count()
}

/// What the chief's Claude recorded of its conversation, as the daemon reads it.
fn record_of(members: &Members) -> Result<Vec<Item>> {
    let pane = format!("p{}-chief", members.project().id());
    let session = members
        .rig()
        .open_frames()
        .into_iter()
        .rev()
        .find(|frame| frame["id"] == pane.as_str())
        .and_then(|frame| session_of(&frame));
    let Some(session) = session else {
        return Ok(Vec::new());
    };
    Ok(ask::records(
        Harness::Claude.kind(),
        &session,
        members.rig().environment(),
    )?
    .map(|record| record.items)
    .unwrap_or_default())
}

/// What the board and the chief's record show of the result `settled` (none where it never
/// settled), given the events the daemon wrote and the record's `items`.
pub fn checks(settled: Option<&Value>, events: &[Value], items: &[Item]) -> Vec<Check> {
    let Some(result) = settled else {
        return vec![Check::failed(
            WHO,
            "the designer's result settled",
            "it was neither delivered nor failed in the time given",
        )];
    };
    let id = result["id"].as_i64().unwrap_or_default();
    let marker = format!("[ConsensFlow m-{id} ");
    let holding: Vec<&Item> = items
        .iter()
        .filter(|item| item.role == "user" && item.text.contains(&marker))
        .collect();
    let attached = holding
        .iter()
        .any(|item| item.text.starts_with("[Image #") && item.text.contains("]["));
    let sources = items
        .iter()
        .any(|item| item.role == "user" && item.text.starts_with("[Image: source: "));
    let begun = events_of(events, "delivery.begun", id);
    let retried = events_of(events, "delivery.retried", id);
    vec![
        Check::new(
            WHO,
            "Claude made attachments of the paths (the case proves nothing otherwise)",
            attached && sources,
            format!(
                "the item that holds the header: {}",
                holding
                    .first()
                    .map_or_else(|| "none".to_owned(), |item| cut(&item.text, 160))
            ),
        ),
        Check::new(
            WHO,
            "the result is delivered",
            result["state"] == "delivered",
            format!(
                "m-{id} is {}, after {} attempts: {}",
                result["state"], result["attempts"], result["reason"]
            ),
        ),
        Check::new(
            WHO,
            "the daemon pasted it once",
            begun == 1 && retried == 0,
            format!("{begun} pastes begun, {retried} retried"),
        ),
        Check::new(
            WHO,
            "the chief's record holds it once",
            holding.len() == 1,
            format!("{} user's items hold the header", holding.len()),
        ),
    ]
}

/// The run: the designer is given its task, and the result is waited for until it is
/// delivered or has failed.
pub fn run(log: &Log) -> Result<Vec<Check>> {
    let mut members = Members::open(&plan(), Path::new(crate::FAKE_AGENT), log)?;
    members.add(STAND_IN_WORKER, &[])?;
    let task = members.give(STAND_IN_WORKER, &format!("SAVED {PICTURES} {WORDS}"))?;
    log.say(format!("T-{task} is the designer's: SAVED {PICTURES}"));
    let settled = find(
        log,
        "the designer's result to be delivered or to fail",
        SETTLE,
        LOOK,
        || {
            Ok(result_of(&members)?
                .filter(|message| message["state"] == "delivered" || message["state"] == "failed"))
        },
    )?;
    let checks = checks(settled.as_ref(), &members.events(), &record_of(&members)?);
    members.close()?;
    Ok(checks)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn item(role: &str, text: &str) -> Item {
        Item {
            id: String::new(),
            role: role.to_owned(),
            text: text.to_owned(),
            complete: true,
        }
    }

    fn events(begun: usize, retried: usize) -> Vec<Value> {
        let of = |kind: &str| json!({ "kind": kind, "data": { "message": 2619 } });
        let mut events = vec![json!({ "kind": "delivery.begun", "data": { "message": 7 } })];
        events.extend((0..begun).map(|_| of("delivery.begun")));
        events.extend((0..retried).map(|_| of("delivery.retried")));
        events
    }

    /// The records of the field: the header behind the placeholders, and the sources after.
    fn field() -> Vec<Item> {
        vec![
            item(
                "user",
                "[Image #1] [Image #2][ConsensFlow m-2619 · T-45 · result from @pygmalion]\nWarm.",
            ),
            item(
                "user",
                "[Image: source: /a/desk.png]\n[Image: source: /a/hero.png]",
            ),
            item("assistant", "SEEN"),
        ]
    }

    fn held(checks: &[Check]) -> Vec<bool> {
        checks.iter().map(|check| check.ok).collect()
    }

    #[test]
    fn a_result_pasted_once_confirmed_and_recorded_once_with_its_pictures_holds() {
        let delivered = json!({ "id": 2619, "state": "delivered", "attempts": 1, "reason": null });
        let found = checks(Some(&delivered), &events(1, 0), &field());
        assert_eq!(held(&found), [true, true, true, true], "{found:?}");
    }

    #[test]
    fn the_field_bug_pasted_it_three_times_and_failed_it() {
        let failed = json!({ "id": 2619, "state": "failed", "attempts": 3, "reason": "the harness record never showed it" });
        let mut items = field();
        items.extend(field());
        items.extend(field());
        let found = checks(Some(&failed), &events(3, 2), &items);
        assert_eq!(held(&found), [true, false, false, false], "{found:?}");
        assert!(found[1].detail.contains("failed"), "{found:?}");
    }

    #[test]
    fn a_claude_that_made_no_attachment_proves_nothing_and_a_result_that_never_settled_fails() {
        let delivered = json!({ "id": 2619, "state": "delivered", "attempts": 1, "reason": null });
        let plain = vec![item(
            "user",
            "[ConsensFlow m-2619 · T-45 · result from @pygmalion]\n/a/desk.png\nWarm.",
        )];
        let found = checks(Some(&delivered), &events(1, 0), &plain);
        assert_eq!(held(&found), [false, true, true, true], "{found:?}");
        let never = checks(None, &[], &[]);
        assert_eq!(held(&never), [false]);
    }

    #[test]
    fn the_chief_is_a_real_claude_and_the_designer_the_stand_in() {
        let plan = plan();
        assert_eq!(plan.chief, "chief");
        assert_eq!(plan.roster[0]["kind"], Harness::Claude.kind());
        assert_eq!(plan.roster[1]["model"], "fake", "no model for the designer");
        assert_eq!(plan.harnesses(), [Harness::Claude]);
        assert!(plan
            .files
            .iter()
            .any(|(path, text)| path == "CLAUDE.md" && text == BRIEF));
    }
}
