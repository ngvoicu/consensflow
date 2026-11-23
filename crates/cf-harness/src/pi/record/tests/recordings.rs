//! Pi's real connection-failure records from T-1939, read at each retry boundary.

use super::*;
use cf_proto::agents::Harness;

const FIRST_ERROR: &str = include_str!(
    "../../../../tests/fixtures/connection-failures/mac-pi-refused-first-error-prefix.jsonl"
);

const PI_RECORDINGS: [(&str, &str); 4] = [
    (
        "mac-pi-refused",
        include_str!("../../../../tests/fixtures/connection-failures/mac-pi-refused.jsonl"),
    ),
    (
        "mac-pi-cut",
        include_str!("../../../../tests/fixtures/connection-failures/mac-pi-cut.jsonl"),
    ),
    (
        "windows-pi-refused",
        include_str!("../../../../tests/fixtures/connection-failures/windows-pi-refused.jsonl"),
    ),
    (
        "windows-pi-cut",
        include_str!("../../../../tests/fixtures/connection-failures/windows-pi-cut.jsonl"),
    ),
];

const OTHER_RECORDINGS: [(&str, Harness, &str); 10] = [
    (
        "mac-claude-refused",
        Harness::Claude,
        include_str!("../../../../tests/fixtures/connection-failures/mac-claude-refused.jsonl"),
    ),
    (
        "mac-claude-cut",
        Harness::Claude,
        include_str!("../../../../tests/fixtures/connection-failures/mac-claude-cut.jsonl"),
    ),
    (
        "mac-claude-print-refused-prefix",
        Harness::Claude,
        include_str!(
            "../../../../tests/fixtures/connection-failures/mac-claude-print-refused-prefix.jsonl"
        ),
    ),
    (
        "mac-claude-print-cut",
        Harness::Claude,
        include_str!("../../../../tests/fixtures/connection-failures/mac-claude-print-cut.jsonl"),
    ),
    (
        "windows-claude-refused",
        Harness::Claude,
        include_str!("../../../../tests/fixtures/connection-failures/windows-claude-refused.jsonl"),
    ),
    (
        "windows-claude-cut",
        Harness::Claude,
        include_str!("../../../../tests/fixtures/connection-failures/windows-claude-cut.jsonl"),
    ),
    (
        "mac-codex-refused-150s",
        Harness::Codex,
        include_str!("../../../../tests/fixtures/connection-failures/mac-codex-refused-150s.jsonl"),
    ),
    (
        "mac-codex-cut",
        Harness::Codex,
        include_str!("../../../../tests/fixtures/connection-failures/mac-codex-cut.jsonl"),
    ),
    (
        "windows-codex-refused",
        Harness::Codex,
        include_str!("../../../../tests/fixtures/connection-failures/windows-codex-refused.jsonl"),
    ),
    (
        "windows-codex-cut",
        Harness::Codex,
        include_str!("../../../../tests/fixtures/connection-failures/windows-codex-cut.jsonl"),
    ),
];

#[test]
fn t1958_pis_first_refused_error_is_a_turn_at_work() {
    let mut stage = Stage::new();
    stage.write_text(FIRST_ERROR);
    let record = stage.at(0);
    assert!(record.in_flight, "the retry is still in flight: {record:?}");
    assert_eq!(record.settlement, Settlement::InFlight);
    assert!(
        !record.failed,
        "the first provider error must not fail the turn: failed={}, in_flight={}",
        record.failed, record.in_flight
    );
}

#[test]
fn t1958_each_pi_prefix_fails_only_at_settlement_and_every_whole_record_fails() {
    for (name, fixture) in PI_RECORDINGS {
        let lines: Vec<&str> = fixture.lines().collect();
        let mut stage = Stage::new();
        for end in 1..=lines.len() {
            stage.write_text(&format!("{}\n", lines[..end].join("\n")));
            for age in [0, 120_000] {
                let record = stage.at(age);
                assert!(
                    !record.failed
                        || (record.settlement == Settlement::Settled && !record.in_flight),
                    "{name} prefix {end}, age {age}: {record:?}"
                );
            }
        }
        let quiet = stage.at(120_000);
        assert!(quiet.failed && !quiet.in_flight, "{name}: {quiet:?}");
        assert_eq!(quiet.settlement, Settlement::Settled, "{name}");

        // The extension boundary is independent evidence; it ends the same
        // recorded last error immediately, without the quiet fallback.
        let last: Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        assert_eq!(last["message"]["stopReason"], "error", "{name}");
        let directory = stage.home.path().join("settled");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("launch.json"),
            json!({
                "launchId": "launch", "sessionId": SESSION, "frontier": { "id": last["id"] }
            })
            .to_string(),
        )
        .unwrap();
        let options = Options {
            pi_settlement: Some(PiSettlement {
                directory: Some(directory.to_str().unwrap().to_owned()),
                launch_id: Some("launch".to_owned()),
            }),
            ..Options::default()
        };
        let native = known(&stage.look(0, &options));
        assert!(native.failed && !native.in_flight, "{name}: {native:?}");
        assert_eq!(native.settlement, Settlement::Settled, "{name}");
    }
}

