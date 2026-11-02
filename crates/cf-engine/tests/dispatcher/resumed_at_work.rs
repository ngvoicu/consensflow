//! A resumed task whose window is at work on it shows working (T-1139, field bug 8; Gabriel's
//! other board, 2026-11-02). `cf tell T-10 "…"` pauses the task and puts the words to its
//! window; the window answered ("ok") and went straight on with its work in the same turn
//! (T-429's and T-442's shape, `paused_answers`); the chief's `cf task resume T-10 "…"` then
//! queued its words behind that turn, and the board read `T-10 [queued]` for as long as the
//! window ran commands on it, because only the paste of those words moved the task to
//! working. The board under-reported work: its label said the window had not taken the task
//! up while its record showed it at work on nothing else.
//!
//! The rule (`resumed_at_work`): the words that resume a task wait for the window's rest
//! (nothing is typed into a turn at work, `pacing`), but the task is what the window is at
//! work on, so it reads **working** as soon as the window's record shows the window acting
//! on it: the turn in flight, and an item of the agent's own (its words, a tool call or its
//! result) written after the resume. A turn that reads in flight and writes nothing is no
//! work (a Pi cut by a quit reads so for ever, T-1130), and a window at rest is given the
//! words as before, the task working from their paste. The words stay queued, and the turn's
//! end is still not the resumed task's result: its answer is the follow-up to them.

use cf_engine::testing::{Context, Made};
use cf_harness::contract::Over;
use cf_harness::records::{Item, Role};
use cf_ledger::GivenUp;
use cf_proto::ledger::{Claim, MessageView};
use serde_json::json;

use crate::fixtures::given;
use crate::paused_answers::{
    answers_and_goes_on, kept_notes, moved_on_words, results, told_at_work, FINAL,
};

/// The agent of T-1's window writes another step of its turn: a tool call and its result, as
/// the harness's record shows them while the turn goes on.
fn works_on(context: &Context, step: &str) {
    let mut call: Item = context.adapter.item(Role::Tool, step);
    call.complete = true;
    context
        .adapter
        .with("zeus", |agent| agent.items.push(call.clone()));
}

/// The resume's words as the ledger has them: T-1's last task message to its window.
fn resume_words(context: &Context, project: i64) -> MessageView {
    context
        .task(project, 1)
        .messages
        .into_iter()
        .rfind(|message| message.kind == "task" && message.body.starts_with("Resumed:"))
        .expect("the resume's words")
}

fn state(context: &Context, project: i64) -> String {
    context.task(project, 1).task.state
}

/// The chief resumes T-1 after the window answered the tell and went on, and the window's
/// turn goes on: two passes, the second after the record grew.
fn resumed_while_the_turn_goes_on(context: &Context) -> (i64, MessageView, MessageView) {
    let (project, told) = told_at_work(context);
    answers_and_goes_on(context, &told);
    context.pass().unwrap();
    assert_eq!(state(context, project), "paused");
    context.resume_task(project, 1, "Go on, or say you finished");
    let words = resume_words(context, project);
    (project, told, words)
}

#[test]
fn a_task_resumed_while_its_window_goes_on_with_the_turn_on_the_tell_shows_working_and_its_words_still_go_in_at_rest(
) {
    let context = Context::new();
    let (project, told, words) = resumed_while_the_turn_goes_on(&context);
    assert_eq!(
        state(&context, project),
        "queued",
        "at the resume: nothing of the window's since"
    );

    // The window goes on: the record grows after the resume.
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(
        state(&context, project),
        "working",
        "the window is at work on it"
    );
    assert_eq!(
        context.message(words.id).state,
        "queued",
        "the words wait for the window's rest, as ever"
    );
    assert!(
        !given(&context, "zeus")
            .iter()
            .any(|text| text.contains("Go on, or say you finished")),
        "nothing is typed into a turn at work"
    );

    // The turn on the tell ends with its answer: it is no result, the task having moved on.
    context.adapter.answer("zeus", FINAL);
    context.pass().unwrap();
    assert_eq!(
        kept_notes(&context, project),
        [moved_on_words("zeus", &told)],
        "told all the same, in the words of a task that has moved on"
    );
    assert_ne!(
        state(&context, project),
        "done",
        "that turn is not the result"
    );
    assert_eq!(results(&context, project), Vec::<String>::new());

    // The words go in once the window rests; the follow-up they get is the result.
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivered");
    context.adapter.answer("zeus", "ALREADY-FINISHED");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(state(&context, project), "done");
    assert_eq!(results(&context, project), ["ALREADY-FINISHED"]);
}

