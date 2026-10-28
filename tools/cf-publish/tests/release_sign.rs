//! The sign job of the release workflow (.github/workflows/release.yml): the
//! SHA256SUMS the one-line installers check a release by, signed with the
//! update key in one step of a job that can write nothing, checked as the
//! installers check it, and published by the publish job, which waits for it.
//! Its text, and its steps run as written with a key made for the test: the
//! signature it makes names the release, and the check after it refuses a
//! signature by any key but the one the installers pin.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_publish::testing::workflow::release::{
    ARTIFACTS, HASH, PUBLISH, PUBLISH_ARTIFACTS, SIGN, SIGNATURE, SIGNED, SIGNED_FILES, SUMS,
};
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
            publish.contains("          name: ConsensFlow-signature\n"),
            "it downloads the signature by name"
        );
    }
}

mod what_it_signs_is_what_was_built {
    use super::*;

    /// The one command every job lists its files with, by the line that runs it.
    fn listings(text: &str) -> Vec<String> {
        text.lines()
            .filter(|line| line.contains("listing=$("))
            .map(|line| line.trim().to_string())
            .collect()
    }

    #[test]
    fn holds_the_downloads_to_the_build_jobs_outputs_before_the_sums_and_hands_on_only_on_a_tag() {
        let text = read("release.yml");
        let sign = job(&text, "sign");
        let at = |name: &str| {
            sign.find(&format!("- name: {name}"))
                .unwrap_or_else(|| panic!("no step {name}"))
        };
        assert!(
            at(ARTIFACTS) < at(SUMS)
                && at(SUMS) < at(SIGN)
                && at(SIGN) < at(SIGNATURE)
                && at(SIGNATURE) < at(SIGNED),
            "the downloads checked, the sums written, signed, checked, then hashed"
        );
        assert!(
            sign.find("name: ConsensFlow-mac\n").unwrap() < at(ARTIFACTS)
                && sign.find("name: ConsensFlow-windows\n").unwrap() < at(ARTIFACTS),
            "each job's files downloaded apart, before they are checked"
        );
        assert!(
            !sign.contains("pattern: ConsensFlow-*"),
            "no merged download that is not checked"
        );
        let env = step(&sign, ARTIFACTS).env();
        for (name, value) in [
            ("MAC_LISTING", "${{ needs.mac.outputs.listing }}"),
            ("WINDOWS_LISTING", "${{ needs.windows.outputs.listing }}"),
        ] {
            assert!(
                env.contains(&(name.to_string(), value.to_string())),
                "{env:?}"
            );
        }
        assert!(sign.contains("      sums: ${{ steps.signed.outputs.sums }}\n"));
        assert!(sign.contains("      signature: ${{ steps.signed.outputs.signature }}\n"));
        let upload = sign
            .split("- if: ")
            .nth(1)
            .expect("the upload of the signature has a condition");
        assert!(
            upload
                .starts_with("github.event_name == 'push'\n        uses: actions/upload-artifact@")
                && upload.contains("name: ConsensFlow-signature"),
            "only a tag's run uploads the signature: {upload}"
        );
        assert!(
            text.contains("    outputs:\n      listing: ${{ steps.listing.outputs.listing }}\n")
        );
        let windows = read("windows-build.yml");
        assert!(windows.contains("        value: ${{ jobs.windows.outputs.listing }}\n"));
        assert!(windows.contains("      listing: ${{ steps.listing.outputs.listing }}\n"));
        // One command lists the files in each of the three jobs.
        let all: Vec<String> = [
            listings(&job(&text, "mac")),
            listings(&sign),
            listings(&windows),
        ]
        .concat();
        assert_eq!(all.len(), 3, "{all:?}");
        assert!(all.iter().all(|line| *line == all[0]), "{all:?}");
    }

