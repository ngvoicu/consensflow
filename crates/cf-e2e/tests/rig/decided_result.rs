//! A result the chief decides on before it is given it, end to end through the
//! real pane host (`npm run test:integration`). The chief's turn of 2026-10-08:
//! T-9 finished while the chief was in a turn of its own, so its result stayed
//! queued; in that turn the chief read it with `cf task get T-9`, accepted the
//! task and put T-10 after it, and ConsensFlow still pasted the result into its
//! window, "Decide with: cf task accept T-9 …", when the turn ended. The
//! decision withdraws the result: the chief is given the next task's result,
//! and never the one it had decided on.

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use regex::Regex;
use serde_json::{json, Value};

use crate::{rig, secs, session_of, Outcome};

/// `cf` as the chief runs it in its window (`frame` is the window's `pane.open`):
/// its own token, the native binary. What it printed; a `cf` that fails is an
/// error, as `execFileSync` made it.
fn cf(rig: &Rig, frame: &Value, words: &[&str]) -> cf_e2e::Result<String> {
    let ran = rig.cf_in_window(frame, words)?;
    if ran.code == Some(0) {
        Ok(ran.stdout)
    } else {
        Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))))
    }
}

/// The results in the chief's inbox, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    let mut results: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect();
    results.sort_by_key(|message| message["id"].as_i64().unwrap_or_default());
    Ok(results)
}

#[test]
fn a_result_the_chief_read_and_decided_on_in_its_own_turn_is_withdrawn_and_the_next_result_reaches_it(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let added = project.add_member("worker")?;
    let chief_frame = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let chief_activity = || -> cf_e2e::Result<Value> {
        let board = project.board()?;
        Ok(board["lanes"]
            .as_array()
            .and_then(|lanes| lanes.iter().find(|l| l["participant"]["handle"] == "chief"))
            .map_or(Value::Null, |lane| lane["activity"]["state"].clone()))
    };

    // T-1 takes its window a few seconds; the chief's next turn takes much longer.
    rig.tell(
        id,
        &format!(
            "DISPATCH --tier {} SLEEP 4 Reply with exactly: ONE_DONE",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;
    rig.tell(id, "SLEEP 40 Reply with exactly: CHIEF_BUSY")?;

    // The window finishes T-1 while the chief is busy: its result waits behind the turn.
    rig.wait_for("T-1's result", secs(30), || {
        Ok(results(&project)?.len() == 1)
    })?;
    let waiting = results(&project)?[0].clone();
    assert_eq!(
        [waiting["body"].clone(), waiting["state"].clone()],
        [json!("ONE_DONE"), json!("queued")]
    );
    assert_eq!(
        chief_activity()?,
        "working",
        "the chief is in its turn, not interrupted"
    );

    // In that turn the chief reads it, whole, and a read receives nothing.
    let read = cf(&rig, &chief_frame, &["task", "get", "T-1"])?;
    let expected = Regex::new(&format!(
        r"m-{} \[queued\] result T-1 from @worker-[a-z]+-[a-z]+\nONE_DONE",
        waiting["id"]
    ))?;
    assert!(expected.is_match(&read), "{read}");
    assert_eq!(results(&project)?[0]["state"], "queued");
    // It accepts the task and puts the next one after it.
    cf(&rig, &chief_frame, &["task", "accept", "T-1"])?;
    cf(
        &rig,
        &chief_frame,
        &[
            "task",
            "add",
            "--after",
            "T-1",
            "Reply with exactly: TWO_DONE",
        ],
    )?;
    let withdrawn = results(&project)?[0].clone();
    assert_eq!(
        [
            withdrawn["id"].clone(),
            withdrawn["state"].clone(),
            withdrawn["reason"].clone()
        ],
        [
            waiting["id"].clone(),
            json!("cancelled"),
            json!("T-1 was accepted")
        ]
    );

    // Its turn ends, T-2 finishes, and the chief is given T-2's result: the dispatcher
    // pastes into the idle window, oldest first, so T-1's would have come before it.
    rig.wait_for("T-2's result to be delivered", secs(90), || {
        Ok(results(&project)?
            .iter()
            .any(|m| m["body"] == "TWO_DONE" && m["state"] == "delivered"))
    })?;
    let all = results(&project)?;
    let (first, second) = (all[0].clone(), all[1].clone());
    assert_eq!(
        [
            first["id"].clone(),
            first["state"].clone(),
            first["reason"].clone()
        ],
        [
            waiting["id"].clone(),
            json!("cancelled"),
            json!("T-1 was accepted")
        ],
        "still withdrawn, not pasted"
    );
    let transcript = rig.transcript(&session_of(&chief_frame));
    assert!(
        !transcript.contains(&format!("[ConsensFlow m-{} ", first["id"])),
        "the chief was never given the result it had decided on"
    );
    assert!(
        !transcript.contains("Decide with: cf task accept T-1"),
        "nor its footer"
    );
    let given = Regex::new(&format!(
        r"\[ConsensFlow m-{} · T-2 · result from @worker-[a-z]+-[a-z]+\]",
        second["id"]
    ))?;
    assert!(given.is_match(&transcript), "{transcript}");

    // A result the chief was given stays given when it decides on its task after.
    cf(&rig, &chief_frame, &["task", "accept", "T-2"])?;
    let states: Vec<Value> = results(&project)?
        .iter()
        .map(|m| json!([m["id"], m["state"]]))
        .collect();
    assert_eq!(
        states,
        [
            json!([first["id"], "cancelled"]),
            json!([second["id"], "delivered"])
        ]
    );
    rig.close()?;
    Ok(())
}
