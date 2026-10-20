//! 7 (B5): a message delivered twice into a conversation, or joined to another in
//! one prompt.

use cf_evals::long_run::invariants;
use cf_evals::long_run::look::Look;
use serde_json::json;

use crate::scene::{found, ms, t, Scene, CHIEF};

/// The chief's conversation, and T-9's results m-246 and m-248 in the ledger once.
fn chief(scene: &Scene) -> i64 {
    chief_on(scene, "devin")
}

/// [`chief`], the chief's window on `harness`.
fn chief_on(scene: &Scene, harness: &str) -> i64 {
    let conversation = scene.conversation(CHIEF, harness, &format!("{harness}-chief"));
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
fn a_message_given_in_two_prompts_breaks_the_seventh_wrapped_as_claude_code_keeps_a_paste_or_not() {
    // Two real prompts of m-246: each a delivery, as it went in.
    let scene = Scene::new(&[("long-worker", "devin")]);
    let conversation = chief(&scene);
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    scene.item(conversation, 2, "user", FIRST, true, &t(2, 2));
    let said: Vec<String> = found(&scene.look(ms(4, 0)), 7)
        .into_iter()
        .map(|found| found.said)
        .collect();
    assert_eq!(said, ["@chief was given m-246 2 times"]);
    // Claude Code 2.1.303 keeps a long paste in its record wrapped, and the copy with it:
    // a wrapped delivery is one, given twice or joined to the next as any is.
    let pasted = |inner: &str| {
        format!("\n\n<pasted_content id=\"a1b2\">\n{inner}\n</pasted_content id=\"a1b2\">\n")
    };
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let conversation = chief_on(&scene, "claude-code");
    scene.item(conversation, 0, "user", &pasted(FIRST), true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    scene.item(
        conversation,
        2,
        "user",
        &format!("{}{}", pasted(FIRST), pasted(SECOND)),
        true,
        &t(3, 2),
    );
    let said: Vec<String> = found(&scene.look(ms(4, 0)), 7)
        .into_iter()
        .map(|found| found.said)
        .collect();
    assert_eq!(
        said,
        [
            "@chief was given m-246 and m-248 as one prompt",
            "@chief was given m-246 2 times"
        ]
    );
}

#[test]
fn a_compact_summary_or_a_reviewers_history_that_quotes_a_delivery_is_none_and_holds_the_seventh() {
    // The Mac's 2-hour run of 2026-10-19: the owner's /compact, and Claude Code's summary
    // of the conversation in the copy as a user's item, the last prompt quoted whole in it
    // (made-up words here). It is no prompt the window was given.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let conversation = chief_on(&scene, "claude-code");
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    scene.item(conversation, 2, "user", "/compact", true, &t(2, 0));
    scene.item(
        conversation,
        3,
        "user",
        &format!(
            "This session is being continued from a previous conversation that ran out of context. \
             The summary below covers the earlier portion of the conversation.\n\n\
             Summary:\n1. Primary Request and Intent: plan the phases and put them on the board.\n\
             2. All user messages:\n   - {FIRST}\n3. Current Work: waiting for T-9.\n"
        ),
        true,
        &t(2, 5),
    );
    assert!(
        found(&scene.look(ms(4, 0)), 7).is_empty(),
        "{:?}",
        found(&scene.look(ms(4, 0)), 7)
    );
    assert!(undecided(&scene.look(ms(4, 0))).is_empty());
    // Codex gives its approval reviewer the agent's history, every delivery's header in it:
    // none is given again.
    let scene = Scene::new(&[("long-worker", "codex")]);
    let conversation = chief_on(&scene, "codex");
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "user", SECOND, true, &t(3, 2));
    scene.item(
        conversation,
        2,
        "user",
        &reviewer(&format!("{FIRST}\n{SECOND}"), ""),
        true,
        &t(3, 30),
    );
    assert!(found(&scene.look(ms(4, 0)), 7).is_empty());
    assert!(undecided(&scene.look(ms(4, 0))).is_empty());
}

/// The prompt Codex gives its approval reviewer, in its shape on the owner's machine
/// (October 2026; made-up words): the history the agent was given (`history`, a line of it
/// each) between its markers, the request between its own, and `after` the request's end.
fn reviewer(history: &str, after: &str) -> String {
    let entries: Vec<String> = history
        .lines()
        .enumerate()
        .map(|(at, line)| format!("[{}] user: {line}", at + 1))
        .collect();
    format!(
        "The following is the Codex agent history whose request action you are assessing.\n\
         >>> TRANSCRIPT START\n{}\n>>> TRANSCRIPT END\n\nAssess the request.\n\
         >>> APPROVAL REQUEST START\nnpm test\n>>> APPROVAL REQUEST END\n{after}",
        entries.join("\n")
    )
}

/// What a look could not judge of the seventh: each key, and what it says.
fn undecided(look: &Look) -> Vec<(String, String)> {
    invariants::unjudged(look, &Default::default())
        .into_iter()
        .filter(|open| open.invariant == 7)
        .map(|open| (open.key, open.said))
        .collect()
}

fn said_of(look: &Look) -> Vec<String> {
    found(look, 7).into_iter().map(|found| found.said).collect()
}

#[test]
fn a_delivery_behind_other_words_is_one_and_given_again_breaks_the_seventh() {
    // GPT-6.1-Sol's review of round b1: Devin pastes behind what its input box holds, and
    // Claude Code keeps a paste wrapped with what was typed with it. A second m-246 behind
    // `continue`, inside the wrapper behind it, or in a wrapper behind it, is m-246 given
    // twice.
    for second in [
        format!("continue\n{FIRST}"),
        format!(
            "\n\n<pasted_content id=\"a1b2\">\ncontinue\n{FIRST}\n</pasted_content id=\"a1b2\">\n"
        ),
        format!(
            "continue\n\n<pasted_content id=\"a1b2\">\n{FIRST}\n</pasted_content id=\"a1b2\">\n"
        ),
    ] {
        let scene = Scene::new(&[("long-worker", "devin")]);
        let conversation = chief(&scene);
        scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
        scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
        scene.item(conversation, 2, "user", &second, true, &t(2, 2));
        let look = scene.look(ms(4, 0));
        assert_eq!(
            said_of(&look),
            ["@chief was given m-246 2 times"],
            "{second}"
        );
        assert!(undecided(&look).is_empty(), "{second}");
    }
}

#[test]
fn a_header_after_other_words_on_its_line_is_unresolved_never_dropped() {
    // Whether a header after other words on its line went in behind them (a paste glued to
    // what the box held) or is quoted by them, the copy does not say: no break, and no pass.
    for (prompt, id) in [
        (format!("continue{FIRST}"), 246),
        (
            "Why is \"[ConsensFlow m-248 · T-9 · result from @long-worker]\" queued?".to_owned(),
            248,
        ),
    ] {
        let scene = Scene::new(&[("long-worker", "devin")]);
        let conversation = chief(&scene);
        scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
        scene.item(conversation, 1, "user", &prompt, true, &t(2, 2));
        let look = scene.look(ms(4, 0));
        assert!(said_of(&look).is_empty(), "{prompt}");
        assert_eq!(
            undecided(&look),
            [(
                format!("7:undecided:{conversation}:i-{conversation}-1"),
                format!("@chief's prompt i-{conversation}-1 holds m-{id}'s header after other words on its line: whether it went in behind them or they quote it, the copy does not say")
            )],
            "{prompt}"
        );
    }
    // Claude Code's notification of a job's end is its own words, a header in its output
    // none: no delivery, and judged.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let conversation = chief_on(&scene, "claude-code");
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(
        conversation,
        1,
        "user",
        &format!("<task-notification>\n<task-id>b1</task-id>\n<output>queued: {FIRST}</output>\n</task-notification>"),
        true,
        &t(2, 2),
    );
    let look = scene.look(ms(4, 0));
    assert!(said_of(&look).is_empty());
    assert!(undecided(&look).is_empty());
}

/// A scene whose chief, on `harness`, was given m-246 (`FIRST`) at 10:01:02, then `second`:
/// what the seventh found of it, and what it could not judge.
fn after_first(harness: &str, second: &str) -> (Vec<String>, Vec<(String, String)>) {
    let scene = Scene::new(&[("long-worker", harness)]);
    let conversation = chief_on(&scene, harness);
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    scene.item(conversation, 2, "user", second, true, &t(2, 2));
    let look = scene.look(ms(4, 0));
    (said_of(&look), undecided(&look))
}

const TWICE: &str = "@chief was given m-246 2 times";

#[test]
fn a_synthetic_block_hides_only_itself_and_only_in_the_harness_that_writes_it() {
    // Both reviewers of round b2: an exemption hid the whole row. Sol's: Claude Code's
    // notification of a job's end, closed, and m-246 again after it.
    let notification = "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>";
    let (said, open) = after_first("claude-code", &format!("{notification}\n{FIRST}"));
    assert_eq!((said, open), (vec![TWICE.to_owned()], Vec::new()));
    // Astra's: a Devin draft that holds such a notification, and the retried m-246 after it.
    // Devin writes none of it: nothing of the row is exempt.
    let (said, open) = after_first("devin", &format!("{notification}\n{FIRST}"));
    assert_eq!((said, open), (vec![TWICE.to_owned()], Vec::new()));
    // The history Codex gives its approval reviewer, and m-246 after the request's end.
    let (said, open) = after_first("codex", &reviewer("the brief", &format!("{FIRST}\n")));
    assert_eq!((said, open), (vec![TWICE.to_owned()], Vec::new()));
    // Only the harness that writes a block has it: in a Devin row, a notification and a
    // reviewer's prompt are words like any, a header in them judged where it stands.
    let inside = format!("<task-notification>\n{FIRST}\n</task-notification>");
    let (said, open) = after_first("devin", &inside);
    assert_eq!((said, open), (vec![TWICE.to_owned()], Vec::new()));
    let (said, open) = after_first("devin", &reviewer(FIRST, ""));
    assert!(said.is_empty(), "{said:?}");
    assert_eq!(open.len(), 1, "{open:?}");
    assert!(
        open[0].1.ends_with(" holds m-246's header after other words on its line: whether it went in behind them or they quote it, the copy does not say"),
        "{open:?}"
    );
    // Inside each, nothing is given.
    let quoted = format!("<task-notification>\n<output>\n{FIRST}\n</output>\n</task-notification>");
    assert_eq!(
        after_first("claude-code", &quoted),
        (Vec::new(), Vec::new())
    );
    assert_eq!(
        after_first("codex", &reviewer(FIRST, "")),
        (Vec::new(), Vec::new())
    );
}

#[test]
fn a_synthetic_block_whose_end_the_copy_does_not_show_is_unresolved() {
    // A notification that does not close, the reviewer's history with no end, and the words
    // a summary opens with in a harness that writes none: whether the header after them went
    // in, the copy does not say.
    for (harness, second, after) in [
        (
            "claude-code",
            format!("<task-notification>\n<task-id>b1</task-id>\n{FIRST}"),
            "Claude Code's notification of a job's end, which does not close",
        ),
        (
            "codex",
            format!(
                "The following is the Codex agent history whose request action you are assessing.\n\
                 >>> TRANSCRIPT START\n[1] user: {FIRST}"
            ),
            "the history Codex gives its approval reviewer, which does not end",
        ),
        (
            "codex",
            format!(
                "The following is the Codex agent history whose request action you are assessing.\n\
                 >>> TRANSCRIPT START\n[1] user: the brief\n>>> TRANSCRIPT END\n\
                 >>> APPROVAL REQUEST START\n{FIRST}"
            ),
            "the request Codex gives its approval reviewer, which does not end",
        ),
    ] {
        let (said, open) = after_first(harness, &second);
        assert!(said.is_empty(), "{harness}: {said:?}");
        assert_eq!(
            open,
            [(
                "7:undecided:1:i-1-2".to_owned(),
                format!("@chief's prompt i-1-2 holds m-246's header after {after}: whether it went in, the copy does not say")
            )],
            "{harness}"
        );
    }
    let summary = format!(
        "This session is being continued from a previous conversation that ran out of context.\n{FIRST}"
    );
    assert_eq!(
        after_first("devin", &summary),
        (vec![TWICE.to_owned()], Vec::new())
    );
}

#[test]
fn a_header_in_a_fenced_block_or_a_quoted_line_is_no_delivery_and_unresolved() {
    // GPT-6-Astra's review of round b2: a person's words that quote a header on its own line,
    // inside a fence, were taken for a delivery.
    for (second, why) in [
        (
            format!("Look at this:\n```\n{FIRST}\n```"),
            "in a quotation (a fenced block or a quoted line)",
        ),
        (
            format!("Look at this:\n~~~text\n{FIRST}\n~~~"),
            "in a quotation (a fenced block or a quoted line)",
        ),
        (
            format!("Look at this:\n> {FIRST}"),
            "in a quotation (a fenced block or a quoted line)",
        ),
    ] {
        let (said, open) = after_first("devin", &second);
        assert!(said.is_empty(), "{second}: {said:?}");
        assert_eq!(
            open,
            [(
                "7:undecided:1:i-1-2".to_owned(),
                format!("@chief's prompt i-1-2 holds m-246's header {why}: whether it went in, the copy does not say")
            )],
            "{second}"
        );
    }
    // A fence that closed before it leaves the header outside it.
    let (said, open) = after_first("devin", &format!("```\nnpm test\n```\n{FIRST}"));
    assert_eq!((said, open), (vec![TWICE.to_owned()], Vec::new()));
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
