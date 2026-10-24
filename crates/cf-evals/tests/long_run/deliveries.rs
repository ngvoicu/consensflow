//! 7 (B5): a message delivered twice into a conversation, or joined to another in
//! one prompt.

use cf_evals::long_run::look::{Look, Reading};
use cf_evals::long_run::record::Record;
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

const TWICE: &str = "@chief was given m-246 2 times";

fn said_of(look: &Look) -> Vec<String> {
    found(look, 7).into_iter().map(|found| found.said).collect()
}

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
    assert_eq!(said_of(&scene.look(ms(4, 0))), [TWICE]);
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
    assert_eq!(
        said_of(&scene.look(ms(4, 0))),
        ["@chief was given m-246 and m-248 as one prompt", TWICE]
    );
}

/// Claude Code's summary of the conversation at a compaction (made-up words), the prompts
/// before it quoted whole, m-246 among them.
fn summary() -> String {
    format!(
        "This session is being continued from a previous conversation that ran out of context. \
         The summary below covers the earlier portion of the conversation.\n\n\
         Summary:\n1. Primary Request and Intent: plan the phases and put them on the board.\n\
         2. All user messages:\n   - {FIRST}\n3. Current Work: waiting for T-9.\n"
    )
}

/// Claude Code's line of the user's record `uuid` that says `text`, marked as its summary of
/// a compaction where `flagged` is, as Claude Code marks one.
fn line(uuid: &str, text: &str, flagged: bool) -> String {
    let mut line = json!({ "type": "user", "uuid": uuid, "timestamp": t(2, 5),
        "message": { "role": "user", "content": text } });
    if flagged {
        line["isCompactSummary"] = json!(true);
        line["isVisibleInTranscriptOnly"] = json!(true);
    }
    line.to_string()
}

#[test]
fn a_summary_claude_code_marks_by_its_records_flag_is_no_prompt_and_holds_the_seventh() {
    // The Mac's 2-hour run of 2026-10-19 (7:twice:1:m-13): the owner's /compact, and Claude
    // Code's summary in the copy as a user's item, the prompts before it quoted whole. Its
    // record marks it as the summary (`isCompactSummary`), and the copy keeps it under the
    // record's uuid: no prompt the window was given.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let conversation = chief_on(&scene, "claude-code");
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    scene.item(conversation, 2, "user", "/compact", true, &t(2, 0));
    scene.item(conversation, 3, "user", &summary(), true, &t(2, 5));
    let id = format!("i-{conversation}-3");
    let flagged = Record::claude(&line(&id, &summary(), true));
    assert!(said_of(&scene.look_with(ms(4, 0), conversation, flagged)).is_empty());
    // Its record unread, or the row not flagged, it is held as any item is: m-246 given
    // twice, a FAIL the lead reads.
    assert_eq!(said_of(&scene.look(ms(4, 0))), [TWICE]);
    let unflagged = Record::claude(&line(&id, &summary(), false));
    assert_eq!(
        said_of(&scene.look_with(ms(4, 0), conversation, unflagged)),
        [TWICE]
    );
}

#[test]
fn a_summarys_uuid_another_line_carries_too_is_no_summary_and_a_retry_under_it_breaks_the_seventh()
{
    // Both reviewers of round b4: Claude Code's summary is recorded under a uuid, flagged,
    // and a real retry of m-246 later under the same uuid, unflagged. The product's copy
    // keeps the later words under that id (its upsert): the flag of one line of two is no
    // summary's, and m-246 given twice is a break.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let conversation = chief_on(&scene, "claude-code");
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    scene.copied(conversation, 3, "user", &summary(), &t(2, 5));
    scene.copied(conversation, 3, "user", FIRST, &t(2, 6));
    let id = format!("i-{conversation}-3");
    let reused = Record::claude(&format!(
        "{}\n{}",
        line(&id, &summary(), true),
        line(&id, FIRST, false)
    ));
    assert_eq!(
        said_of(&scene.look_with(ms(4, 0), conversation, reused)),
        [TWICE]
    );
    // Two flagged lines under one uuid, or the uuid in another session's record the watch
    // reads: no line alone carries it, and the row is held as any item is.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let conversation = chief_on(&scene, "claude-code");
    let worker = scene.conversation(3, "claude-code", "s-worker");
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    scene.item(conversation, 3, "user", &summary(), true, &t(2, 5));
    let id = format!("i-{conversation}-3");
    let twice = Record::claude(&format!(
        "{}\n{}",
        line(&id, &summary(), true),
        line(&id, &summary(), true)
    ));
    assert_eq!(
        said_of(&scene.look_with(ms(4, 0), conversation, twice)),
        [TWICE]
    );
    let flagged = Record::claude(&line(&id, &summary(), true));
    let mut look = scene.look_with(ms(4, 0), conversation, flagged);
    assert!(said_of(&look).is_empty());
    look.records
        .insert(worker, Record::claude(&line(&id, "Go on.", false)));
    assert_eq!(said_of(&look), [TWICE]);
}