#[test]
fn t1958_a_recorded_cut_error_followed_by_a_constructed_success_gives_the_result() {
    // T-1939's cut recording never recovered. This success is constructed from
    // its first recorded error; it is not a recording of a successful retry.
    let mut rows: Vec<Value> = Vec::new();
    for line in PI_RECORDINGS[1].1.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let is_error = row["message"]["stopReason"] == "error";
        rows.push(row);
        if is_error {
            break;
        }
    }
    let failed = rows.last().unwrap().clone();
    assert_eq!(failed["message"]["stopReason"], "error");
    let mut stage = Stage::with(&rows);
    let working = stage.at(0);
    assert!(working.in_flight && !working.failed);

    let mut success = failed.clone();
    success["parentId"] = failed["id"].clone();
    success["id"] = json!("constructed-success");
    success["message"]["stopReason"] = json!("stop");
    success["message"]["content"] = json!([{ "type": "text", "text": "The retry completed." }]);
    success["message"]
        .as_object_mut()
        .unwrap()
        .remove("errorMessage");
    rows.push(success);
    stage.write(&rows);
    let done = stage.at(120_000);
    assert_eq!(done.settlement, Settlement::Settled);
    assert!(!done.failed && !done.in_flight);
    let result = done.items.last().unwrap();
    assert!(result.complete);
    assert_eq!(&*result.text, "The retry completed.");
}

#[test]
fn t1958_every_recorded_claude_codex_and_pi_prefix_never_fails_in_flight() {
    let cases = OTHER_RECORDINGS.into_iter().chain(
        PI_RECORDINGS
            .into_iter()
            .map(|(name, text)| (name, Harness::Pi, text)),
    );
    let mut prefixes = 0;
    let mut ended_failures = 0;
    for (name, harness, fixture) in cases {
        let rows: Vec<&str> = fixture.lines().collect();
        let first: Value = serde_json::from_str(rows[0]).unwrap();
        let session = match harness {
            Harness::Claude => first["sessionId"].as_str(),
            Harness::Codex => first["payload"]["id"].as_str(),
            Harness::Pi => first["id"].as_str(),
            _ => None,
        }
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let file = match harness {
            Harness::Claude => home
                .path()
                .join("projects")
                .join(format!("{session}.jsonl")),
            Harness::Codex => home
                .path()
                .join("sessions")
                .join(format!("rollout-{session}.jsonl")),
            Harness::Pi => home.path().join("pi").join(format!("{session}.jsonl")),
            _ => unreachable!(),
        };
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let env = Env::from_vars([
            ("CLAUDE_CONFIG_DIR", home.path().to_path_buf()),
            ("CODEX_HOME", home.path().to_path_buf()),
            ("PI_CODING_AGENT_SESSION_DIR", home.path().join("pi")),
        ]);
        for end in 1..=rows.len() {
            fs::write(&file, format!("{}\n", rows[..end].join("\n"))).unwrap();
            let now = fs::metadata(&file)
                .unwrap()
                .modified()
                .unwrap()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis();
            let reading = crate::records::answers(
                harness,
                session,
                &env,
                &Options::default(),
                &TimeZone::UTC,
                i64::try_from(now).unwrap(),
            );
            let record = known(&reading);
            assert!(
                !(record.failed && record.in_flight),
                "{name} prefix {end}/{}: failed while in flight: {record:?}",
                rows.len()
            );
            if end == rows.len() && record.failed && !record.in_flight {
                ended_failures += 1;
            }
            prefixes += 1;
        }
    }
    assert_eq!(prefixes, 261, "every native recording prefix was read");
    assert!(
        ended_failures >= 3,
        "terminal failure controls were actually read"
    );
}

