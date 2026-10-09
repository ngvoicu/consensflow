//! No secret is in a word the run says. Every tool here says all of them, on
//! its error stream and, when it fails, on its output too; the run tells it as
//! it must, and a test looks for them in the failure and in every line.

use std::fs;

use super::fake::{refused, said};
use super::{
    credentials, secrets, Console, Fake, Request, Trial, API_KEY_ID, CERTIFICATE_PASSWORD,
    KEYCHAIN_PASSWORD,
};

/// What a tool that says them all says.
fn chatter() -> String {
    secrets().join(" | ")
}

fn assert_clean(what: &str, text: &str) {
    for secret in secrets() {
        assert!(!text.contains(&secret), "{what} holds {secret}: {text}");
    }
}

#[test]
fn whichever_call_fails_its_failure_and_every_line_are_told_with_the_secrets_blanked_out() {
    let mut trial = Trial::new(Fake::new());
    trial.sign(Some(credentials())).unwrap();
    for index in 0..trial.fake.calls().len() {
        let mut trial = Trial::new(Fake::new().failing_at(index).saying(&chatter()));
        let result = trial.sign(Some(credentials()));
        assert_clean("the lines of the run", &trial.err);
        assert_clean("the output of the run", &trial.out);
        let Err(failure) = result else { continue };
        let failure = failure.to_string();
        assert_clean(&format!("the failure at call {index}"), &failure);
        // What the tool said is told, with the secrets blanked out of it.
        if failure.contains(" failed: ") {
            assert!(failure.contains("[redacted]"), "call {index}: {failure}");
        }
    }
}

#[test]
fn a_run_that_works_says_nothing_of_a_secret_though_every_tool_says_all_of_them() {
    let mut trial = Trial::new(Fake::new().saying(&chatter()));
    trial.sign(Some(credentials())).unwrap();
    assert_clean("the lines of the run", &trial.err);
    assert_clean("the output of the run", &trial.out);
}

#[test]
fn the_notarys_log_is_shown_with_the_secrets_blanked_out_of_it() {
    let log = format!("issues for {API_KEY_ID}: {}", chatter());
    let fake = Fake::new()
        .answering(
            "xcrun notarytool submit",
            refused(1, r#"{"id":"abc","status":"Invalid"}"#, ""),
        )
        .answering("xcrun notarytool log", said(&log));
    let mut trial = Trial::new(fake);
    let failure = trial.sign(Some(credentials())).unwrap_err();
    assert_clean("the failure", &failure.to_string());
    assert_clean("the lines of the run", &trial.err);
    // It is shown all the same: what the notary said of the file, round the secrets.
    assert!(
        trial
            .err
            .contains("issues for [redacted]: [redacted] | [redacted]"),
        "{}",
        trial.err
    );
}

#[test]
fn a_secret_that_a_tool_says_in_a_failure_of_the_certificates_import_is_blanked_there() {
    let fake = Fake::new().refusing("security import", 1).saying(&format!(
        "bad passphrase {CERTIFICATE_PASSWORD} for the keychain {KEYCHAIN_PASSWORD}"
    ));
    let mut trial = Trial::new(fake);
    let failure = trial.sign(Some(credentials())).unwrap_err();
    assert_eq!(
        failure.to_string(),
        "security import failed: bad passphrase [redacted] for the keychain [redacted]"
    );
}

#[test]
fn a_failure_no_tool_wrote_is_blanked_too_before_it_leaves() {
    // The bundle's folder is named for a secret: the run's own words name it.
    let tmp = tempfile::tempdir().unwrap();
    let bundle = tmp.path().join(API_KEY_ID);
    fs::create_dir_all(bundle.join("macos")).unwrap();
    let request = Request {
        bundle,
        credentials: Some(credentials()),
        tmp: tmp.path().to_path_buf(),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let failure = super::super::sign(
        &Fake::new(),
        &request,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(!failure.contains(API_KEY_ID), "{failure}");
    assert!(failure.contains("[redacted]"), "{failure}");
    assert!(
        failure.ends_with("holds 0 .app files, not one"),
        "{failure}"
    );
}
