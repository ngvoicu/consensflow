//! A long transcript, made like a big one (`synthetic`), read whole.

use std::collections::BTreeMap;

use super::synthetic::{self, Plan};
use super::*;
use crate::shared::record::reading::Role;

/// A transcript of about `lines` lines, as text.
fn transcript(lines: usize, seed: u64) -> (String, synthetic::Made) {
    let mut bytes = Vec::new();
    let made = synthetic::write(&mut bytes, SESSION, &Plan { lines, seed }).unwrap();
    (String::from_utf8(bytes).unwrap(), made)
}

#[test]
fn a_synthetic_transcript_is_the_same_valid_json_from_the_same_seed() {
    let (text, made) = transcript(600, 7);
    assert_eq!(text.len() as u64, made.bytes);
    assert_eq!(text.lines().count(), made.lines);
    assert!(made.lines >= 600);
    assert_eq!(text, transcript(600, 7).0);
    assert_ne!(text, transcript(600, 8).0);
    let mut kinds = BTreeMap::<String, usize>::new();
    for line in text.lines() {
        let record: Value = serde_json::from_str(line).unwrap();
        let kind = record["type"].as_str().unwrap().to_owned();
        let subtype = record["subtype"].as_str().unwrap_or_default();
        *kinds.entry(format!("{kind}/{subtype}")).or_default() += 1;
    }
    // Records of every kind a reader looks at, and some it does not.
    for kind in [
        "assistant/",
        "user/",
        "attachment/",
        "queue-operation/",
        "system/turn_duration",
        "file-history-snapshot/",
        "mode/",
        "last-prompt/",
    ] {
        assert!(kinds.contains_key(kind), "{kind}: {kinds:?}");
    }
}

#[test]
fn a_synthetic_transcript_reads_as_its_turns_and_is_settled() {
    let (text, made) = transcript(900, 11);
    let mut stage = Stage::new();
    fs::create_dir_all(stage.file().parent().unwrap()).unwrap();
    fs::write(stage.file(), &text).unwrap();
    let reading = stage.reader.look(&Options::default(), 0);
    let Reading::Known(record) = &*reading else {
        panic!("not read: {reading:?}");
    };
    assert_eq!(record.settlement, Settlement::Settled);
    let roles = |role: Role| record.items.iter().filter(|item| item.role == role).count();
    let count = |kind: &str| {
        text.lines()
            .filter(|line| line.contains(&format!(r#""type":"{kind}""#)))
            .count()
    };
    assert!(roles(Role::User) > 5, "a prompt for every turn");
    assert!(roles(Role::Tool) > 20, "a result for every call");
    assert!(roles(Role::Custom) > 0, "a hook's context in some turn");
    assert!(roles(Role::Assistant) > roles(Role::User));
    assert!(record.items.last().is_some_and(|item| item.complete));
    assert!(count("assistant") > count("user"));
    assert!(made.watched.len() == 36);
}

#[test]
fn a_record_claiming_a_uuid_a_decision_looked_up_has_the_transcript_read_again() {
    let (text, made) = transcript(900, 11);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    fs::write(&file, &text).unwrap();
    let located = file.clone();
    let mut followed = Followed::new(
        Box::new(move || Ok(Some(located.clone()))),
        Box::new(|| Transcript::new(Arc::from(SESSION), local())),
    );
    assert_eq!(followed.read().unwrap().unwrap().state.count, made.lines);
    // Another record after them is one more, read on from where the look stopped.
    let mut appended = fs::OpenOptions::new().append(true).open(&file).unwrap();
    let more = |uuid: &str| format!("{{\"type\":\"attachment\",\"uuid\":\"{uuid}\"}}\n");
    std::io::Write::write_all(&mut appended, more("someone-else").as_bytes()).unwrap();
    assert_eq!(
        followed.read().unwrap().unwrap().state.count,
        made.lines + 1
    );
    // One that claims the boundary record of the first turn has them all read again.
    std::io::Write::write_all(&mut appended, more(&made.watched).as_bytes()).unwrap();
    let read = followed.read().unwrap().unwrap();
    assert_eq!(read.state.count, made.lines + 2);
    assert!(read.changed);
}
