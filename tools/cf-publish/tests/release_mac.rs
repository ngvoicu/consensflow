//! The Mac job of the release workflow (.github/workflows/release.yml), the
//! steps that ask the rule of the feeds run as written, as a hand run and as a
//! tag run: the early question to the old feeds, and the plan of the feeds from
//! the archive. The release jobs run on macOS and Linux: so do these.
#![cfg(unix)]
// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_publish::testing::workflow::release::{DOWNLOADS, NOTES, PREREQUISITES};
use cf_publish::testing::workflow::{job, read, step};

/// The release run smokes the bundle it ships, not a copy: the one place the app signed under
/// the Developer ID with its hardened runtime is run (the smoke starts it on GitHub's runner
/// alone, where no live app runs), and the bundle's seal is checked again once it ran.
#[test]
fn the_release_run_smokes_the_bundle_it_ships_and_checks_its_seal_after() {
    let workflow = read("release.yml");
    let smoke = step(&workflow, "The signed app passes the packaged smoke");
    let env = smoke.env();
    let set = |name: &str| {
        env.iter()
            .find(|(it, _)| it == name)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        set("CONSENSFLOW_SMOKE_SHIPPED").as_deref(),
        Some("\"1\""),
        "{env:?}"
    );
    assert!(
        set("CONSENSFLOW_SMOKE_APP")
            .is_some_and(|app| app.ends_with("/bundle/macos/ConsensFlow.app")),
        "{env:?}"
    );
    let sealed = step(&workflow, "Sign the update bundle").script();
    assert!(
        sealed.contains("codesign --verify --deep --strict ConsensFlow.app"),
        "{sealed}"
    );
}

mod the_mac_job_as_a_hand_run_and_as_a_tag_run {
    use std::fs;
    use std::path::Path;

    use cf_publish::manifest::Manifest;
    use cf_publish::testing::workflow::bash;
    use cf_publish::testing::worlds::feed;
    use cf_publish::testing::{
        archive_of, built_files, folder_of, latest_json, published_assets, Github, Ran, Spec,
        TempDir,
    };

    use super::*;

    /// A version that comes after the bridge the repository records, and one before it.
    const LATER: &str = "99.0.0-alpha.1";
    const BEFORE: &str = "1.0.0";

    /// A checkout of nothing but the version `package.json` says, and the built cf-publish
    /// where the Mac job's build leaves it.
    fn project(version: &str) -> TempDir {
        let root = TempDir::new("mac");
        fs::write(
            root.path().join("package.json"),
            format!("{{\"version\":\"{version}\"}}"),
        )
        .unwrap();
        let release = root
            .path()
            .join("app")
            .join("src-tauri")
            .join("target")
            .join("release");
        fs::create_dir_all(&release).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_cf-publish"), release.join("cf-publish"))
            .unwrap();
        root
    }

    fn run(script: &str, root: &Path, event: &str, github: &Github) -> Ran {
        bash(
            script,
            root,
            &[
                ("GITHUB_EVENT_NAME", event),
                ("GITHUB_REPOSITORY", github.repo()),
            ],
            &[],
        )
    }

    #[test]
    fn asks_the_old_feeds_before_the_build_a_tag_is_refused_a_hand_run_is_told_what_a_tag_would_be()
    {
        let github = Github::start();
        let bridge = Manifest::embedded().unwrap().bridge.version.to_string();
        // The bridge's tag is made and its release failed: update-alpha serves the release before it.
        feed(
            &github,
            "update-alpha",
            latest_json(github.base(), "3.0.0-alpha.1", "notes"),
        );
        let files = built_files(github.base(), &bridge);
        github.release(
            &format!("v{bridge}"),
            Spec::new().assets(published_assets(&files).iter()),
        );
        // The workflow gives the command no patience of its own: a read that does not serve the bridge is read again for a minute.
        let mac = job(&read("release.yml"), "mac");
        let script = step(&mac, PREREQUISITES)
            .script()
            .replace(DOWNLOADS, github.base());
        let quick = script.replace(
            "cf-publish feeds prerequisites",
            "cf-publish feeds prerequisites --attempts 2 --wait 1",
        );
        assert_ne!(
            quick, script,
            "the step does not run cf-publish feeds prerequisites"
        );
        let root = project(LATER);

        let tag = run(&quick, root.path(), "push", &github);
        assert_eq!(tag.status, 1, "{}", tag.said());
        assert!(
            tag.stderr
                .lines()
                .any(|line| line
                    .starts_with("feeds: update-alpha serves 3.0.0-alpha.1, not the bridge")),
            "{}",
            tag.stderr
        );

        let hand = run(&quick, root.path(), "workflow_dispatch", &github);
        assert_eq!(hand.status, 0, "{}", hand.said());
        assert!(
            hand.stderr.lines().any(|line| line
                .starts_with("feeds (a tag would be refused): update-alpha serves 3.0.0-alpha.1")),
            "{}",
            hand.stderr
        );

        // The bridge itself has nothing to ask, whatever the feeds serve.
        fs::write(
            root.path().join("package.json"),
            format!("{{\"version\":\"{bridge}\"}}"),
        )
        .unwrap();
        let itself = run(&quick, root.path(), "push", &github);
        assert_eq!(itself.status, 0, "{}", itself.said());
        assert!(
            itself.stdout.contains("it is the bridge"),
            "{}",
            itself.stdout
        );
    }

