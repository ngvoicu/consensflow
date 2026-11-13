//! What a note of a harness's dialog says.

use cf_harness::contract::{Decline, Dialog};

use super::*;

fn offer(closes_after_s: Option<u32>) -> Dialog {
    Dialog {
        question: "Would you like to install this LSP plugin?".to_owned(),
        detail: Vec::new(),
        options: vec!["Yes, install".to_owned(), "No, not now".to_owned()],
        asks: true,
        prompt_below: false,
        closes_after_s,
    }
}

#[test]
fn a_note_names_the_window_the_question_its_choices_and_that_nothing_is_pressed() {
    assert_eq!(
        told("zeus-amber-pine", Some(&offer(Some(30))), None),
        "@zeus-amber-pine's window shows a dialog of its harness that waits for a key, not its prompt: \u{201c}Would you like to install this LSP plugin?\u{201d}, its choices \u{201c}Yes, install\u{201d}, \u{201c}No, not now\u{201d}. The window waits on it, and nothing is typed into it meanwhile. ConsensFlow presses nothing in it: answer it in that window, or leave it. Its harness closes it by itself within 30 s."
    );
    // A dialog not known to close says nothing of it.
    assert!(!told("zeus", Some(&offer(None)), None).contains("closes it by itself"));
}

#[test]
fn a_dialog_whose_words_the_screen_did_not_show_is_told_of_still() {
    assert_eq!(
        told("chief", None, None),
        "@chief's window shows a dialog of its harness that waits for a key, not its prompt; its screen shows no words ConsensFlow can read of it. The window waits on it, and nothing is typed into it meanwhile. ConsensFlow presses nothing in it: answer it in that window, or leave it."
    );
}

#[test]
fn a_quote_hides_a_token_and_is_cut() {
    let token = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ";
    let quote = quoted(&format!("Use the key {token}?"));
    assert!(!quote.contains(token), "{quote}");
    let long = "word ".repeat(100);
    let cut = quoted(&long);
    assert!(cut.ends_with("…\u{201d}"), "{cut}");
    assert_eq!(cut.chars().count(), QUOTED_CHARS + 3);
}

#[test]
fn a_note_of_a_dialog_on_its_harnesss_list_says_it_is_declined_and_how() {
    let decline = Decline {
        choice: "No, not now".to_owned(),
        key: b"2".to_vec(),
        after_ms: None,
        then_tell: None,
        what: "offer".to_owned(),
        listed: true,
        members_only: false,
    };
    let body = told("zeus", Some(&offer(Some(30))), Some(&decline));
    assert!(
        body.ends_with("ConsensFlow declines it for the window now, choosing \u{201c}No, not now\u{201d}, which writes nothing; nothing else is typed into the window meanwhile."),
        "{body}"
    );
    assert!(!body.contains("presses nothing"), "{body}");
}

#[test]
fn a_note_of_a_decline_made_by_a_key_with_no_choice_listed_says_it_is_pressed_not_chosen() {
    let decline = Decline {
        choice: "Esc".to_owned(),
        key: vec![27],
        after_ms: None,
        then_tell: Some("Go on without it.".to_owned()),
        what: "Computer Use permission prompt".to_owned(),
        listed: false,
        members_only: true,
    };
    let body = told("zeus", Some(&offer(None)), Some(&decline));
    assert!(
        body.ends_with("ConsensFlow declines it for the window now, by pressing \u{201c}Esc\u{201d}, which closes it and writes nothing, and tells it to go on without it; nothing else is typed into the window meanwhile."),
        "{body}"
    );
    assert!(!body.contains("choosing"), "{body}");
    // Where the harness has nothing to tell, the note says nothing of telling.
    let silent = Decline {
        then_tell: None,
        ..decline
    };
    let body = told("zeus", Some(&offer(None)), Some(&silent));
    assert!(
        body.ends_with("by pressing \u{201c}Esc\u{201d}, which closes it and writes nothing; nothing else is typed into the window meanwhile."),
        "{body}"
    );
}

#[test]
fn a_note_quotes_what_a_dialog_says_above_its_question() {
    let hook = Dialog {
        question: "Do you want to proceed?".to_owned(),
        detail: vec![
            "Hook tool.check requires confirmation for this".to_owned(),
            "command:".to_owned(),
        ],
        options: vec!["Yes".to_owned(), "No".to_owned()],
        asks: true,
        prompt_below: false,
        closes_after_s: None,
    };
    let body = told("zeus", Some(&hook), None);
    assert!(
        body.contains("its choices \u{201c}Yes\u{201d}, \u{201c}No\u{201d}. Above it, it says: \u{201c}Hook tool.check requires confirmation for this command:\u{201d}."),
        "{body}"
    );
}
