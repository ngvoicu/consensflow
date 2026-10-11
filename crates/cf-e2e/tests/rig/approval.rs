//! Approval required, end to end through the real pane host: what the daemon
//! types into a window, and when, while the words that were for it wait at the
//! gate. The words that resume a paused task, which the human passes on before
//! or after the window stopped; the result the chief decides on in a turn of
//! its own, or is given once that turn ends; the declines; the pause, the cancel
//! and the reopening of a task whose words still wait; approval switched off;
//! and the daemon restarted. The human's two reports from the live app, which
//! ran Node's engine (alpha.81): the words they passed on that were never typed
//! (m-1930), and a result typed into a chief that had accepted its task
//! (m-1929).
//!
//! The windows are the stand-in agent, which stops a turn at the Escape the
//! daemon presses into it, as Claude does: a turn that works (`SLEEP`) writes
//! `[Request interrupted by user]` and no answer, and one still in the hooks of
//! its prompt (`HOOKS`) writes nothing, which only the key says was stopped.

use std::time::Duration;

use cf_e2e::rig::{write_staff, Project, Rig, OPEN};
use regex::Regex;
use serde_json::{json, Value};

use crate::decided_result::{cf, results};
use crate::{config, rig, secs, session_of, Outcome};

/// What Claude writes of a turn it was interrupted in.
const INTERRUPTED: &str = "[Request interrupted by user]";

/// A project with a chief and one worker, whose human approves every hand-off.
fn gated_project(rig: &Rig) -> cf_e2e::Result<Project<'_>> {
    write_staff(rig)?;
    let project = Project::open_with_staff(rig, "chief", Some(true), &[("worker", "worker")])?;
    assert_eq!(project.board()?["project"]["gate"], true);
    Ok(project)
}

/// The tier the worker has on the board.
fn tier(project: &Project) -> cf_e2e::Result<String> {
    Ok(project.tiers()?.remove("worker").unwrap_or_default())
}

/// What waits at the gate, as the board shows it, oldest first.
fn gated(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    let mut held: Vec<Value> = project.board()?["gated"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    held.sort_by_key(|message| message["id"].as_i64().unwrap_or_default());
    Ok(held)
}

/// The one message at the gate, waited for.
fn the_one_held(rig: &Rig, project: &Project, what: &str) -> cf_e2e::Result<Value> {
    rig.wait_for(what, secs(60), || Ok(gated(project)?.len() == 1))?;
    Ok(gated(project)?.remove(0))
}

/// The human passes a message on, as the page does.
fn approve(rig: &Rig, id: &Value) -> cf_e2e::Result<()> {
    let passed = rig.page("message.approve", json!({ "message": id }))?;
    assert_eq!(passed["ok"], true, "{passed}");
    Ok(())
}

/// What the page says when the human declines a message.
fn decline(rig: &Rig, id: &Value) -> cf_e2e::Result<Value> {
    rig.page("message.decline", json!({ "message": id }))
}

/// The chief's window, and the participant its `cf` speaks as.
fn chief_window(rig: &Rig, project: &Project) -> cf_e2e::Result<Value> {
    rig.open_frame(&format!("p{}-chief", project.id()), OPEN)
}

/// The worker's window of T-1: its pane's id, and the conversation it shows.
struct Worker {
    pane: String,
    session: String,
}

/// T-1 given to the worker with `turn` as its words, its brief passed on and
/// received: the window is at work on it.
fn at_work_on(rig: &Rig, project: &Project, turn: &str) -> cf_e2e::Result<Worker> {
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} {turn}", tier(project)?),
    )?;
    let brief = the_one_held(rig, project, "the brief to be held")?;
    assert_eq!(brief["kind"], "task");
    approve(rig, &brief["id"])?;
    let pane = format!(
        "p{}-{}",
        project.id(),
        brief["recipient"].as_str().unwrap_or_default()
    );
    let frame = rig.open_frame(&pane, secs(60))?;
    rig.wait_for("the window to be at work on T-1", secs(60), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    Ok(Worker {
        pane,
        session: session_of(&frame),
    })
}

/// What the worker's window wrote of its conversation, one record a line.
fn transcript(rig: &Rig, worker: &Worker) -> String {
    rig.transcript(&worker.session)
}

/// What a window was given, as its conversation records it: each message a
/// user's turn holds, oldest first. The stand-in answers a message by saying
/// its first line, so what it was given is read from these and not from the
/// whole conversation.
fn given_to(rig: &Rig, session: &str) -> Vec<String> {
    rig.transcript(session)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .filter_map(|record| record["message"]["content"].as_str().map(str::to_owned))
        .collect()
}

/// How many of the messages the worker's window was given say `words`.
fn said(rig: &Rig, worker: &Worker, words: &str) -> usize {
    given_to(rig, &worker.session)
        .iter()
        .filter(|text| text.contains(words))
        .count()
}

/// The lane of the window `pane` is the pane of, by the handle in its name.
fn activity(project: &Project, handle: &str) -> cf_e2e::Result<Value> {
    Ok(project.lane(handle)?["activity"]["state"].clone())
}

/// The worker's handle: its pane's name after the project's.
fn handle_of<'a>(project: &Project, worker: &'a Worker) -> &'a str {
    worker
        .pane
        .strip_prefix(&format!("p{}-", project.id()))
        .unwrap_or_default()
}