    #[test]
    fn plans_the_feeds_from_the_archive_a_tag_is_refused_for_a_release_before_the_bridge_a_hand_run_is_told(
    ) {
        let github = Github::start();
        let dir = folder_of(&cf_publish::testing::Files::default().with(
            "archive.tar.gz",
            archive_of(&["ConsensFlow.app/Contents/MacOS/app"]),
        ));
        let mac = job(&read("release.yml"), "mac");
        let notes = step(&mac, NOTES).script();
        let plan = &notes[notes
            .find("dry=()")
            .expect("the step plans after it sets dry")..];
        assert!(
            plan.contains("cf-publish feeds plan"),
            "the step plans the feeds"
        );
        let root = project(BEFORE);
        let planned = |event: &str, version: &str| {
            let script = format!(
                "set -euo pipefail\nversion={version}\nout={}\narchive=archive.tar.gz\n{plan}",
                dir.path().display()
            );
            run(&script, root.path(), event, &github)
        };

        let tag = planned("push", BEFORE);
        assert_eq!(tag.status, 1, "{}", tag.said());
        assert!(
            tag.stderr
                .lines()
                .any(|line| line.starts_with("feeds: 1.0.0 comes before the bridge")),
            "{}",
            tag.stderr
        );
        assert_eq!(tag.stdout, "");

        let hand = planned("workflow_dispatch", BEFORE);
        assert_eq!(hand.status, 0, "{}", hand.said());
        assert!(
            hand.stderr.lines().any(|line| line
                .starts_with("feeds (a tag would be refused): 1.0.0 comes before the bridge")),
            "{}",
            hand.stderr
        );

        let later = planned("push", LATER);
        assert_eq!(later.status, 0, "{}", later.said());
        assert_eq!(later.stdout, "feed-alpha\n");
    }
}

mod the_notes_a_hand_run_writes {
    //! The part of the notes step that writes `notes.txt` when the run is no
    //! tag's: the dispatch's `notes` input when one reached it, else the commits
    //! since the previous release, cut at a whole line under the feed's 64 KiB
    //! (prepare-update refuses more), the last line saying how many were left
    //! out. Trial 42197090358 failed on this: the range since the tag was
    //! 82,306 bytes of commit lines, which the fallback wrote whole.
    use std::fs;
    use std::path::Path;

    use cf_publish::testing::workflow::{bash, job, read, step};
    use cf_publish::testing::{Ran, TempDir};

    use super::*;

    /// The slice of the step's script that writes `notes.txt`: from the previous
    /// tag's look-up to the archive's copy, needing only `version`, `out` and the
    /// run's variables (`GITHUB_REF_TYPE`, `TRIAL_NOTES`).
    fn writing_notes() -> String {
        let mac = job(&read("release.yml"), "mac");
        let script = step(&mac, NOTES).script();
        let start = script
            .find("previous=")
            .expect("the step looks for the previous tag");
        let end = script
            .find("archive=")
            .expect("the step names the archive after the notes");
        script[start..end].to_string()
    }

