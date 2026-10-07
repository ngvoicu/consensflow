//! What a failure note says of a window's screen and exit, in the words it says
//! them: the quote, its bounds, an empty screen, a host that said nothing, and
//! a secret on the screen.

use serde_json::json;

use super::*;

const PI: [&str; 2] = [
    "No API key found for the selected model.",
    "Use /login to log into a provider.",
];

fn shown(code: Option<u32>, signal: Option<&str>, tail: Option<&[&str]>) -> Shown {
    Shown {
        code,
        signal: signal.map(str::to_owned),
        tail: tail.map(|lines| lines.iter().map(|line| (*line).to_owned()).collect()),
    }
}

#[test]
fn a_window_that_closed_says_its_code_and_what_its_screen_ended_with() {
    assert_eq!(
        shown(Some(3), None, Some(&PI)).after("@zeus's window closed"),
        "@zeus's window closed (exit code 3); its screen ended with: \
         \"No API key found for the selected model. / Use /login to log into a provider.\""
    );
}

#[test]
fn a_window_that_never_showed_its_message_has_a_screen_and_no_code() {
    assert_eq!(
        shown(None, None, Some(&PI)).after("the window never showed its first message"),
        "the window never showed its first message; its screen ended with: \
         \"No API key found for the selected model. / Use /login to log into a provider.\""
    );
}

#[test]
fn a_program_a_signal_ended_is_said_by_the_signal_and_not_the_code_it_was_given() {
    assert_eq!(
        shown(Some(1), Some("Hangup: 1"), Some(&["bye"])).after("@zeus's window closed"),
        "@zeus's window closed (ended by signal Hangup: 1); its screen ended with: \"bye\""
    );
}

#[test]
fn a_screen_with_nothing_on_it_says_so_plainly() {
    for tail in [&[][..], &["", "   ", "\t"][..]] {
        assert_eq!(
            shown(Some(0), None, Some(tail)).after("the window closed"),
            "the window closed (exit code 0); its screen was empty"
        );
    }
    assert_eq!(
        shown(None, None, Some(&[])).after("the window closed"),
        "the window closed; its screen was empty"
    );
}

#[test]
fn a_host_that_did_not_say_adds_nothing() {
    let silent = Shown::default();
    assert!(!silent.is_known());
    assert_eq!(silent.after("the window closed"), "the window closed");
    assert!(shown(Some(0), None, None).is_known());
    assert!(
        shown(None, None, Some(&[])).is_known(),
        "an empty screen was said"
    );
    assert_eq!(
        shown(Some(2), None, None).after("the window closed"),
        "the window closed (exit code 2)"
    );
}

#[test]
fn only_the_last_lines_are_quoted() {
    let tail: Vec<String> = (1..=20).map(|number| format!("line {number}")).collect();
    let lines: Vec<&str> = tail.iter().map(String::as_str).collect();
    assert_eq!(
        shown(None, None, Some(&lines)).after("gone"),
        "gone; its screen ended with: \
         \"line 15 / line 16 / line 17 / line 18 / line 19 / line 20\""
    );
}

#[test]
fn a_quote_is_held_to_its_characters_and_the_older_lines_go_first() {
    let long = "w".repeat(190);
    let lines = ["the oldest", long.as_str(), long.as_str(), "newest of them"];
    let said = shown(None, None, Some(&lines)).after("gone");
    let quote = said
        .strip_prefix("gone; its screen ended with: \"")
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("{said}"));
    // The newest and the two long ones, with their separators, are the 400
    // there is room for: the oldest does not fit, and is left out whole.
    assert_eq!(quote, format!("{long} / {long} / newest of them"));
    assert_eq!(quote.chars().count(), QUOTE_CHARS);
}

#[test]
fn a_last_line_longer_than_the_quote_is_cut_with_a_mark() {
    let said = shown(
        None,
        None,
        Some(&["start of a very long line", &"x".repeat(5_000)]),
    )
    .after("gone");
    let quote = said
        .strip_prefix("gone; its screen ended with: \"")
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("{said}"));
    assert_eq!(quote.chars().count(), QUOTE_CHARS);
    assert!(quote.ends_with('…'));
    assert!(
        !quote.contains("start of"),
        "the older line did not fit beside it"
    );
}

#[test]
fn blanks_become_one_and_quotation_marks_become_apostrophes() {
    assert_eq!(
        shown(None, None, Some(&["  model      \"claude\"    ready  "])).after("gone"),
        "gone; its screen ended with: \"model 'claude' ready\""
    );
}

#[test]
fn a_key_looking_string_is_masked_and_the_note_counts_what_it_hid() {
    let said = shown(
        Some(1),
        None,
        Some(&[
            "Invalid API key: sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv",
            "OPENAI_API_KEY=abcd1234efgh set in the environment",
        ]),
    )
    .after("the window closed");
    assert_eq!(
        said,
        "the window closed (exit code 1); its screen ended with: \
         \"Invalid API key: [masked] / OPENAI_API_KEY=[masked] set in the environment\" \
         (2 key- or token-like strings masked)"
    );
    assert!(!said.contains("AbCdEf") && !said.contains("abcd1234"));
    let one = shown(
        None,
        None,
        Some(&["token ghp_abcdefghijklmnopqrstuvwxyz0123456789"]),
    )
    .after("gone");
    assert!(
        one.ends_with("(1 key- or token-like string masked)"),
        "{one}"
    );
}

#[test]
fn a_secret_in_a_line_the_quote_leaves_out_is_not_counted() {
    let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let mut tail = vec![format!("first {secret}")];
    tail.extend((1..=6).map(|number| format!("line {number}")));
    let lines: Vec<&str> = tail.iter().map(String::as_str).collect();
    let said = shown(None, None, Some(&lines)).after("gone");
    assert!(!said.contains("masked"), "{said}");
    assert!(!said.contains("ghp_"), "{said}");
}

#[test]
fn a_secret_cut_in_the_middle_by_the_bound_is_hidden_before_it_is_cut() {
    let secret = format!("sk-{}", "A1".repeat(30));
    let line = format!("{} {secret}", "x".repeat(380));
    let said = shown(None, None, Some(&[line.as_str()])).after("gone");
    assert!(!said.contains("sk-"), "{said}");
    assert!(!said.contains("A1A1"), "{said}");
}

#[test]
fn an_exit_the_host_sent_is_what_the_engine_keeps() {
    let exit: PaneExit = serde_json::from_value(json!({
        "id": "p1-zeus", "generation": 2, "exitCode": 3, "tail": ["No API key found"],
    }))
    .unwrap();
    assert_eq!(
        Shown::from_exit(exit),
        shown(Some(3), None, Some(&["No API key found"]))
    );
}

#[test]
fn a_snapshot_answer_gives_its_tail_when_it_was_a_success() {
    assert_eq!(
        Shown::from_snapshot(&json!({ "ok": true, "unsent": false, "tail": ["stuck"] })),
        shown(None, None, Some(&["stuck"]))
    );
    assert_eq!(
        Shown::from_snapshot(&json!({ "ok": true, "unsent": false })),
        Shown::default(),
        "a host that gave no tail"
    );
    assert_eq!(
        Shown::from_snapshot(&json!({ "ok": false, "error": "stale", "tail": ["no"] })),
        Shown::default(),
        "a refusal says nothing"
    );
    assert_eq!(
        Shown::from_snapshot(&json!({ "ok": true, "tail": "not a list" })),
        Shown::default()
    );
}
