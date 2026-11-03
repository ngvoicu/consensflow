//! The human's Close while a message is on its way into a window fails nothing, on any
//! harness and however the message goes in (T-1076; found by the alpha.1 release check
//! T-1068, and the shape of 3.0.6-alpha.2's T-534, bug 2). A window that closed with its
//! project gives back what its record does not show it took: the Resume delivers it,
//! once. What its record does show it took is confirmed, and the task pauses with the
//! Close, to go on in the same conversation. A window that ended by itself in an open
//! project is another matter, and its launch's brief fails as before
//! (`first_delivered`'s `a_window_that_ends_by_itself_…`).
//!
//! The field bug: a Devin worker reads its brief from a file its command line names
//! ([`FirstMessage::Opened`]); a Close 500 ms after the brief was on its way, before
//! Devin's record showed it, failed the brief and the task, told the chief and the human
//! so, and the Resume brought nothing back. T-523 had fixed the same for the harnesses
//! whose windows are handed their brief once up, and left this one out.

use cf_engine::testing::Context;
use cf_harness::contract::FirstMessage;
use cf_harness::records::Role;

use crate::fixtures::notes;

/// A project with zeus on its staff, and the brief of T-1 given to zeus.
fn briefed(context: &Context) -> (i64, i64) {
    let project = context.with_staff(&["zeus"]).id;
    let brief = context
        .give(project, "zeus", "Parser")
        .message
        .expect("its brief")
        .id;
    (project, brief)
}

/// Passes of the dispatcher, each settled.
fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
        context.settle();
    }
}

/// How many of zeus's windows opened with the brief to read (`Launch::message`).
fn opened_with_the_brief(context: &Context) -> usize {
    context
        .adapter
        .prepared()
        .iter()
        .filter(|launch| launch["participant"]["handle"] == "zeus")
        .filter(|launch| {
            launch["message"]
                .as_str()
                .is_some_and(|text| text.contains("Parser"))
        })
        .count()
}

/// What the chief and the human were told that says a task failed or a message did not
/// reach its window.
fn told_failed(context: &Context, project: i64) -> Vec<String> {
    ["chief", "human"]
        .iter()
        .flat_map(|who| notes(context, context.id(project, who)))
        .filter(|body| body.contains("failed") || body.contains("did not reach"))
        .collect()
}

#[test]
fn a_brief_its_window_reads_as_it_opens_whose_project_closes_before_its_record_shows_it_goes_back_and_in_once_after_the_resume(
) {
    // Devin's `--prompt-file`: the brief is handed over as the window opens, and the
    // Close comes before its record shows it.
    let context = Context::new();
    assert_eq!(context.adapter.first.get(), FirstMessage::Opened);
    context.adapter.forgets_first_messages.set(true);
    let (project, brief) = briefed(&context);
    passes(&context, 2);
    assert_eq!(
        opened_with_the_brief(&context),
        1,
        "zeus's window opened with it"
    );
    assert_eq!(context.message(brief).state, "delivering");
    context.close_project(project).unwrap();
    context.settle();
    assert_eq!(context.message(brief).state, "queued", "back in its queue");
    assert_eq!(context.message(brief).attempts, 0, "its attempt given back");
    assert_ne!(context.task(project, 1).task.state, "failed");
    assert_eq!(told_failed(&context, project), Vec::<String>::new());
    context.adapter.forgets_first_messages.set(false);
    context.resume_project(project).unwrap();
    passes(&context, 4);
    assert_eq!(context.message(brief).state, "delivered");
    assert_eq!(
        opened_with_the_brief(&context),
        2,
        "once more, after the Resume"
    );
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(told_failed(&context, project), Vec::<String>::new());
    context.close();
}

#[test]
fn a_brief_its_window_read_as_it_opened_and_recorded_before_the_close_is_confirmed_and_never_given_again(
) {
    // The record shows the brief, and no look confirmed it before the Close: it arrived,
    // so the task pauses with the Close and the Resume goes on in the same conversation,
    // with no second brief.
    let context = Context::new();
    // A window opened again on its conversation reads its record again.
    context.adapter.resumes_records.set(true);
    let (project, brief) = briefed(&context);
    passes(&context, 1);
    assert_eq!(opened_with_the_brief(&context), 1);
    assert_eq!(context.message(brief).state, "delivering");
    let native = context.adapter.agent("zeus").native.clone();
    context.close_project(project).unwrap();
    context.settle();
    assert_eq!(
        context.message(brief).state,
        "delivered",
        "its record shows it"
    );
    assert_ne!(context.task(project, 1).task.state, "failed");
    assert_eq!(told_failed(&context, project), Vec::<String>::new());
    context.resume_project(project).unwrap();
    passes(&context, 4);
    assert_eq!(context.message(brief).state, "delivered");
    assert_eq!(opened_with_the_brief(&context), 1, "never given again");
    assert_eq!(
        context.adapter.agent("zeus").native,
        native,
        "the same conversation"
    );
    let briefs = context
        .adapter
        .agent("zeus")
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains("Parser"))
        .count();
    assert_eq!(briefs, 1, "the conversation holds the brief once");
    assert_ne!(context.task(project, 1).task.state, "failed");
    context.close();
}

#[test]
fn a_message_on_its_last_attempt_whose_project_closes_goes_back_and_waits_for_the_resume() {
    // Two pastes went without the record showing them, which spent two of its three
    // attempts; the Close comes while the third is on its way. The window went with its
    // project: the attempt is given back, never the message given up.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]).id;
    passes(&context, 1);
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project, "zeus", "chief", "Parser done");
    context.pass().unwrap();
    for _ in 0..2 {
        context.advance(31_000);
        context.pass().unwrap();
    }
    let on_its_way = context.message(note.id);
    assert_eq!(
        (on_its_way.state.as_str(), on_its_way.attempts),
        ("delivering", 3)
    );
    context.close_project(project).unwrap();
    context.settle();
    let back = context.message(note.id);
    assert_eq!(back.state, "queued", "back in its queue: {:?}", back.reason);
    assert_eq!(back.attempts, 2, "the Close's attempt given back");
    assert_eq!(told_failed(&context, project), Vec::<String>::new());
    context.adapter.with("chief", |agent| agent.arrive = true);
    context.resume_project(project).unwrap();
    passes(&context, 4);
    assert_eq!(context.message(note.id).state, "delivered");
    context.close();
}