#[test]
fn a_task_resumed_while_its_window_rests_stays_queued_until_its_words_go_in() {
    let context = Context::new();
    let (project, _told) = told_at_work(&context);
    // The turn on the tell is over: the window rests with its answer.
    context.adapter.answer("zeus", "Answered, and stopped");
    context.pass().unwrap();
    context.pass().unwrap();
    context.resume_task(project, 1, "Go on");
    let words = resume_words(&context, project);
    assert_eq!(state(&context, project), "queued");

    // At rest nothing writes: the task reads queued while its words wait for the window to
    // rest long enough, and working from their paste.
    for _ in 0..6 {
        context.pass().unwrap();
        if context.message(words.id).state == "delivered" {
            break;
        }
        assert_eq!(
            state(&context, project),
            "queued",
            "no work of the window's is shown while its words wait"
        );
    }
    assert_eq!(context.message(words.id).state, "delivered");
    assert_eq!(state(&context, project), "working");
}

#[test]
fn a_resumed_task_whose_window_reads_at_work_and_writes_nothing_stays_queued() {
    // A turn that reads in flight with nothing written after the resume is no work shown:
    // a harness cut by a quit reads so for ever (T-1130), and the task is not the window's.
    let context = Context::new();
    let (project, _told, words) = resumed_while_the_turn_goes_on(&context);
    for _ in 0..6 {
        context.pass().unwrap();
        context.advance(5_000);
    }
    assert_eq!(state(&context, project), "queued");
    assert_eq!(context.message(words.id).state, "queued");
}

#[test]
fn a_pause_the_window_did_not_heed_resumed_while_it_works_on_shows_working_too() {
    // The class, not the tell: a window that goes on through its pause (T-1117's kept
    // window) and a resume that finds it at work.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    assert_eq!(state(&context, project.id), "paused");
    context.resume_task(project.id, 1, "Carry on");
    let words = resume_words(&context, project.id);
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo build)");
    context.pass().unwrap();
    assert_eq!(state(&context, project.id), "working");
    assert_eq!(context.message(words.id).state, "queued");
}

