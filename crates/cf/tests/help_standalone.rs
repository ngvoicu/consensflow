//! The standalone verbs of `cf` (`setup`, `doctor`, `ui`)
//! answer `--help` and `-h` with their usage, exit code 0, and read and write
//! nothing of the home: Node refused the word as an option no verb has, and
//! `doctor` ran on whatever followed it. None of the verbs takes text of its
//! own, so the word asks for help wherever it stands ahead of a `--`; after one
//! it is a name, as it was. The board's commands are `help.rs`'s.

// The tests' own folders and helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use std::fs;

use common::cf;

/// A home with a settings file that cannot be read: a verb that read it would say so.
fn home_with_a_broken_roster() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("a home");
    fs::write(home.path().join("agents.json"), "{ not json").expect("a settings file");
    home
}

/// The usage lines each standalone verb answers with.
const STANDALONE: [(&[&str], &str); 2] = [
    (
        &["setup"],
        "  setup                                     Prepare private launcher and integrations\n",
    ),
    (
        &["doctor"],
        "  doctor                                    Inspect runtime, harnesses and bundled roles\n",
    ),
];

#[test]
fn every_standalone_verb_answers_help_with_its_usage_and_reads_and_writes_nothing() {
    for (path, usage) in STANDALONE {
        for word in ["--help", "-h"] {
            let home = home_with_a_broken_roster();
            let mut args = path.to_vec();
            args.push(word);
            let ran = cf(
                &args,
                &[("CONSENSFLOW_HOME", home.path().to_str().expect("a path"))],
                "",
            );
            assert_eq!(
                (
                    ran.status.code(),
                    String::from_utf8_lossy(&ran.stdout).as_ref(),
                    String::from_utf8_lossy(&ran.stderr).as_ref()
                ),
                (Some(0), usage, ""),
                "{args:?}"
            );
            let left: Vec<_> = fs::read_dir(home.path())
                .expect("the home")
                .map(|entry| entry.expect("an entry").file_name())
                .collect();
            assert_eq!(left, ["agents.json"], "{args:?}: the home was touched");
        }
    }
}

#[test]
fn a_verb_asked_for_help_beside_its_options_or_a_name_answers_it_too() {
    for args in [&["setup", "extra", "-h"][..], &["doctor", "--json", "--help"]] {
        let home = home_with_a_broken_roster();
        let ran = cf(
            args,
            &[("CONSENSFLOW_HOME", home.path().to_str().expect("a path"))],
            "",
        );
        assert_eq!(ran.status.code(), Some(0), "{args:?}");
        assert!(ran.stderr.is_empty(), "{args:?}");
        assert!(!ran.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn the_whole_usage_names_no_verb_of_saved_agents_or_a_catalog() {
    let home = home_with_a_broken_roster();
    let at = [("CONSENSFLOW_HOME", home.path().to_str().expect("a path"))];
    let everything = String::from_utf8_lossy(&cf(&["-h"], &at, "").stdout).into_owned();
    assert_eq!(
        everything,
        String::from_utf8_lossy(&cf(&["--help"], &at, "").stdout)
    );
    assert!(everything.starts_with("consensflow "));
    assert!(!everything.contains("  agent "), "{everything}");
    assert!(!everything.contains("  catalog "), "{everything}");
}

#[test]
fn ui_answers_help_with_its_usage_and_starts_nothing() {
    let home = tempfile::tempdir().expect("a home");
    let at = [("CONSENSFLOW_HOME", home.path().to_str().expect("a path"))];
    for word in ["--help", "-h"] {
        let ran = cf(&["ui", word], &at, "");
        assert_eq!(ran.status.code(), Some(0), "{word}");
        assert_eq!(
            String::from_utf8_lossy(&ran.stdout),
            "  ui [--json] [--no-open]                     Run the app's daemon; open the harnesses screen\n",
            "{word}"
        );
        assert!(ran.stderr.is_empty(), "{word}");
    }
    assert_eq!(
        fs::read_dir(home.path()).expect("the home").count(),
        0,
        "no daemon, no log, no lock"
    );
}