/// The chief pauses T-1 and, a moment later, resumes it with `words`: what it
/// puts at the gate is the one message there.
fn paused_and_resumed(
    rig: &Rig,
    project: &Project,
    chief: &Value,
    words: &str,
) -> cf_e2e::Result<Value> {
    cf(rig, chief, &["task", "pause", "T-1"])?;
    cf(rig, chief, &["task", "resume", "T-1", words])?;
    assert_eq!(
        project.task(1)?["state"],
        "queued",
        "the task is the chief's to go on with: it waits for the human's words"
    );
    let held = the_one_held(rig, project, "the words that resume it to be held")?;
    assert_eq!(
        [held["kind"].clone(), held["sender"].clone()],
        [json!("task"), json!("chief")]
    );
    assert!(
        held["body"]
            .as_str()
            .is_some_and(|body| body.ends_with(&format!("Resumed: {words}"))),
        "{held}"
    );
    Ok(held)
}

/// How many Escapes the daemon pressed into the worker's window so far. One
/// round stops it, and a second may come on a machine that is slow to show it
/// at rest, so that is what is held, and not exactly one.
fn pressed(rig: &Rig, worker: &Worker) -> usize {
    let keys = escapes(rig, worker);
    assert!((1..=2).contains(&keys), "{keys} Escapes were pressed");
    keys
}

/// How many Escapes the daemon pressed into the worker's window so far, whatever it is.
fn escapes(rig: &Rig, worker: &Worker) -> usize {
    rig.keys_pressed(&worker.pane, &[27])
}

/// What the human's passing the resumed words on comes to once the window has
/// stopped: its turn is over, it is given the words, once, and the task is
/// working again.
fn goes_on_with(rig: &Rig, project: &Project, worker: &Worker, words: &str) -> Outcome {
    rig.wait_for("the window to be given the words", secs(60), || {
        Ok(said(rig, worker, &format!("Resumed: {words}")) == 1)
    })?;
    rig.wait_for("the task to be working again", secs(60), || {
        Ok(project.task(1)?["state"] == "working" || project.task(1)?["state"] == "done")
    })?;
    assert_eq!(
        said(rig, worker, &format!("Resumed: {words}")),
        1,
        "the words were typed once"
    );
    Ok(())
}

#[test]
fn words_that_resume_a_task_paused_mid_turn_wait_for_the_human_and_go_in_once_the_window_stopped_and_they_are_passed_on(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    let worker = at_work_on(&rig, &project, "SLEEP 60 Reply with exactly: ONE_DONE")?;
    let words = "Reply with exactly: RESUMED_OK";

    let held = paused_and_resumed(&rig, &project, &chief, words)?;
    // The window is interrupted, writes what Claude writes, and rests, and
    // the words are still the human's to pass on: nothing is typed meanwhile.
    rig.wait_for("the interrupt to end its turn", secs(30), || {
        Ok(transcript(&rig, &worker).contains(INTERRUPTED))
    })?;
    rig.wait_for("the window to be seen at rest", secs(30), || {
        Ok(activity(&project, handle_of(&project, &worker))? == "idle")
    })?;
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(said(&rig, &worker, "Resumed:"), 0, "nothing typed yet");
    let at_rest = pressed(&rig, &worker);
    assert_eq!(project.task(1)?["state"], "queued");

    // The human passes them on, and the window is given them at once.
    approve(&rig, &held["id"])?;
    goes_on_with(&rig, &project, &worker, words)?;
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(escapes(&rig, &worker), at_rest, "no key went in with them");
    // The task ends with what the words asked for.
    let result = the_one_held(&rig, &project, "the result to be held")?;
    assert_eq!(
        [result["kind"].clone(), result["body"].clone()],
        [json!("result"), json!("RESUMED_OK")]
    );
    // One window, the same one: the chief's and the worker's.
    assert_eq!(rig.started().len(), 2);
    rig.close()?;
    Ok(())
}

#[test]
fn words_the_human_passes_on_before_the_window_has_stopped_wait_for_it_and_go_in_once_it_has(
) -> Outcome {
    // The window takes two seconds to wind down once it is interrupted: longer
    // than the human's passing the words on, and shorter than the three seconds
    // after which the daemon presses the key again.
    let rig = Rig::start(config().var("CF_TEST_WIND_DOWN_MS", "2000"))?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    let worker = at_work_on(&rig, &project, "SLEEP 60 Reply with exactly: ONE_DONE")?;
    let words = "Reply with exactly: RESUMED_OK";

    let held = paused_and_resumed(&rig, &project, &chief, words)?;
    rig.wait_for("the daemon to press Escape", secs(30), || {
        Ok(escapes(&rig, &worker) >= 1)
    })?;
    // The key is pressed, and the window has not stopped: the human passes the words on now.
    assert!(
        !transcript(&rig, &worker).contains(INTERRUPTED),
        "the window is still winding down"
    );
    approve(&rig, &held["id"])?;
    assert_eq!(said(&rig, &worker, "Resumed:"), 0);

    goes_on_with(&rig, &project, &worker, words)?;
    assert!(transcript(&rig, &worker).contains(INTERRUPTED));
    pressed(&rig, &worker);
    let result = the_one_held(&rig, &project, "the result to be held")?;
    assert_eq!(result["body"], "RESUMED_OK");
    rig.close()?;
    Ok(())
}

