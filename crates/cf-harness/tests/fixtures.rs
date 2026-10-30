//! The Claude transcripts the tests of this crate read (`tests/fixtures/claude`)
//! are of live runs, made fixtures by `cargo xtask scrub-transcript`, and the Muse
//! session logs (`tests/fixtures/muse`) are of runs on Muse's fake model, made
//! fixtures by `cargo xtask scrub-muse-session`: no folder of a user or of the
//! machine's temporary folder, no address and no id of an account may be left in
//! one, whatever else a person adds to a folder by hand. How the tools scrub is held
//! by their own tests (`tools/xtask/src/scrub_transcript`,
//! `tools/xtask/src/scrub_muse_session`); this holds what they were made for.

use std::fs;
use std::path::PathBuf;

use regex::Regex;
use serde_json::Value;

/// What a transcript of a live run has in it that is personal, each with a sample:
/// a folder of a user's, on a Mac and on Linux, a Mac's temporary folder (a user's
/// own, with `/private` before it or not), Muse's runtime socket (its folder names the
/// user's uid, its file a profile's id), an address, and the ids of an account's
/// bridge sessions. (A word is what JavaScript's `\w` made it: ASCII.)
const PERSONAL: [(&str, &str); 7] = [
    (r"/Users/", "/Users/jane/Projects/app"),
    (r"/home/", "/home/jane/app"),
    (
        r"/var/folders/",
        "/private/var/folders/5f/cy8ywl/T/muse-rec-1/work",
    ),
    (
        r"/tmp/tbh-",
        "/private/tmp/tbh-501-rt/muse/ms-2ee2d561e12e.sock",
    ),
    (
        r"[A-Za-z0-9_.+-]+@[A-Za-z0-9_-]+\.[A-Za-z0-9_]+",
        "jane@example.com",
    ),
    (r"cse_0[A-Za-z0-9_]{6,}", "cse_01Abcdefg"),
    (r"session_0[A-Za-z0-9_]{6,}", "session_01Abcdefg"),
];

fn folder(harness: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(harness)
}

#[test]
fn the_claude_fixtures_hold_no_folder_no_address_and_no_id_of_an_account() {
    let folder = folder("claude");
    let mut names: Vec<String> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".jsonl"))
        .collect();
    names.sort();
    assert!(names.len() >= 2, "the fixtures are there: {names:?}");
    for name in names {
        let text = fs::read_to_string(folder.join(&name)).unwrap();
        for (pattern, _) in PERSONAL {
            let found = Regex::new(pattern).unwrap();
            assert!(!found.is_match(&text), "{name} matches {pattern}");
        }
        for line in text.lines().filter(|line| !line.is_empty()) {
            let record: Value = serde_json::from_str(line).unwrap();
            if let Some(session) = record.get("sessionId") {
                assert_eq!(session, "$SESSION", "{name}");
            }
        }
    }
}

#[test]
fn the_muse_fixtures_hold_no_folder_no_address_and_no_id_of_an_account() {
    let folder = folder("muse");
    // The echo recordings, and the live ones beside them (`live/`).
    let mut names: Vec<String> = ["", "live"]
        .into_iter()
        .flat_map(|sub| {
            fs::read_dir(folder.join(sub)).unwrap().map(move |entry| {
                let name = entry.unwrap().file_name().to_string_lossy().into_owned();
                if sub.is_empty() {
                    name
                } else {
                    format!("{sub}/{name}")
                }
            })
        })
        .filter(|name| {
            name.ends_with(".jsonl") || name.ends_with(".json") || name.ends_with(".sql")
        })
        .collect();
    names.sort();
    assert!(names.len() >= 11, "the fixtures are there: {names:?}");
    assert!(
        names.iter().any(|name| name.starts_with("live/")),
        "{names:?}"
    );
    for name in names {
        let text = fs::read_to_string(folder.join(&name)).unwrap();
        for (pattern, _) in PERSONAL {
            let found = Regex::new(pattern).unwrap();
            assert!(!found.is_match(&text), "{name} matches {pattern}");
        }
    }
}

#[test]
fn what_the_check_looks_for_is_what_a_transcript_that_was_not_scrubbed_has() {
    // A check is none if what it looks for is found nowhere.
    for (pattern, sample) in PERSONAL {
        assert!(
            Regex::new(pattern).unwrap().is_match(sample),
            "{pattern} does not find {sample}"
        );
    }
}
