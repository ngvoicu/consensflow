//! The sign job of the release workflow (.github/workflows/release.yml): the
//! SHA256SUMS the one-line installers check a release by, signed with the
//! update key in one step of a job that can write nothing, checked as the
//! installers check it, and published by the publish job, which waits for it.
//! Its text, and its steps run as written with a key made for the test: the
//! signature it makes names the release, and the check after it refuses a
//! signature by any key but the one the installers pin.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_publish::testing::workflow::release::{HASH, SIGN, SIGNATURE, SUMS};
use cf_publish::testing::workflow::{job, read, step};

mod its_text {
    use super::*;

    #[test]
    fn holds_the_update_key_in_the_one_step_that_signs_and_can_write_nothing() {
        let sign = job(&read("release.yml"), "sign");
        assert!(
            sign.contains("    environment: release\n"),
            "the key is the release environment's"
        );
        assert!(
            sign.contains("    permissions:\n      contents: read\n"),
            "its token reads the tree and nothing more"
        );
        for word in ["contents: write", "GH_TOKEN", "github.token", "id-token"] {
            assert!(!sign.contains(word), "the sign job holds {word}");
        }
        assert_eq!(
            sign.matches("secrets.").count(),
            1,
            "one secret, in one place"
        );
        let signing = step(&sign, SIGN);
        assert!(
            signing.env().contains(&(
                "TAURI_SIGNING_PRIVATE_KEY".to_string(),
                "${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}".to_string()
            )),
            "{:?}",
            signing.env()
        );
        assert!(!sign.contains("\n    env:"), "the key is not the job's");
        assert!(
            signing.script().contains(
                "\"$GITHUB_WORKSPACE/app/node_modules/.bin/tauri\" signer sign \"$SIGNED\""
            ),
            "{}",
            signing.script()
        );
        assert!(
            sign.contains("- run: npm ci --prefix app --ignore-scripts\n"),
            "Tauri's signer, with no install script run"
        );
        let checkout = sign.split("- uses: actions/checkout").nth(1).unwrap();
        assert!(
            checkout
                .lines()
                .take(4)
                .any(|line| line.trim() == "persist-credentials: false"),
            "the checkout leaves no credential behind"
        );
        for line in sign.lines().filter(|line| line.contains("uses:")) {
            assert!(
                line.contains("uses: actions/"),
                "a third-party action where the key is: {line}"
            );
        }
    }

    #[test]
    fn signs_the_sums_the_publisher_writes_checks_them_and_hands_them_on() {
        let sign = job(&read("release.yml"), "sign");
        let at = |name: &str| {
            sign.find(&format!("- name: {name}"))
                .unwrap_or_else(|| panic!("no step {name}"))
        };
        assert!(
            at(HASH) < at(SUMS) && at(SUMS) < at(SIGN) && at(SIGN) < at(SIGNATURE),
            "the publisher checked, the sums written, signed, then checked"
        );
        assert!(step(&sign, SUMS)
            .script()
            .contains("\"$RUNNER_TEMP/sums-tool/cf-publish\" sums --dir . --version \"$version\""));
        assert!(step(&sign, SIGNATURE)
            .script()
            .contains("node \"$GITHUB_WORKSPACE/install/check-signature.mjs\""));
        assert!(sign.contains("          name: ConsensFlow-signature\n"));
        let publish = job(&read("release.yml"), "publish");
        let needs = publish
            .lines()
            .find_map(|line| line.strip_prefix("    needs: "))
            .unwrap();
        assert!(
            needs.contains("sign"),
            "the publish job does not wait for the signature: {needs}"
        );
        assert!(
            publish.contains("pattern: ConsensFlow-*"),
            "it downloads the signature with the files"
        );
    }
}

#[cfg(unix)]
mod its_steps_as_written {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    use cf_publish::digest::sha256_hex;
    use cf_publish::testing::workflow::{bash, job, read, step};
    use cf_publish::testing::{built_files, folder_of, published_assets, Ran, TempDir};

    use super::*;

    const RELEASE: &str = "3.0.7-alpha.1";

    /// The repository this test is in.
    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// The step `name` of the sign job, run in `dir` as the workflow runs it.
    fn run(name: &str, dir: &Path, workspace: &Path, runner: &Path, env: &[(&str, &str)]) -> Ran {
        let sign = job(&read("release.yml"), "sign");
        let mut all = vec![
            ("GITHUB_WORKSPACE", workspace.to_str().unwrap()),
            ("RUNNER_TEMP", runner.to_str().unwrap()),
            ("HOME", runner.to_str().unwrap()),
        ];
        all.extend_from_slice(env);
        bash(&step(&sign, name).script(), dir, &all, &[])
    }

    /// The outputs a step wrote to `$GITHUB_OUTPUT`.
    fn output(file: &Path, name: &str) -> String {
        fs::read_to_string(file)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")).map(str::to_string))
            .unwrap_or_else(|| panic!("no output {name}"))
    }

