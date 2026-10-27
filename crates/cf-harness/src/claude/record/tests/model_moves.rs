//! A model Claude Code moved a conversation to on its own (MUS-24): the model of the main
//! conversation's assistant messages (`message.model`) changes with no `/model` of the human's
//! between, which Claude Code's `fallbackModel` on overload and its remap of a model it retired
//! do. Provisional: neither was ever recorded on this machine (2026-10-27: 150 transcripts, two
//! with a second model, both the human's), so these are made here from the shapes the
//! transcripts hold.

use super::*;
use crate::records::ModelMoved;

/// An assistant's message `id` of `model`, which ended its turn.
fn on(model: &str, uuid: &str, parent: &str, id: &str) -> Value {
    let mut record = assistant(
        uuid,
        parent,
        json!(id),
        json!([text(json!("Hi"))]),
        json!("end_turn"),
    );
    record["message"]["model"] = json!(model);
    record
}

/// The user's command `command`, as Claude Code writes what one typed at its prompt.
fn typed(uuid: &str, parent: &str, command: &str) -> Value {
    user(
        uuid,
        json!(parent),
        json!(format!("<command-name>{command}</command-name>")),
    )
}

fn moved_of(records: &[Value], options: &Options) -> Option<ModelMoved> {
    let mut stage = Stage::new();
    stage.write(records);
    match &*stage.look_told(options) {
        Reading::Known(record) => record.model_moved.as_deref().cloned(),
        other => panic!("not known: {other:?}"),
    }
}

fn plain(records: &[Value]) -> Option<ModelMoved> {
    moved_of(records, &Options::default())
}

#[test]
fn a_model_that_changes_in_the_main_conversation_with_no_command_of_the_humans_is_a_move() {
    let moved = plain(&[
        hello(),
        on("claude-opus-5-5", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("More")),
        on("claude-sonnet-5-5", "a2", "u2", "m2"),
    ])
    .expect("a move");
    assert_eq!(
        (moved.from.as_str(), moved.to.as_str()),
        ("claude-opus-5-5", "claude-sonnet-5-5")
    );
    assert_eq!(
        moved.because,
        "Claude Code switched models mid-conversation (a fallback model, or a remap)"
    );
}

#[test]
fn the_humans_model_command_a_synthetic_message_a_subagent_and_a_relaunch_are_no_move() {
    // The same model, or a synthetic message (Claude Code's own, `<synthetic>`) between.
    assert_eq!(
        plain(&[
            hello(),
            on("claude-opus-5-5", "a1", "u1", "m1"),
            on("<synthetic>", "a2", "a1", "m2"),
            on("claude-opus-5-5", "a3", "a2", "m3"),
        ]),
        None
    );
    // The human typed `/model`: theirs, and the next change with none is Claude Code's.
    let theirs = [
        hello(),
        on("claude-opus-5-5", "a1", "u1", "m1"),
        typed("u2", "a1", "/model"),
        on("claude-sonnet-5-5", "a2", "u2", "m2"),
    ];
    assert_eq!(plain(&theirs), None);
    let mut later = theirs.to_vec();
    later.push(user("u3", json!("a2"), json!("More")));
    later.push(on("claude-haiku-5-5", "a3", "u3", "m3"));
    let moved = plain(&later).expect("a later move");
    assert_eq!(
        (moved.from.as_str(), moved.to.as_str()),
        ("claude-sonnet-5-5", "claude-haiku-5-5")
    );
    // A subagent's messages (another conversation of the file) run on models of their own.
    let mut child = on("claude-haiku-5-5", "s1", "a1", "m9");
    child["isSidechain"] = json!(true);
    assert_eq!(
        plain(&[hello(), on("claude-opus-5-5", "a1", "u1", "m1"), child]),
        None
    );
    // A change across a relaunch is ConsensFlow's: its launch named the model.
    let records = [
        hello(),
        on("claude-opus-5-5", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("More")),
        on("claude-sonnet-5-5", "a2", "u2", "m2"),
    ];
    let options = Options {
        relaunched_at: Some(Stage::lines(&records[..2]).len() as u64),
        ..Options::default()
    };
    assert_eq!(moved_of(&records, &options), None);
}