#[test]
fn the_words_of_a_task_shown_working_that_never_reach_its_window_pause_it_again() {
    let context = Context::new();
    let (project, _told, words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");

    // The paste begins and the window is lost: the words are given up.
    context
        .ledger
        .borrow_mut()
        .begin_delivery(words.id)
        .unwrap();
    let (failed, came_to) = context
        .ledger
        .borrow_mut()
        .give_up_delivery(words.id, "the window closed")
        .unwrap();
    assert_eq!(came_to, GivenUp::Paused);
    assert_eq!(failed.state, "failed");
    assert_eq!(
        state(&context, project),
        "paused",
        "the task is not left working with nothing in its window"
    );
}

#[test]
fn finish_takes_the_turn_on_the_tell_as_the_result_of_a_task_shown_working_and_its_words_never_go_in(
) {
    let context = Context::new();
    let (project, _told, words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");
    // The turn on the tell ends, and the chief takes its answer before the words go in.
    context.adapter.answer("zeus", FINAL);
    let finished = context.finish_task(project, 1, "chief").expect("finished");
    assert_eq!(finished.task.state, "done");
    assert!(results(&context, project)
        .iter()
        .any(|result| result.starts_with(FINAL)));
    // Nothing is pasted for a task that is done: its words are withdrawn.
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "cancelled");
    assert_eq!(state(&context, project), "done");
}

#[test]
fn a_question_the_window_asks_while_its_resume_words_wait_is_born_with_its_door_shut() {
    // Its turn is the one before the words: the answer comes with them, as a message, when
    // the words go in (`door_born_closed`), exactly as for the queued task it was.
    let context = Context::new();
    let (project, _told, _words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");
    let question = context.ask(project, "zeus", "chief", 1, "Which grammar?");
    let zeus = context.id(project, "zeus");
    let claim = context
        .ledger
        .borrow_mut()
        .claim_answer(question.id, zeus)
        .unwrap();
    assert!(matches!(claim, Claim::Closed), "{claim:?}");
}

#[test]
fn the_window_cannot_finish_a_task_shown_working_before_it_has_read_the_words_that_resumed_it() {
    // What it would finish is the task without them, as for a note on its way.
    let context = Context::new();
    let (project, _told, words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");
    let refused = context
        .ledger
        .borrow_mut()
        .record_result(project, 1, "All done")
        .expect_err("refused");
    assert!(
        refused.to_string().contains("words on their way"),
        "{refused}"
    );
    assert_eq!(state(&context, project), "working");
    assert_eq!(context.message(words.id).state, "queued");
}

#[test]
fn a_turn_that_ends_after_the_resume_shows_nothing_of_work_and_the_task_is_working_from_its_words_paste(
) {
    // The record grew after the resume, but the window is at rest now: no turn is in
    // flight to be at work on it. Its words go in, and the task reads queued until they
    // are in the window.
    let context = Context::new();
    let (project, _told, words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    context.adapter.answer("zeus", FINAL);
    context.pass().unwrap();
    assert_eq!(state(&context, project), "queued");
    assert_ne!(context.message(words.id).state, "delivered");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivered");
    assert_eq!(state(&context, project), "working");
}

#[test]
fn a_record_rewritten_under_the_baseline_claims_no_work_until_the_agent_writes_again() {
    // A compaction rewrites the record: the item the watch stood on is gone, and what the
    // rewritten record holds says nothing of work after the resume.
    let context = Context::new();
    let (project, _told, _words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    let summary = context
        .adapter
        .item(Role::Assistant, "Where the work stands");
    context.adapter.with("zeus", |agent| {
        agent.items.clear();
        agent.items.push(summary);
    });
    context.pass().unwrap();
    assert_eq!(state(&context, project), "queued");
    works_on(&context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");
}

#[test]
fn what_the_harness_writes_in_its_agents_place_and_a_message_typed_in_are_no_work_of_the_agent() {
    let context = Context::new();
    let (project, _told, _words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    // Claude Code's close of a cut turn, a compaction's summary, and the human's own words.
    let mut closing = context
        .adapter
        .item(Role::Assistant, "No response requested.");
    closing.synthetic = true;
    closing.commentary = true;
    let mut summary = context
        .adapter
        .item(Role::User, "The summary of the conversation");
    summary.compaction = true;
    let typed = context
        .adapter
        .item(Role::User, "Please also check the lockfile");
    context.adapter.with("zeus", |agent| {
        agent.items.extend([closing, summary, typed])
    });
    context.pass().unwrap();
    assert_eq!(state(&context, project), "queued");
    // The agent's own step after them is the work.
    works_on(&context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");
}

#[test]
fn the_daemons_bare_words_that_resume_a_held_task_are_no_resume_words_of_a_task_shown_working() {
    // A task held for quota and resumed by the daemon with its own bare words: a window
    // that goes on by itself withdraws them (`went_on_by_itself`), so its turn at work is
    // that path's, and the task is not shown working with them waiting.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.hold_task(project.id, 1);
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .resume_task(project.id, 1, None, cf_ledger::RESUME_WORDS)
        .unwrap();
    assert_eq!(state(&context, project.id), "queued");
    assert_eq!(
        context
            .ledger
            .borrow()
            .resume_words_waiting(project.id, 1)
            .unwrap(),
        None
    );
    context.adapter.busy("zeus");
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo build)");
    context.pass().unwrap();
    assert_eq!(state(&context, project.id), "queued");
}

#[test]
fn the_ledger_shows_no_work_for_words_that_are_being_pasted_already() {
    // Their paste is begun: their arrival moves the task, as it moves any.
    let context = Context::new();
    let (project, _told, words) = resumed_while_the_turn_goes_on(&context);
    context
        .ledger
        .borrow_mut()
        .begin_delivery(words.id)
        .unwrap();
    let refused = context
        .ledger
        .borrow_mut()
        .work_while_resumed(project, 1)
        .expect_err("refused");
    assert!(
        refused.to_string().contains("not waiting for the words"),
        "{refused}"
    );
    assert_eq!(state(&context, project), "queued");
}

#[test]
fn a_first_brief_waiting_for_its_window_is_no_resume_words() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    assert_eq!(
        context
            .ledger
            .borrow()
            .resume_words_waiting(project.id, 1)
            .unwrap(),
        None
    );
}

#[test]
fn a_turn_the_record_says_is_over_but_has_not_said_so_a_moment_shows_no_work() {
    let context = Context::new();
    let (project, _told, _words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo metadata)");
    context
        .adapter
        .with("zeus", |agent| agent.over = Over::Pending);
    context.pass().unwrap();
    assert_eq!(state(&context, project), "queued");
    context.adapter.with("zeus", |agent| agent.over = Over::No);
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");
}

/// A context whose windows are given a message only after the settle, as a live daemon's are.
fn paced() -> Context {
    Context::made(Made {
        settle_ms: 3_000,
        ..Made::default()
    })
}

#[test]
fn a_task_shown_working_reads_queued_again_once_its_window_has_rested_with_the_words_held() {
    // The window's turn ends, and the words that resumed the task do not go in (the human
    // typed in the window and has not sent it; Muse's screen that never went quiet): no
    // window is at work on it, and the board must not say one is.
    let context = paced();
    let (project, _told, words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");

    context.host.set_snapshot(json!({ "typedQuietMs": 100 }));
    context.adapter.answer("zeus", FINAL);
    context.pass().unwrap();
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(
        state(&context, project),
        "working",
        "a rest of seconds is the settle and the paste, not a hold"
    );
    context.advance(6_000);
    context.pass().unwrap();
    assert_eq!(state(&context, project), "queued");
    assert_eq!(context.message(words.id).state, "queued");

    // The hold ends: the words go in, and the task reads working from their paste.
    context.host.set_snapshot(json!({}));
    for _ in 0..6 {
        context.advance(3_000);
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivered");
    assert_eq!(state(&context, project), "working");
}

#[test]
fn a_task_shown_working_does_not_read_queued_while_its_words_go_in_at_the_settle() {
    let context = paced();
    let (project, _told, words) = resumed_while_the_turn_goes_on(&context);
    context.pass().unwrap();
    works_on(&context, "(Bash completed: cargo metadata)");
    context.pass().unwrap();
    assert_eq!(state(&context, project), "working");
    context.adapter.answer("zeus", FINAL);
    for _ in 0..8 {
        context.advance(1_500);
        context.pass().unwrap();
        assert_eq!(
            state(&context, project),
            "working",
            "the words are {}",
            context.message(words.id).state
        );
    }
    assert_eq!(context.message(words.id).state, "delivered");
}

#[test]
fn a_follow_up_that_reopened_a_done_task_is_no_resume_words() {
    // Its move to queued names its message too, but it comes from done, not from paused.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Done.");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(state(&context, project.id), "done");
    // The chief has the result, and sends the task back.
    for _ in 0..4 {
        context.adapter.answer("chief", "Read.");
        context.pass().unwrap();
        context.pass().unwrap();
    }
    context
        .ledger
        .borrow_mut()
        .reopen_task(project.id, 1, "chief", "Fix the typo in its name")
        .unwrap();
    assert_eq!(state(&context, project.id), "queued");
    assert_eq!(
        context
            .ledger
            .borrow()
            .resume_words_waiting(project.id, 1)
            .unwrap(),
        None
    );
}
