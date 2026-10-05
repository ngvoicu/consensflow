//! The switch's own cases. That each harness's reader reads its record as
//! Node's did is the goldens' player's (`tests/records`), which looks
//! through [`open`] and [`answers`].

use std::fs;
use std::path::Path;

use jiff::tz::TimeZoneDatabase;
use serde_json::{json, Value};

use super::*;

fn said(reading: &Reading) -> String {
    match reading {
        Reading::Unknown(reason) => reason.clone(),
        Reading::Known(record) => format!("known: {} items", record.items.len()),
    }
}

/// An environment whose home is `home`.
fn at_home(home: &Path) -> Env {
    Env::from_vars([("HOME", home.to_str().unwrap())])
}

#[test]
fn a_conversation_with_no_session_reads_as_missing_whatever_its_harness() {
    let home = tempfile::tempdir().unwrap();
    let env = at_home(home.path());
    let mut cache = Cache::new(open(TimeZone::UTC), IDLE_MS, 0);
    for harness in Harness::ALL {
        let Err(reading) = reader(harness, "", &env, &TimeZone::UTC) else {
            panic!("{harness:?}: a reader with no session");
        };
        assert_eq!(said(&reading), "missing session id", "{harness:?}");
        let answered = answers(harness, "", &env, &Options::default(), &TimeZone::UTC, 0);
        assert_eq!(*answered, reading, "{harness:?}");
        let looked = cache.look(harness, "", &env, &Options::default(), 0);
        assert_eq!(*looked, reading, "{harness:?}");
    }
}

#[test]
fn each_harness_reads_its_own_record() {
    let home = tempfile::tempdir().unwrap();
    let env = at_home(home.path());
    let read = |harness| {
        said(&answers(
            harness,
            "s",
            &env,
            &Options::default(),
            &TimeZone::UTC,
            0,
        ))
    };
    assert_eq!(read(Harness::Claude), "unreadable: no claude session s");
    assert_eq!(read(Harness::Codex), "unreadable: no codex rollout for s");
    assert_eq!(read(Harness::Pi), "unreadable: no pi session s");
    assert_eq!(
        read(Harness::Opencode),
        "unreadable: no opencode store for s"
    );
    // Devin's store is opened, not found: its failure is the platform's.
    let devin = read(Harness::Devin);
    assert!(devin.starts_with("unreadable: "), "{devin}");
    assert!(!devin.starts_with("unreadable: no "), "{devin}");
}

#[test]
fn a_reset_that_names_no_zone_is_read_in_the_zone_the_switch_is_given() {
    let home = tempfile::tempdir().unwrap();
    let env = at_home(home.path());
    let folder = home.path().join(".claude").join("projects").join("-work");
    fs::create_dir_all(&folder).unwrap();
    let refused = json!({
        "type": "assistant", "uuid": "a1", "sessionId": "s", "isSidechain": false,
        "timestamp": "2026-10-03T12:00:00.000Z", "isApiErrorMessage": true, "apiErrorStatus": 429,
        "message": {
            "id": "m1", "role": "assistant",
            "content": [{ "type": "text", "text": "You've hit your session limit · resets 7:30pm" }],
        },
    });
    fs::write(folder.join("s.jsonl"), format!("{refused}\n")).unwrap();
    let resets = |reading: &Reading| match reading {
        Reading::Known(record) => serde_json::to_value(&record.quota).unwrap()["resetsAt"].clone(),
        Reading::Unknown(reason) => panic!("{reason}"),
    };
    let los_angeles = TimeZoneDatabase::bundled()
        .get("America/Los_Angeles")
        .unwrap();
    let options = Options::default();
    // Node, TZ=America/Los_Angeles and TZ=UTC.
    let (there, utc) = (
        Value::from("2026-10-04T02:30:00.000Z"),
        Value::from("2026-10-03T19:30:00.000Z"),
    );
    let answered = |local| answers(Harness::Claude, "s", &env, &options, local, 0);
    assert_eq!(resets(&answered(&los_angeles)), there);
    assert_eq!(resets(&answered(&TimeZone::UTC)), utc);
    let mut cache = Cache::new(open(los_angeles), IDLE_MS, 0);
    assert_eq!(
        resets(&cache.look(Harness::Claude, "s", &env, &options, 0)),
        there
    );
}

#[test]
fn a_zone_is_the_one_intl_takes_the_name_for() {
    let offset_at_epoch =
        |name: &str| zone(name).map(|zone| zone.to_offset(jiff::Timestamp::UNIX_EPOCH).seconds());
    assert_eq!(offset_at_epoch("America/Los_Angeles"), Some(-8 * 3600));
    assert_eq!(offset_at_epoch("+03:00"), Some(3 * 3600));
    assert_eq!(offset_at_epoch("SystemV/EST5"), Some(-5 * 3600));
    assert_eq!(offset_at_epoch("PST"), Some(-8 * 3600));
    for refused in ["Factory", "Etc/Unknown", "Nowhere/Zone", ""] {
        assert_eq!(offset_at_epoch(refused), None, "{refused:?}");
    }
}

#[test]
fn a_transcript_is_had_where_its_harness_keeps_it_and_never_in_a_store() {
    let home = tempfile::tempdir().unwrap();
    let env = at_home(home.path());
    let had = |harness, session| has_transcript(harness, session, &env).unwrap();
    for harness in Harness::ALL {
        assert!(!had(harness, "s"), "{harness:?}: nothing kept yet");
    }
    let keep = |under: &[&str], name: &str| {
        let folder = under
            .iter()
            .fold(home.path().to_path_buf(), |path, part| path.join(part));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(name), "").unwrap();
    };
    keep(&[".claude", "projects", "-work"], "s.jsonl");
    keep(
        &[".codex", "sessions", "2026", "10", "05"],
        "rollout-2026-10-05T01-00-00-s.jsonl",
    );
    keep(
        &[".pi", "agent", "sessions", "--work--"],
        "2026-10-05T01-00-00-000Z_s.jsonl",
    );
    // A file in each store's place is no transcript.
    keep(&[".local", "share", "opencode"], "opencode.db");
    keep(&[".local", "share", "devin", "cli"], "sessions.db");
    for harness in [Harness::Claude, Harness::Codex, Harness::Pi] {
        assert!(had(harness, "s"), "{harness:?}");
        assert!(!had(harness, "q7"), "{harness:?}: another session");
    }
    for harness in [Harness::Opencode, Harness::Devin] {
        assert!(!had(harness, "s"), "{harness:?}");
    }
    // Claude's file is named by the session whole; the others' hold it.
    assert!(!had(Harness::Claude, "S"));
    assert!(had(Harness::Codex, "05T01"));
    assert!(had(Harness::Pi, "000Z_"));
}

#[test]
fn a_transcript_looked_for_with_no_home_is_the_failure() {
    let env = Env::default();
    for harness in [Harness::Claude, Harness::Codex, Harness::Pi] {
        assert_eq!(
            has_transcript(harness, "s", &env).unwrap_err(),
            "missing home in env",
            "{harness:?}"
        );
    }
    for harness in [Harness::Opencode, Harness::Devin] {
        assert_eq!(has_transcript(harness, "s", &env), Ok(false), "{harness:?}");
    }
    // Where its folder is named, Claude needs no home.
    let named = Env::from_vars([("CLAUDE_CONFIG_DIR", "/nowhere")]);
    assert_eq!(has_transcript(Harness::Claude, "s", &named), Ok(false));
}