#[test]
fn a_turn_stopped_before_its_first_word_leaves_no_record_and_the_brief_goes_in_again_with_the_words_the_human_passes_on(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    // The brief is in the hooks of its prompt: Claude's status reads idle, and
    // nothing is written of the turn but the message it was given.
    let worker = at_work_on(&rig, &project, "HOOKS 60 Reply with exactly: ONE_DONE")?;
    let words = "Reply with exactly: RESUMED_OK";

    let held = paused_and_resumed(&rig, &project, &chief, words)?;
    rig.wait_for("the daemon to press Escape", secs(30), || {
        Ok(escapes(&rig, &worker) >= 1)
    })?;
    // The turn left no record of its end, and only the key says it stopped: a
    // look a second after it reads the window at rest, and its brief taken back.
    rig.wait_for("the window to be seen at rest", secs(30), || {
        Ok(activity(&project, handle_of(&project, &worker))? == "idle")
    })?;
    assert!(
        !transcript(&rig, &worker).contains(INTERRUPTED),
        "Claude wrote nothing of it"
    );
    assert_eq!(said(&rig, &worker, "Resumed:"), 0);
    let at_rest = pressed(&rig, &worker);

    approve(&rig, &held["id"])?;
    goes_on_with(&rig, &project, &worker, words)?;
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(escapes(&rig, &worker), at_rest, "no key went in with them");
    // The window, read at rest by the key, had its brief in the input box again:
    // the daemon cleared it with Control-C before it pasted the words.
    assert!(
        rig.keys_pressed(&worker.pane, &[3]) >= 1,
        "the input box was cleared"
    );
    assert_eq!(
        rig.started().len(),
        2,
        "the Control-C that clears the input box did not end the window: the chief's and the worker's are the only two"
    );
    // The brief that was not received after all came in the same paste.
    let pasted = given_to(&rig, &worker.session);
    assert_eq!(saying(&pasted, "Resumed:"), 1);
    assert!(
        pasted
            .iter()
            .any(|text| text.contains("Resumed:") && text.contains("HOOKS 60")),
        "the words carry the brief: {pasted:?}"
    );
    rig.close()?;
    Ok(())
}

/// The results in the chief's inbox, oldest first, as `decided_result` reads them.
fn the_result(project: &Project) -> cf_e2e::Result<Value> {
    Ok(results(project)?.remove(0))
}

/// Waits until the chief's turn ends.
fn until_the_chief_is_idle(rig: &Rig, project: &Project) -> cf_e2e::Result<()> {
    rig.wait_for("the chief's turn to end", secs(90), || {
        Ok(activity(project, "chief")? == "idle")
    })
}

/// What the chief's window was given.
fn chief_was_given(rig: &Rig, chief: &Value) -> Vec<String> {
    given_to(rig, &session_of(chief))
}

/// How many messages `given` has that are the message `id`: its header opens them.
fn headed(given: &[String], id: impl std::fmt::Display) -> usize {
    given
        .iter()
        .filter(|text| text.starts_with(&format!("[ConsensFlow m-{id} ")))
        .count()
}

/// How many messages `given` has that say `words`.
fn saying(given: &[String], words: &str) -> usize {
    given.iter().filter(|text| text.contains(words)).count()
}

/// T-1 for the worker, whose turn is a few seconds, and the chief in a turn of
/// its own that is longer: the result the worker writes waits at the gate, and
/// is passed on by the human while the chief is in its turn. The result, as the
/// chief's inbox has it, queued behind the turn.
fn passed_on_in_the_chiefs_turn(rig: &Rig, project: &Project) -> cf_e2e::Result<Value> {
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 4 Reply with exactly: ONE_DONE",
            tier(project)?
        ),
    )?;
    let brief = the_one_held(rig, project, "the brief to be held")?;
    approve(rig, &brief["id"])?;
    rig.tell(project.id(), "SLEEP 40 Reply with exactly: CHIEF_BUSY")?;
    let held = the_one_held(rig, project, "T-1's result to be held")?;
    assert_eq!(
        [held["kind"].clone(), held["body"].clone()],
        [json!("result"), json!("ONE_DONE")]
    );
    assert_eq!(
        activity(project, "chief")?,
        "working",
        "the chief is in its turn"
    );
    assert_eq!(
        results(project)?.len(),
        0,
        "not in its inbox while it is held"
    );
    approve(rig, &held["id"])?;
    let result = the_result(project)?;
    assert_eq!(
        result["state"], "queued",
        "it waits behind the chief's turn"
    );
    Ok(result)
}

#[test]
fn a_result_the_human_passed_on_while_the_chief_was_in_a_turn_is_withdrawn_when_the_chief_read_it_and_accepted_its_task_in_that_turn(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    let result = passed_on_in_the_chiefs_turn(&rig, &project)?;
    let id = result["id"].as_i64().unwrap_or_default();

    // In its turn the chief reads it whole, which receives nothing, and accepts the task.
    let read = cf(&rig, &chief, &["inbox", "read", &format!("m-{id}")])?;
    assert!(read.contains("ONE_DONE"), "{read}");
    assert_eq!(
        the_result(&project)?["state"],
        "queued",
        "a read receives nothing"
    );
    cf(&rig, &chief, &["task", "accept", "T-1"])?;
    let withdrawn = the_result(&project)?;
    assert_eq!(
        [withdrawn["state"].clone(), withdrawn["reason"].clone()],
        [json!("cancelled"), json!("T-1 was accepted")]
    );

    // Its turn ends, and nothing is typed into it: not the result, not what it asks the chief to decide.
    until_the_chief_is_idle(&rig, &project)?;
    std::thread::sleep(Duration::from_secs(3));
    let given = chief_was_given(&rig, &chief);
    assert_eq!(
        headed(&given, id),
        0,
        "the chief was given the result it had decided on: {given:?}"
    );
    assert_eq!(saying(&given, "Decide with: cf task accept T-1"), 0);
    assert_eq!(project.task(1)?["state"], "accepted");
    rig.close()?;
    Ok(())
}

