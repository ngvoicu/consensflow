//! 5 (A5): a "turn was cut" note to a window whose record was at rest. The chief's
//! record of 2026-10-14 and 15 (project "patent"): a turn ended, Claude Code compacted
//! the idle conversation, and the Resume told the chief its turn was cut.

use cf_evals::long_run::record::Record;
use serde_json::json;

use crate::scene::{found, ms, t, Scene, CHIEF};

const CHIEF_CUT: &str =
    "ConsensFlow stopped in the middle of your turn, which was cut. Go on where you stopped.";

/// The chief's record: a turn that ended at 10:01, and `after` it.
fn chief_record(after: &[serde_json::Value]) -> Record {
    let mut lines = vec![
        json!({ "timestamp": t(0, 0), "type": "user", "message": { "role": "user", "content": "Plan the phases." } }),
        json!({ "timestamp": t(1, 0), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Planned; three phases on the board." }] } }),
        json!({ "timestamp": t(1, 0), "type": "system", "subtype": "turn_duration", "isSidechain": false, "durationMs": 60000, "messageCount": 2 }),
    ];
    lines.extend(after.iter().cloned());
    Record::claude(
        &lines
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// A chief (2) whose conversation is `session`, told at 11:00 its turn was cut.
fn told_cut(scene: &Scene) -> i64 {
    let conversation = scene.conversation(CHIEF, "claude-code", "s-chief");
    scene.message(2063, (CHIEF, None), "note", None, CHIEF_CUT, &t(60, 0));
    conversation
}

#[test]
fn a_cut_note_to_a_chief_at_rest_after_an_idle_compaction_breaks_the_fifth() {
    let scene = Scene::new(&[]);
    let conversation = told_cut(&scene);
    let record = chief_record(&[
        json!({ "timestamp": t(54, 0), "type": "system", "subtype": "compact_boundary" }),
        json!({ "timestamp": t(54, 1), "type": "user", "isCompactSummary": true, "message": { "role": "user", "content": "This session is being continued from a previous conversation." } }),
    ]);
    let breaks = found(&scene.look_with(ms(61, 0), conversation, record), 5);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].key, "5:m-2063");
    assert!(breaks[0]
        .said
        .starts_with("@chief was told its turn was cut (m-2063)"));
    assert!(
        breaks[0]
            .evidence
            .record
            .iter()
            .any(|line| line.contains("Compaction")),
        "{:?}",
        breaks[0].evidence.record
    );
}

/// What a look could not judge of the fifth, by key.
fn unjudged_fifth(look: &cf_evals::long_run::look::Look) -> Vec<String> {
    cf_evals::long_run::invariants::unjudged(look, &Default::default())
        .into_iter()
        .filter(|open| open.invariant == 5)
        .map(|open| open.key)
        .collect()
}

#[test]
fn a_cut_note_to_a_chief_whose_record_was_not_read_through_it_is_unresolved_never_the_copys() {
    // Astra's: the chief compacted at rest at 10:54, its record read at 10:55 and then
    // never again; at 11:00 it was told its turn was cut. The copy holds the
    // compaction's summary as the user's, which reads as a turn at work.
    let scene = Scene::new(&[]);
    let conversation = told_cut(&scene);
    scene.item(
        conversation,
        0,
        "user",
        "This session is being continued from a previous conversation.",
        true,
        &t(54, 1),
    );
    let record = chief_record(&[
        json!({ "timestamp": t(54, 0), "type": "system", "subtype": "compact_boundary" }),
        json!({ "timestamp": t(54, 1), "type": "user", "isCompactSummary": true, "message": { "role": "user", "content": "This session is being continued from a previous conversation." } }),
    ]);
    let unreadable = "its record could not be read: Input/output error (os error 5)";
    let look = scene.look_read(
        ms(61, 0),
        conversation,
        record.clone(),
        (ms(55, 0), unreadable),
    );
    assert!(found(&look, 5).is_empty(), "{:?}", found(&look, 5));
    assert_eq!(unjudged_fifth(&look), ["5:m-2063"]);
    // Read through the note's moment before its readings failed: the compaction stays
    // read, and the note breaks the fifth.
    let look = scene.look_read(ms(61, 0), conversation, record, (ms(60, 30), unreadable));
    assert_eq!(found(&look, 5).len(), 1, "{:?}", found(&look, 5));
    assert!(unjudged_fifth(&look).is_empty());
}

#[test]
fn a_cut_note_to_a_chief_whose_turn_was_at_work_holds_the_fifth() {
    let scene = Scene::new(&[]);
    let conversation = told_cut(&scene);
    let record = chief_record(&[
        json!({ "timestamp": t(55, 0), "type": "user", "message": { "role": "user", "content": "[ConsensFlow m-2050 · result from @long-worker]\nDone." } }),
    ]);
    assert!(found(&scene.look_with(ms(61, 0), conversation, record), 5).is_empty());
}

#[test]
fn a_cut_note_to_a_chief_whose_stop_hook_still_ran_holds_the_fifth() {
    // The chief's answer is written at 10:55 and its stop hook runs; the app quits at
    // 10:58 before the hook is done, and the Resume says the turn was cut: it was.
    let scene = Scene::new(&[]);
    let conversation = told_cut(&scene);
    let record = chief_record(&[
        json!({ "timestamp": t(54, 0), "type": "user", "message": { "role": "user", "content": "[ConsensFlow m-2050 · result from @long-worker]\nDone." } }),
        json!({ "timestamp": t(55, 0), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Accepted T-3." }] } }),
    ]);
    assert!(found(&scene.look_with(ms(61, 0), conversation, record), 5).is_empty());
    // A hook that ran and prevented the end: the agent went on, and the turn with it.
    let scene = Scene::new(&[]);
    let conversation = told_cut(&scene);
    let record = chief_record(&[
        json!({ "timestamp": t(54, 0), "type": "user", "message": { "role": "user", "content": "[ConsensFlow m-2050 · result from @long-worker]\nDone." } }),
        json!({ "timestamp": t(55, 0), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Accepted T-3." }] } }),
        json!({ "timestamp": t(56, 0), "type": "system", "subtype": "stop_hook_summary", "preventedContinuation": true }),
    ]);
    assert!(found(&scene.look_with(ms(61, 0), conversation, record), 5).is_empty());
    // A subagent's duration is none of the chief's turn's end.
    let scene = Scene::new(&[]);
    let conversation = told_cut(&scene);
    let record = chief_record(&[
        json!({ "timestamp": t(54, 0), "type": "user", "message": { "role": "user", "content": "[ConsensFlow m-2050 · result from @long-worker]\nDone." } }),
        json!({ "timestamp": t(55, 0), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Accepted T-3." }] } }),
        json!({ "timestamp": t(56, 0), "type": "system", "subtype": "turn_duration", "isSidechain": true, "durationMs": 900, "messageCount": 3 }),
    ]);
    assert!(found(&scene.look_with(ms(61, 0), conversation, record), 5).is_empty());
}

#[test]
fn another_harness_is_read_from_the_copy_its_turn_at_rest_or_at_work() {
    let scene = Scene::new(&[("long-worker", "codex")]);
    let conversation = scene.conversation(3, "codex", "thread-1");
    let task = scene.task(7, 3, "working", &t(0, 0));
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-1 · T-7 · task from @chief]\nBuild",
        true,
        &t(0, 0),
    );
    scene.item(conversation, 1, "assistant", "Built.", true, &t(2, 0));
    scene.message(
        90,
        (3, None),
        "note",
        Some(task),
        "ConsensFlow quit while you were working on T-7. Go on with it.",
        &t(10, 0),
    );
    assert_eq!(found(&scene.look(ms(11, 0)), 5).len(), 1);
    let scene = Scene::new(&[("long-worker", "codex")]);
    let conversation = scene.conversation(3, "codex", "thread-1");
    let task = scene.task(7, 3, "working", &t(0, 0));
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-1 · T-7 · task from @chief]\nBuild",
        true,
        &t(0, 0),
    );
    scene.item(conversation, 1, "tool", "npm run build", true, &t(1, 0));
    scene.message(
        90,
        (3, None),
        "note",
        Some(task),
        "ConsensFlow quit while you were working on T-7. Go on with it.",
        &t(10, 0),
    );
    assert!(found(&scene.look(ms(11, 0)), 5).is_empty());
}
