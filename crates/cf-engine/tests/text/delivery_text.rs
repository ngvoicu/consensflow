//! How a message reads in its recipient's pane, and the header that proves it
//! arrived: the cases of Node's delivery-text suite.

use cf_base::text::utf16_len;
use cf_engine::delivery_text::{delivery_text, marker_of};
use cf_proto::cf_word::{CfWord, Shell};
use cf_proto::ledger::MessageView;
use regex::Regex;
use serde_json::json;

use super::support::message;

/// A message as it reads when its paste carries nothing.
fn alone(message: &MessageView) -> String {
    delivery_text(message, &[], &CfWord::bare())
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

#[test]
fn a_note_keeps_its_marker_through_what_windows_console_carries_to_devin() {
    // m-186 of the report of 2026-10-13: a note from ConsensFlow itself, naming
    // windows with @, on two lines a Windows machine lines with CR LF.
    let note = message(
        186,
        "note",
        None,
        Some(17),
        "@human cancelled T-17 (the parser): @zeus's window was stopped.\r\nIts job ran on.",
    );
    let carried = cf_base::text::console_text(&cf_base::text::window_text(&alone(&note)));
    assert!(carried.starts_with(&marker_of(186)), "{carried}");
    assert!(
        carried.starts_with("[ConsensFlow m-186 | T-17 | note from ConsensFlow]\n"),
        "{carried}"
    );
    // As the record gives it back, whichever line ends the window kept.
    assert!(carried.replace('\n', "\r\n").contains(&marker_of(186)));
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
    let text = delivery_text(&carrier, &[note, answer], &CfWord::bare());
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
        delivery_text(&task, &[], &CfWord::bare()),
        "[ConsensFlow m-9 · T-3 · task from @chief]\nParser"
    );
}

#[test]
fn cuts_a_long_composite_whole_and_names_the_command_that_shows_every_row() {
    let carrier = message(9, "task", Some("chief"), Some(3), "Resumed: Go on");
    let long = message(8, "note", Some("chief"), Some(3), &"x".repeat(20_000));
    let text = delivery_text(&carrier, &[long], &CfWord::bare());
    assert!(utf16_len(&text) < 16_000);
    assert!(
        // The label (36), a line break, the note (20,000), a blank line, the words (14).
        text.contains("\n… (20053 characters; read all of it with: cf task get T-3)"),
        "the whole body is cut, as a long body is, and the way to read it is the task's thread: {}",
        &text[text.len() - 120..]
    );
}

/// How a command starts in a window: the path of its own `cf` as its shell takes it (T-1049;
/// a Devin member's login zsh and a Claude Code member's login Git Bash find another `cf`
/// first, and a member told to run `cf answer m-12` ran it).
fn words() -> [CfWord; 3] {
    [
        CfWord::new(
            "/Users/me/consensflow-dist/Contents/Resources/cli/bin/cf",
            Shell::Posix,
        ),
        CfWord::new("C:/Program Files/ConsensFlow/bin/cf.exe", Shell::Posix),
        CfWord::new("C:/Program Files/ConsensFlow/bin/cf.exe", Shell::PowerShell),
    ]
}

/// The commands of the board, said with the word `cf` on its own: what is left of `text` once
/// `word` is taken out of it.
fn bare_commands(text: &str, word: &CfWord) -> Vec<String> {
    let commands = Regex::new(
        r"(?-u:\b)cf (task|ask|inbox|answer|note|tell|staff|history)(?-u:\b)[^\n]{0,40}",
    )
    .unwrap();
    commands
        .find_iter(&text.replace(word.as_str(), ""))
        .map(|found| found.as_str().to_owned())
        .collect()
}

#[test]
fn every_footer_and_every_command_that_reads_the_rest_starts_with_the_cf_of_the_window() {
    for cf in words() {
        let word = cf.as_str();
        let said = |message: &MessageView| delivery_text(message, &[], &cf);
        let question = message(12, "question", Some("zeus"), Some(3), "Which format?");
        assert!(
            said(&question).ends_with(&format!("\n\nRun in your shell: {word} answer m-12 \"…\""))
        );
        let options =
            json!([{ "question": "Which?", "header": "Format", "options": [], "multiple": false }]);
        let asked = MessageView {
            questions: options.clone(),
            ..question.clone()
        };
        assert!(said(&asked).contains(&format!(
            "Run in your shell: {word} answer m-12 \"…\" (a label"
        )));
        let urgent = MessageView {
            urgent: true,
            ..question
        };
        assert!(said(&urgent).contains(&format!(
            "T-3 is paused for this. Run in your shell: {word} answer m-12 \"…\"; the chief resumes the task."
        )));
        let result = message(12, "result", Some("zeus"), Some(3), "Parser done");
        assert!(said(&result).ends_with(&format!(
            "\n\nDecide with: {word} task accept T-3 · {word} task reopen T-3 \"…\""
        )));
        let long = message(7, "result", Some("zeus"), Some(1), &"x".repeat(40_000));
        assert!(said(&long).contains(&format!("read all of it with: {word} inbox read m-7)")));
        let carrier = message(9, "task", Some("chief"), Some(3), "Resumed: Go on");
        let rows = [message(
            8,
            "note",
            Some("chief"),
            Some(3),
            &"x".repeat(20_000),
        )];
        let composite = delivery_text(&carrier, &rows, &cf);
        assert!(composite.contains(&format!("read all of it with: {word} task get T-3)")));
        for text in [said(&question_of_all_kinds()), composite] {
            assert_eq!(bare_commands(&text, &cf), Vec::<String>::new(), "{text}");
        }
    }
}

