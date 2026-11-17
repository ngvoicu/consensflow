//! The page hears writes made through `cf` even when the engine has nothing to
//! move on its next pass. It reads the board only after the notification.

use std::time::{Duration, Instant};

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{rig, secs, Outcome};

fn cf(rig: &Rig, window: &Value, words: &[&str]) -> cf_e2e::Result<String> {
    let ran = rig.cf_in_window(window, words)?;
    if ran.code == Some(0) {
        Ok(ran.stdout)
    } else {
        Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))))
    }
}

/// The earlier changes and their 100 ms notification have settled before the
/// command under test: a notification from setup must not make the case pass.
fn quiet(rig: &Rig) -> cf_e2e::Result<usize> {
    let mut count = rig.board_moves().len();
    let mut since = Instant::now();
    rig.wait_for("the board notifications to settle", secs(10), || {
        let now = rig.board_moves().len();
        if now != count {
            count = now;
            since = Instant::now();
        }
        Ok(since.elapsed() >= Duration::from_millis(500))
    })?;
    Ok(count)
}

/// The 100 ms throttle plus room for a loaded test machine to schedule the
/// daemon and read its frame. The page reads only after this core event.
fn heard(rig: &Rig, before: usize) -> cf_e2e::Result {
    rig.wait_for("state.changed after the cf write", secs(3), || {
        Ok(rig.board_moves().len() > before)
    })
}

fn message_id(printed: &str) -> cf_e2e::Result<String> {
    printed
        .split_whitespace()
        .find(|word| word.starts_with("m-"))
        .map(str::to_owned)
        .ok_or_else(|| Error::Daemon(format!("no message in: {printed}")))
}

/// T-1's stand-in is on a long turn. It cannot take an answer yet.
fn working<'a>(rig: &'a Rig) -> cf_e2e::Result<(Project<'a>, Value, Value, String)> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or("standard");
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    cf(
        rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            tier,
            "SLEEP 600 Reply with exactly: WORKED",
        ],
    )?;
    rig.wait_for("the worker to be at work", secs(60), || {
        Ok(project.task(1)?["state"] == "working"
            && project.lane("worker")?["activity"]["state"] == "working")
    })?;
    let worker = project.lane("worker")?["participant"]["handle"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let window = rig.open_frame(&format!("p{}-{worker}", project.id()), OPEN)?;
    Ok((project, chief, window, worker))
}

#[test]
fn the_page_hears_the_last_answer_while_it_is_queued_for_a_busy_window() -> Outcome {
    let rig = rig()?;
    let (project, chief, worker_window, worker) = working(&rig)?;
    let first = message_id(&cf(&rig, &worker_window, &["ask", "Which format?"])?)?;
    let second = message_id(&cf(&rig, &worker_window, &["ask", "Which tests?"])?)?;
    rig.wait_for("both questions to reach the chief", secs(60), || {
        Ok(project
            .inbox("chief")?
            .iter()
            .filter(|message| message["kind"] == "question" && message["state"] == "delivered")
            .count()
            == 2)
    })?;
    assert_eq!(project.task(1)?["state"], "waiting");
    cf(&rig, &chief, &["answer", &first, "First answer"])?;
    assert_eq!(project.task(1)?["state"], "waiting", "one question remains");
    let before = quiet(&rig)?;

    cf(&rig, &chief, &["answer", &second, "Second answer"])?;
    assert_eq!(project.task(1)?["state"], "working", "the write committed");
    let queued: Vec<_> = project
        .inbox(&worker)?
        .into_iter()
        .filter(|message| message["kind"] == "answer")
        .collect();
    assert_eq!(queued.len(), 2, "{queued:?}");
    assert!(
        queued.iter().all(|answer| answer["state"] == "queued"),
        "{queued:?}"
    );
    heard(&rig, before)?;
    let lane = project.lane("worker")?; // board.get, the page's read after the event
    let task = lane["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|task| task["number"] == 1))
        .ok_or_else(|| Error::Daemon(format!("T-1 is not in its lane: {lane}")))?;
    assert_eq!(task["state"], "working");
    assert_eq!(
        lane["activity"]["state"], "working",
        "the window is still busy"
    );
    let answers: Vec<_> = project
        .inbox(&worker)?
        .into_iter()
        .filter(|message| message["kind"] == "answer")
        .collect();
    assert_eq!(answers.len(), 2, "{answers:?}");
    assert!(
        answers.iter().all(|answer| answer["state"] == "queued"),
        "{answers:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn the_page_hears_a_tell_added_to_an_open_blocked_tasks_brief() -> Outcome {
    let rig = rig()?;
    let (project, chief, _worker, _) = working(&rig)?;
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            "standard",
            "--needs",
            "T-1",
            "Original brief",
        ],
    )?;
    assert_eq!(project.task(2)?["state"], "open");
    let before = quiet(&rig)?;

    cf(&rig, &chief, &["tell", "T-2", "Added instruction"])?;
    assert!(project.task(2)?["body"]
        .as_str()
        .is_some_and(|body| body.contains("Added instruction")));
    heard(&rig, before)?;
    let board = project.board()?;
    let task = board["open"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|task| task["number"] == 2))
        .ok_or_else(|| Error::Daemon(format!("T-2 is not open on the board: {board}")))?;
    assert_eq!(task["state"], "open");
    // A board card has no body. The open drawer reads task.get after board.get.
    let task = project.task(2)?;
    assert!(
        task["body"]
            .as_str()
            .is_some_and(|body| body.contains("Added instruction")),
        "{task}"
    );
    rig.close()?;
    Ok(())
}
