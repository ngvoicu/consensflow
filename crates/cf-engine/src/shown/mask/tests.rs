//! What is hidden, and what is left alone, with keys and tokens made up in the
//! shapes the real ones have.
//! Each sample is written in two pieces (`concat!`), so no line of this file is
//! itself in a token's shape: GitHub's push protection refused one (2026-10-07).

use super::*;

fn hidden(text: &str) -> String {
    mask(text).text
}

#[test]
fn every_pattern_of_the_masker_builds() {
    assert_eq!(RULES.len(), 8);
}

#[test]
fn the_user_and_password_of_a_url_are_hidden_and_the_host_is_not() {
    let masked = mask("git clone https://alice:hunter2pass@github.com/x/y.git failed");
    assert_eq!(
        masked.text,
        "git clone https://[masked]@github.com/x/y.git failed"
    );
    assert_eq!(masked.count, 1);
    assert_eq!(
        hidden("see https://github.com/x/y and ssh://git@host/repo"),
        "see https://github.com/x/y and ssh://git@host/repo"
    );
}

#[test]
fn what_follows_bearer_or_basic_is_hidden_once() {
    let masked = mask("Authorization: Bearer abc123.def456.ghi789");
    assert_eq!(masked.text, "Authorization: Bearer [masked]");
    assert_eq!(masked.count, 1, "the header's name is not a second secret");
    assert_eq!(hidden("basic dXNlcjpwYXNzd29yZA=="), "basic [masked]");
    assert_eq!(hidden("Basic QWxhZGRpbjpvcGVu"), "Basic [masked]");
    // `Basic` is a word as well: what is no credential after it stays.
    for prose in [
        "basic configuration is missing",
        "a basic requirement of Bearer",
    ] {
        assert_eq!(hidden(prose), prose);
    }
}

#[test]
fn the_value_of_a_name_that_says_it_is_secret_is_hidden_and_the_name_is_not() {
    assert_eq!(
        hidden("OPENAI_API_KEY=abcd1234efgh"),
        "OPENAI_API_KEY=[masked]"
    );
    assert_eq!(
        hidden(r#"{"password": "hunter2hunter", "user": "alice"}"#),
        r#"{"password": "[masked]", "user": "alice"}"#
    );
    assert_eq!(
        hidden("client_secret: s3cr3tvalue"),
        "client_secret: [masked]"
    );
    assert_eq!(hidden("db_pwd = hunter2"), "db_pwd = [masked]");
    assert_eq!(
        hidden(r#"{"token": "abc12345", "key": "correct-horse-battery"}"#),
        r#"{"token": "[masked]", "key": "[masked]"}"#
    );
    // Too short to be one, or no value at all.
    assert_eq!(hidden("token: x"), "token: x");
    assert_eq!(hidden("authorized: yes"), "authorized: yes");
}

/// `key` and `token` are words as well as names: the reason a login failed is
/// not hidden, and a long value or one with a digit is.
#[test]
fn a_key_or_token_named_in_a_sentence_loses_its_value_only_when_the_value_looks_like_one() {
    for prose in [
        "Invalid token: expired",
        "token: unauthorized",
        "API key: missing",
        "press any key: continuing",
        "token=abcdefgh",
    ] {
        assert_eq!(hidden(prose), prose, "{prose}");
    }
    assert_eq!(hidden("token=abcd1234"), "token=[masked]");
    assert_eq!(hidden("API key: abcdefghijklmnop"), "API key: [masked]");
}

#[test]
fn a_token_with_a_known_prefix_is_hidden_whatever_its_length() {
    let tokens = [
        concat!("sk-ant", "-api03-AbCdEfGhIjKlMnOp"),
        "sk_live_abcdefghijkl",
        concat!("ghp", "_abcdefghijklmnopqrstuvwxyz0123456789"),
        concat!("github_pat", "_11AAAAAAA0abcdefghij_xyzxyz"),
        concat!("glpat", "-abcdefghijklmnop1234"),
        concat!("xoxb", "-1234567890-abcdefghij"),
        concat!("AKI", "AIOSFODNN7EXAMPLE"),
        concat!("AIz", "aSyA-abcdefghijklmnopqrstuvwxyz01234"),
        concat!("npm", "_abcdefghijklmnopqrstuvwxyz0123456789"),
        concat!("hf", "_abcdefghijklmnopqrstuvwxyzABCDEFGH"),
        concat!("ya29", ".a0AfH6SMBabcdefghijklmnopqrstuv"),
        concat!("SG", ".abcdefghijklmnopqrstuv.abcdefghijklmnopqrstuv"),
        concat!("ey", "JhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.sig-nature_1"),
    ];
    for token in tokens {
        let masked = mask(&format!("the credential was {token} here"));
        assert_eq!(
            (masked.text.as_str(), masked.count),
            ("the credential was [masked] here", 1),
            "{token}"
        );
    }
}

#[test]
fn a_long_run_of_letters_and_digits_is_hidden_and_a_long_word_is_not() {
    assert_eq!(
        hidden("id 0123456789abcdef0123456789abcdef ok"),
        "id [masked] ok"
    );
    assert_eq!(
        hidden("ConsensFlowSupervisorStartupFailedError"),
        "ConsensFlowSupervisorStartupFailedError"
    );
    assert_eq!(
        hidden("0123456789012345678901234567"),
        "0123456789012345678901234567"
    );
    assert_eq!(hidden("session-2c4a"), "session-2c4a");
}

#[test]
fn a_base64_token_with_slashes_is_hidden_whole() {
    assert_eq!(
        hidden("secret wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY!"),
        "secret [masked]!"
    );
}

#[test]
fn prose_and_short_paths_are_left_alone() {
    for text in [
        "No API key found for the selected model. Use /login to log into a provider.",
        "ENOENT: no such file or directory, open '/usr/local/bin/pi'",
        "Error: connect ECONNREFUSED 127.0.0.1:8080",
        "codex exited with code 1 after 100 ms",
    ] {
        let masked = mask(text);
        assert_eq!((masked.text.as_str(), masked.count), (text, 0), "{text}");
    }
}

#[test]
fn each_secret_is_counted_once() {
    let masked = mask(
        "KEY=abcdef12 then ghp_abcdefghijklmnopqrstuvwxyz0123456789 then https://u:p4ssw0rd@h/",
    );
    assert_eq!(
        masked.text,
        "KEY=[masked] then [masked] then https://[masked]@h/"
    );
    assert_eq!(masked.count, 3);
}
