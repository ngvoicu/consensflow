//! The publisher as the release builds it (`cargo build --locked -p cf-publish`,
//! no feature), run: it checks SHA256SUMS against the update key it was built
//! with and no other, whatever `CF_PUBLISH_TEST_KEY` says. Only a build with the
//! tests' kit (`test-support`) takes that variable; this test builds the binary
//! without it, in a target folder of its own, and hands it a release signed by
//! the tests' key, which it must refuse.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use cf_publish::testing::signing::test_key;
use cf_publish::testing::{built_files, folder_of, run_binary, LATER};

/// cf-publish built as the release builds it, with no feature, in a target folder
/// of its own.
#[allow(clippy::disallowed_methods)] // The test builds what it tests.
fn release_build() -> PathBuf {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("release-build");
    let built = Command::new(env!("CARGO"))
        .args([
            "build",
            "--offline",
            "--locked",
            "-p",
            "cf-publish",
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo runs");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    target.join("debug").join(if cfg!(windows) {
        "cf-publish.exe"
    } else {
        "cf-publish"
    })
}

#[test]
fn the_release_build_refuses_a_release_signed_by_the_key_the_environment_names() {
    let files = built_files("http://127.0.0.1:9", LATER);
    let dir = folder_of(&files);
    let tag = format!("v{LATER}");
    let dir_text = dir.path().to_str().unwrap();
    let args = [
        "publish",
        "--dir",
        dir_text,
        "--tag",
        tag.as_str(),
        "--base",
        "http://127.0.0.1:9",
        "--repo",
        "owner/repo",
    ];
    let env = [
        ("GITHUB_EVENT_NAME", "push"),
        ("GITHUB_REF_TYPE", "tag"),
        ("GITHUB_REF_NAME", tag.as_str()),
        ("CF_PUBLISH_TEST_KEY", test_key()),
    ];
    let ran = run_binary(&release_build(), &args, &env, None);
    assert_eq!(ran.status, 1, "{}", ran.said());
    assert!(
        ran.stderr
            .starts_with("publish: SHA256SUMS.sig does not hold: it does not sign SHA256SUMS under the update key"),
        "{}",
        ran.said()
    );
    // The build with the tests' kit takes the variable, and goes past the signature.
    let kit = run_binary(
        Path::new(env!("CARGO_BIN_EXE_cf-publish")),
        &args,
        &env,
        None,
    );
    assert!(!kit.stderr.contains("does not hold"), "{}", kit.said());
}
