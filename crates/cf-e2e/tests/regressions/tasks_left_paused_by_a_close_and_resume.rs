//! The owner's question of 2026-10-16, on ConsensFlow 3.0.4: **after a Close and a
//! Resume, the tasks stayed paused.** "on resume tasks should resume, right?"
//!
//! What the owner saw: workers were at work on their tasks; the owner closed the
//! project (Close) and pressed Resume. The chief came back, and every task the Close
//! had paused stayed paused until the chief resumed it, one by one. When the app
//! quits instead, the tasks come back at work with the Resume. The answer is the
//! third of the brain's `decisions/2026-10-16-gabriels-answers.md` in consensflow-sme.
//!
//! What the owner should have seen: the tasks the project's suspension paused go on
//! with the Resume, each window on its own conversation, as after a quit: told its
//! turn was cut, once, as the first thing it is given, while the chief comes back on
//! its own. A task paused for another reason (here the human's pause, before the
//! Close) stays paused.
//!
//! The case is that shape on the rig: T-1 and T-2 at work in windows of their own,
//! T-3 paused by the human, then Close and Resume.

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::a_trust_question_after_git_init_stopped_every_launch::{git_init, notes_of, trusted};
use crate::{config, secs, Outcome};

/// The chief's own words of a pause, which a task that goes on never needs.
const PAUSED: &str = "is paused:";

/// The conversation a window was opened on: the word after `flag` in its command line.
fn after<'a>(frame: &'a Value, flag: &str) -> &'a str {
    let argv = frame["argv"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    argv.iter()
        .position(|word| word == flag)
        .and_then(|at| argv.get(at + 1))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// The windows of `pane` the daemon opened so far, oldest first.
fn frames_of(rig: &Rig, pane: &str) -> Vec<Value> {
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == pane)
        .collect()
}

/// How many of the messages a conversation was given say `words`.
fn told(rig: &Rig, session: &str, words: &str) -> usize {
    rig.transcript(session)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .filter_map(|record| record["message"]["content"].as_str().map(str::to_owned))
        .filter(|text| text.contains(words))
        .count()
}

/// How many of the messages a conversation was given tell it its turn on T-`number`
/// was cut: the note's sentence ends after the task, or goes on to name the turn (T-814).
fn told_cut(rig: &Rig, session: &str, number: i64) -> usize {
    [".", ", in the turn that began "]
        .iter()
        .map(|after| {
            told(
                rig,
                session,
                &format!("while you were working on T-{number}{after}"),
            )
        })
        .sum()
}

