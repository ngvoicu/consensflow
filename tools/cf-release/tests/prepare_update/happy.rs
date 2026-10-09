//! What the command writes for a release that is right: the entry, byte for
//! byte what the script it replaced wrote, and nothing at all for one that is not.

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use cf_base::env::Env;
use cf_release::process;

use super::fixture::{words, Item, Release, Spec, DATE, STABLE, VERSION};

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("prepare-update")
        .join(name)
}

#[test]
fn builds_correct_deterministic_tauri_metadata_on_the_happy_path() {
    let release = Release::new();
    release.run(&[]).finished();
    let entry = release.entry();
    assert_eq!(entry["version"], VERSION);
    assert_eq!(entry["notes"], "Alpha 99 fixes delivery races.");
    assert_eq!(entry["pub_date"], DATE);
    assert_eq!(
        entry["platforms"]["darwin-aarch64"]["url"],
        format!(
            "https://github.com/ngvoicu/consensflow/releases/download/v{VERSION}/{}",
            release.asset()
        )
    );
    assert_eq!(
        entry["platforms"]["darwin-aarch64"]["signature"],
        fs::read_to_string(&release.signature).unwrap().trim()
    );
    assert!(entry.get("consensflow").is_none());
    assert_eq!(entry.as_object().unwrap().len(), 4);

    // Run again, it writes the same bytes.
    let first = fs::read(&release.output).unwrap();
    let again = release.dir.path().join("latest2.json");
    release
        .run(&[("output", again.to_str().unwrap())])
        .finished();
    assert_eq!(fs::read(&again).unwrap(), first);
    assert!(first.ends_with(b"}\n"));
    assert_eq!(first.iter().filter(|byte| **byte == b'\n').count(), 1);
}

#[test]
fn writes_what_the_script_it_replaced_wrote_for_the_same_files() {
    // app/scripts/prepare-update.mjs, run on a release of 3.0.0-alpha.99 with these
    // notes, this signature (the signer's) and this date: its latest.json is the
    // fixture. The notes hold what JSON has to escape and what JavaScript's trim
    // treats as its own, and the date is not in UTC.
    let release = Release::new();
    let renamed = release
        .dir
        .path()
        .join("ConsensFlow_3.0.0-alpha.99_aarch64.app.tar.gz");
    fs::rename(&release.archive, &renamed).unwrap();
    let path = |path: PathBuf| path.display().to_string();
    release
        .run(&[
            ("archive", &path(renamed)),
            ("notes", &path(golden("notes.txt"))),
            ("signature", &path(golden("signature.sig"))),
            ("date", "2026-10-09T06:30:15.123+02:00"),
        ])
        .finished();
    assert_eq!(
        fs::read_to_string(&release.output).unwrap(),
        fs::read_to_string(golden("latest.json")).unwrap()
    );
}

#[test]
fn alpha_channel_accepts_a_stable_graduation_version() {
    let release = Release::of(&Spec {
        version: Some(STABLE),
        ..Spec::default()
    });
    release.run(&[]).finished();
    assert_eq!(release.entry()["version"], STABLE);
}

#[test]
fn stable_channel_accepts_stable_and_rejects_prereleases() {
    let stable = Release::of(&Spec {
        version: Some(STABLE),
        ..Spec::default()
    });
    stable.run(&[("channel", "stable")]).finished();
    assert_eq!(stable.entry()["version"], STABLE);

    let prerelease = Release::new();
    prerelease
        .run(&[("channel", "stable")])
        .refused("stable channel requires a stable release");
}

#[test]
fn preserves_notes_urls_while_constructing_the_platform_url_itself() {
    let release = Release::new();
    fs::write(
        &release.notes,
        "See https://evil.example/notes for details.\n",
    )
    .unwrap();
    release.run(&[]).finished();
    let raw = fs::read_to_string(&release.output).unwrap();
    assert!(raw.contains("evil.example/notes"), "{raw}");
    assert!(
        raw.contains("github.com/ngvoicu/consensflow/releases/download"),
        "{raw}"
    );
    let entry = release.entry();
    assert_eq!(
        entry["notes"],
        "See https://evil.example/notes for details."
    );
    let url = entry["platforms"]["darwin-aarch64"]["url"]
        .as_str()
        .unwrap();
    assert!(!url.contains("evil.example"), "{url}");
}

