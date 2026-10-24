//! Words the chief tells a task's window before the window has the task (`cf
//! tell` to a task still queued, the field report of 2026-10-15) go in after its
//! brief, once each: in the brief's own paste while it waits, or as the window's
//! next message once a launch has begun with the brief, and then the window's
//! answer to the brief is no result until it has had them. Words that never
//! reach the window fail the task, but for words whose last paste fails while
//! the task is paused, which are kept for its resume (the second review,
//! 2026-10-16); words the window took back go in again with the words that
//! resume it; and words in the first message of a launch called off before its
//! window opened are said to have gone with it, as are those of a task whose
//! member the human removes before they went in (T-389); words still queued
//! when the harness fails the task wait for its reopen (T-448). None of these
//! tests is held to a Node recording: Node refused the tell.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_ledger::{NewNote, NewTask};

use crate::fixtures::{assert_match, given, last_message, notes, Tiers};
use crate::work_in_flight::looks_at;

/// The chief tells the window of T-`number`, still queued, `body`: the id of the
/// note that keeps the words.
pub(crate) fn tell(context: &Context, project: i64, number: i64, body: &str) -> i64 {
    context
        .ledger
        .borrow_mut()
        .tell_queued(project, number, "chief", body)
        .unwrap()
        .id
}

/// T-1, "Parser", given a session of zeus, whose launch has begun with the
/// brief when the chief tells it "Use the v2 grammar"; then the window comes up
/// with the brief and works on it. The staff, the session's handle and the
/// note that keeps the words.
pub(crate) fn told_on_its_way(context: &Context) -> (Tiers<'_>, String, i64) {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open_body("Parser");
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let brief = tiers.task(1).messages[0].id;
    assert_eq!(context.message(brief).state, "delivering");
    let told = tell(context, tiers.project.id, 1, "Use the v2 grammar");
    context.adapter.busy("zeus");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(context.message(told).state, "queued");
    (tiers, session, told)
}

#[test]
fn words_told_before_the_window_opens_ride_in_its_first_message_after_the_brief() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    let told = tell(&context, project.id, 1, "Use the v2 grammar");
    context.pass().unwrap();
    let first = last_message(&context);
    assert_match(
        &first,
        &format!(
            r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nParser\n\n\(kept for you: note m-{told} from @chief\)\nUse the v2 grammar$"
        ),
    );
    assert_eq!(first.matches("[ConsensFlow m-").count(), 1, "one marker");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    assert_eq!(context.message(told).state, "delivered");
    assert_eq!(given(&context, "zeus").len(), 1, "one paste, once");
}

#[test]
fn words_told_while_the_brief_is_on_its_way_go_in_next_and_the_answer_after_them_is_the_result() {
    let context = Context::new();
    let (tiers, session, told) = told_on_its_way(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the answer to the brief is no result while the words wait"
    );
    let words = given(&context, &session);
    assert_eq!(words.len(), 2, "{words:?}");
    assert_match(
        &words[0],
        r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nParser$",
    );
    assert_eq!(
        words[1],
        format!("[ConsensFlow m-{told} · T-1 · note from @chief]\nUse the v2 grammar"),
        "the words, next, on their own"
    );

    // The window answers the words: that answer is the task's result, once.
    context.pass().unwrap();
    assert_eq!(context.message(told).state, "delivered");
    context
        .adapter
        .answer("zeus", "Parser done, on the v2 grammar");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let thread = tiers.task(1);
    assert_eq!(thread.task.state, "done");
    let results: Vec<_> = thread
        .messages
        .iter()
        .filter(|m| m.kind == "result")
        .collect();
    assert_eq!(results.len(), 1, "one result: {results:?}");
    assert_eq!(results[0].body, "Parser done, on the v2 grammar");
    let to_the_chief = given(&context, "chief")
        .iter()
        .filter(|item| item.contains(&marker_of(results[0].id)))
        .count();
    assert_eq!(to_the_chief, 1, "pasted into the chief's window once");
    assert_eq!(given(&context, &session).len(), 2, "each went in once");
}

