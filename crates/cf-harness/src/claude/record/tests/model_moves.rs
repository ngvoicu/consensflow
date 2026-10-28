//! A model Claude Code moved a conversation to on its own (MUS-24, T-816).
//!
//! **What holds a window** is Claude Code's own word for the move: a `fallback` block, the
//! first and only block of an assistant record, `{type: "fallback", from: {model}, to:
//! {model}}` (seen in the wild, session `3afd6cd4`, Claude Code 2.1.205: three records on
//! `claude-opus-4-8` after `claude-fable-5`, each followed by the human's own `/model`
//! back). Nothing but those blocks says the harness switched.
//!
//! **What is only told** is `message.model` drifting with no such block. A rescan of the
//! 1013 transcripts on this Mac (2026-10-27, counts and model names only) found 34 sessions
//! with a model change in the main conversation and 26 changes with no `/model` or `/fast`
//! between: alias repoints that reached three running sessions in three projects within two
//! minutes, in the middle of a tool loop; restarts on another model; and the real fallbacks.
//! Holding on drift would hold every Claude lane on a repoint day, and an `opusplan` member
//! (Opus in plan mode, Sonnet otherwise) at each flip.
//!
//! The blocks here are shaped as the real ones; none holds a message of a transcript.

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

/// The record Claude Code writes when it fell back: one `fallback` block, from a model to
/// another, which the message names as its own.
fn fell_back(from: &str, to: &str, uuid: &str, parent: &str, id: &str) -> Value {
    let mut record = assistant(
        uuid,
        parent,
        json!(id),
        json!([{ "type": "fallback", "from": { "model": from }, "to": { "model": to } }]),
        json!("tool_use"),
    );
    record["message"]["model"] = json!(to);
    record["timestamp"] = json!("2026-09-19T10:00:00.000Z");
    record
}

/// An assistant call of the tool `call`, on `model`.
fn calling(model: &str, call: &str, uuid: &str, parent: &str, id: &str) -> Value {
    let mut record = assistant(
        uuid,
        parent,
        json!(id),
        json!([{ "type": "tool_use", "id": call, "name": "Bash", "input": {} }]),
        json!("tool_use"),
    );
    record["message"]["model"] = json!(model);
    record
}

/// The result of the tool call `call`.
fn result_of(call: &str, uuid: &str, parent: &str) -> Value {
    user(
        uuid,
        json!(parent),
        json!([{ "type": "tool_result", "tool_use_id": call, "content": "ok" }]),
    )
}

/// The user's command `command`, as Claude Code writes what one typed at its prompt.
fn typed(uuid: &str, parent: &str, command: &str) -> Value {
    user(
        uuid,
        json!(parent),
        json!(format!("<command-name>{command}</command-name>")),
    )
}

/// What a reader says of the model: the move it holds a window for, and the drift it only tells.
fn models_of(records: &[Value], options: &Options) -> (Option<ModelMoved>, Option<ModelMoved>) {
    let mut stage = Stage::new();
    stage.write(records);
    match &*stage.look_told(options) {
        Reading::Known(record) => (
            record.model_moved.as_deref().cloned(),
            record.model_drift.as_deref().cloned(),
        ),
        other => panic!("not known: {other:?}"),
    }
}

fn plain(records: &[Value]) -> (Option<ModelMoved>, Option<ModelMoved>) {
    models_of(records, &Options::default())
}

fn pair(moved: &ModelMoved) -> (&str, &str) {
    (moved.from.as_str(), moved.to.as_str())
}

