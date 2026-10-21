//! What the verdict is made of, which needs no harness and no daemon: the board's lane
//! and the host's listings are written here as they are read, and the words the run
//! says of them are held to what they must say.

use serde_json::json;

use super::*;

const NAME: &str = "ZEEWIN";
const NOTE: &str = "T-1's window closed with background work still running: devin.exe; node.exe";

fn at(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// The lane of a worker whose window `state`, and says `reason` where it says one.
fn lane(state: &str, reason: Option<&str>) -> Value {
    let mut activity = json!({ "state": state });
    if let Some(reason) = reason {
        activity["reason"] = json!(reason);
    }
    json!({
        "participant": { "member": "live-devin" },
        "activity": activity,
        "pane": { "id": "p1-live-devin-a", "generation": 3 },
    })
}

fn tree(processes: &[(u64, &str)]) -> Read {
    Ok(Tree::of(&json!({
        "ok": true,
        "total": processes.len(),
        "processes": processes
            .iter()
            .map(|(pid, command)| json!({ "pid": pid, "command": command }))
            .collect::<Vec<_>>(),
    })))
}

/// A worker that finished and told the chief nothing of work, the first look at 3 s and
/// the last at 4.
fn good() -> Seen {
    let mut seen = Seen::default();
    seen.look(at(3), "working", &lane("working", None), Some(tree(&[])));
    seen.look(at(4), "done", &lane("idle", None), Some(tree(&[])));
    seen.result = Some(NAME.to_owned());
    seen
}

#[test]
fn a_task_done_with_its_result_holding_the_output_and_nothing_said_of_work_passes() {
    assert_eq!(judge(&good(), NAME), Ok(()));
    // The board may say other things of a window.
    let mut seen = good();
    seen.look(
        at(5),
        "done",
        &lane("waiting", Some("waiting for the human")),
        None,
    );
    assert_eq!(judge(&seen, NAME), Ok(()));
}

#[test]
fn the_output_may_be_in_any_case_and_between_marks_and_on_a_line_of_words() {
    for result in [
        "ZEEWIN",
        "zeewin",
        "The machine is `zeewin`.",
        "  Zeewin \n",
        "hostname printed:\nZEEWIN",
    ] {
        assert!(says(result, NAME), "{result}");
    }
    for result in ["", "zee win", "the machine's name", "ZEEWI"] {
        assert!(!says(result, NAME), "{result}");
    }
    // An output of two words is a phrase, whatever breaks the line between them.
    assert!(says("git version\n2.51.0", "git version 2.51.0"));
}

#[test]
fn a_worker_whose_chief_was_told_its_turn_left_work_fails_and_says_what_it_was_told() {
    // A command that ended with its turn left nothing: what the daemon says it ended is
    // a process it took for the turn's work, which may be the harness's own.
    let mut seen = good();
    seen.notes = vec![NOTE.to_owned()];
    let problems = judge(&seen, NAME).unwrap_err();
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(problems[0].contains("the chief was told the worker's turn left work"));
    assert!(problems[0].ends_with(NOTE), "{}", problems[0]);
}

#[test]
fn every_way_the_task_fell_short_is_said_and_the_result_without_the_output_is_quoted() {
    let mut seen = Seen::default();
    seen.look(at(10), "working", &lane("working", None), Some(tree(&[])));
    seen.look(at(300), "working", &lane("working", None), Some(tree(&[])));
    let problems = judge(&seen, NAME).unwrap_err();
    assert_eq!(problems.len(), 2, "{problems:#?}");
    // The task took as long as it was watched, from the first look to the last.
    assert_eq!(problems[0], "T-1 is working, not done after 290.0 s");
    assert_eq!(problems[1], "the chief was given no result");

    seen.state = "done".to_owned();
    seen.result = Some("I could not run it".to_owned());
    let problems = judge(&seen, NAME).unwrap_err();
    assert_eq!(
        problems,
        ["the result does not hold what the command printed (ZEEWIN): I could not run it"]
    );
}

#[test]
fn a_task_is_watched_from_the_first_look_to_the_last_and_took_as_long() {
    let mut seen = Seen::default();
    assert_eq!(seen.took(), Duration::ZERO);
    seen.look(at(1210), "working", &lane("working", None), None);
    assert_eq!(seen.took(), Duration::ZERO);
    // A worker run after others begins late on the run's clock, and took the time it did.
    seen.look(at(1222), "done", &lane("idle", None), None);
    assert_eq!((seen.began, seen.until), (Some(at(1210)), at(1222)));
    assert_eq!(seen.took(), at(12));
}

#[test]
fn a_look_notes_what_the_host_listed_and_why_it_could_not_be_read() {
    let mut seen = Seen::default();
    // A lane the board has none of yet says nothing of its window.
    seen.look(at(1), "queued", &Value::Null, None);
    assert_eq!((seen.state.as_str(), seen.until), ("queued", at(1)));
    assert!(seen.end.is_none());
    seen.look(
        at(2),
        "working",
        &lane("working", None),
        Some(Err("(the host said: no such pane)".to_owned())),
    );
    assert_eq!(
        seen.unlisted.as_deref(),
        Some("(the host said: no such pane)")
    );
    assert!(seen.end.is_none(), "nothing was listed");
    seen.look(
        at(3),
        "working",
        &lane("working", None),
        Some(tree(&[(9, "x")])),
    );
    let end = seen.end.as_ref().unwrap();
    assert_eq!((end.window.as_str(), end.resting), ("working", false));
    assert_eq!(end.tree.processes[0].pid, 9);
}

#[test]
fn the_turns_end_is_the_first_look_that_found_the_window_at_rest_and_before_one_the_last() {
    let mut seen = Seen::default();
    seen.look(
        at(1),
        "working",
        &lane("working", None),
        Some(tree(&[(1, "a")])),
    );
    seen.look(
        at(2),
        "working",
        &lane("working", None),
        Some(tree(&[(2, "b")])),
    );
    // No look has found the turn at its end: the last look is what there is.
    let end = seen.end.as_ref().unwrap();
    assert_eq!((end.at, end.resting), (at(2), false));
    seen.look(
        at(3),
        "working",
        &lane("idle", None),
        Some(tree(&[(3, "c")])),
    );
    let end = seen.end.as_ref().unwrap();
    assert_eq!(
        (end.at, end.resting, end.window.as_str()),
        (at(3), true, "idle")
    );
    // What the window is found doing afterwards (it works again, or rests once more) is
    // not the turn's end.
    seen.look(
        at(4),
        "working",
        &lane("working", None),
        Some(tree(&[(4, "d")])),
    );
    seen.look(at(6), "working", &lane("idle", None), Some(tree(&[])));
    assert_eq!(seen.end.as_ref().map(|end| end.at), Some(at(3)));
}

#[test]
fn a_run_is_a_line_that_says_it_ended_nothing_or_the_first_thing_that_was_wrong() {
    let seen = good();
    assert_eq!(
        summary(&seen, &judge(&seen, NAME)),
        "T-1 done after 1.0 s with nothing ended; the host listed no process at the turn's end"
    );
    let mut busy = Seen::default();
    busy.look(at(1), "working", &lane("working", None), Some(tree(&[])));
    busy.look(
        at(5),
        "done",
        &lane("idle", None),
        Some(tree(&[(1, "msedge.exe"), (2, "conhost.exe")])),
    );
    busy.result = Some(NAME.to_owned());
    assert_eq!(
        summary(&busy, &judge(&busy, NAME)),
        "T-1 done after 4.0 s with nothing ended; the host listed 2 processes at the turn's end"
    );
    let mut one = Seen::default();
    one.look(
        at(5),
        "done",
        &lane("idle", None),
        Some(tree(&[(1, "msedge.exe")])),
    );
    one.result = Some(NAME.to_owned());
    assert!(
        summary(&one, &judge(&one, NAME)).ends_with("the host listed 1 process at the turn's end")
    );
    // A run whose chief was told of work says that first.
    let mut told = good();
    told.notes = vec![NOTE.to_owned()];
    assert!(summary(&told, &judge(&told, NAME))
        .starts_with("the chief was told the worker's turn left work"));
    // One that did not end says so.
    let mut waiting = Seen::default();
    waiting.look(at(9), "working", &lane("working", None), None);
    assert_eq!(
        summary(&waiting, &judge(&waiting, NAME)),
        "T-1 is working, not done after 0.0 s"
    );
    // A task done before any look found the window at rest has its last look said as that.
    let mut unrested = Seen::default();
    unrested.look(at(1), "working", &lane("working", None), Some(tree(&[])));
    unrested.look(
        at(3),
        "done",
        &lane("working", None),
        Some(tree(&[(1, "msedge.exe")])),
    );
    unrested.result = Some(NAME.to_owned());
    assert_eq!(
        summary(&unrested, &judge(&unrested, NAME)),
        "T-1 done after 2.0 s with nothing ended; the host listed 1 process at its last look"
    );
    // A run that never saw the host has not seen it list a thing.
    let mut unlisted = Seen::default();
    unlisted.look(at(1), "working", &lane("working", None), None);
    unlisted.look(at(2), "done", &lane("idle", None), None);
    unlisted.result = Some(NAME.to_owned());
    assert_eq!(
        summary(&unlisted, &judge(&unlisted, NAME)),
        "T-1 done after 1.0 s with nothing ended; the host listed nothing"
    );
}

#[test]
fn what_the_window_left_when_it_closed_is_said_with_its_processes_or_that_it_has_not_closed() {
    let mut seen = Seen::default();
    assert_eq!(
        at_the_close(&seen),
        "the pane host said nothing of what the window left when it closed: it had not closed when the run stopped looking"
    );
    seen.exit = Some(Tree::of(&json!({ "processes": [] })));
    assert_eq!(
        at_the_close(&seen),
        "what the pane host said the window left running when it closed (its pane.exit): none"
    );
    seen.exit = Some(Tree::of(&json!({ "processes": [
        { "pid": 4101, "command": "msedge.exe --headless" },
        { "pid": 4102, "command": "conhost.exe" },
    ] })));
    assert_eq!(
        at_the_close(&seen),
        "what the pane host said the window left running when it closed (its pane.exit):\n    \
         4101 msedge.exe --headless\n    4102 conhost.exe"
    );
}

#[test]
fn what_the_chief_was_told_of_work_is_its_notes_a_line_to_each_or_that_it_was_told_none() {
    let mut seen = good();
    assert_eq!(
        at_the_chief(&seen),
        "what the chief was told of work the turn left: nothing"
    );
    seen.notes = vec![NOTE.to_owned()];
    assert_eq!(
        at_the_chief(&seen),
        format!("what the chief was told of work the turn left:\n    {NOTE}")
    );
}

#[test]
fn the_window_of_the_worker_is_known_by_the_id_the_board_gave_it() {
    let mut seen = Seen::default();
    seen.look(at(1), "queued", &Value::Null, None);
    assert_eq!(seen.pane, None);
    seen.look(at(2), "working", &lane("working", None), None);
    assert_eq!(seen.pane.as_deref(), Some("p1-live-devin-a"));
    // A window that is gone from the lane is the window it was.
    seen.look(
        at(3),
        "done",
        &json!({ "activity": { "state": "closed" }, "pane": null }),
        None,
    );
    assert_eq!(seen.pane.as_deref(), Some("p1-live-devin-a"));
}

#[test]
fn the_turns_end_is_said_whether_the_host_listed_nothing_a_tree_or_could_not_be_asked() {
    assert_eq!(
        at_the_turns_end(&good()),
        "what the pane host listed as the turn's work at the turn's end (at 4.0 s, the window idle): none"
    );
    let mut busy = Seen::default();
    busy.look(
        at(5),
        "done",
        &lane("idle", None),
        Some(tree(&[
            (4101, "msedge.exe --headless"),
            (4102, "conhost.exe"),
        ])),
    );
    assert_eq!(
        at_the_turns_end(&busy),
        "what the pane host listed as the turn's work at the turn's end (at 5.0 s, the window idle):\n    \
         4101 msedge.exe --headless\n    4102 conhost.exe"
    );
    // A task done before a look found the window at rest has its last look said as that.
    let mut unrested = Seen::default();
    unrested.look(at(2), "working", &lane("working", None), Some(tree(&[])));
    assert_eq!(
        at_the_turns_end(&unrested),
        "what the pane host listed as the turn's work at the last look, no look having found the turn at its end (at 2.0 s, the window working): none"
    );
    let mut unread = Seen::default();
    unread.look(
        at(1),
        "working",
        &lane("working", None),
        Some(Err("(could not be asked: timed out)".to_owned())),
    );
    assert_eq!(
        at_the_turns_end(&unread),
        "the pane host could not say what the turn left at its end: (could not be asked: timed out)"
    );
    assert_eq!(
        at_the_turns_end(&Seen::default()),
        "the pane host was never asked what the turn left: the worker's window was never seen"
    );
}
