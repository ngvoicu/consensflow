//! Words the chief tells a task's window before the window has the task (the
//! field report of 2026-10-15: `cf tell` to a task still queued was refused). They
//! are kept for the window, nothing is paused, and they go in after the brief:
//! in its paste while it waits, or as the window's next message once it is on its
//! way. A task called off first takes them with it, and whoever told them hears
//! so (`messages/told.rs`).

use cf_ledger::{MessageView, NewTask};

use crate::fixture::{gated_world, ids, world, World};

/// A task opened for the standard workers and given to a session of zeus, its
/// brief queued for the session's window: the task's number, the session's
/// handle and the brief's id.
pub(crate) fn queued(w: &mut World) -> (i64, String, i64) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    (task, session, moved.message.expect("its brief").id)
}

/// The chief tells the window of T-`number`, still queued, `body`.
pub(crate) fn tell(w: &mut World, number: i64, body: &str) -> MessageView {
    w.ledger
        .tell_queued(w.project, number, "chief", body)
        .expect("the words are kept")
}

/// The id of T-`number`, as the ledger's callers ask about a task with it.
pub(crate) fn task_id(w: &World, number: i64) -> i64 {
    w.ledger.task(w.project, number).unwrap().unwrap().task.id
}

/// Whether words told to `session`'s window about T-`number` are still on their way.
pub(crate) fn waiting(w: &World, session: &str, number: i64) -> bool {
    w.ledger
        .notes_waiting(w.id(session), task_id(w, number))
        .unwrap()
}

/// The newest message for `handle`.
pub(crate) fn newest(w: &World, handle: &str) -> MessageView {
    w.ledger.inbox(w.id(handle), 1).unwrap().remove(0)
}

#[test]
fn words_told_while_the_brief_waits_ride_in_its_paste_after_it_in_the_order_told_and_pause_nothing()
{
    let mut w = world();
    let (task, session, brief) = queued(&mut w);
    let first = tell(&mut w, task, "Use the v2 grammar");
    let second = tell(&mut w, task, "And keep the old tests");
    assert_eq!(
        [first.kind.as_str(), first.state.as_str()],
        ["note", "queued"]
    );
    assert_eq!(first.sender.as_deref(), Some("chief"));
    assert_eq!(first.recipient, session);
    assert_eq!(first.task_number, Some(task));
    assert_eq!(w.state(task), "queued", "nothing is paused");
    assert!(waiting(&w, &session, task));
    assert_eq!(w.next(&session), Some(brief), "one paste, the brief's");

    let begun = w.deliver(brief);
    assert_eq!(
        ids(&begun.carried),
        [first.id, second.id],
        "after the brief, in the order they were told"
    );
    assert_eq!(w.states(&[first.id, second.id]), ["delivered", "delivered"]);
    assert_eq!(w.state(task), "working");
    assert!(!waiting(&w, &session, task), "the window has them");
    assert_eq!(w.next(&session), None, "nothing is left to paste");
}

#[test]
fn words_told_once_the_brief_is_on_its_way_go_in_next_and_the_answer_after_them_is_the_result() {
    let mut w = world();
    let (task, session, brief) = queued(&mut w);
    // A launch has begun: the brief is on its way, and nothing more rides in it.
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    assert_eq!(w.state(task), "working");
    assert_eq!(w.states(&[told.id]), ["queued"], "it rode in no paste");
    assert!(
        waiting(&w, &session, task),
        "the window's answer to the brief is no result yet"
    );
    assert_eq!(
        w.next(&session),
        Some(told.id),
        "the window's next message, while it works on its task"
    );

    w.deliver(told.id);
    assert!(!waiting(&w, &session, task));
    let latest = w
        .ledger
        .last_pasted(w.id(&session), task_id(&w, task))
        .unwrap()
        .expect("a message pasted");
    assert_eq!(
        latest.id, told.id,
        "what the window writes after the words is the result"
    );
    assert_eq!(w.state(task), "working", "receiving them moves nothing");
}

#[test]
fn only_a_queued_task_of_a_members_window_keeps_words_and_no_words_are_none() {
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    let blank = w
        .ledger
        .tell_queued(w.project, task, "chief", "  ")
        .unwrap_err();
    assert_eq!(blank.code(), Some("invalid-text"));

    w.deliver(brief);
    let working = w
        .ledger
        .tell_queued(w.project, task, "chief", "Too late")
        .unwrap_err();
    assert_eq!(working.code(), Some("invalid-transition"));
    assert_eq!(
        working.to_string(),
        format!("cannot keep words for the window of T-{task}: it is working")
    );

    let open = w.open("Lexer").task.number;
    let refused = w
        .ledger
        .tell_queued(w.project, open, "chief", "Hey")
        .unwrap_err();
    assert_eq!(
        refused.code(),
        Some("invalid-transition"),
        "it has no window yet"
    );

    // The chief's own step has no window but the chief's.
    let own = w.give("chief", "Check the release").task.number;
    assert_eq!(w.state(own), "queued");
    let refused = w
        .ledger
        .tell_queued(w.project, own, "chief", "Hey")
        .unwrap_err();
    assert_eq!(refused.code(), Some("own-work"));
}

