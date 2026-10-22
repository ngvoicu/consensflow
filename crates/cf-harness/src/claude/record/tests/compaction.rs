//! A compaction while the conversation is idle: Claude Code writes a
//! `compact_boundary`, which starts a new chain of records (its parent is none, its
//! logical parent the record before), and the summary as a user's record
//! (`isCompactSummary`), and no turn runs. The owner's report of 2026-10-15
//! (3.0.4): a Resume read the summary as a prompt never answered, so the turn
//! before it as cut, and told the chief so.

use super::*;
use crate::shared::record::reading::Role;

/// The `compact_boundary` record `uuid`, after the record `logical`.
pub(super) fn compacted(uuid: &str, logical: &str) -> Value {
    of_session(json!({
        "type": "system", "subtype": "compact_boundary", "uuid": uuid, "parentUuid": null,
        "logicalParentUuid": logical, "content": "Conversation compacted", "isMeta": false,
        "level": "info",
    }))
}

/// The summary a compaction writes, under the boundary `parent`.
pub(super) fn compact_summary(uuid: &str, parent: &str) -> Value {
    having(
        user(
            uuid,
            json!(parent),
            json!("This session is being continued from a previous conversation that ran out of context."),
        ),
        json!({ "isCompactSummary": true, "isVisibleInTranscriptOnly": true }),
    )
}

#[test]
fn a_compaction_while_idle_leaves_the_turn_before_it_at_rest() {
    let at_rest = [hello(), answer("a1", "u1"), duration("d1", "a1")];
    assert_eq!(settlement_of(&at_rest), Settlement::Settled);
    let mut compacting = at_rest.to_vec();
    compacting.extend([compacted("c1", "d1"), compact_summary("s1", "c1")]);
    assert_eq!(
        settlement_of(&compacting),
        Settlement::Settled,
        "the summary is no prompt"
    );
    // A prompt after it is the user's, and opens a turn.
    compacting.push(user("u2", json!("s1"), json!("Go on")));
    assert_eq!(settlement_of(&compacting), Settlement::InFlight);
}

#[test]
fn the_summary_is_a_user_s_item_marked_as_the_compaction_s_and_no_other_item_is() {
    let mut stage = Stage::new();
    stage.write(&[
        hello(),
        answer("a1", "u1"),
        duration("d1", "a1"),
        compacted("c1", "d1"),
        compact_summary("s1", "c1"),
        user("u2", json!("s1"), json!("Go on")),
    ]);
    let Reading::Known(record) = &*stage.reader.look(&Options::default(), 0) else {
        panic!("an unknown reading");
    };
    let marked: Vec<(&str, Role, bool)> = record
        .items
        .iter()
        .map(|item| (&*item.id, item.role, item.compaction))
        .collect();
    assert_eq!(
        marked,
        [
            ("u1", Role::User, false),
            ("m1", Role::Assistant, false),
            ("s1", Role::User, true),
            ("u2", Role::User, false),
        ]
    );
}