/// What a paste of ConsensFlow's would be in `harness`'s own record after `rows`: the same
/// rows the recording's own prompt was written in, made over with a new id and the paste's
/// words, in a new turn where the harness writes one.
fn paste_after(harness: Harness, rows: &[&str], words: &str) -> Vec<String> {
    let parsed: Vec<Value> = rows
        .iter()
        .map(|row| serde_json::from_str(row).unwrap())
        .collect();
    let later = "2026-11-22T00:00:00.000Z";
    match harness {
        Harness::Claude => {
            let mut prompt = parsed
                .iter()
                .find(|row| row["type"] == "user")
                .unwrap()
                .clone();
            let last = parsed.iter().rev().find_map(|row| row.get("uuid")).unwrap();
            prompt["parentUuid"] = last.clone();
            prompt["uuid"] = json!("pasted-by-consensflow");
            prompt["promptId"] = json!("pasted-prompt");
            prompt["timestamp"] = json!(later);
            prompt["message"]["content"] = json!(words);
            vec![prompt.to_string()]
        }
        Harness::Codex => {
            let mut began = parsed
                .iter()
                .find(|row| row["payload"]["type"] == "task_started")
                .unwrap()
                .clone();
            let mut prompt = parsed
                .iter()
                .rev()
                .find(|row| row["payload"]["role"] == "user")
                .unwrap()
                .clone();
            began["timestamp"] = json!(later);
            began["payload"]["turn_id"] = json!("pasted-turn");
            began["payload"]["root_turn_id"] = json!("pasted-turn");
            began["payload"]["turn_attribution"]["turn_id"] = json!("pasted-turn");
            began["payload"]["turn_attribution"]["root_turn_id"] = json!("pasted-turn");
            prompt["timestamp"] = json!(later);
            prompt["payload"]["id"] = json!("pasted-item");
            prompt["payload"]["content"] = json!([{ "type": "input_text", "text": words }]);
            prompt["payload"]["internal_chat_message_metadata_passthrough"]["turn_id"] =
                json!("pasted-turn");
            vec![began.to_string(), prompt.to_string()]
        }
        Harness::Pi => {
            let mut prompt = parsed
                .iter()
                .find(|row| row["message"]["role"] == "user")
                .unwrap()
                .clone();
            let last = parsed.last().unwrap();
            prompt["parentId"] = last["id"].clone();
            prompt["id"] = json!("pasted01");
            prompt["timestamp"] = json!(later);
            prompt["message"]["content"] = json!([{ "type": "text", "text": words }]);
            vec![prompt.to_string()]
        }
        _ => unreachable!(),
    }
}

#[test]
fn n35_a_failed_end_is_out_of_view_once_a_later_paste_is_in_the_record() {
    // The count of a retry stands on this: the Resume of a held task is pasted after the failed
    // end, and the look after its paste must not read that failed end as the turn it opened.
    // Every prefix of every recording, with a paste written after it.
    let cases = OTHER_RECORDINGS.into_iter().chain(
        PI_RECORDINGS
            .into_iter()
            .map(|(name, text)| (name, Harness::Pi, text)),
    );
    let (mut failed_ends, mut checked) = (0, 0);
    for (name, harness, fixture) in cases {
        let rows: Vec<&str> = fixture.lines().collect();
        let first: Value = serde_json::from_str(rows[0]).unwrap();
        let session = match harness {
            Harness::Claude => first["sessionId"].as_str(),
            Harness::Codex => first["payload"]["id"].as_str(),
            Harness::Pi => first["id"].as_str(),
            _ => None,
        }
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let file = match harness {
            Harness::Claude => home
                .path()
                .join("projects")
                .join(format!("{session}.jsonl")),
            Harness::Codex => home
                .path()
                .join("sessions")
                .join(format!("rollout-{session}.jsonl")),
            Harness::Pi => home.path().join("pi").join(format!("{session}.jsonl")),
            _ => unreachable!(),
        };
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let env = Env::from_vars([
            ("CLAUDE_CONFIG_DIR", home.path().to_path_buf()),
            ("CODEX_HOME", home.path().to_path_buf()),
            ("PI_CODING_AGENT_SESSION_DIR", home.path().join("pi")),
        ]);
        let read = |text: String| {
            fs::write(&file, text).unwrap();
            let now = fs::metadata(&file)
                .unwrap()
                .modified()
                .unwrap()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis();
            // A quiet minute later: a Pi error end settles by the quiet fallback.
            let reading = crate::records::answers(
                harness,
                session,
                &env,
                &Options::default(),
                &TimeZone::UTC,
                i64::try_from(now).unwrap() + 120_000,
            );
            known(&reading).clone()
        };
        for end in 1..=rows.len() {
            let prefix = format!("{}\n", rows[..end].join("\n"));
            let before = read(prefix.clone());
            checked += 1;
            if !before.failed {
                continue;
            }
            failed_ends += 1;
            let pasted = paste_after(harness, &rows[..end], "Resumed: Go on where you stopped.");
            let after = read(format!("{prefix}{}\n", pasted.join("\n")));
            assert!(
                !after.failed,
                "{name} prefix {end}/{}: the failed end is still in view after a later paste: {after:?}",
                rows.len()
            );
        }
    }
    assert!(checked >= 200, "every prefix was read: {checked}");
    assert!(failed_ends >= 3, "failed ends were actually met: {failed_ends}");
}