#[test]
fn a_task_the_human_cancels_first_takes_the_words_and_the_chief_hears_which_went() {
    let mut w = world();
    let (task, session, _) = queued(&mut w);
    let first = tell(&mut w, task, "Use the v2 grammar");
    let second = tell(&mut w, task, "And keep the old tests");
    let unreceived = w.ledger.told_unreceived(w.project, task).unwrap();
    assert_eq!(ids(&unreceived), [first.id, second.id]);

    w.ledger
        .cancel_task(w.project, task, "human")
        .expect("cancelled");
    assert_eq!(w.states(&[first.id, second.id]), ["cancelled", "cancelled"]);
    assert!(!waiting(&w, &session, task));
    assert_eq!(
        newest(&w, "chief").body,
        format!(
            "@human cancelled T-{task} (Parser). What you told its window, m-{}, m-{}, went with it: it never had them.",
            first.id, second.id
        )
    );
}

#[test]
fn a_teller_who_neither_asked_for_the_task_nor_called_it_off_hears_which_of_its_words_went() {
    let mut w = world();
    // The human asked for this one, and calls it off itself; the chief told its window.
    let task = w
        .ledger
        .create_task(
            w.project,
            &NewTask {
                from: "human".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Parser".into(),
                ..NewTask::default()
            },
        )
        .expect("the human's task")
        .task
        .number;
    w.assign(task, "zeus");
    let told = tell(&mut w, task, "Use the v2 grammar");
    let notes = w.ledger.inbox(w.id("chief"), 100).unwrap().len();
    w.ledger
        .cancel_task(w.project, task, "human")
        .expect("cancelled");
    let chief = w.ledger.inbox(w.id("chief"), 100).unwrap();
    assert_eq!(chief.len(), notes + 1, "one note, of the words alone");
    assert_eq!(
        chief[0].body,
        format!(
            "@human cancelled T-{task} (Parser). What you told its window, m-{}, went with it: it never had it.",
            told.id
        )
    );
}

#[test]
fn a_chief_that_cancels_hears_nothing_more_and_words_the_window_had_never_went() {
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.deliver(brief);
    assert!(
        w.ledger
            .told_unreceived(w.project, task)
            .unwrap()
            .is_empty(),
        "the window had them with the brief"
    );
    let notes = w.ledger.inbox(w.id("chief"), 100).unwrap().len();
    w.ledger
        .cancel_task(w.project, task, "chief")
        .expect("cancelled");
    assert_eq!(w.states(&[told.id]), ["delivered"]);
    assert_eq!(
        w.ledger.inbox(w.id("chief"), 100).unwrap().len(),
        notes,
        "who called it off is told nothing"
    );
}

#[test]
fn a_brief_the_human_declines_takes_the_words_held_with_it_and_the_chief_hears_which_went() {
    let mut w = gated_world();
    let (task, _, brief) = queued(&mut w);
    assert_eq!(w.message(brief).state, "gated");
    let told = tell(&mut w, task, "Use the v2 grammar");
    assert_eq!(told.state, "gated", "one agent's word to another");

    w.ledger.decline_message(brief, "human").expect("declined");
    assert_eq!(w.state(task), "cancelled");
    assert_eq!(w.states(&[told.id]), ["cancelled"]);
    assert_eq!(
        newest(&w, "chief").body,
        format!(
            "@human declined T-{task} (Parser). It is cancelled. What you told its window, m-{}, went with it: it never had it.",
            told.id
        )
    );
}

#[test]
fn words_held_at_the_gate_join_the_brief_when_the_human_passes_them_on() {
    let mut w = gated_world();
    let (task, _, brief) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.ledger
        .approve_message(told.id, "human")
        .expect("passed on");
    w.ledger.approve_message(brief, "human").expect("passed on");
    let begun = w.deliver(brief);
    assert_eq!(ids(&begun.carried), [told.id]);
}

#[test]
fn a_human_pause_keeps_the_words_for_the_resume_and_the_chiefs_takes_them_back_with_its_brief() {
    let mut w = world();
    let (task, session, _) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    let carrier = w.resume(task, "Go on").message.expect("its words");
    assert_eq!(w.next(&session), Some(carrier.id));
    let begun = w.deliver(carrier.id);
    assert!(ids(&begun.carried).contains(&told.id), "kept, and carried");

    // The chief's own pause takes back what it wrote for the window and had not
    // reached it, its words as its brief: what is new goes in its resume words.
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.pause(task);
    assert_eq!(w.states(&[brief, told.id]), ["cancelled", "cancelled"]);
}

#[test]
fn a_task_taken_back_to_the_board_carries_the_words_into_the_next_windows_brief() {
    let mut w = world();
    let (task, session, _) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.ledger
        .release_task(w.project, task, "ran out of quota")
        .expect("taken back");
    let body = w.ledger.task(w.project, task).unwrap().unwrap().task.body;
    assert!(
        body.contains(&format!(
            "Kept from before, never delivered to @{session}:\n\n(note m-{} from @chief)\nUse the v2 grammar",
            told.id
        )),
        "{body}"
    );
}
