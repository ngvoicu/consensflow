//! Paced delivery end to end through the real pane host (3.0.7 chief-meeting, CMT-09): two
//! workers finish together, and the chief is given their results one at a time, each once
//! and on its first attempt, each after its window has rested the daemon's own settle (three
//! seconds, which no variable here shortens) since its last turn. The look that let each
//! go is in its `delivery.begun`.

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::decided_result::results;
use crate::{rig, secs, Outcome};

/// The daemon's settle (`Limits::default`).
const SETTLE_MS: i64 = 3_000;

/// The roster of the case: the chief, and two workers on the stand-in, in one tier.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "claude-code", "model": "fake" },
            { "id": "worker-b", "kind": "claude-code", "model": "fake" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// The daemon's events of `kind` for the message `id`.
fn of(events: &[Value], kind: &str, id: i64) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
        .cloned()
        .collect()
}

/// The milliseconds of an event's `at`.
fn at_ms(event: &Value) -> i64 {
    event["at"]
        .as_str()
        .and_then(|at| at.parse::<jiff::Timestamp>().ok())
        .map_or(0, |at| at.as_millisecond())
}

#[test]
fn two_results_that_come_together_go_in_one_at_a_time_each_once_a_settle_after_the_last_turn(
) -> Outcome {
    let rig: Rig = rig()?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let first = project.add_member("worker")?;
    project.add_member("worker-b")?;
    let tier = first["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    rig.tell(
        id,
        &format!(
            "DISPATCH --tier {tier} Reply with exactly: ONE_DONE\nDISPATCH --tier {tier} Reply with exactly: TWO_DONE"
        ),
    )?;
    rig.wait_for("both results to be delivered", secs(120), || {
        let results = results(&project)?;
        Ok(results.len() == 2 && results.iter().all(|result| result["state"] == "delivered"))
    })?;
    let results = results(&project)?;
    let events = rig.events();
    for result in &results {
        let message = result["id"].as_i64().unwrap_or_default();
        let begun = of(&events, "delivery.begun", message);
        assert_eq!(
            (
                begun.len(),
                of(&events, "delivery.retried", message).len(),
                result["attempts"].clone()
            ),
            (1, 0, json!(1)),
            "m-{message} pasted once, on its first attempt"
        );
        let rested = begun[0]["data"]["look"]["restMs"]
            .as_i64()
            .unwrap_or_default();
        assert!(
            rested >= SETTLE_MS,
            "m-{message} went after {rested} ms of rest: {}",
            begun[0]
        );
    }
    // The second goes a settle or more after the first was confirmed.
    let mut confirmed: Vec<(i64, i64)> = results
        .iter()
        .map(|result| {
            let message = result["id"].as_i64().unwrap_or_default();
            let at = of(&events, "delivery.confirmed", message)
                .first()
                .map_or(0, at_ms);
            (at, message)
        })
        .collect();
    confirmed.sort_unstable();
    let next = of(&events, "delivery.begun", confirmed[1].1);
    let gap = at_ms(&next[0]) - confirmed[0].0;
    assert!(
        gap >= SETTLE_MS,
        "{gap} ms between the first's arrival and the second's paste"
    );
    rig.close()?;
    Ok(())
}
