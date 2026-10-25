//! A start leaves what was open suspended, end to end through the real daemon
//! and pane host: the app quits in the middle of a worker's turn, with its chief
//! at rest, and starts again on the same home. Nothing opens by itself, and the
//! board shows the project as it was; the human's Resume brings back the chief
//! and the worker on their own conversations, the worker told its turn was cut
//! (and the chief, whose turn was over, told nothing), and the worker finishes
//! and the chief gets its result.

use cf_e2e::rig::{Project, OPEN};
use serde_json::{json, Value};

use crate::approval::quit_and_start_again;
use crate::decided_result::results;
use crate::{after, rig, secs, session_of, Outcome};

/// What a worker is told when the quit cut its turn on T-1.
const WORKER_CUT: &str =
    "ConsensFlow stopped while you were working on T-1. Go on where you stopped.";

/// What the stand-in chief says when it dispatched a task.
const DISPATCHED: &str = "\"text\":\"dispatched:";

/// How many of the messages a conversation was given say `words`.
fn told(rig: &cf_e2e::rig::Rig, session: &str, words: &str) -> usize {
    rig.transcript(session)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .filter_map(|record| record["message"]["content"].as_str().map(str::to_owned))
        .filter(|text| text.contains(words))
        .count()
}

#[test]
fn a_restart_opens_nothing_and_the_humans_resume_brings_back_the_chief_and_the_worker_told_its_turn_was_cut(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let added = project.add_member("worker")?;
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let chief_session = session_of(&chief);

    // The chief hands the worker a task whose turn works a long time, and is at rest.
    rig.tell(
        id,
        &format!(
            "DISPATCH --tier {} SLEEP 600 Reply with exactly: WORKED",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;
    rig.wait_for("the chief to have dispatched", secs(60), || {
        Ok(rig.transcript(&chief_session).contains(DISPATCHED))
    })?;
    rig.wait_for("the worker to be at work on T-1", secs(60), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    let assignee = project.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let pane = format!("p{id}-{assignee}");
    let worker = session_of(&rig.open_frame(&pane, OPEN)?);

    // The app quits and starts again on the same home.
    let rig = quit_and_start_again(rig)?;
    let project = Project::new(&rig, id);
    std::thread::sleep(secs(3));
    assert!(
        rig.open_frames().is_empty(),
        "no window opens by itself: {:?}",
        rig.open_frames()
    );
    let board = project.board()?;
    assert_eq!(board["project"]["state"], "suspended");
    assert_eq!(
        project.task(1)?["state"],
        "working",
        "the board shows the task as it was"
    );
    assert_eq!(
        board["resume"],
        json!({ "tasks": 1, "messages": 0 }),
        "and says what Resume will start"
    );

    // The human resumes it.
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let again = rig.open_frame(&pane, secs(60))?;
    assert_eq!(after(&again, "--resume"), worker, "{:?}", again["argv"]);
    let chief_again = rig.open_frame(&format!("p{id}-chief"), secs(60))?;
    assert_eq!(after(&chief_again, "--resume"), chief_session);

    // The worker is told its turn was cut, once, finishes, and the chief gets the result.
    rig.wait_for("the worker to be told", secs(60), || {
        Ok(told(&rig, &worker, WORKER_CUT) == 1)
    })?;
    rig.wait_for("T-1's result to reach the chief", secs(60), || {
        Ok(results(&project)?
            .first()
            .is_some_and(|result| result["state"] == "delivered"))
    })?;
    assert_eq!(project.task(1)?["state"], "done");
    assert_eq!(told(&rig, &worker, WORKER_CUT), 1, "told once");
    assert_eq!(
        told(&rig, &chief_session, "ConsensFlow stopped"),
        0,
        "the chief's turn was over when the app quit: it is told nothing"
    );
    rig.close()?;
    Ok(())
}
