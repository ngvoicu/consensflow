//! T-1937's account-free probe, reconstructed from its recorded event kinds and
//! synthetic result. Its original scratch rollout was removed after the probe;
//! the cwd, thread and turn here are test data, with no account information.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use cf_base::env::Env;
use cf_proto::agents::Harness;

use crate::records::{self, Options, Reading, Settlement};

const SESSION: &str = "00000000-0000-4000-8000-000000000001";
const PLAIN: &[u8] = include_bytes!("fixtures/t1937-synthetic.jsonl");
const COMPRESSED: &[u8] = include_bytes!("fixtures/t1937-synthetic.jsonl.zst");

fn home() -> (tempfile::TempDir, Env, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let env = Env::from_vars([("CODEX_HOME", home.path().to_str().unwrap())]);
    let day = home.path().join("sessions/2026/11/22");
    fs::create_dir_all(&day).unwrap();
    let file = day.join(format!("rollout-2026-11-22T00-00-00-{SESSION}.jsonl"));
    (home, env, file)
}

fn read(env: &Env) -> Reading {
    (*records::answers(
        Harness::Codex,
        SESSION,
        env,
        &Options::default(),
        &jiff::tz::TimeZone::UTC,
        1_800_000_000_000,
    ))
    .clone()
}

#[test]
fn compressed_probe_equals_plain_and_replays_the_result() {
    let (_plain_home, plain_env, plain) = home();
    let (_compressed_home, compressed_env, compressed) = home();
    fs::write(&plain, PLAIN).unwrap();
    fs::write(compressed.with_extension("jsonl.zst"), COMPRESSED).unwrap();
    let expected = read(&plain_env);
    let Reading::Known(record) = &expected else {
        panic!("{expected:?}")
    };
    assert_eq!(record.settlement, Settlement::Settled);
    assert!(record
        .items
        .iter()
        .any(|item| &*item.text == "T1937-OFFLINE-RESULT"));
    assert_eq!(read(&compressed_env), expected);
}

#[test]
fn compressed_truncation_is_unreadable_not_empty() {
    let (_home, env, file) = home();
    for end in [0, 1, 10, COMPRESSED.len() / 2, COMPRESSED.len() - 1] {
        fs::write(file.with_extension("jsonl.zst"), &COMPRESSED[..end]).unwrap();
        let Reading::Unknown(reason) = read(&env) else {
            panic!("truncated rollout was read at {end}")
        };
        assert!(reason.starts_with("unreadable:"), "{reason}");
        assert!(!reason.contains("empty codex rollout"), "{end}: {reason}");
    }
}

#[test]
fn a_follower_recovers_when_a_truncated_compressed_rollout_is_replaced() {
    let (_home, env, file) = home();
    let zipped = file.with_extension("jsonl.zst");
    fs::write(&zipped, &COMPRESSED[..COMPRESSED.len() - 1]).unwrap();
    let mut reader = super::super::reader(SESSION, &env);
    let options = Options::default();
    assert!(matches!(
        &*reader.look(&options, 1_800_000_000_000),
        Reading::Unknown(_)
    ));
    fs::write(&zipped, COMPRESSED).unwrap();
    assert!(matches!(
        &*reader.look(&options, 1_800_000_000_001),
        Reading::Known(_)
    ));
}

#[test]
fn an_overlarge_compressed_record_is_unreadable_with_bounded_memory() {
    let (_home, env, file) = home();
    let output = fs::File::create(file.with_extension("jsonl.zst")).unwrap();
    let mut encoder = zstd::stream::write::Encoder::new(output, 3).unwrap();
    let chunk = [b'x'; 64 * 1024];
    for _ in 0..1025 {
        encoder.write_all(&chunk).unwrap();
    }
    encoder.finish().unwrap();
    let Reading::Unknown(reason) = read(&env) else {
        panic!("oversized record was read")
    };
    assert!(
        reason.contains("compressed rollout record exceeds 64 MiB"),
        "{reason}"
    );
}

