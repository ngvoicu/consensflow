//! The form of an account hold names only literals of the lists, whatever the words say.

use cf_proto::hold::{BILLING_CODES, CODES, KINDS, LOGIN_CODES, STATUSES};

use super::*;

fn form(because: &str) -> Option<(&'static str, Option<u16>, Option<&'static str>)> {
    hold_form(because).map(|form| (form.kind(), form.status(), form.code()))
}

#[test]
fn tells_which_was_refused_its_status_and_its_provider_s_error_type() {
    for (because, expected) in [
        (
            r#"its login was refused: 401 {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
            ("login", Some(401), Some("authentication_error")),
        ),
        (
            "its login was refused: OAuth refresh failed for anthropic: Anthropic token refresh request failed. details=Error: HTTP request failed. status=401; body={\"error\":\"invalid_grant\"}",
            ("login", None, Some("invalid_grant")),
        ),
        (
            "its login was refused: OAuth refresh failed for openai-codex: OpenAI Codex token refresh failed (401)",
            ("login", None, Some("refresh_failed")),
        ),
        (
            "its login was refused: OAuth refresh failed for meta: Meta session expired (status 401). Run `/login meta` to sign in again.",
            ("login", None, Some("session_expired")),
        ),
        (
            "its login was refused: 401 OAuth token has expired.",
            ("login", Some(401), None),
        ),
        (
            r#"its billing was refused: 402 {"error":{"code":"insufficient_quota"}}"#,
            ("billing", Some(402), Some("insufficient_quota")),
        ),
        (
            r#"its billing was refused: 400 {"message":"Your credit balance is too low to access the Anthropic API."}"#,
            ("billing", None, Some("credit_balance_too_low")),
        ),
        ("its billing was refused: Payment required.", ("billing", None, None)),
        ("its login was refused", ("login", None, None)),
        // Muse's fixed sentences say which, and nothing else.
        (crate::muse::record::NO_ALLOWANCE, ("billing", None, None)),
        (crate::muse::record::LOGIN_REFUSED, ("login", None, None)),
        // A code inside a longer word is no code.
        (
            r#"its login was refused: 401 {"error":{"code":"previous_invalid_grant_pending"}}"#,
            ("login", Some(401), None),
        ),
        // A status the form does not name, a code of the other kind.
        (
            r#"its login was refused: 429 {"error":{"code":"insufficient_quota"}}"#,
            ("login", None, None),
        ),
    ] {
        assert_eq!(form(because), Some(expected), "{because}");
    }
    // A reason that is no account's refusal has no form.
    for because in [
        "its model moved from a to b on its own (it said 401 authentication_error)",
        "503 overloaded_error",
        "Muse refused: something else",
        "",
    ] {
        assert_eq!(form(because), None, "{because}");
    }
}

#[test]
fn muse_s_two_readers_say_the_same_billing_sentence() {
    assert_eq!(
        crate::muse::record::NO_ALLOWANCE,
        crate::muse::usage::NO_ALLOWANCE
    );
}

#[test]
fn whatever_the_words_say_each_value_of_the_form_is_a_literal_of_the_lists() {
    // Every text of the repository's refusal fixtures, as either kind, and with hostile words.
    let rows = include_str!("../tests/account-refusals.jsonl");
    let mut texts: Vec<String> = rows
        .lines()
        .map(|row| {
            serde_json::from_str::<serde_json::Value>(row).unwrap()["text"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    texts.extend([
        "401 owner.account@example.invalid authentication_error password: hunter2".to_owned(),
        "402 jos\u{e9}@ex\u{e4}mple.invalid insufficient_quota sk-ant-api03-Zq8vN2mLk4Rt7XyP1wUc5Hd9".to_owned(),
    ]);
    let mut seen = 0;
    for text in &texts {
        for lead in ["its login was refused: ", "its billing was refused: "] {
            if let Some(form) = hold_form(&format!("{lead}{text}")) {
                assert!(KINDS.contains(&form.kind()), "{text}");
                assert!(
                    form.status().is_none_or(|s| STATUSES.contains(&s)),
                    "{text}"
                );
                assert!(form.code().is_none_or(|c| CODES.contains(&c)), "{text}");
                let own = if form.kind() == "login" {
                    &LOGIN_CODES[..]
                } else {
                    &BILLING_CODES[..]
                };
                assert!(form.code().is_none_or(|c| own.contains(&c)), "{text}");
                seen += 1;
            }
        }
    }
    assert_eq!(seen, texts.len() * 2);
}
