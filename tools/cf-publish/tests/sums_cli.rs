//! `cf-publish sums`, as the signing job runs it: the SHA256SUMS of the release
//! built in a folder, the copy of it the job signs, named for the release, and
//! that name on stdout.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use cf_publish::testing::{built_files, folder_of, published_assets, run_binary, Files, Ran};

const RELEASE: &str = "3.0.7-alpha.1";

fn sums(dir: &Path) -> Ran {
    run_binary(
        Path::new(env!("CARGO_BIN_EXE_cf-publish")),
        &["sums", "--dir", dir.to_str().unwrap(), "--version", RELEASE],
        &[],
        None,
    )
}

/// The files of the release as the build jobs leave them: no sums, no signature.
fn built() -> Files {
    built_files("https://example.invalid", RELEASE)
        .without("SHA256SUMS")
        .without("SHA256SUMS.sig")
}

#[test]
fn writes_the_sums_and_the_copy_that_is_signed_and_names_it() {
    let files = built();
    let dir = folder_of(&files);
    let ran = sums(dir.path());
    assert_eq!(ran.status, 0, "{}", ran.said());
    let signed = format!("ConsensFlow_{RELEASE}_SHA256SUMS");
    assert_eq!(ran.stdout, format!("{signed}\n"));
    let written = fs::read_to_string(dir.path().join("SHA256SUMS")).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join(&signed)).unwrap(),
        written
    );
    assert_eq!(
        written,
        published_assets(&files).text("SHA256SUMS"),
        "the sums the publisher publishes"
    );
}

#[test]
fn is_refused_for_a_file_of_the_release_that_is_missing() {
    let dir = folder_of(&built().without(&format!("nsis/ConsensFlow_{RELEASE}_x64-setup.exe")));
    let ran = sums(dir.path());
    assert_eq!(ran.status, 1);
    assert_eq!(
        ran.stderr,
        format!("sums: missing nsis/ConsensFlow_{RELEASE}_x64-setup.exe\n")
    );
    assert!(!dir.path().join("SHA256SUMS").exists());
}