    #[test]
    fn sign_the_sums_under_the_releases_name_and_refuse_a_signature_by_a_key_not_pinned() {
        let tauri = repo().join("app/node_modules/.bin/tauri");
        if !tauri.exists() {
            eprintln!("skipped: no app/node_modules (npm ci --prefix app)");
            return;
        }
        // A checkout of the version, the signer, and the installers with their check.
        let workspace = TempDir::new("workspace");
        fs::write(
            workspace.path().join("package.json"),
            format!("{{\"version\":\"{RELEASE}\"}}"),
        )
        .unwrap();
        fs::create_dir_all(workspace.path().join("app/node_modules/.bin")).unwrap();
        symlink(&tauri, workspace.path().join("app/node_modules/.bin/tauri")).unwrap();
        fs::create_dir_all(workspace.path().join("app/src-tauri")).unwrap();
        symlink(
            repo().join("app/src-tauri/tauri.conf.json"),
            workspace.path().join("app/src-tauri/tauri.conf.json"),
        )
        .unwrap();
        symlink(repo().join("install"), workspace.path().join("install")).unwrap();
        let runner = TempDir::new("runner");
        fs::create_dir(runner.path().join("sums-tool")).unwrap();
        let tool = runner.path().join("sums-tool/cf-publish");
        fs::copy(env!("CARGO_BIN_EXE_cf-publish"), &tool).unwrap();
        let built = sha256_hex(&fs::read(&tool).unwrap());

        // The publisher is run only as the binary the publisher job built.
        let ran = run(
            HASH,
            runner.path(),
            workspace.path(),
            runner.path(),
            &[("BUILT_SHA256", &"0".repeat(64))],
        );
        assert_eq!(
            ran.status,
            1,
            "a publisher of another hash is run: {}",
            ran.said()
        );
        assert!(
            ran.stderr.contains("the publisher downloaded hashes to"),
            "{}",
            ran.said()
        );
        let ran = run(
            HASH,
            runner.path(),
            workspace.path(),
            runner.path(),
            &[("BUILT_SHA256", "")],
        );
        assert_eq!(
            ran.status,
            1,
            "a publisher with no hash said is run: {}",
            ran.said()
        );
        let ran = run(
            HASH,
            runner.path(),
            workspace.path(),
            runner.path(),
            &[("BUILT_SHA256", &built)],
        );
        assert_eq!(ran.status, 0, "{}", ran.said());
        let files = built_files("https://example.invalid", RELEASE)
            .without("SHA256SUMS")
            .without("SHA256SUMS.sig");
        let dist = folder_of(&files);
        let outputs = runner.path().join("outputs");
        fs::write(&outputs, "").unwrap();
        let out = outputs.to_str().unwrap();

        let ran = run(
            SUMS,
            dist.path(),
            workspace.path(),
            runner.path(),
            &[("GITHUB_OUTPUT", out)],
        );
        assert_eq!(ran.status, 0, "{}", ran.said());
        let signed = format!("ConsensFlow_{RELEASE}_SHA256SUMS");
        assert_eq!(output(&outputs, "version"), RELEASE);
        assert_eq!(output(&outputs, "signed"), signed);
        assert_eq!(
            fs::read_to_string(dist.path().join("SHA256SUMS")).unwrap(),
            published_assets(&files).text("SHA256SUMS")
        );

        // A key made for the test, as Tauri's signer makes one.
        let keys = TempDir::new("keys");
        let made = bash(
            "\"$TAURI\" signer generate --ci -p '' -w \"$KEYS/test.key\" >/dev/null",
            keys.path(),
            &[
                ("TAURI", tauri.to_str().unwrap()),
                ("KEYS", keys.path().to_str().unwrap()),
                ("HOME", runner.path().to_str().unwrap()),
            ],
            &[],
        );
        assert_eq!(made.status, 0, "{}", made.said());
        let key = fs::read_to_string(keys.path().join("test.key")).unwrap();
        let ran = run(
            SIGN,
            dist.path(),
            workspace.path(),
            runner.path(),
            &[
                ("TAURI_SIGNING_PRIVATE_KEY", key.trim()),
                ("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", ""),
                ("SIGNED", &signed),
            ],
        );
        assert_eq!(ran.status, 0, "{}", ran.said());
        let decoded = bash(
            "base64 -d < \"$SIG\"",
            dist.path(),
            &[(
                "SIG",
                dist.path().join(format!("{signed}.sig")).to_str().unwrap(),
            )],
            &[],
        );
        let comment = decoded
            .stdout
            .lines()
            .nth(2)
            .unwrap_or_default()
            .to_string();
        assert!(
            comment.starts_with("trusted comment: timestamp:")
                && comment.ends_with(&format!("\tfile:{signed}")),
            "the signature names the release: {comment}"
        );

        let ran = run(
            SIGNATURE,
            dist.path(),
            workspace.path(),
            runner.path(),
            &[("VERSION", RELEASE), ("SIGNED", &signed)],
        );
        assert_eq!(ran.status, 1, "{}", ran.said());
        assert!(
            ran.stderr.contains(
                "check-signature: as Node checks it: the signature was made by another key"
            ),
            "{}",
            ran.said()
        );
    }
}