#[test]
fn a_result_the_human_passed_on_while_the_chief_was_in_a_turn_reaches_it_once_when_its_turn_ends(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    let result = passed_on_in_the_chiefs_turn(&rig, &project)?;
    let id = result["id"].as_i64().unwrap_or_default();
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        the_result(&project)?["state"],
        "queued",
        "still behind the turn, whatever the passes"
    );

    until_the_chief_is_idle(&rig, &project)?;
    rig.wait_for("the result to be delivered", secs(60), || {
        Ok(the_result(&project)?["state"] == "delivered")
    })?;
    // Passes later find nothing more to give.
    std::thread::sleep(Duration::from_secs(3));
    let given = chief_was_given(&rig, &chief);
    assert_eq!(headed(&given, id), 1, "once: {given:?}");
    assert_eq!(
        saying(&given, "Decide with: cf task accept T-1"),
        1,
        "with the way to decide"
    );
    assert_eq!(project.task(1)?["state"], "done", "undecided");
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_that_accepts_a_task_whose_result_waits_at_the_gate_is_refused_and_is_given_the_result_once_it_is_passed_on(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: ONE_DONE",
            tier(&project)?
        ),
    )?;
    let brief = the_one_held(&rig, &project, "the brief to be held")?;
    approve(&rig, &brief["id"])?;
    let result = the_one_held(&rig, &project, "the result to be held")?;
    assert_eq!(result["kind"], "result");

    // What the human has not passed on is not the chief's to decide.
    let accepted = rig.cf_in_window(&chief, ["task", "accept", "T-1"])?;
    let reopened = rig.cf_in_window(&chief, ["task", "reopen", "T-1", "Again"])?;
    for ran in [&accepted, &reopened] {
        assert_ne!(ran.code, Some(0), "{ran}");
        assert!(
            ran.stderr.contains(
                "T-1's result waits for the human's approval: it reaches you once they pass it on"
            ),
            "{ran}"
        );
    }
    let read = rig.cf_in_window(&chief, ["inbox", "read", &format!("m-{}", result["id"])])?;
    assert_ne!(
        read.code,
        Some(0),
        "the chief cannot read it either: {read}"
    );
    let thread = cf(&rig, &chief, &["task", "get", "T-1"])?;
    assert!(
        !thread.contains(" result T-1 from @"),
        "its thread leaves out what waits: {thread}"
    );
    // The human sees it still waiting, in the thread and at the gate, and
    // cannot decline a result: only pass it on.
    assert_eq!(project.task(1)?["state"], "done");
    assert!(project.task(1)?["messages"]
        .as_array()
        .is_some_and(|messages| messages
            .iter()
            .any(|message| message["id"] == result["id"] && message["state"] == "gated")));
    assert_eq!(gated(&project)?.len(), 1);
    let declined = decline(&rig, &result["id"])?;
    assert_eq!(declined["ok"], false, "{declined}");
    assert!(
        declined["error"]
            .as_str()
            .is_some_and(|error| error.contains("a result is passed on, not declined")),
        "{declined}"
    );
    assert_eq!(gated(&project)?.len(), 1, "still the human's to pass on");

    approve(&rig, &result["id"])?;
    rig.wait_for("the result to be delivered", secs(60), || {
        Ok(the_result(&project)?["state"] == "delivered")
    })?;
    let given = chief_was_given(&rig, &chief);
    assert_eq!(saying(&given, "Decide with: cf task accept T-1"), 1);
    cf(&rig, &chief, &["task", "accept", "T-1"])?;
    assert_eq!(project.task(1)?["state"], "accepted");
    assert_eq!(
        the_result(&project)?["state"],
        "delivered",
        "given stays given"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_declined_brief_cancels_its_task_and_tells_the_chief_and_no_window_opens_for_it() -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: NEVER",
            tier(&project)?
        ),
    )?;
    let brief = the_one_held(&rig, &project, "the brief to be held")?;
    let declined = decline(&rig, &brief["id"])?;
    assert_eq!(declined["ok"], true, "{declined}");
    assert_eq!(project.task(1)?["state"], "cancelled");
    assert!(gated(&project)?.is_empty());

    let told = Regex::new(r"^@human declined T-1 \(.*\)\. It is cancelled\.$")?;
    rig.wait_for("the chief to be told", secs(60), || {
        Ok(project.inbox("chief")?.iter().any(|message| {
            message["kind"] == "note"
                && message["state"] == "delivered"
                && told.is_match(message["body"].as_str().unwrap_or_default())
        }))
    })?;
    assert_eq!(saying(&chief_was_given(&rig, &chief), "declined T-1"), 1);
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !rig.open_frames().iter().any(|frame| frame["id"]
            .as_str()
            .is_some_and(|id| id.starts_with(&format!("p{}-worker", project.id())))),
        "no window opened for the task the human declined"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_declined_answer_leaves_the_question_open_and_the_chief_answers_it_again() -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    // The worker asks the chief, who answers what it was asked to reply with.
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} CF ask :: Which format? REPLY JSON",
            tier(&project)?
        ),
    )?;
    let brief = the_one_held(&rig, &project, "the brief to be held")?;
    approve(&rig, &brief["id"])?;
    let question = the_one_held(&rig, &project, "the question to be held")?;
    assert_eq!(question["kind"], "question");
    let asked = question["id"].as_i64().unwrap_or_default();
    // A question is passed on and never declined.
    let refused = decline(&rig, &question["id"])?;
    assert_eq!(refused["ok"], false, "{refused}");
    approve(&rig, &question["id"])?;
    let answer = the_one_held(&rig, &project, "the chief's answer to be held")?;
    assert_eq!(
        [
            answer["kind"].clone(),
            answer["sender"].clone(),
            answer["body"].clone()
        ],
        [json!("answer"), json!("chief"), json!("JSON")]
    );
    assert_eq!(project.task(1)?["state"], "waiting");

    // The human declines it: the question is the chief's to answer still.
    let declined = decline(&rig, &answer["id"])?;
    assert_eq!(declined["ok"], true, "{declined}");
    assert!(gated(&project)?.is_empty());
    assert_eq!(
        project.task(1)?["state"],
        "waiting",
        "the task waits for another answer"
    );
    let thread = project.task(1)?["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let state_of = |id: i64| {
        thread
            .iter()
            .find(|message| message["id"] == id)
            .map_or(Value::Null, |message| message["state"].clone())
    };
    assert_eq!(state_of(asked), "delivered", "the question stands");
    assert_eq!(
        state_of(answer["id"].as_i64().unwrap_or_default()),
        "cancelled"
    );
    let told = format!(
        "@human declined your answer to m-{asked}. Answer it again: cf answer m-{asked} \"…\""
    );
    rig.wait_for("the chief to be told to answer again", secs(60), || {
        Ok(project
            .inbox("chief")?
            .iter()
            .any(|message| message["state"] == "delivered" && message["body"] == told.as_str()))
    })?;
    assert_eq!(saying(&chief_was_given(&rig, &chief), &told), 1);

    // It does, and the answer waits at the gate again; passed on, it reaches the worker.
    cf(&rig, &chief, &["answer", &format!("m-{asked}"), "YAML"])?;
    let again = the_one_held(&rig, &project, "the new answer to be held")?;
    assert_eq!(again["body"], "YAML");
    approve(&rig, &again["id"])?;
    rig.wait_for("the worker to be given the answer", secs(60), || {
        Ok(project.task(1)?["messages"]
            .as_array()
            .is_some_and(|messages| {
                messages
                    .iter()
                    .any(|message| message["id"] == again["id"] && message["state"] == "delivered")
            }))
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_note_a_member_writes_waits_at_the_gate_the_human_can_only_pass_it_on_and_so_a_result(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} CF note :: Mind the tests",
            tier(&project)?
        ),
    )?;
    let brief = the_one_held(&rig, &project, "the brief to be held")?;
    approve(&rig, &brief["id"])?;
    rig.wait_for("the note and the result to be held", secs(60), || {
        Ok(gated(&project)?.len() == 2)
    })?;
    let held = gated(&project)?;
    assert_eq!(
        [held[0]["kind"].clone(), held[1]["kind"].clone()],
        [json!("note"), json!("result")]
    );
    for message in &held {
        let refused = decline(&rig, &message["id"])?;
        assert_eq!(refused["ok"], false, "{refused}");
    }
    assert_eq!(
        gated(&project)?.len(),
        2,
        "both are still the human's to pass on"
    );
    let note = held[0]["id"].as_i64().unwrap_or_default();
    assert_eq!(
        headed(&chief_was_given(&rig, &chief), note),
        0,
        "the chief is given nothing while the human has not passed it on"
    );
    approve(&rig, &held[0]["id"])?;
    rig.wait_for("the note to reach the chief", secs(60), || {
        Ok(headed(&chief_was_given(&rig, &chief), note) == 1)
    })?;
    assert_eq!(
        saying(&chief_was_given(&rig, &chief), "note from @worker-"),
        1
    );
    assert_eq!(gated(&project)?.len(), 1, "the result still waits");
    assert_eq!(results(&project)?.len(), 0);
    rig.close()?;
    Ok(())
}