#[test]
fn words_that_never_reach_the_window_fail_the_task_and_the_answer_to_the_brief_is_no_result() {
    let context = Context::new();
    let (tiers, session, told) = told_on_its_way(&context);
    // The window answers the brief, and takes no paste after it.
    context.adapter.answer("zeus", "Parser done");
    context.adapter.with("zeus", |agent| agent.admit = false);
    for _ in 0..5 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(told).state, "failed");
    let thread = tiers.task(1);
    assert_eq!(thread.task.state, "failed");
    assert!(
        !thread.messages.iter().any(|m| m.kind == "result"),
        "the answer to the brief alone is no result"
    );
    assert!(
        tiers.notes("chief").contains(&format!(
            "T-1 failed: what @chief told its window, m-{told}, did not reach it (refused by the test). Reopen it with: cf task reopen T-1 \"…\""
        )),
        "{:?}",
        tiers.notes("chief")
    );
    assert_eq!(given(&context, &session).len(), 1, "the brief alone");
}

#[test]
fn words_the_window_took_back_at_a_pause_go_in_again_with_the_words_that_resume_the_task() {
    let context = Context::new();
    let (tiers, session, told) = told_on_its_way(&context);
    let project = tiers.project.id;
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    // The words are pasted; stopped before a word of its answer, the window has
    // them in its input box again, as Claude does.
    context.adapter.busy_until_taken_back("zeus");
    context.pass().unwrap();
    assert_eq!(context.message(told).state, "delivered");
    context.human_pause(project, 1);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.message(told).state,
        "queued",
        "kept for the window again"
    );

    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    let words = given(&context, &session);
    assert_match(
        words.last().unwrap(),
        &format!(
            r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\n\(kept for you: note m-{told} from @chief\)\nUse the v2 grammar\n\nResumed: Carry on$"
        ),
    );
    context.pass().unwrap();
    assert_eq!(context.message(told).state, "delivered");
    context
        .adapter
        .answer("zeus", "Parser done, on the v2 grammar");
    context.pass().unwrap();
    let thread = tiers.task(1);
    assert_eq!(thread.task.state, "done");
    let result = thread.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(result.body, "Parser done, on the v2 grammar");
}

#[test]
fn words_whose_last_paste_fails_while_the_human_pauses_are_kept_for_the_resume_and_why_is_told_and_no_failure(
) {
    let context = Context::new();
    let (tiers, session, told) = told_on_its_way(&context);
    let project = tiers.project.id;
    context.adapter.answer("zeus", "Parser done");
    context.adapter.with("zeus", |agent| agent.admit = false);
    // Two pastes refused, each tried again.
    for _ in 0..10 {
        let words = context.message(told);
        if words.attempts == 2 && words.state == "queued" {
            break;
        }
        context.pass().unwrap();
    }
    assert_eq!(context.message(told).attempts, 2);
    // The last is under way when the human pauses the task, and is refused.
    let launch = context.adapter.agent("zeus").launch;
    let held = context.adapter.deliver_holds.hold(looks_at(launch));
    context.pass().unwrap();
    assert_eq!(context.message(told).state, "delivering");
    context.human_pause(project, 1);
    held.open();
    context.settle();
    let kept = context.message(told);
    assert_eq!(
        (kept.state.as_str(), kept.reason.as_deref()),
        ("queued", Some("kept for T-1's resume: refused by the test")),
        "never given up"
    );
    assert_eq!(tiers.task(1).task.state, "paused");
    assert!(
        !tiers
            .notes("human")
            .iter()
            .any(|note| note.contains("did not reach")),
        "{:?}",
        tiers.notes("human")
    );
    assert!(
        !tiers
            .notes("chief")
            .iter()
            .any(|note| note.contains("failed")),
        "{:?}",
        tiers.notes("chief")
    );
    // Whoever is told hears why the words wait, once each: the chief in a note of
    // its own, there being no note of a pause the human made; the human as main
    // told it, but with no "did not reach".
    let named = |handle: &str| -> Vec<String> {
        tiers
            .notes(handle)
            .into_iter()
            .filter(|said| said.starts_with(&format!("m-{told},")))
            .collect()
    };
    assert_eq!(
        named("chief"),
        [format!(
            "m-{told}, what @chief told T-1's window, waits for T-1's resume: refused by the test."
        )]
    );
    assert_eq!(
        named("human"),
        [format!(
            "m-{told}, a note from @chief on T-1, waits for T-1's resume to reach @{session}: refused by the test."
        )]
    );

    context.adapter.with("zeus", |agent| agent.admit = true);
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    assert_match(
        given(&context, &session).last().unwrap(),
        &format!(
            r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\n\(kept for you: note m-{told} from @chief\)\nUse the v2 grammar\n\nResumed: Carry on$"
        ),
    );
}

