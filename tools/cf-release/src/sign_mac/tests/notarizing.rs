//! Apple's notary: what is sent, what its answer decides, and what a refusal shows.

use super::fake::{refused, said};
use super::{credentials, Fake, Trial, API_ISSUER, API_KEY_ID, DMG};

const ACCEPTED: &str = r#"{"id":"3d1c0a52-aaaa","status":"Accepted"}"#;
const INVALID: &str = r#"{"id":"3d1c0a52-bbbb","status":"Invalid"}"#;

#[test]
fn the_app_goes_to_the_notary_as_a_zip_and_the_dmg_as_it_is_and_each_waits_an_hour() {
    let mut trial = Trial::new(Fake::new());
    trial.sign(Some(credentials())).unwrap();
    let calls = trial.transcript();
    let sent = |what: &str| -> Vec<&String> {
        calls
            .iter()
            .filter(|call| call.starts_with("xcrun notarytool submit") && call.contains(what))
            .collect()
    };
    let wait = format!(
        "--key $SCRATCH/notary.p8 --key-id {API_KEY_ID} --issuer {API_ISSUER} \
         --wait --timeout 60m --output-format json"
    );
    assert_eq!(
        sent("ConsensFlow.app.zip"),
        [&format!(
            "xcrun notarytool submit $SCRATCH/ConsensFlow.app.zip {wait}"
        )]
    );
    assert_eq!(
        sent("$DMG"),
        [&format!("xcrun notarytool submit $DMG {wait}")]
    );
    // Only the app is zipped, and with its folder.
    let zips: Vec<_> = calls
        .iter()
        .filter(|call| call.starts_with("ditto -c"))
        .collect();
    assert_eq!(
        zips,
        ["ditto -c -k --keepParent $APP $SCRATCH/ConsensFlow.app.zip"]
    );
}

#[test]
fn a_refused_app_is_neither_stapled_nor_put_in_a_dmg_and_the_notarys_log_is_shown_whole() {
    let log = "{\"issues\":[{\"path\":\"ConsensFlow.app/Contents/Resources/cli/bin/cf\",\
               \"message\":\"The binary is not signed with a valid Developer ID certificate.\"}]}";
    let fake = Fake::new()
        .answering(
            "xcrun notarytool submit",
            refused(1, INVALID, "Error: Submission invalid"),
        )
        .answering("xcrun notarytool log", said(&format!("{log}\n")));
    let mut trial = Trial::new(fake);
    let failure = trial.sign(Some(credentials())).unwrap_err();
    assert_eq!(
        failure.to_string(),
        "the notary answered Invalid for ConsensFlow.app"
    );

    let calls = trial.transcript();
    let submitted = calls
        .iter()
        .position(|call| call.starts_with("xcrun notarytool submit"))
        .unwrap();
    // The log of the submission, asked for by the id the notary gave.
    assert_eq!(
        calls[submitted + 1],
        format!(
            "xcrun notarytool log 3d1c0a52-bbbb --key $SCRATCH/notary.p8 \
             --key-id {API_KEY_ID} --issuer {API_ISSUER}"
        )
    );
    // The one place that says which file it refused, and why.
    assert!(trial.err.contains(&format!("{log}\n")), "{}", trial.err);
    // Then nothing is stapled, and no DMG is made: only the tidying is left.
    assert!(!calls
        .iter()
        .any(|call| call.contains("stapler") || call.starts_with("hdiutil")));
}

#[test]
fn the_dmg_is_notarized_after_the_app_and_a_refused_dmg_is_named() {
    let fake = Fake::new().answering_in_turn(
        "xcrun notarytool submit",
        [said(ACCEPTED), refused(1, INVALID, "")],
    );
    let mut trial = Trial::new(fake);
    let failure = trial.sign(Some(credentials())).unwrap_err();
    assert_eq!(
        failure.to_string(),
        format!("the notary answered Invalid for {DMG}")
    );

    let calls = trial.transcript();
    // The app's ticket is on the app; the DMG's is not asked for.
    assert!(calls.iter().any(|call| call == "xcrun stapler staple $APP"));
    assert!(!calls.iter().any(|call| call == "xcrun stapler staple $DMG"));
    assert!(!calls.iter().any(|call| call.starts_with("spctl")));
}

