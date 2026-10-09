//! The command line: what its words ask for, the identity it needs and the
//! bundle it looks in.

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use super::{Env, Failure, Fake, Options, Trial};

fn words(line: &str) -> Vec<OsString> {
    line.split_whitespace().map(OsString::from).collect()
}

/// What `cf-release <line>` answers when the environment holds `vars`: its
/// status, and what it said.
fn answer(vars: &[(&str, &str)], line: &str) -> (u8, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = crate::run(
        &Env::from_vars(vars.iter().copied()),
        &words(line),
        &mut out,
        &mut err,
    );
    assert!(out.is_empty());
    (status, String::from_utf8(err).unwrap())
}

#[test]
fn the_bundle_is_where_tauri_leaves_it_unless_it_is_named() {
    let options = Options::parse(&words("")).unwrap();
    assert_eq!(
        options.bundle,
        PathBuf::from("app/src-tauri/target/release/bundle")
    );
    assert!(!options.adhoc);

    for line in ["--adhoc --bundle some/where", "--bundle some/where --adhoc"] {
        let options = Options::parse(&words(line)).unwrap();
        assert_eq!(options.bundle, PathBuf::from("some/where"), "{line}");
        assert!(options.adhoc, "{line}");
    }
}

#[test]
fn a_word_it_does_not_take_and_a_bundle_with_no_folder_are_refused_as_usage() {
    for (line, refusal) in [
        ("--sign", "unknown argument: --sign"),
        ("some/where", "unknown argument: some/where"),
        ("--bundle", "--bundle needs a value"),
        ("--bundle --adhoc", "--bundle needs a value"),
    ] {
        match Options::parse(&words(line)) {
            Err(Failure::Usage(said)) => assert_eq!(said, refusal, "{line}"),
            other => panic!("{line}: {other:?}"),
        }
    }
    let (status, said) = answer(&[], "sign-mac --sign");
    assert_eq!(status, 2);
    assert!(
        said.starts_with("cf-release sign-mac: unknown argument: --sign\n"),
        "{said}"
    );
}

#[test]
fn a_release_without_its_identity_is_refused_naming_what_is_missing_in_the_scripts_order() {
    let (status, said) = answer(
        &[("PATH", "/usr/bin"), ("APPLE_API_KEY_ID", "KEY")],
        "sign-mac --bundle /nowhere",
    );
    assert_eq!(status, 1);
    assert_eq!(
        said,
        "cf-release sign-mac: APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, APPLE_API_KEY, \
         APPLE_API_ISSUER not set; --adhoc signs with no identity\n"
    );
    let (status, said) = answer(&[], "sign-mac");
    assert_eq!(status, 1);
    assert!(
        said.contains(
            "APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, APPLE_API_KEY, APPLE_API_KEY_ID, \
             APPLE_API_ISSUER not set"
        ),
        "{said}"
    );
}

#[test]
fn a_variable_that_is_set_to_nothing_is_not_set() {
    let all = [
        ("APPLE_CERTIFICATE", "Y2VydA=="),
        ("APPLE_CERTIFICATE_PASSWORD", "password"),
        ("APPLE_API_KEY", "key"),
        ("APPLE_API_KEY_ID", ""),
        ("APPLE_API_ISSUER", "issuer"),
    ];
    let (status, said) = answer(&all, "sign-mac --bundle /nowhere");
    assert_eq!(status, 1);
    assert_eq!(
        said,
        "cf-release sign-mac: APPLE_API_KEY_ID not set; --adhoc signs with no identity\n"
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn off_a_mac_it_says_so_before_it_runs_anything() {
    let (status, said) = answer(&[], "sign-mac --adhoc --bundle /nowhere");
    assert_eq!(status, 1);
    assert_eq!(
        said,
        "cf-release sign-mac: it signs with Apple's tools, which are on a Mac\n"
    );
}

#[test]
fn a_bundle_holds_one_app_and_one_dmg_and_a_run_that_finds_otherwise_leaves_nothing() {
    let mut trial = Trial::new(Fake::new());
    let macos = trial.bundle.path().join("macos");
    let dmg = trial.bundle.path().join("dmg");

    fs::create_dir(macos.join("Other.app")).unwrap();
    let failure = trial.sign(None).unwrap_err().to_string();
    assert_eq!(
        failure,
        format!("{} holds 2 .app files, not one", macos.display())
    );
    assert_eq!(trial.left_behind(), Vec::<String>::new());

    fs::remove_dir_all(macos.join("Other.app")).unwrap();
    fs::write(dmg.join("Second.dmg"), "").unwrap();
    let failure = trial.sign(None).unwrap_err().to_string();
    assert_eq!(
        failure,
        format!("{} holds 2 .dmg files, not one", dmg.display())
    );

    fs::remove_dir_all(&dmg).unwrap();
    let failure = trial.sign(None).unwrap_err().to_string();
    assert!(
        failure.starts_with(&format!("could not read {}: ", dmg.display())),
        "{failure}"
    );
    // No program was run for any of it.
    assert_eq!(trial.fake.calls(), Vec::<String>::new());
}
