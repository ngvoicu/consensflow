//! The board after a re-plan, end to end through the real pane host
//! (`npm run test:daemons`). The chief eval `six-decisions` (2026-10-07, on the
//! native daemon) cancelled tasks that were still open for a tier, and its
//! check "the board showed every task" counted fewer than it had made: both
//! daemons left such a task off the board, as a task paused in the backlog and a
//! removed member's. The owner decided (2026-10-07) that none leaves it. The
//! board lists a task by the lane of whoever has it, or among the open ones when
//! no lane has it (waiting for a member, paused or called off before any member
//! had it, or a removed member's), and `cf task list` lists both: the page draws
//! the open ones on their requester's row.

use cf_e2e::rig::{Project, OPEN};
use regex::Regex;
use serde_json::{json, Value};

use crate::{rig, secs, session_of, Outcome};

/// The pattern of a session's handle, which `listed` masks.
const SESSION: &str = r"^(\w+)-[a-z]+-[a-z]+$";

/// The pattern of a session's name in a message, which `printed` masks.
const WORKER: &str = r"@worker-[a-z]+-[a-z]+";

/// Where each task the board draws is: `open T-n state`, or the lane's handle (a
/// session's name masked by `session`) and the same.
fn listed(board: &Value, session: &Regex) -> Vec<String> {
    let mut places: Vec<String> = board["open"]
        .as_array()
        .map(|open| {
            open.iter()
                .map(|task| format!("open T-{} {}", task["number"], text(&task["state"])))
                .collect()
        })
        .unwrap_or_default();
    for lane in board["lanes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let handle = text(&lane["participant"]["handle"]);
        let handle = session.replace(&handle, "${1}-*").into_owned();
        for task in lane["tasks"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            places.push(format!(
                "{handle} T-{} {}",
                task["number"],
                text(&task["state"])
            ));
        }
    }
    places.sort();
    places
}

/// A JSON string as the text it is.
fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// What the chief's `cf` printed each time it ran one for the board, in order:
/// its window's record of the turns, a session's name masked by `worker`.
fn printed(record: &str, worker: &Regex) -> cf_e2e::Result<Vec<String>> {
    let mut said = Vec::new();
    for line in record.split('\n').filter(|line| !line.trim().is_empty()) {
        let entry: Value = serde_json::from_str(line).map_err(|source| cf_e2e::Error::Json {
            text: line.to_owned(),
            source,
        })?;
        if entry["type"] != "assistant" {
            continue;
        }
        let words: String = entry["message"]["content"]
            .as_array()
            .map(|parts| parts.iter().map(|part| text(&part["text"])).collect())
            .unwrap_or_default();
        if let Some(rest) = words.strip_prefix("ran cf: ") {
            said.push(worker.replace_all(rest, "@worker-*").into_owned());
        }
    }
    Ok(said)
}

