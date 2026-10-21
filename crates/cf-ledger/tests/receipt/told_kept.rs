//! Words told to a task's window before it was up, where the second review of
//! the change (2026-10-16) found them misread or lost: a note about the task to
//! anyone but its window is no such words, and its failure fails nothing; a
//! resume keeps the order they were told in, whatever order the human passes
//! them on in; words whose last paste failed while the task was paused are
//! kept for its resume, and its requester hears why and which words wait; and words that rode in a brief the daemon stopped while
//! pasting keep their place in it across the start (the reviews of round 4:
//! they were let go of and pasted again on their own).

use cf_ledger::GivenUp;

use crate::fixture::{gated_world, ids, world, World};
use crate::queued_tells::{newest, queued, tell, waiting};

#[test]
fn a_workers_note_to_the_chief_that_never_arrives_fails_nothing_of_its_task() {
    let mut w = world();
    let (task, session, brief) = queued(&mut w);
    w.deliver(brief);
    assert_eq!(w.state(task), "working");
    let note = w.note(&session, "chief", task, "Halfway there");
    w.begin(note.id);
    let (_, given_up) = w
        .ledger
        .give_up_delivery(note.id, "refused by the window")
        .expect("given up");
    assert_eq!(given_up, GivenUp::Failed, "said as failed");
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
    let (settled, given_up) = w
        .ledger
        .give_up_delivery(told.id, "refused by the window")
        .expect("settled");
    assert_eq!(
        given_up,
        GivenUp::Kept,
        "said as kept, not read from its state"
    );
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
    // The human's pause wrote the chief no note to name them in: one of their own.
    assert_eq!(
        newest(&w, "chief").body,
        format!(
            "m-{0}, what @chief told T-{task}'s window, waits for T-{task}'s resume: refused by the window.",
            told.id
        )
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

/// ConsensFlow pauses the task as its window closes, and its note to the chief is
/// queued: the words told the window whose pastes then fail are named in that note,
/// with why, the first with the why and the next added to it, and in no other.
#[test]
fn told_words_kept_as_consensflow_pauses_their_task_are_named_in_its_pause_note() {
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    w.begin(brief);
    let first = tell(&mut w, task, "Use the v2 grammar").id;
    let second = tell(&mut w, task, "And keep the old tests").id;
    w.confirm(brief);
    w.begin(first);
    let closed = "@zeus's window closed";
    w.ledger
        .pause_task(w.project, task, None, Some(closed))
        .expect("paused");
    let note = w
        .ledger
        .note_pause(w.project, "chief", task, closed)
        .expect("told")
        .id;
    w.ledger
        .fail_delivery(first, "unfit for work")
        .expect("kept");
    // The second's paste was under way too, and fails after.
    w.begin(second);
    w.ledger
        .fail_delivery(second, "unfit for work")
        .expect("kept");
    assert_eq!(w.states(&[first, second]), ["queued", "queued"]);
    assert_eq!(
        w.message(note).body,
        format!(
            "T-{task} is paused: {closed}: unfit for work; the words told its window wait for the resume: m-{first}, m-{second}. Resume it with: cf task resume T-{task} \"…\"; its window comes back on its own conversation."
        )
    );
    assert_eq!(newest(&w, "chief").id, note, "no note of their own");
}

/// The same in a note that names several tasks paused together: the clause of the
/// task names its words, and the note still reads as one of a pause.
#[test]
fn told_words_kept_in_a_pause_of_several_tasks_are_named_in_their_tasks_line() {
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar").id;
    w.confirm(brief);
    w.begin(told);
    let other = w.give("diana", "Lexer").task.number;
    let closed = "@zeus's window closed";
    for number in [task, other] {
        w.ledger
            .pause_task(w.project, number, None, Some(closed))
            .expect("paused");
    }
    let note = w
        .ledger
        .note_pause(w.project, "chief", task, closed)
        .expect("told")
        .id;
    w.ledger
        .join_pause_note(note, other, "@diana's window closed")
        .expect("joined")
        .expect("it takes the other");
    w.ledger
        .fail_delivery(told, "unfit for work")
        .expect("kept");
    let (low, high) = (task.min(other), task.max(other));
    let line = |number: i64| {
        if number == task {
            format!("T-{task} is paused: {closed}: unfit for work; the words told its window wait for the resume: m-{told}. Resume it with: cf task resume T-{task} \"…\"")
        } else {
            format!("T-{other} is paused: @diana's window closed. Resume it with: cf task resume T-{other} \"…\"")
        }
    };
    assert_eq!(
        w.message(note).body,
        format!(
            "{}\n{}\nEach window comes back on its own conversation.",
            line(low),
            line(high)
        )
    );
    // It is a pause note still: a resume takes the task out of it.
    w.resume(task, "Go on");
    assert_eq!(
        w.message(note).body,
        format!(
            "{}; its window comes back on its own conversation.",
            line(other)
        )
    );
}