/// A question with a footer and a long body in one: every `cf` the engine writes of its own.
fn question_of_all_kinds() -> MessageView {
    message(5, "question", Some("zeus"), Some(2), &"y".repeat(30_000))
}

/// What ConsensFlow itself writes of a note, in the engine and in the ledger, says its
/// commands with the word `cf` (the board shows them so, to a human): each is said to a window
/// with the window's own, as the samples here, one of each place that writes one, show. The
/// first words are the real ones (`handoff_text` is held in `handoff.rs`).
const OWN_WORDS: [&str; 14] = [
    // stops.rs
    "T-3's window (@zeus) did not stop: it ignored the interrupt, and would not close. ConsensFlow tries again every minute; until then your words wait, and what it writes is not taken as T-3's result (cf task get T-3 --transcript shows it).",
    // turn_end.rs
    "T-3: another Claude than @zeus's window has shown its conversation. `cf task finish T-3` takes the last answer.",
    "T-3's turn looks finished. `cf task finish T-3` takes that answer.",
    // arrival.rs
    "m-5's arrival couldn't be proven from the record; it was not pasted again. @zeus can read it with `cf inbox read m-5`.",
    "m-5's arrival couldn't be proven from your window's record; it was not pasted again. Read it with `cf inbox read m-5`.",
    // deliveries.rs
    "T-3 failed: the harness reported a failure. Reopen it with: cf task reopen T-3 \"…\"",
    // cf-ledger: pauses.rs, paused_answers.rs, again.rs, messages.rs
    "T-1 is paused: @zeus's window is gone. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation.",
    "Its member runs on a harness whose windows ConsensFlow cannot open: cancel it with cf task cancel T-4 and add it again with cf task add once a member can.",
    "If that is the result, take it: cf task finish T-3. To send it on: cf task resume T-3 \"…\".",
    "Answer it again: cf answer m-9 \"…\"",
    "T-4 failed, so nobody waits for this answer: reopen T-4 to go on with it (cf task reopen T-4 \"…\")",
    // the first message of a chief the human switched in (`handoff.rs`)
    "Read what the human and the earlier chief said before you act: cf history (2 pages, newest first; cf history --page 2 for older; cf history --find \"words\" to search).",
    "- @zeus asks on T-1: \"Which grammar?\" (cf inbox read m-6, then cf answer m-6 \"…\")",
    "- T-2 \"Lexer\": @diana's result waits for your decision (cf task get T-2)",
];

#[test]
fn what_consensflow_wrote_of_a_note_starts_each_command_with_the_cf_of_the_window() {
    for cf in words() {
        for (at, own) in OWN_WORDS.iter().enumerate() {
            let note = message(20 + at as i64, "note", None, Some(3), own);
            let text = delivery_text(&note, &[], &cf);
            assert_eq!(bare_commands(&text, &cf), Vec::<String>::new(), "{text}");
            assert!(text.contains(&format!("{} ", cf.as_str())), "{text}");
        }
        // The same note, rows a carrier keeps for its window: each row ConsensFlow wrote says it
        // so too, and its sender's words are their own.
        let carrier = message(
            40,
            "task",
            None,
            Some(3),
            "Resumed: Go on where you stopped.",
        );
        let rows = [
            message(41, "note", None, Some(3), OWN_WORDS[6]),
            message(
                42,
                "note",
                Some("chief"),
                Some(3),
                "Run cf task get T-3 yourself",
            ),
        ];
        let text = delivery_text(&carrier, &rows, &cf);
        assert!(text.contains(&format!("Resume it with: {} task resume T-1", cf.as_str())));
        assert!(text.contains("\nRun cf task get T-3 yourself"), "{text}");
    }
}

#[test]
fn what_a_participant_wrote_is_never_changed_whatever_the_window_s_word_is() {
    let theirs = "Run cf task get T-1, then `cf inbox read m-2` and cf ask \"…\"";
    for cf in words() {
        for kind in ["note", "answer", "task"] {
            for sender in ["chief", "human", "zeus"] {
                let said = message(3, kind, Some(sender), Some(1), theirs);
                assert!(
                    delivery_text(&said, &[], &cf).ends_with(&format!("\n{theirs}")),
                    "{kind} from {sender}"
                );
            }
        }
    }
}

#[test]
fn with_no_path_a_note_reads_as_it_always_did() {
    let cf = CfWord::bare();
    for (at, own) in OWN_WORDS.iter().enumerate() {
        let note = message(20 + at as i64, "note", None, Some(3), own);
        assert_eq!(
            delivery_text(&note, &[], &cf),
            format!(
                "[ConsensFlow m-{} · T-3 · note from ConsensFlow]\n{own}",
                20 + at
            )
        );
    }
}
