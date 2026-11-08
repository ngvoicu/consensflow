//! The bundle signed is the release's (T-903): Tauri's own configuration names another
//! identifier, so that no build made on a developer's Mac carries the release's, and only
//! the release workflows merge `tauri.release.conf.json` over it. A workflow that forgot
//! would sign, notarize and publish an app the installed ones refuse to update to (they
//! check what they install is `dev.ngvoicu.consensflow`), so the signing refuses it first.

use super::fake::said;
use super::{credentials, Fake, Trial};

fn the_bundle_says(identifier: &str) -> Fake {
    Fake::new().answering(
        "plutil -extract CFBundleIdentifier",
        said(&format!("{identifier}\n")),
    )
}

#[test]
fn a_bundle_built_with_another_identifier_is_refused_before_anything_is_signed() {
    for credentials in [Some(credentials()), None] {
        let mut trial = Trial::new(the_bundle_says("dev.ngvoicu.consensflow.local"));
        let said = trial.sign(credentials).unwrap_err().to_string();
        assert_eq!(
            said,
            "ConsensFlow.app is dev.ngvoicu.consensflow.local, not dev.ngvoicu.consensflow: \
             the release builds with --config src-tauri/tauri.release.conf.json"
        );
        let calls = trial.transcript();
        assert!(
            calls.iter().all(|call| !call.starts_with("codesign")),
            "{calls:#?}"
        );
    }
}

#[test]
fn a_bundle_with_the_release_identifier_is_signed() {
    let mut trial = Trial::new(the_bundle_says(super::ID));
    trial.sign(Some(credentials())).unwrap();
    assert!(trial
        .transcript()
        .iter()
        .any(|call| call.starts_with("codesign")));
}

/// The identifier the signing holds the bundle to is the one the release's configuration gives.
#[test]
fn the_identifier_it_holds_the_bundle_to_is_the_release_configurations() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../app/src-tauri/tauri.release.conf.json");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    assert_eq!(
        config["identifier"],
        super::super::signing::RELEASE_IDENTIFIER
    );
    assert_eq!(super::ID, super::super::signing::RELEASE_IDENTIFIER);
}
