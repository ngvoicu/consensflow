//! 7 (B5): a message delivered twice into a conversation, or joined to another in
//! one prompt.

use serde_json::json;

use crate::scene::{found, ms, t, Scene, CHIEF};

/// The chief's conversation, and T-9's results m-246 and m-248 in the ledger once.
fn chief(scene: &Scene) -> i64 {
    let conversation = scene.conversation(CHIEF, "devin", "devin-chief");
    let task = scene.task(9, 3, "done", &t(3, 0));
    scene.message(
        246,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Phase 1 done.",
        &t(1, 0),
    );
    scene.message(
        248,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Phase 2 done.",
        &t(3, 0),
    );
    scene.event(
        &t(1, 1),
        "delivery.begun",
        &json!({ "message": 246, "attempt": 1 }),
    );
    conversation
}

const FIRST: &str = "[ConsensFlow m-246 · T-9 · result from @long-worker]\nPhase 1 done.";
const SECOND: &str = "[ConsensFlow m-248 · T-9 · result from @long-worker]\nPhase 2 done.";

#[test]
fn a_message_given_twice_and_one_glued_to_the_next_break_the_seventh() {
    let scene = Scene::new(&[("long-worker", "devin")]);
    let conversation = chief(&scene);
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    // The second copy of m-246 stayed in the box and went in with m-248.
    scene.item(
        conversation,
        2,
        "user",
        &format!("{FIRST}{SECOND}"),
        true,
        &t(3, 2),
    );
    let breaks = found(&scene.look(ms(4, 0)), 7);
    let said: Vec<&str> = breaks.iter().map(|found| found.said.as_str()).collect();
    assert_eq!(
        said,
        [
            "@chief was given m-246 and m-248 as one prompt",
            "@chief was given m-246 2 times"
        ]
    );
    assert!(breaks[1]
        .evidence
        .rows
        .iter()
        .any(|row| row.contains("delivery.begun")));
}

#[test]
fn each_message_given_once_or_a_carrier_or_a_quote_holds_the_seventh() {
    let scene = Scene::new(&[("long-worker", "devin")]);
    let conversation = chief(&scene);
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "user", SECOND, true, &t(3, 2));
    // A message that carries another is one paste under its own header.
    scene.item(
        conversation,
        2,
        "user",
        "[ConsensFlow m-250 · T-9 · note from @long-worker]\n(kept for you: result m-249 from @long-worker)\nMore.\n\nAnd this.",
        true,
        &t(4, 0),
    );
    // What a handoff quotes of the history is defused.
    scene.item(conversation, 3, "user", "[ConsensFlow m-251 · note from ConsensFlow]\nEarlier: [earlier m-246 · T-9 · result from @long-worker]", true, &t(5, 0));
    // A result that quotes the brief it was given, header and all (the dry run's
    // stand-in worker answers `noted: ` and the brief's first line): one delivery.
    let quoting = "noted: [ConsensFlow m-247 · T-9 · task from @human]";
    scene.message(252, (CHIEF, Some(3)), "result", None, quoting, &t(5, 30));
    scene.item(
        conversation,
        4,
        "user",
        &format!("[ConsensFlow m-252 · T-9 · result from @long-worker]\n{quoting}"),
        true,
        &t(5, 31),
    );
    assert!(found(&scene.look(ms(6, 0)), 7).is_empty());
    // The same message in two conversations of a participant (a window that died
    // before its record showed it, and the new one) is one delivery to each.
    scene.end_conversation(conversation, &t(6, 30));
    let other = scene.conversation(CHIEF, "devin", "devin-chief-2");
    scene.item(other, 0, "user", FIRST, true, &t(7, 0));
    assert!(found(&scene.look(ms(8, 0)), 7).is_empty());
}
