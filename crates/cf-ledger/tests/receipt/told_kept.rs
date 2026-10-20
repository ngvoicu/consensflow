//! Words told to a task's window before it was up, where the second review of
//! the change (2026-10-16) found them misread or lost: a note about the task to
//! anyone but its window is no such words, and its failure fails nothing; a
//! resume keeps the order they were told in, whatever order the human passes
//! them on in; words whose last paste failed while the task was paused are
//! kept for its resume; and words that rode in a brief the daemon stopped while
//! pasting keep their place in it across the start (the reviews of round 4:
//! they were let go of and pasted again on their own).

use crate::fixture::{gated_world, ids, world, World};
use crate::queued_tells::{queued, tell, waiting};

#[test]
fn a_workers_note_to_the_chief_that_never_arrives_fails_nothing_of_its_task() {
    let mut w = world();
    let (task, session, brief) = queued(&mut w);
    w.deliver(brief);
    assert_eq!(w.state(task), "working");
    let note = w.note(&session, "chief", task, "Halfway there");
    w.begin(note.id);
    w.ledger
        .fail_delivery(note.id, "refused by the window")
        .expect("given up");
    assert_eq!(w.message(note.id).state, "failed");
    assert_eq!(
        w.state(task),
        "working",
        "words to the chief are not words for the task's window"
    );
    assert!(!cf_ledger::is_told(&w.message(note.id), Some(&session)));
    let result = w
        .ledger
        .record_result(w.project, task, "Parser done")
        .expect("the window's answer is its result");
    assert_eq!(result.task.state, "done");
}

/// Two words told while the brief is on its way, the human holding both and
/// passing on the second; then the human pauses the task at work and the chief
/// resumes it, which the human passes on: the world, the task, the session, the
/// two words and the resume.
fn second_passed_on_then_resumed() -> (World, i64, String, i64, i64, i64) {
    let mut w = gated_world();
    let (task, session, brief) = queued(&mut w);
    w.ledger.approve_message(brief, "human").expect("passed on");
    w.begin(brief);
    let first = tell(&mut w, task, "First").id;
    let second = tell(&mut w, task, "Second").id;
    w.confirm(brief);
    w.ledger
        .approve_message(second, "human")
        .expect("passed on");
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    let resumed = w.resume(task, "Go on").message.expect("its words").id;
    w.ledger
        .approve_message(resumed, "human")
        .expect("passed on");
    (w, task, session, first, second, resumed)
}

#[test]
fn a_resume_keeps_the_order_the_words_were_told_in_whatever_order_the_human_passes_them_on_in() {
    // The resume goes before the human passes on the first.
    let (mut w, _, session, first, second, resumed) = second_passed_on_then_resumed();
    assert!(
        w.deliver(resumed).carried.is_empty(),
        "the second does not ride in the resume ahead of the first"
    );
    assert_eq!(
        w.next(&session),
        None,
        "it waits behind the first, still held"
    );
    w.ledger.approve_message(first, "human").expect("passed on");
    assert_eq!(w.next(&session), Some(first));
    w.deliver(first);
    assert_eq!(w.next(&session), Some(second));

    // The human passes on the first while the resume waits: both ride in it, in order.
    let (mut w, _, session, first, second, resumed) = second_passed_on_then_resumed();
    w.ledger.approve_message(first, "human").expect("passed on");
    assert_eq!(w.next(&session), Some(resumed), "one paste, the resume's");
    assert_eq!(ids(&w.deliver(resumed).carried), [first, second]);
    assert_eq!(w.next(&session), None);
}

#[test]
fn told_words_whose_last_paste_fails_while_the_task_is_paused_are_kept_for_its_resume() {
    let mut w = world();
    let (task, session, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    w.begin(told.id);
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    let settled = w
        .ledger
        .fail_delivery(told.id, "refused by the window")
        .expect("settled");
    assert_eq!(
        (settled.state.as_str(), settled.reason.as_deref()),
        (
            "queued",
            Some(format!("kept for T-{task}'s resume: refused by the window").as_str())
        ),
        "kept, never given up"
    );
    assert_eq!(w.state(task), "paused");
    assert!(
        waiting(&w, &session, task),
        "the window is still to have them"
    );
    let resumed = w.resume(task, "Go on").message.expect("its words").id;
    assert_eq!(
        ids(&w.deliver(resumed).carried),
        [told.id],
        "the resume carries them"
    );
    assert!(!waiting(&w, &session, task));
}

/// Words told while the brief waited ride in its paste, and the daemon stops while
/// it is being pasted: the start keeps them in the brief (`release_stranded`),
/// whatever the window's record shows: received with it where the record shows its
/// marker, and in its paste again, once, where it does not.
#[test]
fn told_words_in_a_brief_the_daemon_stopped_while_pasting_go_in_once_after_the_start() {
    for shown in [true, false] {
        let mut w = world();
        let (task, session, brief) = queued(&mut w);
        let told = tell(&mut w, task, "Use the v2 grammar").id;
        assert_eq!(
            ids(&w.begin(brief).carried),
            [told],
            "it rides in the brief"
        );
        w.edit(|_| {});
        assert_eq!(ids(&w.ledger.in_flight().unwrap()), [brief]);
        assert_eq!(
            w.next(&session),
            None,
            "nothing goes on its own while the brief is on its way"
        );
        if shown {
            w.confirm(brief);
        } else {
            w.ledger
                .retry_delivery(brief, "the daemon stopped before it arrived", true)
                .expect("queued again");
            assert_eq!(w.next(&session), Some(brief), "one paste, the brief's");
            assert_eq!(ids(&w.deliver(brief).carried), [told], "in its paste again");
        }
        assert_eq!(w.states(&[brief, told]), ["delivered", "delivered"]);
        assert_eq!(w.receipt(told)["carrier"], brief);
        assert_eq!(w.next(&session), None, "nothing is pasted again");
        assert!(!waiting(&w, &session, task), "the window has them");
    }
}
