use serde_json::json;

use super::*;

const TEMP: &str = "/var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/";

fn scrubbed_line(record: Value) -> Value {
    let (records, text) = scrub(&format!("{record}\n")).unwrap();
    assert_eq!(records, 1);
    serde_json::from_str(text.trim_end()).unwrap()
}

fn run_event(event: Value) -> Value {
    json!({
        "schema_version": 1,
        "id": "r1",
        "payload_type": "runtime.session",
        "payload": { "kind": "run", "run_id": "run1", "event": event },
    })
}

#[test]
fn the_recorders_home_and_the_temporary_folder_are_replaced_wherever_they_are_written() {
    let home = format!("{TEMP}muse-rec-zdhncg32");
    let record = json!({
        "payload": { "kind": "metadata", "record": {
            "workspace_root": format!("/private{home}/work"),
            "log": format!("{home}/data/muse/sessions/x/session.jsonl"),
            "sandbox": format!("temp root: {TEMP}muse-shell-sandbox-1, TMPDIR: {TEMP} (rw)"),
            "bare": TEMP.trim_end_matches('/'),
            "other": "/var/folders/not/a/temporary/one",
        }},
    });
    let kept = scrubbed_line(record);
    let record = &kept["payload"]["record"];
    assert_eq!(record["workspace_root"], "/muse-home/work");
    assert_eq!(
        record["log"],
        "/muse-home/data/muse/sessions/x/session.jsonl"
    );
    assert_eq!(
        record["sandbox"],
        "temp root: /tmp/muse-shell-sandbox-1, TMPDIR: /tmp/ (rw)"
    );
    assert_eq!(record["bare"], "/tmp/");
    assert_eq!(record["other"], "/var/folders/not/a/temporary/one");
}

#[test]
fn muses_own_long_words_go_and_the_conversations_stay() {
    let long = "x".repeat(LONGEST + 1);
    let short = "y".repeat(LONGEST);
    for kind in MUSE_S_OWN {
        let kept = scrubbed_line(run_event(
            json!({ "kind": kind, "text": long, "id": short }),
        ));
        let event = &kept["payload"]["event"];
        assert_eq!(
            event["text"],
            format!("[scrubbed: {} bytes]", LONGEST + 1),
            "{kind}"
        );
        assert_eq!(event["id"], short, "{kind}: a short text stays");
        assert_eq!(event["kind"], kind);
    }
    // A context block Muse wrote goes; one a hook wrote is the conversation's.
    let block = |source: &str| {
        let kept = scrubbed_line(run_event(
            json!({ "kind": "context_block_updated", "source": source, "text": long }),
        ));
        kept["payload"]["event"]["text"].clone()
    };
    assert_eq!(
        block("session_identity"),
        format!("[scrubbed: {} bytes]", LONGEST + 1)
    );
    assert_eq!(block("runtime_hook"), long);
    // The user's words and the model's stay, however long.
    let said = scrubbed_line(run_event(
        json!({ "kind": "assistant_message_committed", "text": long }),
    ));
    assert_eq!(said["payload"]["event"]["text"], long);
    let asked = scrubbed_line(json!({
        "payload_type": "runtime.user_intent.accepted",
        "payload": { "refill_blocks": [{ "kind": "text", "text": long }] },
    }));
    assert_eq!(asked["payload"]["refill_blocks"][0]["text"], long);
}

#[test]
fn the_frame_that_begins_a_log_keeps_its_keys_and_loses_its_copies_of_records() {
    let long = "z".repeat(200);
    let kept = scrubbed_line(json!({
        "retained_frame": true,
        "children": [{ "record_json": long, "kind": "k" }],
        "content_sha256": "abc",
    }));
    assert_eq!(kept["children"][0]["record_json"], "[scrubbed: 200 bytes]");
    assert_eq!(kept["children"][0]["kind"], "k");
    assert_eq!(kept["content_sha256"], "abc");
}

#[test]
fn every_record_stays_in_its_order_with_its_keys_where_they_were() {
    let log = "{\"b\":1,\"a\":2}\n\n{\"a\":3}\n";
    let (records, text) = scrub(log).unwrap();
    assert_eq!(records, 2);
    assert_eq!(text, "{\"b\":1,\"a\":2}\n{\"a\":3}\n");
}

#[test]
fn a_line_that_is_no_record_is_said_by_its_number() {
    assert!(matches!(
        scrub("{}\n[1]\n"),
        Err(Error::NotRecord { line: 2 })
    ));
    assert!(matches!(
        scrub("{}\nnope\n"),
        Err(Error::NotJson { line: 2, .. })
    ));
}
