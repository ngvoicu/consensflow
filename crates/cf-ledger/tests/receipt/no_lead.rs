//! What a project with one chief and no lead does today, pinned before leads are built
//! (the leads design, step 0, baselines of design section 1): who gives a task, who is
//! told its result and its failure, who a member's question and note go to, and who a
//! stranded wait is told. Each test is a sequence of what the callers do and asserts what
//! the ledger then says. A later step must leave every one of them green.

use super::fixture::{world, World};
use cf_ledger::{MessageView, NewTask};

/// The messages `handle` holds of `kind`, oldest first.
fn of_kind(w: &World, handle: &str, kind: &str) -> Vec<MessageView> {
    let mut found: Vec<_> = w
        .ledger
        .inbox(w.id(handle), 100)
        .expect("the inbox")
        .into_iter()
        .filter(|message| message.kind == kind)
        .collect();
    found.sort_by_key(|message| message.id);
    found
}

/// A task `requester` gave for a standard worker, at work in zeus with its brief delivered.
fn at_work(w: &mut World, requester: &str) -> (i64, String) {
    let created = w
        .ledger
        .create_task(
            w.project,
            &NewTask {
                from: requester.into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Parser".into(),
                ..NewTask::default()
            },
        )
        .expect("a task for the tier");
    let task = created.task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    (task, session)
}

#[test]
fn a_task_the_human_gives_for_a_pool_is_open_and_the_human_is_its_requester() {
    let mut w = world();
    let created = w
        .ledger
        .create_task(
            w.project,
            &NewTask {
                from: "human".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "From the page".into(),
                ..NewTask::default()
            },
        )
        .expect("the human gives a task");
    assert_eq!(created.task.requester, "human");
    assert_eq!(created.task.state, "open");
    assert_eq!(created.task.assignee, None);
    assert!(created.message.is_none(), "no window has it yet");
    // The chief's own task for the same pool: the same, with the chief as requester.
    let chief = w.open("From the chief");
    assert_eq!(chief.task.requester, "chief");
    assert_eq!(chief.task.state, "open");
}

#[test]
fn a_task_the_human_gives_the_chief_by_name_is_queued_for_the_chief_with_the_human_as_requester() {
    let mut w = world();
    let created = w
        .ledger
        .create_task(
            w.project,
            &NewTask {
                from: "human".into(),
                to: Some("chief".into()),
                body: "Plan the week".into(),
                ..NewTask::default()
            },
        )
        .expect("the human gives the chief a task");
    assert_eq!(created.task.requester, "human");
    assert_eq!(created.task.assignee.as_deref(), Some("chief"));
    assert_eq!(created.task.state, "queued");
    let brief = created.message.expect("its brief");
    assert_eq!(brief.recipient, "chief");
    assert_eq!(brief.sender.as_deref(), Some("human"));
    assert_eq!(brief.kind, "task");
    assert_eq!(w.next("chief"), Some(brief.id));
}

#[test]
fn a_member_hands_out_no_task_and_is_refused_in_the_words_the_board_gives() {
    let mut w = world();
    let (_, session) = at_work(&mut w, "chief");
    for to in [None, Some("diana".to_owned())] {
        let refused = w
            .ledger
            .create_task(
                w.project,
                &NewTask {
                    from: session.clone(),
                    to,
                    pool: Some("worker".into()),
                    tier: Some("standard".into()),
                    body: "More".into(),
                    ..NewTask::default()
                },
            )
            .expect_err("a member gives no task");
        assert_eq!(refused.code(), Some("not-a-coordinator"));
        assert_eq!(
            refused.to_string(),
            format!("@{session} is a worker: members do not hand out tasks")
        );
    }
}

#[test]
fn a_result_is_queued_for_the_requester_the_chief_or_the_human() {
    for requester in ["chief", "human"] {
        let mut w = world();
        let (task, session) = at_work(&mut w, requester);
        w.ledger
            .record_finished(w.project, task, "Done and checked", (0, 0))
            .expect("finished");
        let results = of_kind(&w, requester, "result");
        assert_eq!(results.len(), 1, "{requester}");
        assert_eq!(results[0].sender.as_deref(), Some(session.as_str()));
        assert_eq!(results[0].task_number, Some(task));
        assert_eq!(results[0].state, "queued");
        // The other of the two is told nothing of it.
        let other = if requester == "chief" {
            "human"
        } else {
            "chief"
        };
        assert!(of_kind(&w, other, "result").is_empty(), "{requester}");
    }
}

#[test]
fn a_failed_task_tells_its_requester_the_chief_or_the_human_and_not_the_other() {
    for requester in ["chief", "human"] {
        let mut w = world();
        let (task, _) = at_work(&mut w, requester);
        // The daemon gives up on it, as it does for a window that cannot go on.
        w.ledger
            .give_up_task(w.project, task, "its window failed")
            .expect("failed");
        let told = of_kind(&w, requester, "note");
        let failure = told
            .iter()
            .find(|message| {
                message
                    .body
                    .starts_with(&format!("T-{task} failed: its window failed"))
            })
            .unwrap_or_else(|| panic!("{requester} is told: {told:?}"));
        assert_eq!(failure.task_number, Some(task));
        let other = if requester == "chief" {
            "human"
        } else {
            "chief"
        };
        assert!(
            of_kind(&w, other, "note")
                .iter()
                .all(|message| !message.body.contains(&format!("T-{task} failed"))),
            "{requester}"
        );
    }
}

#[test]
fn a_member_s_question_is_the_chief_s_whoever_gave_its_task() {
    for requester in ["chief", "human"] {
        let mut w = world();
        let (task, session) = at_work(&mut w, requester);
        let question = w.ask(&session, task, "Which branch?");
        assert_eq!(question.recipient, "chief", "{requester}");
        assert_eq!(question.task_number, Some(task));
        assert_eq!(of_kind(&w, "chief", "question").len(), 1, "{requester}");
        assert!(of_kind(&w, "human", "question").is_empty(), "{requester}");
    }
}
