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
    assert_eq!(RULES.len(), 12);
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
fn an_email_address_is_hidden_whole_and_a_host_with_no_domain_is_not() {
    let masked = mask("Your account ci.bot+team@example.co.uk has no seats left.");
    assert_eq!(masked.text, "Your account [masked] has no seats left.");
    assert_eq!(masked.count, 1);
    assert_eq!(hidden("git@host:x/y and @zeus"), "git@host:x/y and @zeus");
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

/// Whether any six characters in a row of `value` are in `text`: a part of the value that
/// the mask let through.
fn survives(text: &str, value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars
        .windows(6)
        .any(|piece| text.contains(&piece.iter().collect::<String>()))
}

/// T-823, the review of `a26e0848e` (P1): a base64 value with no digit in it was no token
/// to the masker, and a row that names nothing, which the note quotes, held it.
#[test]
fn a_long_base64_value_is_hidden_whether_or_not_a_digit_is_in_it() {
    for value in [
        "YWJj".repeat(12),
        "QUJD".repeat(12),
        "YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXpBQkNERUZHSElK".to_owned(),
        format!("{}==", "YWJj".repeat(11)),
    ] {
        let masked = mask(&format!("value {value} end"));
        assert_eq!(masked.text, "value [masked] end", "{value}");
        assert_eq!(masked.count, 1);
    }
    // A base64 run with a `+`, or padded, is one with no digit, whatever its slashes.
    let with_plus = "abcdefghij+klmnopqrstuvwxyzABCDEFGHIJKLMNOP";
    assert_eq!(hidden(&format!("x {with_plus} y")), "x [masked] y");
}

/// What is no base64 stays: a word, a camel-case name, a path, a number.
#[test]
fn a_name_a_path_and_a_number_are_not_taken_for_base64() {
    for text in [
        "ConsensFlowSupervisorStartupFailedError",
        "/Users/gabrielvoicu/Projects/ngvoicu/consensflow/crates",
        "cf_engine_dispatcher_tests_a_very_long_name_that_goes_on",
        "01234567890123456789012345678901234567890123456789",
    ] {
        assert_eq!(hidden(text), text, "{text}");
    }
}

/// The spans of every rule are found in the text as it was, and joined, before any is
/// replaced: a rule that took a piece of a value first left the rest of it to the next rule,
/// which saw a shorter run (`[masked]+/abcdefghijklmnopqr/AA==`, the review's).
#[test]
fn no_part_of_a_value_survives_the_rules_that_overlap_on_it() {
    let values = [
        // A digit-bearing run, then a base64 tail with no digit.
        "Zm9vYmFyMTIzNDU2Nzg5MGFiY2RlZg+/abcdefghijklmnopqr/AA==".to_owned(),
        // The same, with the tail first.
        "abcdefghijklmnopqr/AA==+Zm9vYmFyMTIzNDU2Nzg5MGFiY2RlZg".to_owned(),
        // A long base64url run with its digits in the middle.
        format!(
            "{}0123456789{}",
            "AbCdEfGhIj".repeat(3),
            "KlMnOpQrSt".repeat(3)
        ),
        // Under a label, so the label's rule and the run's overlap.
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY+/abcdefghijklmnopqr/AA==".to_owned(),
    ];
    for value in &values {
        for text in [
            value.clone(),
            format!("secret: {value}"),
            format!("DB_PASSWORD={value} done"),
            format!("token {value}."),
        ] {
            let masked = mask(&text);
            assert!(!survives(&masked.text, value), "{text} -> {}", masked.text);
            assert!(masked.count >= 1, "{text}");
        }
    }
    // One secret is one, however many rules found it.
    assert_eq!(mask(&format!("secret: {}", values[0])).count, 1);
}

/// A command a screen echoes: curl's user and password are a pair with a colon in it, which
/// no name says is secret.
#[test]
fn the_user_and_password_of_a_commands_dash_u_are_hidden() {
    for (text, value) in [
        (
            "$ curl -u alice:swordfish https://example.test/",
            "swordfish",
        ),
        (
            "curl --user alice:swordfish https://example.test/",
            "swordfish",
        ),
        (
            "curl --user=alice:swordfish https://example.test/",
            "swordfish",
        ),
        (
            "curl --proxy-user alice:swordfish https://example.test/",
            "swordfish",
        ),
        ("curl -ualice:swordfish https://example.test/", "swordfish"),
        ("curl https://example.test/ -U proxy:swordfish", "swordfish"),
    ] {
        let masked = mask(text);
        assert!(!masked.text.contains(value), "{text}: {}", masked.text);
        assert_eq!(masked.count, 1, "{text}");
    }
    // A flag that is no user's, and a user alone, stay.
    for text in [
        "sort -u file.txt",
        "git push -u origin main",
        "curl -u alice https://x.test",
        "ls -U",
    ] {
        assert_eq!(hidden(text), text, "{text}");
    }
}

/// The review of `860010398` found a 48-letter base64 value of small letters alone survive: no
/// word is forty letters long, so a run of that length is hidden in any case.
#[test]
fn a_long_run_of_small_letters_is_hidden_too() {
    for run in [
        "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuv".to_owned(),
        "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz".to_owned(),
        "ZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXphYmNkZWZnaGlqa2w".to_owned(),
    ] {
        let masked = mask(&format!("value {run} end"));
        assert_eq!(masked.text, "value [masked] end", "{run}");
        assert_eq!(masked.count, 1);
    }
    // Thirty-nine is a long word, and a run of digits is a number.
    for text in [
        "abcdefghijklmnopqrstuvwxyzabcdefghijklm",
        "01234567890123456789012345678901234567890123456789",
    ] {
        assert_eq!(hidden(text), text, "{text}");
    }
}

/// The beginning of a token of a known kind is enough to withhold a screen (T-1205 round 4):
/// the re-check read `sk-abcd efgh …` out of the ledger, for the masker needs the body whole.
#[test]
fn the_beginning_of_a_token_of_a_known_kind_is_found_with_a_few_characters_of_its_body() {
    for text in [
        "sk-abcd",
        "see sk-abcd efgh ijkl",
        "pk_live_abcd",
        "sk_live_abcd",
        "rk_test_abcd",
        "ghp_abcd",
        "gho_abcd",
        "ghu_abcd",
        "ghs_abcd",
        "ghr_abcd",
        "github_pat_abcd",
        "glpat-abcd",
        "xoxb-abcd",
        "xoxa-abcd",
        "xoxp-abcd",
        "xoxr-abcd",
        "xoxs-abcd",
        "AKIAABCD",
        "ASIAABCD",
        "AIzaabcd",
        "eyJhbGci",
        "npm_abcdefghijkl",
        "hf_abcdefghijkl",
        "ya29.abcdefghijkl",
        "SG.abcdefgh",
        // The prefix with nothing after it, a blank, or the end of the text.
        "sk-",
        "token sk- then",
        "glpat- abcd",
        // A Basic credential of base64's, as short as four characters.
        "Basic YTpi",
        "basic dXNlcjpwYXNz",
    ] {
        assert!(begins_a_token(text), "{text}");
    }
    // What begins like a kind and is a word or a name.
    for text in [
        "sk",
        // Stripe's other keys are only the live and test ones; the rest are names.
        "pk_users_id",
        "pk_id",
        "rk_orders",
        "pk-abcd",
        "rk-abcd",
        // A word of capitals after Basic is no credential.
        "Basic HTTP server",
        "Basic JSON",
        "task-list",
        "disk-usage is high",
        "ask-me anything",
        "npm_config_cache",
        "npm_abc",
        "hf_hub_download",
        "SG.Assert",
        "ASIA",
        "ASIAN",
        "AKIA",
        "eyJ",
        "ya29.short",
        "Basic setup",
        "Basic Authentication",
        "basic",
        "ghx_abcd",
        "xoxz-abcd",
    ] {
        assert!(!begins_a_token(text), "{text}");
    }
}

/// A4-08: the words of a program are withheld for a credential's name with a value after it, not
/// for the name. Which names, and which values, are the masker's own.
#[test]
fn a_credentials_name_with_a_value_after_it_is_found_and_a_name_alone_is_not() {
    for text in [
        // A name of a password, a secret or a credential: any value, on the line or the next.
        "password: x",
        "DB_PWD=red green blue",
        "\"passphrase\": \"a b\"",
        "SECRET = 'a b'",
        "credential:\nthe value below",
        "x_Credentials=1",
        "Authorization: abc",
        "bearer: abc",
        // A key or a token: a value the masker takes for one.
        "api_key=abcdefgh12",
        "\"apiKey\": \"abcdefghijklmnop\"",
        "token: abcd1234",
        // A scheme and its credential.
        "Bearer abcdefgh1234",
        "Basic YWxhZGRpbjpvcGVuc2VzYW1l",
    ] {
        assert!(names_a_value(text), "{text}");
    }
    for text in [
        // A name alone, in a message, a path or a package.
        "OPENAI_API_KEY is not set",
        "OAuth login expired. Run /login",
        "Cannot find module 'oauth-helper'",
        "/home/h/.pi/agent/auth.json",
        "Invalid auth.json credential for provider \"openai-codex\"",
        "Credentials may have expired",
        "Authorization failed for the account",
        "No API key found for openrouter.",
        // `key` and `token` are words too: a message after them is no value.
        "Invalid key: expected string",
        "Unexpected token: }",
        "token: expired",
        "key not found",
        // The scheme with nothing after it that is a credential.
        "Bearer abc",
        "basic setup",
    ] {
        assert!(!names_a_value(text), "{text}");
    }
}