    /// A checkout whose `git log` since v1.0.0 is over 64 KiB of commit lines:
    /// the checkout, and the lines the fallback's log lists.
    fn checkout_over_64_kib() -> (TempDir, Vec<String>) {
        let root = TempDir::new("notes");
        // 60 empty commits of a ~1.4 KiB subject each: 84 KiB of commit lines.
        let made = bash(
            r#"set -e
cd "$CHECKOUT"
git init -q -b main
git config user.name Notes && git config user.email notes@example.com
git commit -q --allow-empty -m "the first commit"
git tag v1.0.0
subject=$(printf 'x%.0s' $(seq 1 1400))
for i in $(seq 1 60); do
    git commit -q --allow-empty -m "commit $i of a long range since the tag $subject"
done"#,
            root.path(),
            &[("CHECKOUT", &root.path().to_string_lossy())],
            &[],
        );
        assert_eq!(made.status, 0, "{}", made.said());
        let listed = bash(
            "git log --no-merges --format='- %s' v1.0.0..HEAD",
            root.path(),
            &[],
            &[],
        );
        assert_eq!(listed.status, 0, "{}", listed.said());
        assert!(
            listed.stdout.len() > 64 * 1024,
            "the log is {} bytes, not over 64 KiB",
            listed.stdout.len()
        );
        (
            root,
            listed.stdout.lines().map(str::to_owned).collect(),
        )
    }

    /// The step's notes slice, run in `checkout` as a run of `ref_type` given
    /// `trial_notes` as its input: how it ended, and what `notes.txt` holds.
    fn the_notes(checkout: &Path, ref_type: &str, trial_notes: &str) -> (Ran, TempDir, String) {
        let out = TempDir::new("out");
        let script = format!(
            "set -euo pipefail\nversion=9.9.9-alpha.9\nout={}\nmkdir -p \"$out\"\n{}",
            out.path().display(),
            writing_notes()
        );
        let ran = bash(
            &script,
            checkout,
            &[
                ("GITHUB_REF_TYPE", ref_type),
                ("TRIAL_NOTES", trial_notes),
            ],
            &[],
        );
        let notes = fs::read_to_string(out.path().join("notes.txt")).unwrap_or_default();
        (ran, out, notes)
    }

    #[test]
    fn the_fallback_is_cut_under_the_feeds_limit_at_a_whole_line_saying_what_was_left_out() {
        let (checkout, listed) = checkout_over_64_kib();
        let (ran, _out, notes) = the_notes(checkout.path(), "branch", "");
        assert_eq!(ran.status, 0, "{}", ran.said());
        assert!(
            notes.len() < 64 * 1024,
            "the notes are {} bytes, not under the feed's 64 KiB",
            notes.len()
        );
        let lines: Vec<&str> = notes.lines().collect();
        assert_eq!(
            &lines[..2],
            ["ConsensFlow 9.9.9-alpha.9, the changes since 1.0.0:", ""]
        );
        // Whole lines of the log, in order, then the line that counts the rest.
        let kept = lines.len() - 3;
        let left_out = format!(
            "- {} commits left out, the log cut to keep the notes under 64 KiB",
            listed.len() - kept
        );
        assert_eq!(lines[lines.len() - 1], left_out, "{lines:?}");
        assert_eq!(
            lines[2..lines.len() - 1],
            listed[..kept],
            "the lines kept are not the log's first whole lines"
        );
    }

    #[test]
    fn a_run_given_the_dispatch_s_notes_reads_them_as_the_tag_s_run_reads_the_tag_s() {
        let (checkout, _listed) = checkout_over_64_kib();
        let given = "ConsensFlow 9.9.9-alpha.9: the title\n\n- the change the release carries\n";
        let (ran, _out, notes) = the_notes(checkout.path(), "branch", given);
        assert_eq!(ran.status, 0, "{}", ran.said());
        assert_eq!(notes, format!("{given}\n"), "the input's notes, verbatim");
    }

    #[test]
    fn a_hand_run_takes_its_notes_as_the_dispatch_s_input() {
        let workflow = read("release.yml");
        let dispatch = workflow
            .split("workflow_dispatch:")
            .nth(1)
            .expect("the workflow is run by hand");
        assert!(
            dispatch.contains("inputs:") && dispatch.contains("notes:"),
            "the hand run has no `notes` input the dispatch can carry:\n{dispatch}"
        );
    }
}
