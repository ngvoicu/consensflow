//! A turn that looks finished though its record never reads it as over (T-526, the
//! consensflow board, 2026-10-25): the worker's window was opened again on its conversation
//! with no proof that the program before was gone, Claude Code's reader kept what that
//! program had queued as a turn still to come, and the task stayed working four hours after
//! the worker's complete answer, nobody told. Where such a window waits at its prompt with
//! its answer complete and nothing left running for a minute, the task's requester is told
//! once how to take the answer; the task is never finished for it.

use cf_engine::testing::Context;
use cf_harness::contract::Waiting;

use cf_ledger::NewNote;

/// The window's turns: what a look reads of each.
enum Shape {
    /// Answered, at its prompt, the record never saying the turn is over.
    NeverOver,
    /// Answered, but its harness says it is at work still.
    StillAtWork,
    /// At its prompt, with no answer written yet.
    Unanswered,
    /// Answered, a job it started still running (Claude Code's `shell` status reads idle).
    JobRunning,
    /// Answered, its own dialog open.
    Dialog,
}

/// T-1 given to zeus, its window closed with the host's word `proven` or none, and T-1
/// resumed on its conversation: zeus's window answers in the shape `shape`.
fn reopened(proven: bool, shape: &Shape) -> (Context, i64) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Land it");
    context.pass().unwrap();
    context.pass().unwrap();
    if proven {
        context.host.exits_showing(Some(0), None, None);
    }
    context.exit("zeus");
    context.resume_task(project.id, 1, "Land it again");
    context.pass().unwrap();
    context.pass().unwrap();
    match shape {
        Shape::NeverOver => context.adapter.answer_never_over("zeus", "Landed."),
        Shape::StillAtWork => {
            context.adapter.answer_never_over("zeus", "Landed.");
            context
                .adapter
                .with("zeus", |agent| agent.at_prompt = false);
        }
        Shape::Unanswered => context.adapter.with("zeus", |agent| agent.at_prompt = true),
        Shape::JobRunning => {
            context.adapter.answer_never_over("zeus", "Landed.");
            context.adapter.with("zeus", |agent| agent.pending.jobs = 1);
        }
        Shape::Dialog => {
            context.adapter.answer_never_over("zeus", "Landed.");
            context.adapter.with("zeus", |agent| {
                agent.waiting = Some(Waiting {
                    reason: Some("its own question dialog is open".to_owned()),
                });
            });
        }
    }
    context.pass().unwrap();
    (context, project.id)
}

/// The notes the chief was given that T-1 looks finished.
fn looks_finished(context: &Context, project: i64) -> Vec<String> {
    context
        .task(project, 1)
        .messages
        .iter()
        .filter(|message| message.kind == "note" && message.body.contains("looks finished"))
        .map(|message| message.body.clone())
        .collect()
}

#[test]
fn a_reopened_turn_that_looks_finished_and_never_reads_as_over_is_told_once_after_a_minute() {
    let (context, project) = reopened(false, &Shape::NeverOver);
    assert!(
        looks_finished(&context, project).is_empty(),
        "not yet a minute"
    );
    context.advance(59_000);
    context.pass().unwrap();
    assert!(looks_finished(&context, project).is_empty(), "59 s");
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(
        looks_finished(&context, project),
        ["T-1 looks finished: its last answer is complete and @zeus's window has been idle since 12:00 UTC, but its turn never read as over. `cf task finish T-1` takes that answer."]
    );
    context.advance(600_000);
    context.pass().unwrap();
    assert_eq!(looks_finished(&context, project).len(), 1, "told once");
    let task = context.task(project, 1).task;
    assert_eq!(task.state, "working", "never finished for the chief");
}

#[test]
fn a_reopened_turn_whose_harness_says_it_is_at_work_is_told_nothing() {
    let (context, project) = reopened(false, &Shape::StillAtWork);
    context.advance(600_000);
    context.pass().unwrap();
    assert!(looks_finished(&context, project).is_empty());
}

#[test]
fn a_turn_in_a_window_opened_on_a_proof_that_the_program_before_was_gone_is_told_nothing() {
    let (context, project) = reopened(true, &Shape::NeverOver);
    context.advance(600_000);
    context.pass().unwrap();
    assert!(looks_finished(&context, project).is_empty());
}

#[test]
fn a_reopened_window_at_its_prompt_with_no_answer_yet_is_told_nothing() {
    let (context, project) = reopened(false, &Shape::Unanswered);
    context.advance(600_000);
    context.pass().unwrap();
    assert!(looks_finished(&context, project).is_empty());
}

#[test]
fn a_reopened_turn_with_a_job_still_running_is_told_nothing() {
    let (context, project) = reopened(false, &Shape::JobRunning);
    context.advance(600_000);
    context.pass().unwrap();
    assert!(looks_finished(&context, project).is_empty());
}

#[test]
fn a_reopened_turn_with_its_dialog_open_is_told_nothing() {
    let (context, project) = reopened(false, &Shape::Dialog);
    context.advance(600_000);
    context.pass().unwrap();
    assert!(looks_finished(&context, project).is_empty());
}

/// Words told to the window wait for it to rest, which this window never does: Finish
/// refuses while they wait (`words-on-their-way`), so the note that says to finish it is
/// not given (Calliope's P3-6, T-621).
#[test]
fn a_reopened_turn_that_looks_finished_with_words_told_on_their_way_is_told_nothing() {
    let (context, project) = reopened(false, &Shape::NeverOver);
    // The chief's `cf tell`: words to T-1's window, which go in when it is next at rest.
    let told = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some("chief".to_owned()),
                to: "zeus".to_owned(),
                task: Some(1),
                body: "Tag the release too".to_owned(),
            },
        )
        .unwrap()
        .id;
    context.advance(600_000);
    context.pass().unwrap();
    assert_eq!(context.message(told).state, "queued", "on its way still");
    assert!(looks_finished(&context, project).is_empty());
}