#[test]
fn takes_a_bundle_that_ships_nothing_of_nodes() {
    // No package.json, cf.mjs, src or hosts beside the cf; no node in MacOS.
    let release = Release::new();
    let cli = release
        .bundle
        .join("Contents")
        .join("Resources")
        .join("cli");
    let names = |dir: &std::path::Path| -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(names(&cli), ["bin"]);
    assert_eq!(names(&cli.join("bin")), ["cf"]);
    assert!(!release
        .bundle
        .join("Contents")
        .join("MacOS")
        .join("node")
        .exists());
    release.run(&[]).finished();
}

#[test]
fn an_entry_that_was_there_is_replaced() {
    let release = Release::new();
    fs::write(
        &release.output,
        "an old entry that is longer than the new one will be, by far, and then some\n".repeat(40),
    )
    .unwrap();
    release.run(&[]).finished();
    let raw = fs::read_to_string(&release.output).unwrap();
    assert!(raw.starts_with(r#"{"version":"3.0.0-alpha.99","#), "{raw}");
    assert_eq!(raw.lines().count(), 1);
}

#[test]
fn a_release_that_is_refused_leaves_the_entry_as_it_was() {
    let release = Release::new();
    release
        .run(&[("channel", "beta")])
        .refused("channel must be alpha or stable");
    assert!(!release.output.exists());

    fs::write(&release.output, "the last entry\n").unwrap();
    release
        .run(&[("date", "next friday")])
        .refused("publication date must be RFC3339");
    assert_eq!(
        fs::read_to_string(&release.output).unwrap(),
        "the last entry\n"
    );
}

#[test]
fn an_entry_that_cannot_be_written_says_where() {
    let release = Release::new();
    let nowhere = release
        .dir
        .path()
        .join("no")
        .join("such")
        .join("latest.json");
    release
        .run(&[("output", nowhere.to_str().unwrap())])
        .refused(&format!("could not write {}: ", nowhere.display()));
}

#[test]
fn an_app_with_more_in_it_is_matched_file_for_file() {
    // Folders, files that are no text, a file longer than any one read.
    let release = Release::of(&Spec {
        extra: vec![
            Item::dir("ConsensFlow.app/Contents/Frameworks"),
            Item::file(
                "ConsensFlow.app/Contents/Frameworks/lib.dylib",
                0o755,
                &[0, 159, 146, 150],
            ),
            Item::file(
                "ConsensFlow.app/Contents/Resources/icon.icns",
                0o644,
                &vec![7; 3 << 20],
            ),
            Item::file("ConsensFlow.app/Contents/Resources/empty", 0o600, b""),
            Item::file(
                "ConsensFlow.app/Contents/Resources/Résumé ✓ (1).txt",
                0o644,
                b"text\n",
            ),
        ],
        ..Spec::default()
    });
    release.run(&[]).finished();
}

#[test]
fn the_binary_ends_with_the_status_the_workflow_reads_and_says_it_on_stderr() {
    let binary = OsStr::new(env!("CARGO_BIN_EXE_cf-release"));
    let release = Release::new();
    let ran = |changes: &[(&str, &str)]| {
        let mut flags = release.flags();
        for (name, value) in changes {
            flags.iter_mut().find(|(flag, _)| flag == name).unwrap().1 = (*value).to_string();
        }
        process::capture(binary, &words(&flags), &Env::default()).unwrap()
    };

    let done = ran(&[]);
    assert_eq!(
        (done.code, done.stdout.as_str(), done.stderr.as_str()),
        (0, "", "")
    );
    assert_eq!(release.entry()["version"], VERSION);

    let refused = ran(&[("channel", "beta")]);
    assert_eq!((refused.code, refused.stdout.as_str()), (1, ""));
    assert_eq!(
        refused.stderr,
        "cf-release prepare-update: channel must be alpha or stable\n"
    );
}
