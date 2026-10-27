//! The daemon's runs: a real Claude Code or Devin as the chief of a project on the native daemon,
//! the stand-in as the worker whose result the chief is sent.
//!
//! - **Images**: the stand-in is an image designer, whose result starts with the absolute path of
//!   each picture it saved, as the designer's role asks. Claude makes attachments of those paths,
//!   and the daemon has to find the result's header behind their placeholders to confirm the paste
//!   (T-215). Devin is asked the same, to find out whether its TUI does it too (T-763).
//! - **Long**: the stand-in's result is a report of 12,000 characters. Claude Code, when the machine
//!   is loaded, is given a paste in pieces, wraps each and leaves the last short piece bare after
//!   the last closing tag, and the daemon has to take that for the paste (T-763). Run it under
//!   load (`tools/under-load.py` of `wt/load-timing-flakes`) to bring the pieces about.
//!
//! The run holds the board and the chief's own record to one thing: the result was pasted once,
//! confirmed on the first attempt, and is in the record once.

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

/// Which result the stand-in sends the chief.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// A designer's, starting with the paths of two pictures.
    Images,
    /// A worker's report of [`LONG`] characters.
    Long,
}

/// How long the report of [`Case::Long`] is, in characters: the size of a worker's result in
/// the field (m-4272 to m-4518 were 4 to 11 KB).
pub const LONG: usize = 12_000;

/// Who the checks are of.
fn who(harness: Harness) -> String {
    format!("{} chief", harness.name())
}

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