#[test]
fn a_fallback_block_is_claude_codes_own_word_for_a_move_and_the_only_one_that_holds() {
    // As the real ones are: the block alone in its record, even where nothing answered before.
    let (moved, drift) = plain(&[
        hello(),
        fell_back("claude-fable-5", "claude-opus-4-8", "a1", "u1", "m1"),
    ]);
    let moved = moved.expect("a move");
    assert_eq!(pair(&moved), ("claude-fable-5", "claude-opus-4-8"));
    assert_eq!(
        moved.because,
        "Claude Code fell back to another model (its own `fallback` record)"
    );
    assert_eq!(moved.at.as_deref(), Some("2026-09-19T10:00:00.000Z"));
    assert_eq!(drift, None, "the move is told by the hold, not twice");
    // After answers on the model it left, the move is the same.
    let (moved, drift) = plain(&[
        hello(),
        on("claude-fable-5", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("More")),
        fell_back("claude-fable-5", "claude-opus-4-8", "a2", "u2", "m2"),
    ]);
    assert_eq!(
        pair(&moved.expect("a move")),
        ("claude-fable-5", "claude-opus-4-8")
    );
    assert_eq!(drift, None);
}

#[test]
fn a_model_that_only_drifts_in_the_records_is_told_and_never_held() {
    // An alias repointed from under a running session: the next record names the new model.
    let (moved, drift) = plain(&[
        hello(),
        on("claude-opus-5", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("More")),
        on("claude-opus-5-5", "a2", "u2", "m2"),
    ]);
    assert_eq!(moved, None, "no word of the harness's: no hold");
    let drift = drift.expect("a drift to tell");
    assert_eq!(pair(&drift), ("claude-opus-5", "claude-opus-5-5"));
    assert_eq!(
        drift.because,
        "its records name another model with no word of Claude Code's own for it (an alias repointed, a restart, or opusplan)"
    );
}

#[test]
fn a_repoint_in_the_middle_of_a_tool_loop_and_opusplans_flip_are_notes_and_no_hold() {
    // Three seconds after the last record, the loop goes on on the new model: no restart.
    let (moved, drift) = plain(&[
        hello(),
        calling("claude-opus-5", "c1", "a1", "u1", "m1"),
        result_of("c1", "u2", "a1"),
        calling("claude-opus-5-5", "c2", "a2", "u2", "m2"),
        result_of("c2", "u3", "a2"),
        calling("claude-opus-5-5", "c3", "a3", "u3", "m3"),
    ]);
    assert_eq!(moved, None);
    assert_eq!(
        pair(&drift.expect("a note")),
        ("claude-opus-5", "claude-opus-5-5")
    );
    // `opusplan`: Opus plans, Sonnet acts, and back.
    let (moved, drift) = plain(&[
        hello(),
        on("claude-opus-5-5", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("Go")),
        on("claude-sonnet-5-5", "a2", "u2", "m2"),
        user("u3", json!("a2"), json!("Plan")),
        on("claude-opus-5-5", "a3", "u3", "m3"),
    ]);
    assert_eq!(
        moved, None,
        "a flip of the alias is no switch of the harness's"
    );
    assert_eq!(
        pair(&drift.expect("the flip is told")),
        ("claude-sonnet-5-5", "claude-opus-5-5")
    );
}

#[test]
fn the_humans_model_command_a_synthetic_message_a_subagent_and_fast_are_neither_held_nor_told() {
    // The same model, or a synthetic message (Claude Code's own, `<synthetic>`) between.
    assert_eq!(
        plain(&[
            hello(),
            on("claude-opus-5-5", "a1", "u1", "m1"),
            on("<synthetic>", "a2", "a1", "m2"),
            on("claude-opus-5-5", "a3", "a2", "m3"),
        ]),
        (None, None)
    );
    // The human typed `/model`: theirs.
    let theirs = [
        hello(),
        on("claude-opus-5-5", "a1", "u1", "m1"),
        typed("u2", "a1", "/model"),
        on("claude-sonnet-5-5", "a2", "u2", "m2"),
    ];
    assert_eq!(plain(&theirs), (None, None));
    // `/fast` switches the model Claude Code runs a turn on too (its fast mode): the human's.
    assert_eq!(
        plain(&[
            hello(),
            on("claude-opus-5-5", "a1", "u1", "m1"),
            typed("u2", "a1", "/fast"),
            on("claude-sonnet-5-5", "a2", "u2", "m2"),
        ]),
        (None, None)
    );
    // The command is the next change's alone: a later one with none is a drift to tell.
    let mut later = theirs.to_vec();
    later.push(user("u3", json!("a2"), json!("More")));
    later.push(on("claude-haiku-5-5", "a3", "u3", "m3"));
    let (moved, drift) = plain(&later);
    assert_eq!(moved, None);
    assert_eq!(
        pair(&drift.expect("a later drift")),
        ("claude-sonnet-5-5", "claude-haiku-5-5")
    );
    // A subagent's messages (another conversation of the file) run on models of their own.
    let mut child = on("claude-haiku-5-5", "s1", "a1", "m9");
    child["isSidechain"] = json!(true);
    let mut child_fallback = fell_back("claude-fable-5", "claude-opus-4-8", "s2", "s1", "m10");
    child_fallback["isSidechain"] = json!(true);
    assert_eq!(
        plain(&[
            hello(),
            on("claude-opus-5-5", "a1", "u1", "m1"),
            child,
            child_fallback
        ]),
        (None, None)
    );
}

