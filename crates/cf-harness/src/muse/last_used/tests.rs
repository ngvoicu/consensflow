//! What a Muse conversation ran on last, each case of CHF-05 a test, its records shaped
//! as Muse 1.4.14 writes them (`tests/fixtures/muse`, T-705's and T-727's logs).

use serde_json::{json, Value};

use super::used_in;
use crate::contract::Used;

const BOUND: u64 = 1 << 20;

fn envelope(payload_type: &str, payload: Value) -> Value {
    json!({ "schema_version": 1, "record_type": "event", "payload_type": payload_type, "payload": payload })
}

fn configured(model: &str) -> Value {
    envelope(
        "run.model.configured",
        json!({ "kind": "run_model", "record": { "model_id": model, "source": "startup" } }),
    )
}

fn reconfigured(from: &str, to: &str, previous_source: &str) -> Value {
    envelope(
        "runtime.model_reconfigure.completed",
        json!({ "record": {
            "apply_outcome": "completed",
            "previous": { "model_id": from, "source": previous_source },
            "effective": { "model_id": to, "source": "model_reconfigure" },
        } }),
    )
}

fn typed(command: &str) -> Value {
    envelope(
        "command.invoked",
        json!({ "record": { "command": command } }),
    )
}

fn read(records: &[Value], asked: Option<&str>) -> Option<Used> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("session.jsonl");
    let text: String = records.iter().map(|record| format!("{record}\n")).collect();
    std::fs::write(&file, text).unwrap();
    used_in(&file, asked, BOUND).unwrap()
}

fn used(model: &str, by_harness: bool) -> Option<Used> {
    Some(Used {
        model: model.to_owned(),
        effort: None,
        thinking: None,
        by_harness,
    })
}

#[test]
fn the_newest_turns_model_is_the_model_used() {
    let records = [configured("muse-spark-1.3"), configured("muse-spark-1.3")];
    assert_eq!(
        read(&records, Some("muse-spark-1.3")),
        used("muse-spark-1.3", false)
    );
    assert_eq!(read(&[], Some("muse-spark-1.3")), None, "no turn: none");
}

#[test]
fn a_move_muse_made_by_itself_is_the_harnesss_and_never_the_humans() {
    // As after T-705's refused turn: Muse put the session on the contributor model.
    let records = [
        configured("muse-spark-1.3"),
        reconfigured("muse-spark-1.3", "muse-spark-1.3-contributor", "startup"),
        configured("muse-spark-1.3-contributor"),
    ];
    assert_eq!(
        read(&records, Some("muse-spark-1.3")),
        used("muse-spark-1.3-contributor", true)
    );
}

#[test]
fn a_move_the_human_typed_model_for_is_theirs() {
    let records = [
        configured("muse-spark-1.3"),
        typed("/model"),
        reconfigured("muse-spark-1.3", "muse-wonder-1.0", "startup"),
        configured("muse-wonder-1.0"),
    ];
    assert_eq!(
        read(&records, Some("muse-spark-1.3")),
        used("muse-wonder-1.0", false)
    );
}

#[test]
fn a_resumed_session_put_back_on_its_launchs_model_is_no_move() {
    let records = [
        reconfigured("muse-spark-1.3-contributor", "muse-spark-1.3", "replay"),
        configured("muse-spark-1.3"),
    ];
    assert_eq!(read(&records, None), used("muse-spark-1.3", false));
}

#[test]
fn a_move_with_no_turn_since_shows_at_the_next_turn() {
    let records = [
        configured("muse-spark-1.3"),
        reconfigured("muse-spark-1.3", "muse-wonder-1.0", "startup"),
    ];
    assert_eq!(
        read(&records, Some("muse-spark-1.3")),
        used("muse-spark-1.3", false)
    );
}

#[test]
fn a_recorded_log_says_the_model_its_last_turn_ran_on() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("session.jsonl");
    std::fs::write(
        &file,
        include_str!("../../../tests/fixtures/muse/billing-spark.jsonl"),
    )
    .unwrap();
    let read = used_in(&file, Some("muse-spark-1.3"), BOUND).unwrap();
    assert_eq!(
        read.map(|used| used.model),
        Some("muse-spark-1.3".to_owned())
    );
}
