//! What a task shown working for the words that resumed it breaks, and what holds the claim
//! (T-1153's review of T-1139, 2026-11-03: F1 to F5, the churn, and the guards no plant held).
//!
//! The claim (`resumed_at_work`) is the ledger's `working` with the resume's words still
//! queued. Every place that read "a task message pending for the window" as "the task is
//! queued" has to read it so, and the claim has to go wherever its window goes: a window
//! that exits, a start that finds none, a stop that closes it.

use cf_engine::testing::Context;
use cf_harness::contract::{Over, Waiting};
use cf_proto::ledger::{Claim, MessageView};
use serde_json::json;

use crate::fixtures::{after, exhausted, given, notes};
use crate::paused_answers::FINAL;
use crate::resume::{cut_notes, quit_and_start};
use crate::resumed_at_work::{
    paced, resume_words, resumed_while_the_turn_goes_on, state, works_on,
};

/// The task shown working for the words that resumed it: tell, the window's answer, the
/// resume, and the window going on with its turn. The project, and the resume's words.
fn shown_working(context: &Context) -> (i64, MessageView) {
    let (project, _told, words) = resumed_while_the_turn_goes_on(context);
    context.pass().unwrap();
    works_on(context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(state(context, project), "working");
    (project, words)
}

/// The words the chief was sent that say T-1 is paused.
fn paused_notes(context: &Context, project: i64) -> Vec<String> {
    notes(context, context.id(project, "chief"))
        .into_iter()
        .filter(|note| note.starts_with("T-1 is paused"))
        .collect()
}

/// Passes enough for a window that went to open again and be given what waits for it.
fn settle(context: &Context) {
    for _ in 0..8 {
        context.pass().unwrap();
        context.advance(3_100);
    }
}

// ---- F1: the window goes while the task reads working ----------------------------------

#[test]
fn a_window_that_exits_while_its_task_reads_working_for_the_resume_words_does_not_undo_the_resume()
{
    let context = Context::new();
    let (project, words) = shown_working(&context);
    context.exit("zeus");
    context.pass().unwrap();
    assert_eq!(
        state(&context, project),
        "queued",
        "the claim goes with the window: its words wait for the one that opens"
    );
    assert_eq!(context.message(words.id).state, "queued");
    assert_eq!(paused_notes(&context, project), Vec::<String>::new());

    // The words reopen the window, as they do for a window that exited before it worked.
    settle(&context);
    assert_eq!(context.message(words.id).state, "delivered");
    assert_eq!(state(&context, project), "working");
    assert_eq!(paused_notes(&context, project), Vec::<String>::new());
}

#[test]
fn the_claim_a_window_leaves_when_it_exits_says_so_in_the_log() {
    let context = Context::new();
    let (project, words) = shown_working(&context);
    context.exit("zeus");
    context.pass().unwrap();
    let said = context
        .recorder
        .events()
        .into_iter()
        .filter(|event| event["seam"] == "event" && event["event"]["kind"] == "task.state")
        .map(|event| event["event"]["data"].clone())
        .last()
        .expect("a move of the task");
    assert_eq!(
        (said["from"].as_str(), said["to"].as_str()),
        (Some("working"), Some("queued"))
    );
    assert_eq!(
        said["because"],
        json!(format!(
            "its window closed, and m-{}, the words that resumed it, have not gone in",
            words.id
        )),
        "{said}"
    );
    assert_eq!(state(&context, project), "queued");
}

#[test]
fn a_waiting_task_whose_window_exits_is_not_paused_for_the_resume_words_either() {
    let context = Context::new();
    let (project, words) = shown_working(&context);
    context.ask(project, "zeus", "chief", 1, "Which grammar?");
    assert_eq!(state(&context, project), "waiting");
    context.exit("zeus");
    context.pass().unwrap();
    assert_ne!(state(&context, project), "paused");
    assert_eq!(context.message(words.id).state, "queued");
    assert_eq!(paused_notes(&context, project), Vec::<String>::new());
}

#[test]
fn a_close_of_the_project_while_the_task_reads_working_leaves_the_resume_for_the_resume_of_the_project(
) {
    let context = Context::new();
    let (project, words) = shown_working(&context);
    context.close_project(project).unwrap();
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(
        state(&context, project),
        "queued",
        "not paused by the Close"
    );
    assert_eq!(
        context.message(words.id).state,
        "queued",
        "a closed project is given nothing"
    );

    context.resume_project(project).unwrap();
    settle(&context);
    assert_eq!(context.message(words.id).state, "delivered");
    assert_eq!(state(&context, project), "working");
    // F2: the window's one paste is the resume's words, with no stray note of a cut turn.
    let pasted = given(&context, "zeus");
    assert!(
        !pasted
            .iter()
            .any(|text| text.contains("ConsensFlow stopped while you were working on")),
        "{pasted:?}"
    );
}

// ---- F2: a quit and the Resume ----------------------------------------------------------

#[test]
fn a_quit_while_the_task_reads_working_for_the_resume_words_gives_its_window_no_cut_note() {
    let context = Context::new();
    let (project, words) = shown_working(&context);
    let zeus = context.id(project, "zeus");
    let after_the_quit = quit_and_start(&context);
    after_the_quit.resume_project(project).unwrap();
    for _ in 0..8 {
        after_the_quit.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(cut_notes(&context, zeus), Vec::<MessageView>::new());
    let pasted = given(&context, "zeus");
    assert!(
        !pasted
            .iter()
            .any(|text| text.contains("ConsensFlow stopped while you were working on")),
        "{pasted:?}"
    );
    assert_eq!(context.message(words.id).state, "delivered");
    assert_eq!(state(&context, project), "working");
}

// ---- F3: a turn the quota cut --------------------------------------------------------------

#[test]
fn a_quota_refusal_whose_reset_has_passed_does_not_hold_and_resume_a_task_shown_working() {
    let context = Context::new();
    let (project, words) = shown_working(&context);
    context.adapter.refuse(
        "zeus",
        exhausted(
            Some(&after(&context, -7_200_000)),
            Some(&after(&context, -60_000)),
        ),
    );
    let seen: Vec<String> = (0..6)
        .map(|_| {
            context.pass().unwrap();
            state(&context, project)
        })
        .collect();
    assert!(
        !seen.iter().any(|state| state == "paused"),
        "the board churned: {seen:?}"
    );
    let pasted = given(&context, "zeus");
    assert!(
        !pasted
            .iter()
            .any(|text| text.contains("Resumed: Go on where you stopped.")),
        "the window is not also told to go on: {pasted:?}"
    );
    assert_ne!(context.message(words.id).state, "cancelled");
}

// ---- F4: every door is shut while the window is on the turn before the words -------------

#[test]
fn a_question_asked_after_the_first_has_its_door_shut_too() {
    let context = Context::new();
    let (project, _words) = shown_working(&context);
    let zeus = context.id(project, "zeus");
    context.ask(project, "zeus", "chief", 1, "Which grammar?");
    let second = context.ask(project, "zeus", "chief", 1, "And which parser?");
    let claim = context
        .ledger
        .borrow_mut()
        .claim_answer(second.id, zeus)
        .unwrap();
    assert!(matches!(claim, Claim::Closed), "{claim:?}");
}

// ---- F5: a second tell ------------------------------------------------------------------

#[test]
fn a_second_tell_keeps_the_resume_words_queued_and_rides_in_their_paste() {
    let context = Context::new();
    let (project, words) = shown_working(&context);
    let told = context
        .ledger
        .borrow_mut()
        .tell_queued(project, 1, "chief", "Also check the lexer")
        .expect("the words are kept for the window");
    assert_eq!(context.message(words.id).state, "queued");
    assert_eq!(context.message(told.id).state, "queued");
    assert_eq!(state(&context, project), "working");

    // The turn on the first tell ends: both go in one paste, and their answer is the result.
    context.adapter.answer("zeus", FINAL);
    settle(&context);
    assert_eq!(context.message(words.id).state, "delivered");
    let pasted = given(&context, "zeus");
    assert!(
        pasted
            .iter()
            .any(|text| text.contains("Go on, or say you finished")
                && text.contains("Also check the lexer")),
        "{pasted:?}"
    );
    context.adapter.answer("zeus", "ALL-DONE");
    settle(&context);
    assert_eq!(state(&context, project), "done");
}

// ---- the guards that held no plant ------------------------------------------------------

/// The window goes on working after the resume, with `flag` set on what a look reads of it:
/// the claim is not made while it is, and is made once it is not.
fn not_claimed_while(flag: impl Fn(&Context, bool)) {
    let context = Context::new();
    let (project, _told, _words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    flag(&context, true);
    works_on(&context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(
        state(&context, project),
        "queued",
        "no claim while it is set"
    );
    flag(&context, false);
    works_on(&context, "(Bash completed: cargo build)");
    context.pass().unwrap();
    assert_eq!(
        state(&context, project),
        "working",
        "the claim once it is not"
    );
}

#[test]
fn a_failed_turn_is_no_work_shown() {
    not_claimed_while(|context, on| {
        context.adapter.with("zeus", |agent| {
            agent.failed = on;
            agent.failure = on.then(|| "the model failed".to_owned());
        });
    });
}

#[test]
fn a_conversation_another_program_writes_into_is_no_work_shown() {
    not_claimed_while(|context, on| {
        context.adapter.with("zeus", |agent| agent.shared = on);
    });
}

#[test]
fn a_window_that_has_not_named_its_conversation_is_no_work_shown() {
    not_claimed_while(|context, on| {
        context.adapter.with("zeus", |agent| {
            agent.unnamed = on.then(|| "the window is reconnecting".to_owned());
        });
    });
}

#[test]
fn a_window_waiting_on_a_dialog_of_its_own_is_no_work_shown() {
    not_claimed_while(|context, on| {
        context.adapter.with("zeus", |agent| {
            agent.waiting = on.then_some(Waiting { reason: None });
        });
    });
}

#[test]
fn a_window_in_a_closed_project_is_shown_no_work() {
    // Its Close could not close it (the pane host refuses the kill): the project is
    // suspended, the window is still looked at, and nothing is moved in a closed project.
    let context = Context::new();
    let (project, _told, _words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    context.host.refuse_kills.set(true);
    assert!(context.close_project(project).is_err(), "its windows stay");
    context.host.refuse_kills.set(false);
    works_on(&context, "(Bash completed: cargo metadata)");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(state(&context, project), "queued");
}

#[test]
fn a_claim_in_a_closed_project_is_not_withdrawn_for_a_window_at_rest() {
    let context = paced();
    let (project, _words) = shown_working(&context);
    context.host.refuse_kills.set(true);
    assert!(context.close_project(project).is_err(), "its windows stay");
    context.host.refuse_kills.set(false);
    context.host.set_snapshot(json!({ "typedQuietMs": 100 }));
    context.adapter.answer("zeus", FINAL);
    for _ in 0..3 {
        context.advance(6_000);
        context.pass().unwrap();
    }
    assert_eq!(state(&context, project), "working");
}

#[test]
fn the_work_a_window_did_before_a_second_resume_is_not_shown_as_work_after_it() {
    // The first resume's words were seen waiting; before the next look the task was paused
    // and resumed again, and the window wrote a step between the two. That step is not the
    // window acting on the second resume.
    let context = Context::new();
    let (project, _told, first) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    {
        let mut ledger = context.ledger.borrow_mut();
        ledger.pause_task(project, 1, Some("chief"), None).unwrap();
    }
    works_on(&context, "(Bash completed: cargo metadata)");
    context.resume_task(project, 1, "Go on, the second time");
    let second = resume_words(&context, project);
    assert_ne!(first.id, second.id);
    context.pass().unwrap();
    assert_eq!(state(&context, project), "queued");
    works_on(&context, "(Bash completed: cargo build)");
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");
}

// ---- the churn --------------------------------------------------------------------------

#[test]
fn a_task_sent_back_to_queued_does_not_read_working_again_for_thirty_seconds_unless_its_words_went_in(
) {
    let context = paced();
    let (project, words) = shown_working(&context);
    // The words are held (the human typed in the window and has not sent it): the window
    // rests, and the task goes back to queued.
    context.host.set_snapshot(json!({ "typedQuietMs": 100 }));
    context.adapter.answer("zeus", FINAL);
    context.pass().unwrap();
    context.advance(11_000);
    context.pass().unwrap();
    assert_eq!(state(&context, project), "queued");
    let moves = |context: &Context| {
        context
            .recorder
            .events()
            .into_iter()
            .filter(|event| event["seam"] == "event" && event["event"]["kind"] == "task.state")
            .count()
    };
    let before = moves(&context);

    // The window acts again, twice, inside the thirty seconds: no move.
    for _ in 0..2 {
        context.adapter.busy("zeus");
        context.pass().unwrap();
        works_on(&context, "(Bash completed: cargo metadata)");
        context.pass().unwrap();
        assert_eq!(state(&context, project), "queued");
        context.adapter.with("zeus", |agent| {
            agent.settled = true;
            agent.over = Over::No;
        });
        context.advance(5_000);
        context.pass().unwrap();
    }
    assert_eq!(moves(&context), before, "the board did not churn");

    // Past them, it acts once more and the claim is made again.
    context.advance(30_000);
    context.adapter.busy("zeus");
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo build)");
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");
    assert_eq!(moves(&context), before + 1);
    assert_eq!(context.message(words.id).state, "queued");
}

#[test]
fn a_task_sent_back_to_queued_reads_working_the_moment_its_words_go_in() {
    let context = paced();
    let (project, words) = shown_working(&context);
    context.host.set_snapshot(json!({ "typedQuietMs": 100 }));
    context.adapter.answer("zeus", FINAL);
    context.pass().unwrap();
    context.advance(11_000);
    context.pass().unwrap();
    assert_eq!(state(&context, project), "queued");

    // The hold ends well inside the thirty seconds: the paste moves the task, whatever the
    // look would say.
    context.host.set_snapshot(json!({}));
    for _ in 0..4 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivered");
    assert_eq!(state(&context, project), "working");
}