#[test]
fn a_change_before_the_relaunch_boundary_is_neither_held_nor_told_and_one_after_it_is() {
    let records = [
        hello(),
        fell_back("claude-fable-5", "claude-opus-4-8", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("More")),
        on("claude-opus-4-8", "a2", "u2", "m2"),
        user("u3", json!("a2"), json!("Again")),
        on("claude-sonnet-5-5", "a3", "u3", "m3"),
    ];
    // ConsensFlow started the program that wrote the records from the `before`th on: what
    // the old one wrote is history, and the window runs on the model its launch named.
    let after = |before: usize| Options {
        relaunched_at: Some(Stage::lines(&records[..before]).len() as u64),
        ..Options::default()
    };
    // Past the fallback and the answer after it, the first answer is the new baseline.
    assert_eq!(models_of(&records, &after(4)), (None, None));
    // The fallback is the old program's, and a change the new one wrote is a drift to tell.
    let (moved, drift) = models_of(&records, &after(2));
    assert_eq!(moved, None, "the old program's fallback is history");
    assert_eq!(
        pair(&drift.expect("the change after the boundary")),
        ("claude-opus-4-8", "claude-sonnet-5-5")
    );
    // A fallback written by the program the boundary begins is held for.
    let (moved, _) = models_of(&records, &after(1));
    assert_eq!(
        pair(&moved.expect("a move after the boundary")),
        ("claude-fable-5", "claude-opus-4-8")
    );
    // No boundary told (a daemon that did not start the program): the move is read, dated
    // by its record, and the engine ignores it as older than the window (`dated_after_open`).
    let (moved, _) = plain(&records);
    assert_eq!(
        moved.and_then(|moved| moved.at).as_deref(),
        Some("2026-09-19T10:00:00.000Z")
    );
}

#[test]
fn the_humans_model_command_back_ends_a_move_and_a_drift() {
    // The real session `3afd6cd4`: a fallback, then the human's `/model` back.
    let (moved, drift) = plain(&[
        hello(),
        fell_back("claude-fable-5", "claude-opus-4-8", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("More")),
        typed("u3", "u2", "/model"),
        on("claude-fable-5", "a2", "u3", "m2"),
    ]);
    assert_eq!((moved, drift), (None, None), "the human put it back");
    let (moved, drift) = plain(&[
        hello(),
        on("claude-opus-5", "a1", "u1", "m1"),
        on("claude-opus-5-5", "a2", "a1", "m2"),
        typed("u2", "a2", "/model"),
        on("claude-opus-5", "a3", "u2", "m3"),
    ]);
    assert_eq!((moved, drift), (None, None));
}