    #[test]
    fn has_the_publish_job_hold_every_built_file_to_its_jobs_listing_before_it_publishes() {
        let text = read("release.yml");
        let publish = job(&text, "publish");
        let at = |name: &str| {
            publish
                .find(&format!("- name: {name}"))
                .unwrap_or_else(|| panic!("no step {name}"))
        };
        assert!(
            at(PUBLISH_ARTIFACTS) < at(SIGNED_FILES) && at(SIGNED_FILES) < at(PUBLISH),
            "the files held to their listings before anything is published"
        );
        assert!(
            publish.find("name: ConsensFlow-mac\n").unwrap() < at(PUBLISH_ARTIFACTS)
                && publish.find("name: ConsensFlow-windows\n").unwrap() < at(PUBLISH_ARTIFACTS)
        );
        let env = step(&publish, PUBLISH_ARTIFACTS).env();
        for (name, value) in [
            ("MAC_LISTING", "${{ needs.mac.outputs.listing }}"),
            ("WINDOWS_LISTING", "${{ needs.windows.outputs.listing }}"),
        ] {
            assert!(
                env.contains(&(name.to_string(), value.to_string())),
                "{env:?}"
            );
        }
        // The same command as the jobs that listed the files.
        let listing = |text: &str| {
            text.lines()
                .find(|line| line.contains("listing=$("))
                .map(|line| line.trim().to_string())
                .unwrap()
        };
        assert_eq!(listing(&publish), listing(&job(&text, "sign")));
        // Every upload a listing holds carries the hidden files the listing counts.
        let windows = read("windows-build.yml");
        for (body, artifact) in [
            (job(&text, "mac"), "ConsensFlow-mac"),
            (job(&windows, "windows"), "ConsensFlow-windows"),
            (job(&text, "sign"), "ConsensFlow-signature"),
        ] {
            let upload = body
                .split("uses: actions/upload-artifact@")
                .find(|part| part.contains(&format!("name: {artifact}\n")))
                .unwrap_or_else(|| panic!("no upload of {artifact}"));
            let upload: String = upload.lines().take(8).collect::<Vec<_>>().join("\n");
            assert!(
                upload.contains("include-hidden-files: true"),
                "{artifact}: {upload}"
            );
        }
    }

    #[test]
    fn has_the_publish_job_hold_the_sums_and_the_signature_to_the_sign_jobs_outputs_before_it_publishes(
    ) {
        let publish = job(&read("release.yml"), "publish");
        let at = |name: &str| {
            publish
                .find(&format!("- name: {name}"))
                .unwrap_or_else(|| panic!("no step {name}"))
        };
        assert!(at(HASH) < at(SIGNED_FILES) && at(SIGNED_FILES) < at(PUBLISH));
        let env = step(&publish, SIGNED_FILES).env();
        for (name, value) in [
            ("SUMS_SHA256", "${{ needs.sign.outputs.sums }}"),
            ("SIGNATURE_SHA256", "${{ needs.sign.outputs.signature }}"),
        ] {
            assert!(
                env.contains(&(name.to_string(), value.to_string())),
                "{env:?}"
            );
        }
        assert!(
            !env.iter().any(|(name, _)| name == "GH_TOKEN"),
            "the token is not this step's"
        );
    }
}

mod the_installers_workflow {
    use super::*;

    #[test]
    fn installs_a_real_release_only_after_a_tags_release_run_that_succeeded() {
        let real = job(&read("installers.yml"), "real-release");
        let condition = real
            .lines()
            .find_map(|line| line.strip_prefix("    if: "))
            .expect("the real install has a condition");
        assert_eq!(
            condition,
            "github.event_name != 'workflow_run' || (github.event.workflow_run.conclusion == 'success' && github.event.workflow_run.event == 'push')"
        );
    }
}

#[cfg(unix)]
mod its_steps_as_written {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    use cf_publish::digest::sha256_hex;
    use cf_publish::testing::workflow::release::{MAC_LISTING, PUBLISH_ARTIFACTS, WINDOWS_LISTING};
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

    /// A step of `workflow`'s job `name`, run as written in `dir`.
    fn run_in(
        workflow: &str,
        name: &str,
        step_name: &str,
        dir: &Path,
        env: &[(&str, &str)],
    ) -> Ran {
        bash(
            &step(&job(&read(workflow), name), step_name).script(),
            dir,
            env,
            &[],
        )
    }