#[test]
fn an_answer_that_is_not_json_is_the_tools_failure_with_what_it_said_and_asks_no_log() {
    let fake = Fake::new().answering(
        "xcrun notarytool submit",
        refused(69, "", "Error: could not reach the notary\n"),
    );
    let mut trial = Trial::new(fake);
    let failure = trial.sign(Some(credentials())).unwrap_err();
    assert_eq!(
        failure.to_string(),
        "notarytool submit failed: Error: could not reach the notary"
    );
    assert!(!trial
        .transcript()
        .iter()
        .any(|call| call.contains("notarytool log")));
}

#[test]
fn any_status_but_accepted_is_a_refusal_named_and_one_with_no_id_asks_no_log() {
    for (answer, status, asks_log) in [
        (r#"{"id":"x1","status":"In Progress"}"#, "In Progress", true),
        (r#"{"id":"x2","status":"Rejected"}"#, "Rejected", true),
        (r#"{"status":"Invalid"}"#, "Invalid", false),
        ("{}", "no status", false),
        ("null", "no status", false),
        (r#"{"status":7}"#, "no status", false),
    ] {
        let fake = Fake::new().answering("xcrun notarytool submit", refused(1, answer, ""));
        let mut trial = Trial::new(fake);
        let failure = trial.sign(Some(credentials())).unwrap_err();
        assert_eq!(
            failure.to_string(),
            format!("the notary answered {status} for ConsensFlow.app"),
            "{answer}"
        );
        let asked = trial
            .transcript()
            .iter()
            .any(|call| call.contains("notarytool log"));
        assert_eq!(asked, asks_log, "{answer}");
    }
}

#[test]
fn a_ticket_is_stapled_and_then_checked_on_each_file() {
    let mut trial = Trial::new(Fake::new());
    trial.sign(Some(credentials())).unwrap();
    let calls = trial.transcript();
    for target in ["$APP", "$DMG"] {
        let staple = calls
            .iter()
            .position(|call| *call == format!("xcrun stapler staple {target}"))
            .unwrap();
        assert_eq!(
            calls[staple + 1],
            format!("xcrun stapler validate {target}")
        );
    }
}

#[test]
fn a_file_gatekeeper_does_not_take_as_notarized_ends_the_run_naming_it() {
    // It accepts the app, as any signed code is, but not as notarized; or it refuses.
    for (assessed, said) in [
        (
            refused(0, "", "accepted\nsource=Developer ID\n"),
            "accepted\nsource=Developer ID",
        ),
        (
            refused(3, "", "rejected\nsource=Notarized Developer ID\n"),
            "rejected\nsource=Notarized Developer ID",
        ),
    ] {
        let mut trial = Trial::new(Fake::new().answering_in_turn("spctl", [assessed]));
        let failure = trial.sign(Some(credentials())).unwrap_err();
        assert_eq!(
            failure.to_string(),
            format!("Gatekeeper refuses ConsensFlow.app: {said}")
        );
        // The DMG is not assessed once the app is refused.
        let assessed = trial
            .transcript()
            .iter()
            .filter(|call| call.starts_with("spctl"))
            .count();
        assert_eq!(assessed, 1);
    }
    // Then the DMG's turn: the app is taken, and the DMG is not.
    let fake = Fake::new().answering_in_turn(
        "spctl",
        [
            refused(0, "", "source=Notarized Developer ID\n"),
            refused(0, "", "source=Unnotarized Developer ID\n"),
        ],
    );
    let failure = Trial::new(fake).sign(Some(credentials())).unwrap_err();
    assert_eq!(
        failure.to_string(),
        format!("Gatekeeper refuses {DMG}: source=Unnotarized Developer ID")
    );
}

#[test]
fn what_is_signed_ad_hoc_is_held_to_its_seal_and_not_to_gatekeeper() {
    let mut trial = Trial::new(Fake::new().answering("spctl", refused(3, "", "rejected")));
    trial.sign(None).unwrap();
    let calls = trial.transcript();
    assert!(calls
        .iter()
        .any(|call| call == "codesign --verify --deep --strict $APP"));
    assert!(calls
        .iter()
        .any(|call| call == "codesign --verify --strict $DMG"));
    assert!(!calls.iter().any(|call| call.starts_with("spctl")));
}

#[test]
fn a_ticket_that_cannot_be_stapled_ends_the_run_there() {
    let fake = Fake::new()
        .refusing("xcrun stapler staple", 1)
        .saying("no ticket yet");
    let mut trial = Trial::new(fake);
    let failure = trial.sign(Some(credentials())).unwrap_err();
    assert_eq!(failure.to_string(), "xcrun stapler failed: no ticket yet");
    assert!(!trial
        .transcript()
        .iter()
        .any(|call| call.starts_with("hdiutil")));
}