#[test]
fn a_flip_between_the_models_an_alias_runs_on_is_no_drift_where_the_launch_names_the_alias() {
    // `opusplan` plans on Opus and works on Sonnet: its records name each in turn, all day, and
    // the human chose both by choosing it (T-841). A launch that names another model, or a model
    // the alias does not run on, is told as any change is.
    let flip = [
        hello(),
        on("claude-opus-5-5", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("Go")),
        on("claude-sonnet-5-5", "a2", "u2", "m2"),
        user("u3", json!("a2"), json!("Plan")),
        on("claude-opus-5-5", "a3", "u3", "m3"),
    ];
    let launched = |model: &str| Options {
        launch_model: Some(model.to_owned()),
        ..Options::default()
    };
    for named in ["opusplan", "OpusPlan", "opusplan[1m]"] {
        assert_eq!(models_of(&flip, &launched(named)), (None, None), "{named}");
    }
    for named in ["claude-opus-5-5", "sonnet", "opus"] {
        let (moved, drift) = models_of(&flip, &launched(named));
        assert_eq!(moved, None, "{named}");
        assert_eq!(
            pair(&drift.unwrap_or_else(|| panic!("{named}: a drift to tell"))),
            ("claude-sonnet-5-5", "claude-opus-5-5"),
            "{named}"
        );
    }
    assert!(plain(&flip).1.is_some(), "no launch told: a drift");
    // A model outside what the alias runs on is still news, and a fallback is held for.
    let other = [
        hello(),
        on("claude-opus-5-5", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("Go")),
        on("claude-haiku-5-5", "a2", "u2", "m2"),
    ];
    let (_, drift) = models_of(&other, &launched("opusplan"));
    assert_eq!(
        pair(&drift.expect("outside the alias")),
        ("claude-opus-5-5", "claude-haiku-5-5")
    );
    let fallback = [
        hello(),
        on("claude-opus-5-5", "a1", "u1", "m1"),
        fell_back("claude-opus-5-5", "claude-sonnet-5-5", "a2", "a1", "m2"),
    ];
    assert!(models_of(&fallback, &launched("opusplan")).0.is_some());
}

/// Opus in plan mode and Sonnet at work, then Opus again: the flips of an alias that runs on two.
fn flips() -> Vec<Value> {
    vec![
        hello(),
        on("claude-opus-5-5", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("Go")),
        on("claude-sonnet-5-5", "a2", "u2", "m2"),
        user("u3", json!("a2"), json!("Plan")),
        on("claude-opus-5-5", "a3", "u3", "m3"),
    ]
}

fn drift_after(stage: &mut Stage, options: &Options) -> Option<ModelMoved> {
    match &*stage.look_told(options) {
        Reading::Known(record) => record.model_drift.as_deref().cloned(),
        other => panic!("not known: {other:?}"),
    }
}

#[test]
fn a_look_launched_with_no_model_reads_the_flips_again_where_an_earlier_one_named_the_alias() {
    // The reader is kept per conversation and told the launch's model by each look (T-853): a
    // later window on the conversation launched with another model, or none, is not an
    // `opusplan` window, and its Opus-to-Sonnet changes are told.
    let mut stage = Stage::new();
    stage.write(&flips());
    let alias = Options {
        launch_model: Some("opusplan".to_owned()),
        ..Options::default()
    };
    assert_eq!(drift_after(&mut stage, &alias), None);
    let drift = drift_after(&mut stage, &Options::default()).expect("a drift to tell");
    assert_eq!(pair(&drift), ("claude-sonnet-5-5", "claude-opus-5-5"));
    // And an alias named again is no news again.
    assert_eq!(drift_after(&mut stage, &alias), None);
}

#[test]
fn the_default_alias_flips_between_opus_and_sonnet_by_design_too() {
    // Claude Code's `default` model is Opus, and Sonnet past half the limit on Max (T-853).
    let named = |model: &str| Options {
        launch_model: Some(model.to_owned()),
        ..Options::default()
    };
    assert_eq!(models_of(&flips(), &named("default")), (None, None));
    assert_eq!(models_of(&flips(), &named("Default")), (None, None));
    // A model it does not run on is still news.
    let other = [
        hello(),
        on("claude-opus-5-5", "a1", "u1", "m1"),
        user("u2", json!("a1"), json!("Go")),
        on("claude-haiku-5-5", "a2", "u2", "m2"),
    ];
    let (_, drift) = models_of(&other, &named("default"));
    assert_eq!(
        pair(&drift.expect("outside the alias")),
        ("claude-opus-5-5", "claude-haiku-5-5")
    );
}