#[test]
fn compressed_only_rollout_is_kept_for_resume() {
    let (_home, env, file) = home();
    fs::write(file.with_extension("jsonl.zst"), COMPRESSED).unwrap();
    assert!(records::has_transcript(Harness::Codex, SESSION, &env).unwrap());
}

#[test]
fn compressed_rollout_keeps_the_last_used_model() {
    let (_home, _env, file) = home();
    fs::write(&file, PLAIN).unwrap();
    let zipped = file.with_extension("jsonl.zst");
    fs::write(&zipped, COMPRESSED).unwrap();
    let plain = crate::codex::last_used::used_in(&file, 16 * 1024 * 1024).unwrap();
    assert_eq!(plain.as_ref().unwrap().model, "synthetic-model");
    assert_eq!(
        crate::codex::last_used::used_in(&zipped, 16 * 1024 * 1024).unwrap(),
        plain
    );
    for bound in [0, 1, 100, 400, 700, 1024, 4096] {
        assert_eq!(
            crate::codex::last_used::used_in(&zipped, bound).unwrap(),
            crate::codex::last_used::used_in(&file, bound).unwrap(),
            "bound {bound}"
        );
    }
    let context = std::str::from_utf8(PLAIN)
        .unwrap()
        .find("{\"timestamp\":\"2026-11-22T00:00:01.000Z\",\"type\":\"turn_context\"")
        .unwrap();
    let at_context = PLAIN.len() as u64 - context as u64;
    for bound in [at_context - 1, at_context, at_context + 1] {
        assert_eq!(
            crate::codex::last_used::used_in(&zipped, bound).unwrap(),
            crate::codex::last_used::used_in(&file, bound).unwrap(),
            "boundary {bound}"
        );
    }
}

#[test]
fn a_follower_changes_form_and_prefers_the_plain_sibling_when_both_exist() {
    let (_home, env, plain) = home();
    let zipped = plain.with_extension("jsonl.zst");
    fs::write(&plain, PLAIN).unwrap();
    let mut reader = super::super::reader(SESSION, &env);
    let options = Options::default();
    let first = reader.look(&options, 1_800_000_000_000);
    assert!(matches!(&*first, Reading::Known(_)));
    fs::write(&zipped, COMPRESSED).unwrap();
    fs::remove_file(&plain).unwrap();
    assert_eq!(reader.look(&options, 1_800_000_000_001), first);

    // Codex can publish the plain sibling before removing the compressed one.
    // It takes precedence even if the old compressed file remains.
    let newer = std::str::from_utf8(PLAIN)
        .unwrap()
        .replace("T1937-OFFLINE-RESULT", "a newer result");
    fs::write(&plain, newer).unwrap();
    let next = reader.look(&options, 1_800_000_000_002);
    let Reading::Known(record) = &*next else {
        panic!("{next:?}")
    };
    assert!(record
        .items
        .iter()
        .any(|item| &*item.text == "a newer result"));
    assert_eq!(read(&env), *next);
}

#[cfg(unix)]
#[test]
fn a_compressed_rollout_does_not_follow_a_plain_sibling_symlink() {
    use std::os::unix::fs::symlink;

    let (_home, env, plain) = home();
    let other = tempfile::tempdir().unwrap();
    let outside = other.path().join("outside.jsonl");
    fs::write(&outside, b"{}").unwrap();
    fs::write(plain.with_extension("jsonl.zst"), COMPRESSED).unwrap();
    symlink(&outside, &plain).unwrap();
    let reading = read(&env);
    let Reading::Known(record) = reading else {
        panic!("{reading:?}")
    };
    assert!(record
        .items
        .iter()
        .any(|item| &*item.text == "T1937-OFFLINE-RESULT"));
    fs::remove_file(&plain).unwrap();
    let mut reader = super::super::reader(SESSION, &env);
    let options = Options::default();
    let before = reader.look(&options, 1_800_000_000_000);
    symlink(&outside, &plain).unwrap();
    assert_eq!(reader.look(&options, 1_800_000_000_001), before);
}