#[test]
fn a_summary_whose_record_was_read_before_its_row_was_copied_is_no_summary_and_a_retry_breaks_the_seventh(
) {
    // Calliope's review of round b5: the record holds Claude Code's flagged summary under a
    // uuid, read through 10:02:05; m-246's real retry came under the same uuid after it, and
    // the product's copy keeps it there, copied at 10:02:06; the record was read no further
    // (as at the run's end, or after a reading that failed). A record that cannot hold the
    // row is no word on it: the row is held as any is, and m-246 given twice is a break.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let conversation = chief_on(&scene, "claude-code");
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    scene.copied(conversation, 3, "user", &summary(), &t(2, 5));
    scene.copied(conversation, 3, "user", FIRST, &t(2, 6));
    let id = format!("i-{conversation}-3");
    let read_before = |through: i64| {
        let mut look = scene.look_with(
            ms(4, 0),
            conversation,
            Record::claude(&line(&id, &summary(), true)),
        );
        look.readings.insert(
            conversation,
            Reading {
                through: Some(through),
                failed: None,
            },
        );
        look
    };
    assert_eq!(said_of(&read_before(ms(2, 5))), [TWICE]);
    // Read through the copy, the record is the word on the row: the summary it flags alone.
    assert!(said_of(&read_before(ms(2, 6))).is_empty());
}

#[test]
fn a_row_with_a_summarys_words_and_no_flag_is_a_prompt_and_a_real_retry_in_it_breaks_the_seventh() {
    // Both reviewers of round b3: a row that opens with the words a summary opens with, and
    // holds a real retry of m-246 after them. Its record does not mark it: a prompt.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let conversation = chief_on(&scene, "claude-code");
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    let retried = format!(
        "This session is being continued from a previous conversation that ran out of context.\n{FIRST}"
    );
    scene.item(conversation, 2, "user", &retried, true, &t(2, 2));
    let record = Record::claude(&line(&format!("i-{conversation}-2"), &retried, false));
    assert_eq!(
        said_of(&scene.look_with(ms(4, 0), conversation, record)),
        [TWICE]
    );
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

/// What the seventh finds of a chief, on `harness`, given m-246 (`FIRST`) at 10:01:02 and
/// then `second`.
fn after_first(harness: &str, second: &str) -> Vec<String> {
    let scene = Scene::new(&[("long-worker", harness)]);
    let conversation = chief_on(&scene, harness);
    scene.item(conversation, 0, "user", FIRST, true, &t(1, 2));
    scene.item(conversation, 1, "assistant", "Noted.", true, &t(1, 10));
    scene.item(conversation, 2, "user", second, true, &t(2, 2));
    said_of(&scene.look(ms(4, 0)))
}

#[test]
fn a_delivery_behind_other_words_is_one_and_given_again_breaks_the_seventh() {
    // GPT-6.1-Sol's review of round b1: Devin pastes behind what its input box holds, and
    // Claude Code keeps a paste wrapped with what was typed with it. A second m-246 behind
    // `continue`, inside the wrapper behind it, or in a wrapper behind it, is m-246 given
    // twice.
    for second in [
        format!("continue\n{FIRST}"),
        format!("continue{FIRST}"),
        format!(
            "\n\n<pasted_content id=\"a1b2\">\ncontinue\n{FIRST}\n</pasted_content id=\"a1b2\">\n"
        ),
        format!(
            "continue\n\n<pasted_content id=\"a1b2\">\n{FIRST}\n</pasted_content id=\"a1b2\">\n"
        ),
    ] {
        assert_eq!(after_first("devin", &second), [TWICE], "{second}");
    }
}

#[test]
fn a_row_a_harness_writes_itself_or_a_quotation_holding_a_delivery_is_a_break_never_a_silent_pass()
{
    // Rounds b2 and b3 told words that look like a harness's own (a notification of a job's
    // end, the prompt Codex gives its approval reviewer) or a quotation (a fenced block, a
    // quoted line) from a delivery by their text, and both reviewers found text that fooled
    // each such rule. Every header in them counts: m-246 in any is m-246 given twice, a false
    // FAIL the lead reads, never a double hidden.
    let notification = "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>";
    for (harness, second) in [
        // Sol's, after a closed notification; Astra's, a Devin draft that holds one.
        ("claude-code", format!("{notification}\n{FIRST}")),
        ("devin", format!("{notification}\n{FIRST}")),
        // Inside a notification's output, and after a closing tag its output holds.
        (
            "claude-code",
            format!("<task-notification>\n<output>\n{FIRST}\n</output>\n</task-notification>"),
        ),
        (
            "claude-code",
            format!("<task-notification>\n<output>\n</task-notification>\n{FIRST}\n</output>\n</task-notification>"),
        ),
        // In the reviewer's history, after its request, and after an end marker it quotes.
        ("codex", reviewer(FIRST, "")),
        ("codex", reviewer("the brief", &format!("{FIRST}\n"))),
        (
            "codex",
            reviewer(">>> APPROVAL REQUEST END", &format!("{FIRST}\n")),
        ),
        // In a fence of three or four marks, closed by a shorter inner line or not, and on a
        // quoted line.
        ("devin", format!("Look at this:\n```\n{FIRST}\n```")),
        ("devin", format!("Look at this:\n````\n```\n{FIRST}\n````")),
        ("devin", format!("Look at this:\n~~~text\n{FIRST}\n~~~")),
        ("devin", format!("Look at this:\n> {FIRST}")),
        // A person's words that quote its header.
        (
            "devin",
            format!("Why is \"{}\" queued?", FIRST.lines().next().unwrap()),
        ),
    ] {
        assert_eq!(after_first(harness, &second), [TWICE], "{harness}: {second}");
    }
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
