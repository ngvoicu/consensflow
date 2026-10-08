//! The standalone verbs of `cf` (`setup`, `catalog`, `agent`, `doctor`, `ui`)
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

/// A home with an agents file that cannot be read: a verb that read it would say so.
fn home_with_a_broken_roster() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("a home");
    fs::write(home.path().join("agents.json"), "{ not json").expect("a roster");
    home
}

/// The usage lines each standalone verb answers with.
const STANDALONE: [(&[&str], &str); 7] = [
    (
        &["setup"],
        "  setup                                     Prepare private launcher and integrations\n",
    ),
    (
        &["catalog"],
        "  catalog [--harness <h>] [--json]            List available agent presets\n",
    ),
    (
        &["agent", "add"],
        concat!(
            "  agent add <name>                           Add an agent of your own\n",
            "    [--harness <h>] [--model <m>] [--effort <e>] [--description <d>]\n",
            "    [--designer]                            An image agent (Codex only): an image designer\n",
        ),
    ),
    (&["agent", "list"], "  agent list [--json]\n"),
    (
        &["agent", "edit"],
        concat!(
            "  agent edit <name> [--model <m>] [--effort <e>] [--description <d>]\n",
            "    [--work-tier critical|complex|standard|light|auto]\n",
        ),
    ),
    (&["agent", "remove"], "  agent remove <name>\n"),
    (
        &["doctor"],
        "  doctor                                    Inspect runtime, roster and bundled roles\n",
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
    for args in [
        &["catalog", "--harness", "pi", "--help"][..],
        &["catalog", "--json", "-h"],
        &["agent", "add", "mine", "--model", "x", "-h"],
        &["agent", "edit", "mine", "--help"],
        &["agent", "list", "--json", "--help"],
        &["setup", "extra", "-h"],
    ] {
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
fn agent_alone_answers_with_all_its_actions_and_the_whole_usage_with_all_there_is() {
    let home = home_with_a_broken_roster();
    let at = [("CONSENSFLOW_HOME", home.path().to_str().expect("a path"))];
    let agent = cf(&["agent", "--help"], &at, "");
    let expected: String = STANDALONE
        .iter()
        .filter(|(path, _)| path[0] == "agent")
        .map(|(_, usage)| *usage)
        .collect();
    assert_eq!(String::from_utf8_lossy(&agent.stdout), expected);
    let everything = String::from_utf8_lossy(&cf(&["-h"], &at, "").stdout).into_owned();
    assert_eq!(
        everything,
        String::from_utf8_lossy(&cf(&["--help"], &at, "").stdout)
    );
    assert!(everything.starts_with("consensflow "));
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
            "  ui [--json] [--no-open]                     Run the app's daemon; open the agents screens\n",
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

#[test]
fn a_name_after_a_double_dash_is_a_name_and_not_a_request_for_help() {
    // `agent add -- --help` names an agent `--help`: the roster says what it
    // makes of the name, not the usage.
    let home = tempfile::tempdir().expect("a home");
    let at = [("CONSENSFLOW_HOME", home.path().to_str().expect("a path"))];
    let ran = cf(&["agent", "add", "--", "--help"], &at, "");
    assert_eq!(ran.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&ran.stderr).starts_with("cf: "),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(ran.stdout.is_empty());
}