#[test]
fn a_task_no_lane_has_stays_on_the_board_and_in_cf_task_list_called_off_paused_or_a_removed_members(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let added = project.add_member("worker")?;
    let tier = text(&added["member"]["tier"]);
    let chief_frame = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let chief_session = session_of(&chief_frame);
    let session = Regex::new(SESSION)?;
    let worker = Regex::new(WORKER)?;
    let chief_said = || printed(&rig.transcript(&chief_session), &worker);

    // T-1 is done and not accepted, so the three tasks that need it wait, open for a tier.
    rig.tell(
        id,
        &format!("DISPATCH --tier {tier} Reply with exactly: ONE"),
    )?;
    rig.wait_for("T-1 to be done", secs(30), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    for word in ["TWO", "THREE", "FOUR"] {
        rig.tell(
            id,
            &format!("DISPATCH --tier {tier} --needs T-1 Reply with exactly: {word}"),
        )?;
    }
    rig.wait_for("three tasks to be waiting", secs(30), || {
        Ok(project.board()?["open"].as_array().map(Vec::len) == Some(3))
    })?;
    assert_eq!(
        listed(&project.board()?, &session),
        [
            "open T-2 open",
            "open T-3 open",
            "open T-4 open",
            "worker-* T-1 done"
        ]
    );

    // The chief calls T-2 off with its own `cf`: the next board it reads lists it apart
    // from what waits for a member.
    rig.tell(id, "CF task list")?;
    rig.tell(id, "CF task cancel T-2")?;
    rig.wait_for("T-2 to be cancelled", secs(30), || {
        Ok(project.task(2)?["state"] == "cancelled")
    })?;
    rig.tell(id, "CF task list")?;
    rig.wait_for("the chief to have run three cf", secs(30), || {
        Ok(chief_said()?.len() == 3)
    })?;
    let said = chief_said()?;
    assert!(
        Regex::new(r"^Waiting for a member\nT-2 \[open\] blocked by T-1 .*\nT-3 ")?
            .is_match(&said[0]),
        "{}",
        said[0]
    );
    assert!(
        Regex::new(
            r"^Waiting for a member\nT-3 \[open\] .*\nT-4 \[open\] .*\nWith no member\nT-2 \[cancelled\] blocked by T-1 "
        )?
        .is_match(&said[2]),
        "the chief reads the task it called off, no longer among those waiting: {}",
        said[2]
    );
    assert_eq!(
        listed(&project.board()?, &session),
        [
            "open T-2 cancelled",
            "open T-3 open",
            "open T-4 open",
            "worker-* T-1 done"
        ]
    );

    // The human cancels T-3 and pauses T-4, as the page's buttons do: both stay.
    let cancelled = rig.page("task.cancel", json!({ "project": id, "task": 3 }))?;
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    let paused = rig.page("task.pause", json!({ "project": id, "task": 4 }))?;
    assert_eq!(paused["ok"], true, "{paused}");
    assert_eq!(
        listed(&project.board()?, &session),
        [
            "open T-2 cancelled",
            "open T-3 cancelled",
            "open T-4 paused",
            "worker-* T-1 done"
        ],
        "all four are drawn: the paused one in the backlog, the two called off to be deleted"
    );
    let mut states = Vec::new();
    for number in [2, 3, 4] {
        states.push(json!([number, project.task(number)?["state"]]));
    }
    assert_eq!(
        states,
        [
            json!([2, "cancelled"]),
            json!([3, "cancelled"]),
            json!([4, "paused"])
        ]
    );
    for number in [2, 3, 4] {
        assert_eq!(
            project.task(number)?.get("assignee"),
            Some(&Value::Null),
            "no member ever had T-{number}"
        );
    }
    let fifth = rig.page("task.get", json!({ "project": id, "task": 5 }))?;
    assert_eq!(fifth["ok"], false, "and there are four, not five");

    // The human resumes T-4 from the backlog and deletes the two called off, as the page does.
    let resumed = rig.page("task.resume", json!({ "project": id, "task": 4 }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let deleted = rig.page("tasks.delete", json!({ "project": id, "tasks": [2, 3] }))?;
    assert_eq!(deleted["ok"], true, "{deleted}");
    assert_eq!(
        listed(&project.board()?, &session),
        ["open T-4 open", "worker-* T-1 done"]
    );

    // The human removes the worker: T-1, which it did and the chief has not accepted, stays on
    // the board, no lane's, and the chief reads it so.
    let removed = rig.page("member.remove", json!({ "project": id, "agent": "worker" }))?;
    assert_eq!(removed["ok"], true, "{removed}");
    assert_eq!(
        listed(&project.board()?, &session),
        ["open T-1 done", "open T-4 open"]
    );
    rig.tell(id, "CF task list")?;
    rig.wait_for("the chief to have run four cf", secs(30), || {
        Ok(chief_said()?.len() == 4)
    })?;
    let last = chief_said()?.pop().unwrap_or_default();
    assert!(
        Regex::new(
            r"^Waiting for a member\nT-4 \[open\] blocked by T-1 .*\nWith no member\nT-1 \[done\] @worker-\* ← @chief: .*$"
        )?
        .is_match(&last),
        "{last}"
    );
    rig.close()?;
    Ok(())
}