#[test]
fn declined_words_that_resume_a_paused_task_cancel_it_and_nothing_is_typed_into_its_window(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    let worker = at_work_on(&rig, &project, "SLEEP 60 Reply with exactly: ONE_DONE")?;
    let held = paused_and_resumed(&rig, &project, &chief, "Reply with exactly: RESUMED_OK")?;
    rig.wait_for("the window to be seen at rest", secs(30), || {
        Ok(transcript(&rig, &worker).contains(INTERRUPTED)
            && activity(&project, handle_of(&project, &worker))? == "idle")
    })?;
    let declined = decline(&rig, &held["id"])?;
    assert_eq!(declined["ok"], true, "{declined}");
    assert_eq!(project.task(1)?["state"], "cancelled");
    assert!(gated(&project)?.is_empty());

    // The window of a task that is over closes, and was never given the words.
    rig.wait_for("the window to close", secs(60), || {
        Ok(project.lane(handle_of(&project, &worker))?["pane"] == Value::Null)
    })?;
    assert_eq!(said(&rig, &worker, "Resumed:"), 0);
    let told = Regex::new(r"^@human declined T-1 \(.*\)\. It is cancelled\.$")?;
    rig.wait_for("the chief to be told", secs(60), || {
        Ok(project.inbox("chief")?.iter().any(|message| {
            message["kind"] == "note" && told.is_match(message["body"].as_str().unwrap_or_default())
        }))
    })?;
    rig.close()?;
    Ok(())
}

