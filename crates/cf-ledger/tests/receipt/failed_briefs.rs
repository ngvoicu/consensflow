//! A brief given back for a task the daemon failed meanwhile (T-270, on top of B4's
//! `withdrawn_late`, which holds for the chief's own step): a task's pane died while
//! its brief was being pasted, the task failed, and the paste is given back. The brief
//! was queued again, for a window the task no longer has; nothing but the task's
//! reopening wants it. It is cancelled instead. A note about the failed task still goes.

use crate::fixture::world;

#[test]
fn a_brief_given_back_after_its_task_failed_is_cancelled_not_queued_for_a_window_again() {
    // The daemon failed the task (its pane died) while its brief was being
    // pasted; the paste is given back. Only the brief of a task that failed is
    // cancelled: a note about it still reaches its window.
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (parser, brief) = (created.task.number, created.message.expect("a brief").id);
    w.begin(brief);
    w.ledger
        .fail_task(w.project, parser, "its pane died")
        .expect("the task fails");

    let given_back = w
        .ledger
        .retry_delivery(brief, "the window went before it landed", true)
        .expect("settled");

    assert_eq!(
        (given_back.state.as_str(), given_back.reason.as_deref()),
        (
            "cancelled",
            Some(format!("T-{parser} failed before it landed").as_str())
        )
    );
    let note = w.note("chief", "zeus", parser, "It failed").id;
    w.begin(note);
    let note = w
        .ledger
        .retry_delivery(note, "the window went before it landed", true)
        .expect("settled");
    assert_eq!(
        note.state, "queued",
        "a note about a failed task still goes"
    );
}