#[test]
fn a_workers_note_to_the_chief_that_never_arrives_fails_nothing_and_the_human_hears_of_it_once() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open_body("Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    context.adapter.with("chief", |agent| agent.admit = false);
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some(session.clone()),
                to: "chief".to_owned(),
                task: Some(1),
                body: "Halfway there".to_owned(),
            },
        )
        .unwrap()
        .id;
    for _ in 0..5 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(note).state, "failed");
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "words to the chief are no words for the task's window"
    );
    assert_eq!(
        tiers.notes("human"),
        [format!(
            "m-{note}, a note from @{session} on T-1, did not reach @chief: refused by the test."
        )]
    );
    assert!(
        !tiers
            .notes("chief")
            .iter()
            .any(|said| said.contains("T-1 failed")),
        "{:?}",
        tiers.notes("chief")
    );
}

#[test]
fn words_in_the_first_message_of_a_launch_cancelled_before_its_window_opened_are_said_to_have_gone()
{
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let preparing = context.adapter.prepare_holds.hold(|_| true);
    let brief = context.give(project.id, "zeus", "Parser").message.unwrap();
    let told = tell(&context, project.id, 1, "Use the v2 grammar");
    context.pass().unwrap();
    assert_eq!(context.message(brief.id).state, "delivering");

    // The human calls the task off while the window is prepared.
    context
        .ledger
        .borrow_mut()
        .cancel_task(project.id, 1, "human")
        .unwrap();
    preparing.open();
    context.settle();
    assert!(context.host.last("zeus").is_none(), "no pane opened");
    let chief = context.id(project.id, "chief");
    assert!(
        notes(&context, chief).contains(&format!(
            "T-1 was cancelled before its window opened. What you told its window, m-{told}, went with it: it never had it."
        )),
        "{:?}",
        notes(&context, chief)
    );
}

#[test]
fn words_told_to_the_window_of_a_task_whose_member_the_human_removes_are_said_to_have_gone() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    // The human asked for it; the chief told its window, which is not up yet.
    context.create_task(
        project.id,
        NewTask {
            from: "human".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Parser".to_owned(),
            ..NewTask::default()
        },
    );
    let told = tell(&context, project.id, 1, "Use the v2 grammar");

    let removed = context.remove_member(project.id, "zeus").unwrap();
    assert_eq!(removed.cancelled, [1]);
    assert_eq!(context.message(told).state, "cancelled");
    let chief = context.id(project.id, "chief");
    assert!(
        notes(&context, chief).contains(&format!(
            "T-1 was cancelled: @zeus left the staff. What you told its window, m-{told}, went with it: it never had it."
        )),
        "{:?}",
        notes(&context, chief)
    );
}

#[test]
fn words_still_on_their_way_when_the_harness_fails_the_task_wait_for_its_reopen() {
    let context = Context::new();
    let (tiers, session, told) = told_on_its_way(&context);
    context.adapter.with("zeus", |agent| agent.failed = true);
    context.adapter.answer("zeus", "Parser, half done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "failed");
    // Kept for the reopen, as words a failed delivery leaves are (the chief's
    // decision on Calliope's P3-4 of T-446); one note says why, which words wait
    // and how to reopen it (her P3 of T-437: the words came in a note of their own).
    assert_eq!(context.message(told).state, "queued");
    assert_eq!(
        tiers.notes("chief"),
        [format!("T-1 failed: @{session}'s harness reported a failure; the words told its window wait for the reopen: m-{told}. Reopen it with: cf task reopen T-1 \"…\"")]
    );
}

#[test]
fn words_that_never_reach_the_window_are_told_to_whoever_told_them_as_to_the_requester() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    // The human asked for it; the chief told its window while the brief was on
    // its way (Calliope's P3 of T-437: only the requester heard).
    context.create_task(
        project.id,
        NewTask {
            from: "human".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    let brief = context.task(project.id, 1).messages[0].id;
    assert_eq!(context.message(brief).state, "delivering");
    let told = tell(&context, project.id, 1, "Use the v2 grammar");
    context.adapter.busy("zeus");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser done");
    context.adapter.with("zeus", |agent| agent.admit = false);
    for _ in 0..5 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(told).state, "failed");
    assert_eq!(context.task(project.id, 1).task.state, "failed");
    let said = format!(
        "T-1 failed: what @chief told its window, m-{told}, did not reach it (refused by the test). Reopen it with: cf task reopen T-1 \"…\""
    );
    let chief = context.id(project.id, "chief");
    assert!(
        notes(&context, chief).contains(&said),
        "{:?}",
        notes(&context, chief)
    );
    let human = context.id(project.id, "human");
    assert!(
        notes(&context, human).contains(&said),
        "{:?}",
        notes(&context, human)
    );
}