/// Whether a window opened for a member of the project's staff: any of the
/// worker's sessions.
fn a_worker_window_opened(rig: &Rig, project: &Project) -> bool {
    rig.open_frames().iter().any(|frame| {
        frame["id"]
            .as_str()
            .is_some_and(|id| id.starts_with(&format!("p{}-worker", project.id())))
    })
}

/// What the thread of T-1 says of the message `id`: its state and its reason.
fn fate_of(project: &Project, id: &Value) -> cf_e2e::Result<[Value; 2]> {
    let thread = project.task(1)?["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let message = thread.iter().find(|message| message["id"] == *id);
    Ok(message.map_or([Value::Null, Value::Null], |message| {
        [message["state"].clone(), message["reason"].clone()]
    }))
}

#[test]
fn a_pause_of_a_task_whose_brief_waits_at_the_gate_takes_the_brief_back_and_the_window_opens_only_for_the_words_that_resume_it(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: ONE_DONE",
            tier(&project)?
        ),
    )?;
    let brief = the_one_held(&rig, &project, "the brief to be held")?;

    // The chief pauses it: it takes its own words back, and there is nothing to pass on.
    cf(&rig, &chief, &["task", "pause", "T-1"])?;
    assert!(gated(&project)?.is_empty());
    assert_eq!(project.task(1)?["state"], "paused");
    assert_eq!(
        fate_of(&project, &brief["id"])?,
        [json!("cancelled"), json!("withdrawn by @chief's pause")]
    );
    let refused = rig.page("message.approve", json!({ "message": brief["id"] }))?;
    assert_eq!(refused["ok"], false, "{refused}");

    // Its words wait at the gate, with the brief in front of them, and no window has opened.
    cf(
        &rig,
        &chief,
        &["task", "resume", "T-1", "Reply with exactly: RESUMED_OK"],
    )?;
    let words = the_one_held(&rig, &project, "the words to be held")?;
    assert_eq!(
        words["body"],
        "Reply with exactly: ONE_DONE\n\nResumed: Reply with exactly: RESUMED_OK"
    );
    std::thread::sleep(Duration::from_secs(2));
    assert!(!a_worker_window_opened(&rig, &project));

    approve(&rig, &words["id"])?;
    let result = the_one_held(&rig, &project, "the result to be held")?;
    assert_eq!(result["kind"], "result");
    assert!(a_worker_window_opened(&rig, &project));
    rig.close()?;
    Ok(())
}

#[test]
fn cancelling_a_task_whose_brief_waits_at_the_gate_withdraws_it_and_no_window_ever_opens() -> Outcome
{
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: NEVER",
            tier(&project)?
        ),
    )?;
    let brief = the_one_held(&rig, &project, "the brief to be held")?;
    cf(&rig, &chief, &["task", "cancel", "T-1"])?;
    assert!(gated(&project)?.is_empty());
    assert_eq!(project.task(1)?["state"], "cancelled");
    assert_eq!(
        fate_of(&project, &brief["id"])?,
        [json!("cancelled"), json!("cancelled by @chief")]
    );
    let refused = rig.page("message.approve", json!({ "message": brief["id"] }))?;
    assert_eq!(refused["ok"], false, "{refused}");
    std::thread::sleep(Duration::from_secs(3));
    assert!(!a_worker_window_opened(&rig, &project));
    rig.close()?;
    Ok(())
}

#[test]
fn cancelling_a_task_whose_resume_words_wait_at_the_gate_withdraws_them_and_nothing_is_typed_after(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    let worker = at_work_on(&rig, &project, "SLEEP 60 Reply with exactly: ONE_DONE")?;
    let held = paused_and_resumed(&rig, &project, &chief, "Reply with exactly: RESUMED_OK")?;
    rig.wait_for("the window to be seen at rest", secs(30), || {
        Ok(transcript(&rig, &worker).contains(INTERRUPTED)
            && activity(&project, handle_of(&project, &worker))? == "idle")
    })?;
    cf(&rig, &chief, &["task", "cancel", "T-1"])?;
    assert!(gated(&project)?.is_empty());
    assert_eq!(
        fate_of(&project, &held["id"])?,
        [json!("cancelled"), json!("cancelled by @chief")]
    );
    let refused = rig.page("message.approve", json!({ "message": held["id"] }))?;
    assert_eq!(refused["ok"], false, "{refused}");
    rig.wait_for("the window to close", secs(60), || {
        Ok(project.lane(handle_of(&project, &worker))?["pane"] == Value::Null)
    })?;
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(said(&rig, &worker, "Resumed:"), 0, "never typed");
    rig.close()?;
    Ok(())
}

