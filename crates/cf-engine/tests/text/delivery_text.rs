//! How a message reads in its recipient's pane, and the header that proves it
//! arrived: the cases of Node's delivery-text suite.

use cf_base::text::utf16_len;
use cf_engine::delivery_text::{delivery_text, marker_of};
use cf_proto::ledger::MessageView;
use serde_json::json;

use super::support::message;

/// A message as it reads when its paste carries nothing.
fn alone(message: &MessageView) -> String {
    delivery_text(message, &[])
}

#[test]
fn names_the_message_the_task_and_the_sender_and_tells_the_reader_how_to_answer_a_question() {
    let question = || message(12, "question", Some("zeus"), Some(3), "Which format?");
    assert_eq!(
        alone(&question()),
        "[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nRun in your shell: cf answer m-12 \"…\""
    );
    let options =
        json!([{ "question": "Which?", "header": "Format", "options": [], "multiple": false }]);
    let asked = |questions| MessageView {
        questions,
        ..question()
    };
    assert_eq!(
        alone(&asked(options.clone())),
        "[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nRun in your shell: cf answer m-12 \"…\" (a label or your own words)"
    );
    assert_eq!(
        alone(&asked(json!([options[0], options[0]]))),
        "[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nRun in your shell: cf answer m-12 \"…\" (a label or your own words; one line per question)"
    );
    assert_eq!(
        alone(&message(12, "note", None, None, "hi")),
        "[ConsensFlow m-12 · note from ConsensFlow]\nhi"
    );
    assert_eq!(
        alone(&message(12, "result", Some("zeus"), Some(3), "Parser done")),
        "[ConsensFlow m-12 · T-3 · result from @zeus]\nParser done\n\nDecide with: cf task accept T-3 · cf task reopen T-3 \"…\"",
        "a result says what to do with it: it is not a request"
    );
}

#[test]
fn sends_a_long_body_as_its_opening_and_the_command_that_reads_the_rest() {
    let text = alone(&message(
        7,
        "result",
        Some("zeus"),
        Some(1),
        &"x".repeat(40_000),
    ));
    assert!(utf16_len(&text) < 16_000);
    assert!(
        text.contains(
            "\n… (40000 characters; read all of it with: cf inbox read m-7)\n\nDecide with: "
        ),
        "{}",
        &text[text.len() - 200..]
    );
    let whole = alone(&message(
        8,
        "task",
        Some("chief"),
        Some(1),
        &"y".repeat(16_000),
    ));
    assert!(
        whole.ends_with(&"y".repeat(16_000)),
        "a body of 16,000 characters goes whole"
    );
}

#[test]
fn tells_the_reader_of_an_urgent_question_that_its_task_waits_for_it_and_who_resumes_the_task() {
    let tell = MessageView {
        urgent: true,
        ..message(12, "question", Some("chief"), Some(3), "Stop: use v2")
    };
    assert_eq!(
        alone(&tell),
        "[ConsensFlow m-12 · T-3 · question from @chief]\nStop: use v2\n\nT-3 is paused for this. Run in your shell: cf answer m-12 \"…\"; the chief resumes the task."
    );
    assert_eq!(
        alone(&MessageView {
            task_number: None,
            ..tell
        }),
        "[ConsensFlow m-12 · question from @chief]\nStop: use v2\n\nRun in your shell: cf answer m-12 \"…\"",
        "with no task, nothing is paused"
    );
}

/// Not a case of the JavaScript suite: Node left half of a pair at the cut and
/// `windowText` dropped it; here it is never there.
#[test]
fn drops_the_half_of_an_emoji_the_cut_would_leave_as_the_window_drops_it() {
    // The emoji's halves are units 14,999 and 15,000: the cut is between them.
    let body = format!("{}🙂{}", "x".repeat(14_999), "y".repeat(2_000));
    let text = alone(&message(7, "result", Some("zeus"), Some(1), &body));
    assert!(
        text.contains(&format!(
            "\n{}\n… (17001 characters; read all of it with: cf inbox read m-7)",
            "x".repeat(14_999)
        )),
        "the text goes on from the last whole character"
    );
    assert!(!text.contains('\u{FFFD}') && !text.contains('🙂'));
    assert_eq!(
        cf_base::text::window_text(&text),
        text,
        "the window changes nothing"
    );
}

#[test]
fn starts_with_the_marker_a_window_s_record_is_searched_for_which_names_no_other_message() {
    let text = alone(&message(12, "task", Some("chief"), Some(3), "x"));
    assert!(text.starts_with(&marker_of(12)));
    assert!(!text.starts_with(&marker_of(1)), "m-1 is not m-12");
    // A window that did not keep the · still shows the marker: Devin on
    // Windows got it as |, and before that lost it.
    for kept in [text.replace('·', "|"), text.replace('·', "")] {
        assert!(kept.starts_with(&marker_of(12)));
    }
}

/// What a carrier's paste says: its own words and the rows it carries, in the
/// order of their ids, under the one header that proves it arrived.
#[test]
fn reads_a_carrier_as_its_words_and_the_rows_it_carries_in_the_order_of_their_ids_under_one_header()
{
    let carrier = message(
        9,
        "task",
        None,
        Some(3),
        "Resumed: Go on where you stopped.",
    );
    let answer = MessageView {
        reply_to: Some(5),
        ..message(7, "answer", Some("chief"), Some(3), "JSON")
    };
    let note = message(11, "note", Some("chief"), Some(3), "Mind the tests");
    let text = delivery_text(&carrier, &[note, answer]);
    assert_eq!(
        text,
        "[ConsensFlow m-9 · T-3 · task from ConsensFlow]\n(kept for you: answer m-7 from @chief, to your m-5)\nJSON\n\nResumed: Go on where you stopped.\n\n(kept for you: note m-11 from @chief)\nMind the tests",
        "the carrier's words are its own, and each row says what it is and whose"
    );
    assert_eq!(
        text.matches("[ConsensFlow m-").count(),
        1,
        "only the carrier's marker proves the paste arrived"
    );
    assert!(text.starts_with(&marker_of(9)));
}

#[test]
fn a_message_that_carries_nothing_reads_byte_for_byte_as_it_did() {
    let task = message(9, "task", Some("chief"), Some(3), "Parser");
    assert_eq!(
        delivery_text(&task, &[]),
        "[ConsensFlow m-9 · T-3 · task from @chief]\nParser"
    );
}

#[test]
fn cuts_a_long_composite_whole_and_names_the_command_that_shows_every_row() {
    let carrier = message(9, "task", Some("chief"), Some(3), "Resumed: Go on");
    let long = message(8, "note", Some("chief"), Some(3), &"x".repeat(20_000));
    let text = delivery_text(&carrier, &[long]);
    assert!(utf16_len(&text) < 16_000);
    assert!(
        // The label (36), a line break, the note (20,000), a blank line, the words (14).
        text.contains("\n… (20053 characters; read all of it with: cf task get T-3)"),
        "the whole body is cut, as a long body is, and the way to read it is the task's thread: {}",
        &text[text.len() - 120..]
    );
}