/// The project: a real `harness` is the chief, and the stand-in is the worker.
pub fn plan(harness: Harness, case: Case) -> Plan {
    let folder = match case {
        Case::Images => "image-paste-daemon",
        Case::Long => "long-paste-daemon",
    };
    Plan::new(
        &format!("{folder}-{}", harness.name()),
        member_as(harness, "chief"),
    )
    .with(stand_in_worker())
    .told(BRIEF)
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

/// The native session of the conversation `handle` has newest, as the daemon's `events` say it
/// was bound (`conversation.started`, then `conversation.bound`): a harness that is given no
/// session id in its command line (Devin) is known by it alone.
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

/// What the chief's `harness` recorded of its conversation, as the daemon reads it.
fn record_of(members: &Members, harness: Harness) -> Result<Vec<Item>> {
    let pane = format!("p{}-chief", members.project().id());
    let session = members
        .rig()
        .open_frames()
        .into_iter()
        .rev()
        .find(|frame| frame["id"] == pane.as_str())
        .and_then(|frame| session_of(&frame))
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

/// How many pieces Claude Code wrapped a paste in (`<pasted_content …>` tags) and how many
/// characters follow the last closing tag: the pieces a paste was given in and the bare tail
/// the last, short piece left (T-763).
pub fn pieces_of(text: &str) -> (usize, usize) {
    let wrapped = text.matches("<pasted_content").count();
    let tail = text
        .rsplit_once("</pasted_content")
        .and_then(|(_, after)| after.split_once('>'))
        .map_or(0, |(_, tail)| tail.trim().chars().count());
    (wrapped, tail)
}

/// What the board and the chief's record show of the result `settled` (none where it never
/// settled), given the events the daemon wrote and the record's `items`.
pub fn checks(
    harness: Harness,
    case: Case,
    settled: Option<&Value>,
    events: &[Value],
    items: &[Item],
) -> Vec<Check> {
    let who = who(harness);
    let Some(result) = settled else {
        return vec![Check::failed(
            &who,
            "the result settled",
            "it was neither delivered nor failed in the time given",
        )];
    };
    let id = result["id"].as_i64().unwrap_or_default();
    let marker = format!("[ConsensFlow m-{id} ");
    let holding: Vec<&Item> = items
        .iter()
        .filter(|item| item.role == "user" && item.text.contains(&marker))
        .collect();
    let first = holding
        .first()
        .map_or_else(|| "none".to_owned(), |item| cut(&item.text, 160));
    let attached = holding
        .iter()
        .any(|item| item.text.starts_with("[Image #") && item.text.contains("]["));
    // The sources of the attachments are the harness's own item, read as its companion and no
    // message of the user's (T-265): whatever its role, they are what Claude Code wrote beside.
    let sources = items
        .iter()
        .any(|item| item.text.starts_with("[Image: source: "));
    let begun = events_of(events, "delivery.begun", id);
    let retried = events_of(events, "delivery.retried", id);
    let shape = match (case, harness) {
        (Case::Images, Harness::Claude) => Check::new(
            &who,
            "Claude made attachments of the paths (the case proves nothing otherwise)",
            attached && sources,
            format!("the item that holds the header: {first}"),
        ),
        // Devin's TUI made no attachment of a pasted path (the run of 2026-10-26, T-763, Devin
        // 3000.11.205): its record holds the paste as it was given. A run that finds it does is
        // the answer changing, and the delivery checks below say what came of it.
        (Case::Images, _) => Check::new(
            &who,
            "no attachment of the paths (the answer Devin gave on 2026-10-26)",
            !attached && !sources,
            format!(
                "attachments: {}; sources item: {}; the item that holds the header: {first}",
                if attached { "yes" } else { "none" },
                if sources { "yes" } else { "none" },
            ),
        ),
        // The tail is the case: a record whose item is one block was not given the paste in
        // pieces, and proves nothing of the loose tail. Reported with the delivery, held by it.
        (Case::Long, _) => {
            let (wrapped, tail) = holding.first().map_or((0, 0), |item| pieces_of(&item.text));
            Check::new(
                &who,
                "the shape of the record's item (reported, not held)",
                true,
                format!(
                    "{wrapped} wrapped pieces, {tail} characters bare after the last closing tag, in an item of {} characters",
                    holding.first().map_or(0, |item| item.text.chars().count())
                ),
            )
        }
    };
    vec![
        shape,
        Check::new(
            &who,
            "the result is delivered",
            result["state"] == "delivered",
            format!(
                "m-{id} is {}, after {} attempts: {}",
                result["state"], result["attempts"], result["reason"]
            ),
        ),
        Check::new(
            &who,
            "the daemon pasted it once",
            begun == 1 && retried == 0,
            format!("{begun} pastes begun, {retried} retried"),
        ),
        Check::new(
            &who,
            "the chief's record holds it once",
            holding.len() == 1,
            format!("{} user's items hold the header", holding.len()),
        ),
    ]
}

/// The run: the worker is given its task, and the result is waited for until it is
/// delivered or has failed.
pub fn run(log: &Log, harness: Harness, case: Case) -> Result<Vec<Check>> {
    let mut members = Members::open(&plan(harness, case), Path::new(crate::FAKE_AGENT), log)?;
    members.add(STAND_IN_WORKER, &[])?;
    let brief = match case {
        Case::Images => format!("SAVED {PICTURES} {WORDS}"),
        Case::Long => format!("LONG {LONG}"),
    };
    let task = members.give(STAND_IN_WORKER, &brief)?;
    log.say(format!("T-{task} is the worker's: {brief}"));
    let settled = find(
        log,
        "the worker's result to be delivered or to fail",
        SETTLE,
        LOOK,
        || {
            Ok(result_of(&members)?
                .filter(|message| message["state"] == "delivered" || message["state"] == "failed"))
        },
    )?;
    let checks = checks(
        harness,
        case,
        settled.as_ref(),
        &members.events(),
        &record_of(&members, harness)?,
    );
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
                "custom",
                "[Image: source: /a/desk.png]\n[Image: source: /a/hero.png]",
            ),
            item("assistant", "SEEN"),
        ]
    }

    fn held(checks: &[Check]) -> Vec<bool> {
        checks.iter().map(|check| check.ok).collect()
    }

    fn images(settled: Option<&Value>, events: &[Value], items: &[Item]) -> Vec<Check> {
        checks(Harness::Claude, Case::Images, settled, events, items)
    }

    #[test]
    fn a_result_pasted_once_confirmed_and_recorded_once_with_its_pictures_holds() {
        let delivered = json!({ "id": 2619, "state": "delivered", "attempts": 1, "reason": null });
        let found = images(Some(&delivered), &events(1, 0), &field());
        assert_eq!(held(&found), [true, true, true, true], "{found:?}");
    }

    #[test]
    fn the_field_bug_pasted_it_three_times_and_failed_it() {
        let failed = json!({ "id": 2619, "state": "failed", "attempts": 3, "reason": "the harness record never showed it" });
        let mut items = field();
        items.extend(field());
        items.extend(field());
        let found = images(Some(&failed), &events(3, 2), &items);
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
        let found = images(Some(&delivered), &events(1, 0), &plain);
        assert_eq!(held(&found), [false, true, true, true], "{found:?}");
        let never = images(None, &[], &[]);
        assert_eq!(held(&never), [false]);
    }

    #[test]
    fn a_devin_is_held_to_the_delivery_and_to_making_no_attachment_of_the_paths() {
        let delivered = json!({ "id": 2619, "state": "delivered", "attempts": 1, "reason": null });
        let plain = vec![item(
            "user",
            "[ConsensFlow m-2619 · T-45 · result from @pygmalion]\n/a/desk.png\nWarm.",
        )];
        let found = checks(
            Harness::Devin,
            Case::Images,
            Some(&delivered),
            &events(1, 0),
            &plain,
        );
        assert_eq!(held(&found), [true, true, true, true], "{found:?}");
        assert_eq!(found[0].who, "devin chief");
        // Attachments are the answer changing: the first check says so, the delivery is judged on its own.
        let found = checks(
            Harness::Devin,
            Case::Images,
            Some(&delivered),
            &events(1, 0),
            &field(),
        );
        assert_eq!(held(&found), [false, true, true, true], "{found:?}");
        let asked_twice =
            json!({ "id": 2619, "state": "delivered", "attempts": 2, "reason": null });
        let found = checks(
            Harness::Devin,
            Case::Images,
            Some(&asked_twice),
            &events(2, 1),
            &plain,
        );
        assert_eq!(held(&found), [true, true, false, true], "{found:?}");
    }

    #[test]
    fn the_pieces_a_paste_was_given_in_are_counted_with_the_tail_the_last_left_bare() {
        let piece = |at: usize| {
            format!(
                "<pasted_content id=\"b118\">\n{}\n</pasted_content id=\"b118\">",
                "x".repeat(at)
            )
        };
        assert_eq!(pieces_of("[ConsensFlow m-1 · T-1]\nplain"), (0, 0));
        assert_eq!(pieces_of(&format!("\n\n{}\n", piece(5))), (1, 0));
        assert_eq!(
            pieces_of(&format!(
                "\n\n{}\n\n\n{}\n\nthe rest of it\n",
                piece(5),
                piece(6)
            )),
            (2, 14)
        );
    }

    #[test]
    fn a_long_result_is_held_to_the_delivery_and_the_shape_of_its_item_is_reported() {
        let delivered = json!({ "id": 2619, "state": "delivered", "attempts": 1, "reason": null });
        let piece = "<pasted_content id=\"b118\">\n[ConsensFlow m-2619 · T-1 · result from @w]\nwords\n</pasted_content id=\"b118\">";
        let items = vec![item(
            "user",
            &format!("\n\n{piece}\n\n\n{piece}\n\nlast words"),
        )];
        let found = checks(
            Harness::Claude,
            Case::Long,
            Some(&delivered),
            &events(1, 0),
            &items,
        );
        assert_eq!(held(&found), [true, true, true, true], "{found:?}");
        assert!(
            found[0]
                .detail
                .starts_with("2 wrapped pieces, 10 characters bare"),
            "{found:?}"
        );
        // Pasted again after the first look took the record for lacking: not held.
        let retried = checks(
            Harness::Claude,
            Case::Long,
            Some(&delivered),
            &events(2, 1),
            &items,
        );
        assert_eq!(held(&retried), [true, true, false, true], "{retried:?}");
    }

    #[test]
    fn the_native_session_of_a_chief_is_the_one_its_newest_conversation_was_bound_to() {
        let started = |conversation: i64, handle: &str| json!({ "kind": "conversation.started", "data": { "participant": handle, "conversation": conversation } });
        let bound = |conversation: i64, session: &str| json!({ "kind": "conversation.bound", "data": { "conversation": conversation, "nativeSession": session } });
        let events = vec![
            started(1, "chief"),
            bound(1, "old"),
            started(2, "worker-olive-comet"),
            bound(2, "worker's"),
            started(3, "chief"),
        ];
        // The newest conversation of the chief has not been bound yet.
        assert_eq!(session_in(&events, "chief"), None);
        let mut events = events;
        events.push(bound(3, "sess-3"));
        assert_eq!(session_in(&events, "chief").as_deref(), Some("sess-3"));
        assert_eq!(
            session_in(&events, "worker-olive-comet").as_deref(),
            Some("worker's")
        );
        assert_eq!(session_in(&events, "nobody"), None);
    }

    #[test]
    fn the_chief_is_a_real_harness_and_the_worker_the_stand_in() {
        for (harness, case, folder) in [
            (Harness::Claude, Case::Images, "image-paste-daemon-claude"),
            (Harness::Devin, Case::Images, "image-paste-daemon-devin"),
            (Harness::Claude, Case::Long, "long-paste-daemon-claude"),
        ] {
            let plan = plan(harness, case);
            assert_eq!(plan.folder, folder);
            assert_eq!(plan.chief, "chief");
            assert_eq!(plan.roster[0]["kind"], harness.kind());
            assert_eq!(plan.roster[1]["model"], "fake", "no model for the worker");
            assert_eq!(plan.harnesses(), [harness]);
            assert_eq!(plan.brief, BRIEF);
        }
    }
}