#[test]
fn words_that_send_a_finished_task_back_wait_at_the_gate_again_and_the_window_comes_back_for_them_once_they_are_passed_on(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: ONE_DONE",
            tier(&project)?
        ),
    )?;
    let brief = the_one_held(&rig, &project, "the brief to be held")?;
    approve(&rig, &brief["id"])?;
    let result = the_one_held(&rig, &project, "the result to be held")?;
    approve(&rig, &result["id"])?;
    rig.wait_for("the result to be delivered", secs(60), || {
        Ok(the_result(&project)?["state"] == "delivered")
    })?;
    rig.wait_for("the worker's window to close", secs(60), || {
        Ok(project.lane("worker")?["pane"] == Value::Null)
    })?;
    let started = rig.started().len();

    // The chief sends it back: its words wait at the gate, and no window comes back for them.
    cf(
        &rig,
        &chief,
        &["task", "reopen", "T-1", "Reply with exactly: AGAIN_OK"],
    )?;
    let words = the_one_held(&rig, &project, "the words to be held")?;
    assert_eq!(words["kind"], "task");
    assert_eq!(project.task(1)?["state"], "queued");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(rig.started().len(), started, "no window came back");

    // The human passes them on: the window comes back on its own conversation, and ends the task again.
    approve(&rig, &words["id"])?;
    rig.wait_for("the window to come back", secs(60), || {
        Ok(rig.started().len() == started + 1)
    })?;
    let again = the_one_held(&rig, &project, "the new result to be held")?;
    assert_eq!(
        [again["kind"].clone(), again["body"].clone()],
        [json!("result"), json!("AGAIN_OK")]
    );
    rig.close()?;
    Ok(())
}

#[test]
fn declined_words_that_send_a_finished_task_back_cancel_it_and_the_window_does_not_come_back(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: ONE_DONE",
            tier(&project)?
        ),
    )?;
    let brief = the_one_held(&rig, &project, "the brief to be held")?;
    approve(&rig, &brief["id"])?;
    let result = the_one_held(&rig, &project, "the result to be held")?;
    approve(&rig, &result["id"])?;
    rig.wait_for("the worker's window to close", secs(60), || {
        Ok(the_result(&project)?["state"] == "delivered"
            && project.lane("worker")?["pane"] == Value::Null)
    })?;
    let started = rig.started().len();

    cf(
        &rig,
        &chief,
        &["task", "reopen", "T-1", "Reply with exactly: AGAIN_OK"],
    )?;
    let words = the_one_held(&rig, &project, "the words to be held")?;
    let declined = decline(&rig, &words["id"])?;
    assert_eq!(declined["ok"], true, "{declined}");
    assert_eq!(project.task(1)?["state"], "cancelled");
    assert!(gated(&project)?.is_empty());
    let told = Regex::new(r"^@human declined T-1 \(.*\)\. It is cancelled\.$")?;
    rig.wait_for("the chief to be told", secs(60), || {
        Ok(project.inbox("chief")?.iter().any(|message| {
            message["kind"] == "note" && told.is_match(message["body"].as_str().unwrap_or_default())
        }))
    })?;
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(rig.started().len(), started, "no window came back");
    rig.close()?;
    Ok(())
}

#[test]
fn a_tell_stops_the_window_at_work_and_waits_at_the_gate_the_human_passes_it_on_and_cannot_decline_it(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let chief = chief_window(&rig, &project)?;
    let worker = at_work_on(&rig, &project, "SLEEP 60 Reply with exactly: ONE_DONE")?;
    cf(
        &rig,
        &chief,
        &["tell", "T-1", "Reply with exactly: TOLD_OK"],
    )?;
    let told = the_one_held(&rig, &project, "the tell to be held")?;
    assert_eq!(
        [told["kind"].clone(), told["urgent"].clone()],
        [json!("question"), json!(true)]
    );
    assert_eq!(project.task(1)?["state"], "paused", "the tell paused it");
    // The window stops all the same, and rests; the tell is the human's to pass on.
    rig.wait_for("the window to be seen at rest", secs(30), || {
        Ok(transcript(&rig, &worker).contains(INTERRUPTED)
            && activity(&project, handle_of(&project, &worker))? == "idle")
    })?;
    let refused = decline(&rig, &told["id"])?;
    assert_eq!(refused["ok"], false, "{refused}");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(said(&rig, &worker, "TOLD_OK"), 0, "nothing typed yet");

    approve(&rig, &told["id"])?;
    rig.wait_for("the window to be given the tell", secs(60), || {
        Ok(said(&rig, &worker, "TOLD_OK") == 1)
    })?;
    pressed(&rig, &worker);
    assert_eq!(project.task(1)?["state"], "paused", "the chief resumes it");
    rig.close()?;
    Ok(())
}

#[test]
fn the_handoff_a_switched_chief_is_given_is_not_held_for_the_human() -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    let first = chief_window(&rig, &project)?;
    let id = project.id();
    // The handoff is made of what the first chief had said and heard: it has to have some.
    rig.tell(id, "Reply with exactly: BEFORE_THE_SWITCH")?;
    rig.wait_for("the first chief to answer", secs(30), || {
        Ok(rig
            .transcript(&session_of(&first))
            .contains("\"text\":\"BEFORE_THE_SWITCH\""))
    })?;
    let switched = rig.page("chief.switch", json!({ "project": id, "agent": "worker" }))?;
    assert_eq!(switched["ok"], true, "{switched}");
    let chiefs = || -> Vec<Value> {
        rig.open_frames()
            .into_iter()
            .filter(|frame| frame["id"] == format!("p{id}-chief").as_str())
            .collect()
    };
    rig.wait_for("a second chief window to be opened", secs(30), || {
        Ok(chiefs().len() == 2)
    })?;
    let second = chiefs()[1].clone();
    assert_ne!(session_of(&second), session_of(&first));
    rig.wait_for("the new chief to be told it is the chief", secs(30), || {
        Ok(saying(
            &given_to(&rig, &session_of(&second)),
            "You are the chief now",
        ) == 1)
    })?;
    assert!(gated(&project)?.is_empty(), "nothing waited for the human");
    rig.close()?;
    Ok(())
}