    #[test]
    fn the_files_signed_are_the_ones_the_jobs_listed_and_publish_takes_only_what_was_signed() {
        // The Mac job's files, and the Windows build's, where each job leaves them.
        let mac = TempDir::new("mac");
        fs::create_dir(mac.path().join("release")).unwrap();
        for (name, data) in [
            (format!("ConsensFlow_{RELEASE}_aarch64.dmg"), "dmg"),
            (
                format!("ConsensFlow_{RELEASE}_aarch64.app.tar.gz"),
                "archive",
            ),
            ("latest.json".to_string(), "{}"),
            ("notes.txt".to_string(), "the notes"),
        ] {
            fs::write(mac.path().join("release").join(name), data).unwrap();
        }
        let mac_out = mac.path().join("outputs");
        fs::write(&mac_out, "").unwrap();
        let ran = run_in(
            "release.yml",
            "mac",
            MAC_LISTING,
            mac.path(),
            &[
                ("RUNNER_TEMP", mac.path().to_str().unwrap()),
                ("GITHUB_OUTPUT", mac_out.to_str().unwrap()),
            ],
        );
        assert_eq!(ran.status, 0, "{}", ran.said());
        let mac_listing = output(&mac_out, "listing");

        let windows = TempDir::new("windows");
        let bundle = windows.path().join("app/src-tauri/target/release/bundle");
        for (folder, name) in [
            ("nsis", format!("ConsensFlow_{RELEASE}_x64-setup.exe")),
            (
                "portable",
                format!("ConsensFlow_{RELEASE}_x64-portable.exe"),
            ),
        ] {
            fs::create_dir_all(bundle.join(folder)).unwrap();
            fs::write(bundle.join(folder).join(name), folder).unwrap();
        }
        let windows_out = windows.path().join("outputs");
        fs::write(&windows_out, "").unwrap();
        let ran = run_in(
            "windows-build.yml",
            "windows",
            WINDOWS_LISTING,
            windows.path(),
            &[
                ("RUNNER_TEMP", windows.path().to_str().unwrap()),
                ("GITHUB_OUTPUT", windows_out.to_str().unwrap()),
            ],
        );
        assert_eq!(ran.status, 0, "{}", ran.said());
        let windows_listing = output(&windows_out, "listing");
        assert_eq!(mac_listing.len(), 64);
        assert_eq!(windows_listing.len(), 64);

        // What each job uploaded, as a job of the run downloads it, with `replace` replaced.
        let lay_out = |replace: Option<&str>| {
            let runner = TempDir::new("sign");
            let built = runner.path().join("built");
            fs::create_dir_all(built.join("mac")).unwrap();
            for entry in fs::read_dir(mac.path().join("release")).unwrap() {
                let entry = entry.unwrap();
                fs::copy(entry.path(), built.join("mac").join(entry.file_name())).unwrap();
            }
            for folder in ["nsis", "portable"] {
                fs::create_dir_all(built.join("windows").join(folder)).unwrap();
                for entry in
                    fs::read_dir(windows.path().join("windows-artifact").join(folder)).unwrap()
                {
                    let entry = entry.unwrap();
                    fs::copy(
                        entry.path(),
                        built.join("windows").join(folder).join(entry.file_name()),
                    )
                    .unwrap();
                }
            }
            if let Some(path) = replace {
                fs::write(built.join(path), "replaced by another job of the run").unwrap();
            }
            runner
        };
        // The sign job, given what each job uploaded: as built, then with a file replaced.
        let sign_job = |replace: Option<&str>| {
            let runner = lay_out(replace);
            let ran = run(
                ARTIFACTS,
                runner.path(),
                runner.path(),
                runner.path(),
                &[
                    ("MAC_LISTING", &mac_listing),
                    ("WINDOWS_LISTING", &windows_listing),
                ],
            );
            (ran, runner)
        };
        let (ran, runner) = sign_job(None);
        assert_eq!(ran.status, 0, "{}", ran.said());
        for path in [
            format!("ConsensFlow_{RELEASE}_aarch64.dmg"),
            format!("nsis/ConsensFlow_{RELEASE}_x64-setup.exe"),
            format!("portable/ConsensFlow_{RELEASE}_x64-portable.exe"),
        ] {
            assert!(
                runner.path().join("dist").join(&path).is_file(),
                "{path} is in the folder signed"
            );
        }
        for (replaced, job_name) in [
            (
                format!("windows/nsis/ConsensFlow_{RELEASE}_x64-setup.exe"),
                "windows",
            ),
            (format!("mac/ConsensFlow_{RELEASE}_aarch64.dmg"), "mac"),
            ("mac/another-file".to_string(), "mac"),
        ] {
            let (ran, runner) = sign_job(Some(&replaced));
            assert_eq!(ran.status, 1, "{replaced}: {}", ran.said());
            assert!(
                ran.stderr
                    .contains(&format!("the {job_name} job's files downloaded hash to")),
                "{replaced}: {}",
                ran.said()
            );
            assert!(
                !runner.path().join("dist").exists(),
                "nothing is laid out to be signed"
            );
        }

        // The publish job's check of the same downloads, the notes among them, before it publishes:
        // the folder published from holds the signature already.
        let publish_job = |replace: Option<&str>| {
            let runner = lay_out(replace);
            fs::create_dir(runner.path().join("dist")).unwrap();
            fs::write(runner.path().join("dist/SHA256SUMS"), "the sums\n").unwrap();
            let ran = run_in(
                "release.yml",
                "publish",
                PUBLISH_ARTIFACTS,
                runner.path(),
                &[
                    ("RUNNER_TEMP", runner.path().to_str().unwrap()),
                    ("MAC_LISTING", &mac_listing),
                    ("WINDOWS_LISTING", &windows_listing),
                ],
            );
            (ran, runner)
        };
        let (ran, runner) = publish_job(None);
        assert_eq!(ran.status, 0, "{}", ran.said());
        assert!(runner.path().join("dist/notes.txt").is_file());
        assert!(runner.path().join("dist/SHA256SUMS").is_file());
        for (replaced, job_name) in [
            ("mac/notes.txt".to_string(), "mac"),
            (
                format!("windows/portable/ConsensFlow_{RELEASE}_x64-portable.exe"),
                "windows",
            ),
        ] {
            let (ran, runner) = publish_job(Some(&replaced));
            assert_eq!(ran.status, 1, "{replaced}: {}", ran.said());
            assert!(
                ran.stderr
                    .contains(&format!("the {job_name} job's files downloaded hash to")),
                "{replaced}: {}",
                ran.said()
            );
            assert!(
                !runner.path().join("dist/notes.txt").exists(),
                "nothing laid out to publish"
            );
        }

        // What was signed, as its outputs say it, and the publish job's check of its download.
        let signature = runner.path().join("signature");
        fs::create_dir(&signature).unwrap();
        fs::write(signature.join("SHA256SUMS"), "the sums\n").unwrap();
        fs::write(signature.join("SHA256SUMS.sig"), "the signature").unwrap();
        let signed_out = runner.path().join("signed-outputs");
        fs::write(&signed_out, "").unwrap();
        let ran = run(
            SIGNED,
            &signature,
            runner.path(),
            runner.path(),
            &[("GITHUB_OUTPUT", signed_out.to_str().unwrap())],
        );
        assert_eq!(ran.status, 0, "{}", ran.said());
        let (sums, sig) = (
            output(&signed_out, "sums"),
            output(&signed_out, "signature"),
        );
        assert_eq!(sums, sha256_hex(b"the sums\n"));
        let publish = |dir: &Path| {
            run_in(
                "release.yml",
                "publish",
                SIGNED_FILES,
                dir,
                &[("SUMS_SHA256", &sums), ("SIGNATURE_SHA256", &sig)],
            )
        };
        let ran = publish(&signature);
        assert_eq!(ran.status, 0, "{}", ran.said());
        for name in ["SHA256SUMS", "SHA256SUMS.sig"] {
            let dist = TempDir::new("dist");
            for file in ["SHA256SUMS", "SHA256SUMS.sig"] {
                fs::copy(signature.join(file), dist.path().join(file)).unwrap();
            }
            fs::write(dist.path().join(name), "replaced").unwrap();
            let ran = publish(dist.path());
            assert_eq!(ran.status, 1, "{name}: {}", ran.said());
            assert!(
                ran.stderr.contains(&format!("{name} downloaded hashes to")),
                "{}",
                ran.said()
            );
        }
    }

    #[test]
    fn sign_the_sums_under_the_releases_name_and_refuse_a_signature_by_a_key_not_pinned() {
        let tauri = repo().join("app/node_modules/.bin/tauri");
        assert!(
            tauri.exists(),
            "Tauri's signer is not in app/node_modules: run npm ci --prefix app"
        );
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
