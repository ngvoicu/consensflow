//! `quotable`: a line that may hold a credential is withheld whole, the others come out as they
//! went in, masked as a note masks them.

use super::*;

/// What the review's probe wrote (T-1270, second reading, F2): a key with blanks in it, behind a
/// name the mask knows only for a value with none.
const KEY_WITH_BLANKS: &str = "extension failed: API_KEY: sk-abcd efghijkl mnopqrst";

fn withheld(words: &str) -> bool {
    quotable(words) == WITHHELD_LINE
}

#[test]
fn a_key_with_blanks_in_it_is_withheld_and_none_of_it_is_quoted() {
    let said = quotable(KEY_WITH_BLANKS);
    assert_eq!(said, WITHHELD_LINE);
    for piece in [
        "sk-abcd",
        "efghijkl",
        "mnopqrst",
        "API_KEY",
        "extension failed",
    ] {
        assert!(!said.contains(piece), "{piece} in {said}");
    }
}

#[test]
fn a_password_of_several_words_is_withheld_not_masked_at_its_first() {
    assert!(withheld(
        "login failed, password: correct horse battery staple"
    ));
    assert!(withheld("PASSWORD=hunter two"));
}

#[test]
fn a_credential_s_word_with_a_value_after_it_is_withheld_in_any_case() {
    for line in [
        "key: abcdefgh ijklmnop",
        "KEY=abc",
        "ssh_key = a b",
        "x-api-key: 1",
        "token: a b c",
        "Token=abc",
        "\"secret\": \"two words\"",
        "SECRET = 'a b'",
        "password: x y",
        "Authorization: Basic YWxhZGRpbjpvcGVuc2VzYW1l",
        "authorization bearer zzzz yyyy",
        "Bearer abc def",
        "bearer: abc",
    ] {
        assert!(withheld(line), "{line} was quoted: {}", quotable(line));
    }
}

#[test]
fn a_credential_s_word_with_no_value_after_it_is_a_word_like_another() {
    for line in [
        "key not found",
        "Pi lists no model: no provider is set up",
        "the key",
        "keyboard: us",
        "Error: Failed to load extension \"/x/e.ts\": Cannot find module 'left-pad'",
    ] {
        assert!(!withheld(line), "{line} was withheld");
    }
}

#[test]
fn the_screens_rule_holds_for_a_line_a_credential_is_in_with_no_word_to_say_so() {
    for line in [
        "request with sk-abcd was refused",
        "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB",
        "deadbeef cafebabe feedface 0badf00d",
        "see https%3A%2F%2Falice%3Aswordfish%40example.test",
    ] {
        assert!(withheld(line), "{line} was quoted: {}", quotable(line));
    }
}

#[test]
fn only_the_lines_that_may_hold_a_credential_are_withheld_and_the_others_keep_their_place() {
    let words = format!(
        "pi --mode rpc ended before it answered (exit 1); it said:\nError: Failed to load \
         extension\n{KEY_WITH_BLANKS}\nHint: start with pi -ne\npassword: correct horse"
    );
    assert_eq!(
        quotable(&words),
        format!(
            "pi --mode rpc ended before it answered (exit 1); it said:\nError: Failed to load \
             extension\n{WITHHELD_LINE}\nHint: start with pi -ne\n{WITHHELD_LINE}"
        )
    );
}

#[test]
fn a_line_that_is_quoted_is_masked_as_a_note_masks_it() {
    let said = quotable("fetch failed: https://alice:swordfish@example.invalid/x");
    assert_eq!(said, "fetch failed: https://[masked]@example.invalid/x");
}

#[test]
fn words_with_no_line_are_none_and_a_carriage_return_is_no_part_of_a_line() {
    assert_eq!(quotable(""), "");
    assert_eq!(
        quotable("one\r\ntwo: 2\r\n"),
        "one\ntwo: 2",
        "lines, whatever ends them"
    );
}