#[test]
fn switching_approval_off_leaves_what_waits_at_the_gate_there_and_what_comes_after_is_not_held(
) -> Outcome {
    let rig = rig()?;
    let project = gated_project(&rig)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: ONE_DONE",
            tier(&project)?
        ),
    )?;
    let brief = the_one_held(&rig, &project, "the brief to be held")?;
    let off = rig.page(
        "project.gate",
        json!({ "project": project.id(), "gate": false }),
    )?;
    assert_eq!(off["ok"], true, "{off}");
    assert_eq!(project.board()?["project"]["gate"], false);
    // What was held is still the human's to decide.
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(gated(&project)?.len(), 1);
    assert_eq!(gated(&project)?[0]["id"], brief["id"]);
    assert!(!a_worker_window_opened(&rig, &project));

    // Passed on, the task goes on, and what the worker writes is not held: the
    // chief is given the result without the human.
    approve(&rig, &brief["id"])?;
    rig.wait_for("the result to be delivered", secs(60), || {
        Ok(results(&project)?
            .first()
            .is_some_and(|result| result["state"] == "delivered"))
    })?;
    assert!(gated(&project)?.is_empty());
    assert_eq!(project.task(1)?["state"], "done");
    rig.close()?;
    Ok(())
}

/// The app quits as it does, the daemon dying first and then the pane host, and
/// is started again on the same home: the rig it is now.
fn quit_and_start_again(mut rig: Rig) -> cf_e2e::Result<Rig> {
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    Rig::start(config().existing_root(root))
}

/// What the human finds when the app is back, and does: the chief's window is
/// resumed and at rest, the brief that waited at the gate is still there and
/// waits, passed on it goes to a window, and its result waits at the gate too
/// and reaches the chief once passed on.
fn goes_on_after_the_restart(rig: &Rig, id: i64, brief: &Value) -> cf_e2e::Result<()> {
    rig.open_frame(&format!("p{id}-chief"), secs(30))?;
    let project = Project::new(rig, id);
    rig.wait_for("the resumed chief to be at rest", secs(30), || {
        Ok(activity(&project, "chief")? == "idle")
    })?;
    assert_eq!(project.board()?["project"]["gate"], true);
    let held = the_one_held(rig, &project, "the brief to be held after the restart")?;
    assert_eq!(held["id"], brief["id"], "the same brief, still waiting");
    std::thread::sleep(Duration::from_secs(3));
    assert!(!a_worker_window_opened(rig, &project));

    approve(rig, &held["id"])?;
    let result = the_one_held(rig, &project, "the result to be held")?;
    assert_eq!(result["kind"], "result");
    approve(rig, &result["id"])?;
    rig.wait_for("the result to be delivered", secs(60), || {
        Ok(the_result(&project)?["state"] == "delivered")
    })?;
    Ok(())
}

#[test]
fn a_restart_of_the_daemon_leaves_what_waits_at_the_gate_there_and_passing_it_on_goes_on() -> Outcome
{
    // The chief takes a moment to write its answer once its work is done, as a
    // window on a slow machine does (CI's Windows is one): the app is quit once
    // it is written, as it is when the chief is at rest.
    let rig = Rig::start(config().var("CF_TEST_ANSWER_MS", "1500"))?;
    let (id, brief) = {
        let project = gated_project(&rig)?;
        let chief = chief_window(&rig, &project)?;
        rig.tell(
            project.id(),
            &format!(
                "DISPATCH --tier {} Reply with exactly: ONE_DONE",
                tier(&project)?
            ),
        )?;
        let brief = the_one_held(&rig, &project, "the brief to be held")?;
        let session = session_of(&chief);
        rig.wait_for("the chief's answer to be written", secs(30), || {
            Ok(rig.transcript(&session).contains("\"text\":\"dispatched:"))
        })?;
        (project.id(), brief)
    };

    let rig = quit_and_start_again(rig)?;
    goes_on_after_the_restart(&rig, id, &brief)?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_restart_that_cuts_the_chiefs_turn_leaves_its_window_at_rest_and_what_waits_for_it_reaches_it(
) -> Outcome {
    // The chief's answer is a long way off when the app quits: its turn is cut,
    // and its record ends in the words it was given (the dispatch), with no
    // answer. Claude resumed on it waits at its prompt and goes on with nothing
    // (`npm run live:resume-cut`): the window is at rest, and the daemon has to
    // read it so.
    let rig = Rig::start(config().var("CF_TEST_ANSWER_MS", "20000"))?;
    let (id, brief) = {
        let project = gated_project(&rig)?;
        let chief = chief_window(&rig, &project)?;
        rig.tell(
            project.id(),
            &format!(
                "DISPATCH --tier {} Reply with exactly: ONE_DONE",
                tier(&project)?
            ),
        )?;
        let brief = the_one_held(&rig, &project, "the brief to be held")?;
        let record = rig.transcript(&session_of(&chief));
        assert!(
            record.contains("DISPATCH") && !record.contains("\"text\":\"dispatched:"),
            "the chief is at work on its words, with no answer written: {record}"
        );
        (project.id(), brief)
    };

    let rig = quit_and_start_again(rig)?;
    goes_on_after_the_restart(&rig, id, &brief)?;
    rig.close()?;
    Ok(())
}
