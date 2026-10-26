//! Words told to a task still open (`cf tell`, T-534 item 4 of 2026-10-25): a task that
//! waits on the board for what it needs, or for a member, has no window to put them to,
//! and 3.0.6-alpha.2 refused the tell ("T-2 has no window to tell: it is open") while its
//! notes promised that words told to a task that has not started wait for its window.
//!
//! What the board does: the words wait in the task's brief, as the words a resume says to a
//! task with no window do (`Resumed:`) and the words kept for a window that ended do
//! (`Kept from before`), and whichever window takes the task is given them once, with the
//! brief. A held-back note cannot wait for a window that does not exist: a message is
//! addressed to a participant, and an open task of a tier has none until the daemon picks
//! a member. A task a window has or is queued for is told as before.

use cf_ledger::NewTask;

use crate::fixture::{world, World};

/// Task `number`'s brief as the ledger holds it.
fn brief(w: &World, number: i64) -> String {
    w.ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .body
}

/// What the chief's tells to open tasks left in the log: the task, who, the words.
fn told(w: &World) -> Vec<(i64, String, String)> {
    w.ledger
        .events(w.project, 0, 500)
        .expect("the events")
        .into_iter()
        .filter(|event| event.kind == "task.told")
        .map(|event| {
            let text = |field: &str| event.data[field].as_str().unwrap_or_default().to_owned();
            (
                event.data["task"].as_i64().unwrap_or_default(),
                text("by"),
                text("words"),
            )
        })
        .collect()
}

/// How many times `words` is in `text`.
fn times(text: &str, words: &str) -> usize {
    text.matches(words).count()
}