/// The state of the task `number`.
fn state(project: &Project, number: i64) -> cf_e2e::Result<String> {
    Ok(project.task(number)?["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

/// A task at work in a window of its own: its window's pane, and the
/// conversation that window was opened on.
struct AtWork {
    number: i64,
    pane: String,
    session: String,
}

/// The chief hands out a task that works a long time, and it is at work in a
/// window of its own.
fn at_work(rig: &Rig, project: &Project, tier: &str, number: i64) -> cf_e2e::Result<AtWork> {
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} SLEEP 600 Reply with exactly: WORKED_{number}"),
    )?;
    rig.wait_for(&format!("T-{number} to be at work"), secs(60), || {
        Ok(state(project, number)? == "working")
    })?;
    let assignee = project.task(number)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let pane = format!("p{}-{assignee}", project.id());
    let frame = rig.open_frame(&pane, OPEN)?;
    // Its turn is under way, as the owner's workers' were.
    rig.wait_for(
        &format!("T-{number}'s window to be at work"),
        secs(60),
        || Ok(project.lane(&assignee)?["activity"]["state"] == "working"),
    )?;
    Ok(AtWork {
        number,
        pane,
        session: after(&frame, "--session-id").to_owned(),
    })
}

#[test]
fn a_close_and_a_resume_bring_back_the_tasks_the_close_paused_in_their_own_windows_and_a_task_the_human_paused_stays_paused(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let tier = project.add_member("worker")?["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let chief_session = after(&chief, "--session-id").to_owned();

    let working = [
        at_work(&rig, &project, &tier, 1)?,
        at_work(&rig, &project, &tier, 2)?,
    ];
    let paused = at_work(&rig, &project, &tier, 3)?;
    // The human pauses T-3 before the Close.
    let pause = rig.page("task.pause", json!({ "project": id, "task": 3 }))?;
    assert_eq!(pause["ok"], true, "{pause}");
    assert_eq!(state(&project, 3)?, "paused");

    // The human closes the project: every window goes, and the work in them pauses.
    let closed = rig.page("project.close", json!({ "project": id }))?;
    assert_eq!(closed["ok"], true, "{closed}");
    rig.wait_for("every window of the project to close", secs(60), || {
        let board = project.board()?;
        Ok(board["lanes"]
            .as_array()
            .is_some_and(|lanes| lanes.iter().all(|lane| lane["pane"].is_null())))
    })?;
    for number in [1, 2, 3] {
        assert_eq!(state(&project, number)?, "paused", "T-{number}, closed");
    }
    let before: Vec<usize> = working
        .iter()
        .map(|task| frames_of(&rig, &task.pane).len())
        .collect();

    // The human resumes it: the tasks the Close paused are at work again.
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    for task in &working {
        assert_ne!(
            state(&project, task.number)?,
            "paused",
            "T-{} goes on with the Resume",
            task.number
        );
    }
    assert_eq!(state(&project, 3)?, "paused", "the human's pause stays");

    // Each in its own window, on its own conversation, told once its turn was cut.
    for (task, before) in working.iter().zip(before) {
        rig.wait_for(
            &format!("T-{}'s window to be opened again", task.number),
            secs(60),
            || Ok(frames_of(&rig, &task.pane).len() > before),
        )?;
        let again = frames_of(&rig, &task.pane).remove(before);
        assert_eq!(
            after(&again, "--resume"),
            task.session,
            "T-{} on its own conversation: {:?}",
            task.number,
            again["argv"]
        );
        rig.wait_for(
            &format!("T-{}'s window to be told its turn was cut", task.number),
            secs(60),
            || Ok(told_cut(&rig, &task.session, task.number) == 1),
        )?;
    }
    // The chief came back on its own, and was never told of a pause that no longer is.
    let chief_pane = format!("p{id}-chief");
    rig.wait_for("the chief's window to be opened again", secs(60), || {
        Ok(frames_of(&rig, &chief_pane).len() >= 2)
    })?;
    let chief_again = frames_of(&rig, &chief_pane).remove(1);
    assert_eq!(after(&chief_again, "--resume"), chief_session);
    for task in &working {
        rig.wait_for(&format!("T-{} to be done", task.number), secs(60), || {
            Ok(state(&project, task.number)? == "done")
        })?;
    }
    assert_eq!(
        told(&rig, &chief_session, PAUSED),
        0,
        "the chief was told of a pause: {}",
        rig.transcript(&chief_session)
    );
    for task in &working {
        assert_eq!(told_cut(&rig, &task.session, task.number), 1, "told once");
    }

    // T-3 stays paused, and its window does not come back.
    assert_eq!(state(&project, 3)?, "paused");
    assert_eq!(
        frames_of(&rig, &paused.pane).len(),
        1,
        "T-3's window was opened again"
    );
    rig.close()?;
    Ok(())
}

/// Where the Resume meets folder trust (the merge of T-392): a window the Resume
/// opens again on its conversation may ask whether to trust its folder before
/// anything else, here after `git init` in the project's folder while it was
/// closed. ConsensFlow answers yes in the project's own folder, and the word to go
/// on goes in once the window is past the question, once.
#[test]
fn a_window_the_resume_opens_again_on_the_trust_question_is_answered_and_told_once_its_turn_was_cut(
) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_TRUST", "worker"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let tier = project.add_member("worker")?["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let task = at_work(&rig, &project, &tier, 1)?;
    assert_eq!(
        trusted(&rig),
        "",
        "no question in a folder outside a repository"
    );

    let closed = rig.page("project.close", json!({ "project": id }))?;
    assert_eq!(closed["ok"], true, "{closed}");
    rig.wait_for("every window of the project to close", secs(60), || {
        let board = project.board()?;
        Ok(board["lanes"]
            .as_array()
            .is_some_and(|lanes| lanes.iter().all(|lane| lane["pane"].is_null())))
    })?;
    let before = frames_of(&rig, &task.pane).len();
    git_init(rig.workspace())?;

    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    rig.wait_for("T-1's window to be opened again", secs(60), || {
        Ok(frames_of(&rig, &task.pane).len() > before)
    })?;
    let again = frames_of(&rig, &task.pane).remove(before);
    assert_eq!(
        after(&again, "--resume"),
        task.session,
        "{:?}",
        again["argv"]
    );
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(state(&project, 1)? == "done")
    })?;
    assert_eq!(told_cut(&rig, &task.session, task.number), 1, "told once");
    let same = |line: &str| {
        std::fs::canonicalize(line).ok() == std::fs::canonicalize(rig.workspace()).ok()
    };
    assert!(
        trusted(&rig).lines().any(same),
        "the window was never told yes: {}",
        trusted(&rig)
    );
    let notes = notes_of(&project, rig.workspace())?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    rig.close()?;
    Ok(())
}
