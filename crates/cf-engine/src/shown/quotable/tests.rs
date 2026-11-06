//! `quotable`: the words of a program are screened whole and cut after; a reason with no words
//! of a program in it is only masked.

use super::*;

/// What a read of Pi that ended says before the words of Pi.
const ENDED: &str = "pi --mode rpc ended before it answered (exit 1)";

/// A reason of Pi's read, with `said` after the mark.
fn pi_said(said: &str) -> String {
    format!("{ENDED}{IT_SAID}{said}")
}

/// What `quotable` keeps of Pi's words when they are withheld: the description, the code and the
/// sentence.
fn withheld() -> String {
    pi_said(WORDS_WITHHELD)
}

#[test]
fn a_key_with_blanks_in_it_withholds_the_words_of_pi_whole() {
    let said = quotable(&pi_said(
        "extension failed: API_KEY: sk-abcd efghijkl mnopqrst",
    ));
    assert_eq!(said, withheld());
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

/// The third reading's probe: the first line was withheld and both continuations were kept.
#[test]
fn a_value_that_runs_over_lines_withholds_the_lines_after_it_too() {
    let said = quotable(&pi_said(
        "Error: extension failed\nAPI_KEY: sk-abcd\nefghijkl\nmnopqrst",
    ));
    assert_eq!(said, withheld());
}

/// The third reading's probe: five lines that begin `password: correct`; a cut to the last four
/// before the look took the label and left the password.
#[test]
fn five_lines_that_begin_with_a_password_are_screened_before_the_cut() {
    let said = quotable(&pi_said(
        "password: correct\nhorse\nbattery\nstaple\nprovider unavailable",
    ));
    assert_eq!(said, withheld());
}

#[test]
fn a_label_far_above_the_last_lines_withholds_them_all() {
    let lines: Vec<String> = std::iter::once("token: abc".to_owned())
        .chain((1..=20).map(|n| format!("line {n}")))
        .collect();
    assert_eq!(quotable(&pi_said(&lines.join("\n"))), withheld());
}

#[test]
fn a_pretty_printed_error_object_holding_an_api_key_is_withheld() {
    let object = "{\n  \"error\": {\n    \"message\": \"provider failed\",\n    \"apiKey\": \"sk-abcd efghijkl\",\n    \"retries\": 3\n  }\n}";
    assert_eq!(quotable(&pi_said(object)), withheld());
}

#[test]
fn a_credential_word_with_a_value_after_it_withholds_the_block_in_any_case() {
    for words in [
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
        "key:\nthe value on the next line",
        "the line before\nPassword=hunter two\nthe line after",
    ] {
        assert_eq!(quotable(&pi_said(words)), withheld(), "{words}");
    }
}

#[test]
fn the_screens_rule_holds_for_words_a_credential_is_in_with_no_word_to_say_so() {
    for words in [
        "request with sk-abcd was refused",
        "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB",
        "deadbeef cafebabe feedface 0badf00d",
        "see https%3A%2F%2Falice%3Aswordfish%40example.test",
        "MIIEvQIBADANBgkq\nhkiG9w0BAQEFAASC",
    ] {
        assert_eq!(quotable(&pi_said(words)), withheld(), "{words}");
    }
}

/// What the user is told when Pi's words hold no credential: the description, the code, and the
/// last lines of what it said, one to a line.
#[test]
fn the_words_of_pi_that_hold_no_credential_are_quoted_as_they_were() {
    let said = "Error: Failed to load extension \"/x/e.ts\": Cannot find module 'left-pad'\nHint: Start without extensions using \"pi -ne\".";
    assert_eq!(quotable(&pi_said(said)), pi_said(said));
    for words in [
        "key not found",
        "the key",
        "keyboard: us",
        "Pi lists no model: no provider is set up",
    ] {
        assert_eq!(quotable(&pi_said(words)), pi_said(words), "{words}");
    }
}

#[test]
fn only_the_last_lines_that_passed_are_quoted_and_a_stack_is_never_quoted() {
    let said: String = (1..=9).map(|n| format!("line {n}\n")).collect();
    assert_eq!(
        quotable(&pi_said(&said)),
        pi_said("line 6\nline 7\nline 8\nline 9")
    );
    let with_frames = "Error: boom\n    at one (a.js:1:1)\n    at two (b.js:2:2)\n";
    assert_eq!(quotable(&pi_said(with_frames)), pi_said("Error: boom"));
    // Nothing but frames and blanks: nothing of Pi's words is said, nor the mark.
    assert_eq!(quotable(&pi_said("    at one (a.js:1:1)\n\n  \n")), ENDED);
    let wide = "é".repeat(2_000);
    let cut = quotable(&pi_said(&wide));
    let words = cut.strip_prefix(&format!("{ENDED}{IT_SAID}")).unwrap();
    assert_eq!(words.chars().count(), QUOTE_CHARS + 1);
    assert!(words.ends_with('…'));
}

/// A frame is never quoted, so a frame whose path names a token library withholds nothing: it
/// is set aside before the look. The message above it is what the user needs, and it is told.
#[test]
fn a_stack_frame_that_names_a_credential_word_withholds_nothing_because_it_is_never_quoted() {
    let said = "Error: Failed to load extension \"/x/e.ts\": boom\n    at load (/x/node_modules/token-lib/auth/index.js:1:1)";
    assert_eq!(
        quotable(&pi_said(said)),
        pi_said("Error: Failed to load extension \"/x/e.ts\": boom")
    );
}

#[test]
fn the_lines_that_are_quoted_are_masked_as_a_note_masks_them() {
    let said = quotable(&pi_said(
        "fetch failed: https://alice:swordfish@example.invalid/x",
    ));
    assert_eq!(
        said,
        pi_said("fetch failed: https://[masked]@example.invalid/x")
    );
}

/// ConsensFlow's own description stays before the mark whatever Pi said, and is masked as a
/// note masks, not screened: it names packages and extensions by name, and a package called
/// `token-provider` is not a credential.
#[test]
fn the_description_before_the_mark_is_kept_and_what_pi_was_loading_is_named() {
    let reason = format!(
        "pi --mode rpc did not answer within 30 s. Pi loads the packages and extensions it has \
         before it lists, and one of them may be waiting for something (packages \
         npm:@scope/token-provider@1.2.3; extensions ./oauth.ts){IT_SAID}password: correct\nhorse"
    );
    let told = quotable(&reason);
    assert!(
        told.starts_with(
            "pi --mode rpc did not answer within 30 s. Pi loads the packages and extensions it has \
             before it lists, and one of them may be waiting for something (packages \
             npm:@scope/token-provider@1.2.3; extensions ./oauth.ts)"
        ),
        "{told}"
    );
    assert!(
        told.ends_with(WORDS_WITHHELD) && !told.contains("horse"),
        "{told}"
    );
}

/// The first mark is where the program's words begin: a mark among them, forged to make what
/// follows pass for ConsensFlow's, only makes the block longer.
#[test]
fn a_mark_forged_among_the_words_of_pi_begins_nothing() {
    let reason = pi_said(&format!("fine\n{IT_SAID}password: correct\nhorse"));
    assert_eq!(quotable(&reason), withheld());
    let reason = pi_said(&format!("password: correct\nhorse{IT_SAID}the rest"));
    assert_eq!(quotable(&reason), withheld());
}

/// A reason with no mark is no program's words. It is masked as it was in round 2, and the
/// login, key and billing messages of the other harnesses read as they did (the third reading,
/// F9). The words of the screens' rule, applied to them, withheld every one.
#[test]
fn a_reason_with_no_mark_is_only_masked_so_the_login_and_billing_messages_read_as_they_did() {
    for words in [
        "Invalid API key · Please run /login",
        "its login was refused: Invalid API key",
        "Not logged in. Run `devin auth login` to authenticate.",
        "devin models list --format json: Not logged in. Run `devin auth login` to authenticate.",
        "No Meta credentials were found. Run `muse login`.",
        "API error 401 [request_id=x]: invalid api key (authentication_error)",
        "credential error: authentication failed for the account",
        "deferred_token_held",
        "key: abc",
        "Invalid key: expected string",
    ] {
        assert_eq!(quotable(words), words, "{words}");
    }
    // What the mask hides, it hides in them too: a key a harness echoes.
    assert_eq!(
        quotable("it said sk-live_0123456789abcdefghijklmnop"),
        "it said [masked]"
    );
    // The mask is the weaker rule, and it is all those harnesses' own messages get: it hides the
    // first word of a password and no more. Pi's words, which a program of the human's writes,
    // are what the block rule is for.
    assert_eq!(
        quotable("password: correct horse battery staple"),
        "password: [masked] horse battery staple"
    );
}

#[test]
fn a_reason_with_nothing_is_nothing() {
    assert_eq!(quotable(""), "");
    assert_eq!(quotable(&pi_said("")), ENDED);
}