/// A task done by a session, and a follow-up of it that waits for a task of `diana`'s.
fn waiting_follow_up(w: &mut World) -> (String, i64, i64, i64) {
    let a = w.open("Parser").task.number;
    let moved = w.assign(a, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    w.ledger
        .record_result(w.project, a, "Parser, done")
        .expect("a result");
    let need = w.open("Lexer").task.number;
    let follow_up = w.follow_up(a, "Tests", &[need]).expect("a follow-up");
    assert_eq!(follow_up.task.state, "open");
    (session, a, need, follow_up.task.number)
}

#[test]
fn words_told_to_an_open_task_wait_in_its_brief_and_the_member_that_takes_it_is_given_them_once() {
    let mut w = world();
    let task = w.open("Parse the old way").task.number;

    let told_task = w
        .ledger
        .tell_open(w.project, task, "chief", "Use the v2 grammar")
        .expect("an open task is told");
    assert_eq!(told_task.state, "open", "nothing is paused or queued");
    assert_eq!(
        told_task.body,
        "Parse the old way\n\nTold by @chief before a window had it: Use the v2 grammar"
    );
    assert_eq!(
        told_task.title, "Parse the old way",
        "the title is the brief's"
    );
    assert_eq!(brief(&w, task), told_task.body);
    assert_eq!(
        told(&w),
        [(task, "chief".to_owned(), "Use the v2 grammar".to_owned())]
    );

    let moved = w.assign(task, "zeus");
    let given = moved.message.expect("its brief");
    assert_eq!(
        given.body, told_task.body,
        "the member has them with the brief"
    );
    assert_eq!(times(&given.body, "Use the v2 grammar"), 1);
    let begun = w.deliver(given.id);
    assert!(
        begun.carried.is_empty(),
        "no message of its own waits for it"
    );
    assert_eq!(w.state(task), "working");
}

#[test]
fn several_tells_keep_the_order_they_were_told_in_and_each_goes_in_once() {
    let mut w = world();
    let task = w.open("Parse it").task.number;
    for words in ["First", "Second\nover two lines", "Third"] {
        w.ledger
            .tell_open(w.project, task, "chief", words)
            .expect("told");
    }
    let moved = w.assign(task, "diana");
    let body = moved.message.expect("its brief").body;
    let at = |words: &str| {
        body.find(words)
            .unwrap_or_else(|| panic!("{words} is not said: {body}"))
    };
    assert!(
        at("First") < at("Second\nover two lines") && at("Second\nover two lines") < at("Third")
    );
    for words in ["First", "Second\nover two lines", "Third"] {
        assert_eq!(times(&body, words), 1, "{words}: {body}");
    }
}

#[test]
fn a_follow_up_waiting_on_the_board_is_told_and_its_window_has_the_words_once_what_it_needs_is_accepted(
) {
    let mut w = world();
    let (session, _, need, later) = waiting_follow_up(&mut w);

    w.ledger
        .tell_open(w.project, later, "chief", "Cover the empty file")
        .expect("a follow-up that waits is told");
    assert_eq!(w.state(later), "open", "it still waits for what it needs");
    assert_eq!(w.next(&session), None, "nothing goes into its window yet");

    let moved = w.assign(need, "diana");
    w.deliver(moved.message.expect("its brief").id);
    w.ledger
        .record_result(w.project, need, "Lexer, done")
        .expect("a result");
    w.ledger
        .accept_task(w.project, need, "chief")
        .expect("accepted");
    assert_eq!(w.state(later), "queued");
    let head = w.next(&session).expect("the follow-up for the window");
    let said = w.message(head).body;
    assert_eq!(
        said,
        "Tests\n\nTold by @chief before a window had it: Cover the empty file"
    );
    let begun = w.deliver(head);
    assert!(begun.carried.is_empty());
    assert_eq!(w.state(later), "working");
    assert_eq!(w.next(&session), None, "the words go in once");
}

#[test]
fn a_task_a_window_has_is_not_told_as_an_open_one_and_says_where_its_words_go() {
    let mut w = world();
    let queued = w.give("zeus", "Parser").task.number;
    let refused = w
        .ledger
        .tell_open(w.project, queued, "chief", "More")
        .expect_err("a queued task has its words kept for the window");
    assert_eq!(refused.code(), Some("invalid-transition"));
    assert!(
        refused
            .to_string()
            .contains("cannot tell the window of T-1: it is queued"),
        "{refused}"
    );
    assert_eq!(brief(&w, queued), "Parser", "its brief is as it was");

    let done = w.open("Lexer").task.number;
    let moved = w.assign(done, "diana");
    w.deliver(moved.message.expect("its brief").id);
    w.ledger
        .record_result(w.project, done, "done")
        .expect("a result");
    assert!(w
        .ledger
        .tell_open(w.project, done, "chief", "More")
        .is_err());
    assert!(told(&w).is_empty());
}

#[test]
fn no_words_and_words_that_would_outgrow_the_brief_are_refused_and_change_nothing() {
    let mut w = world();
    let task = w.open("Parse it").task.number;
    let blank = w
        .ledger
        .tell_open(w.project, task, "chief", "  ")
        .expect_err("no words");
    assert_eq!(blank.code(), Some("invalid-text"));
    let long = "x".repeat(cf_ledger::model::MAX_BODY);
    let too_long = w
        .ledger
        .tell_open(w.project, task, "chief", &long)
        .expect_err("the brief would outgrow its limit");
    assert_eq!(too_long.code(), Some("invalid-text"));
    assert_eq!(brief(&w, task), "Parse it");
    assert!(told(&w).is_empty());
}

#[test]
fn the_chiefs_own_step_is_not_told_its_brief_is_the_chiefs_to_change() {
    let mut w = world();
    let blocker = w.open("Parser").task.number;
    let step = w
        .ledger
        .create_task(
            w.project,
            &NewTask {
                from: "chief".into(),
                to: Some("chief".into()),
                needs: vec![u64::try_from(blocker).unwrap()],
                body: "Review the parser".into(),
                ..NewTask::default()
            },
        )
        .expect("an own step")
        .task
        .number;
    assert_eq!(w.state(step), "open");
    let refused = w
        .ledger
        .tell_open(w.project, step, "chief", "More")
        .expect_err("it has no window but the chief's");
    assert_eq!(refused.code(), Some("own-work"));
    assert!(
        refused.to_string().contains("cf task edit T-2"),
        "{refused}"
    );
    assert_eq!(brief(&w, step), "Review the parser");
}
