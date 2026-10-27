//! What a pause keeps, and what rides in the paste of the words that resume:
//! one carrier, a flat set, each payload once, the task moved only by receipt.

use cf_ledger::GivenUp;
use serde_json::json;

use crate::fixture::{frozen_world, ids, world};

#[test]
fn a_hold_ends_in_one_carrier_with_what_was_kept_in_the_order_of_its_ids_and_its_confirm_settles_every_row(
) {
    // T-202, whole: a question, its answer queued, a note queued, a hold, the resume.
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    assert_eq!(w.state(task), "working");
    let question = w.ask("zeus", task, "Which format?");
    assert_eq!(w.state(task), "waiting");
    let answer = w.answer(question.id, "JSON");
    let note = w.note("chief", "zeus", task, "Mind the tests");

    w.hold(task);
    assert_eq!(
        w.states(&[answer.id, note.id]),
        ["queued", "queued"],
        "a hold takes back nothing that was on its way"
    );
    assert_eq!(
        w.next("zeus"),
        None,
        "a paused task's window is given nothing"
    );

    let carrier = w.daemon_resumes(task).message.expect("its words");
    assert_eq!(
        carrier.body, "Resumed: Go on where you stopped.",
        "the brief was received: the words do not rebuild it"
    );
    assert_eq!(
        w.state(task),
        "queued",
        "the task waits for its words to be received"
    );
    assert_eq!(w.next("zeus"), Some(carrier.id), "one paste, the carrier's");

    let begun = w.deliver(carrier.id);
    assert_eq!(
        ids(&begun.carried),
        [answer.id, note.id],
        "a flat set, in the order of its ids"
    );
    assert_eq!(w.states(&[answer.id, note.id]), ["delivered", "delivered"]);
    assert_eq!(w.receipt(answer.id), json!({ "carrier": carrier.id }));
    assert_eq!(
        w.state(task),
        "working",
        "its question has an answer the window received"
    );
    assert_eq!(w.next("zeus"), None, "nothing is left to paste");
}

#[test]
fn an_answer_given_during_a_hold_is_kept_and_carried() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    w.hold(task);
    let answer = w.answer(question.id, "JSON");
    assert_eq!(w.state(task), "paused");
    assert_eq!(w.next("zeus"), None, "it waits for the words that resume");

    let carrier = w.daemon_resumes(task).message.expect("its words");
    let begun = w.deliver(carrier.id);
    assert_eq!(ids(&begun.carried), [answer.id]);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_hold_with_the_question_open_lands_waiting_and_the_answers_confirm_makes_it_work() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    w.hold(task);
    let carrier = w.daemon_resumes(task).message.expect("its words");
    w.deliver(carrier.id);
    assert_eq!(
        w.state(task),
        "waiting",
        "the question is still outstanding: a turn that ends now makes no result"
    );

    let answer = w.answer(question.id, "JSON");
    assert_eq!(
        w.next("zeus"),
        Some(answer.id),
        "no carrier waits: it is pasted on its own"
    );
    assert_eq!(w.state(task), "waiting", "until it is received");
    w.deliver(answer.id);
    assert_eq!(w.state(task), "working");
}

#[test]
fn the_chiefs_pause_keeps_an_answer_and_takes_back_the_chiefs_words_and_notes() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    let answer = w.answer(question.id, "JSON");
    let note = w.note("chief", "zeus", task, "Mind the tests");

    w.pause(task);
    assert_eq!(w.states(&[answer.id, note.id]), ["queued", "cancelled"]);
    assert_eq!(
        w.message(note.id).reason.as_deref(),
        Some("withdrawn by @chief's pause"),
        "the chief's resume words carry what is new"
    );

    // Resumed, and paused again before the words are delivered: the answer
    // goes with the next words, and the first words go.
    let first = w.resume(task, "Use JSON").message.expect("its words");
    w.pause(task);
    assert_eq!(w.states(&[first.id, answer.id]), ["cancelled", "queued"]);
    let second = w
        .resume(task, "Use JSON, and test it")
        .message
        .expect("its words");
    assert_eq!(second.body, "Resumed: Use JSON, and test it");
    let begun = w.deliver(second.id);
    assert_eq!(
        ids(&begun.carried),
        [answer.id],
        "carried once more, and once"
    );
}

#[test]
fn the_humans_pause_and_a_stall_take_back_nothing_of_what_the_chief_wrote() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let note = w.note("chief", "zeus", task, "Mind the tests");
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    assert_eq!(w.states(&[note.id]), ["queued"]);
    let resumed = w.resume(task, "Go on").message.expect("its words");
    w.ledger
        .pause_task(w.project, task, None, Some("@zeus's window closed"))
        .expect("a stall");
    assert_eq!(w.states(&[note.id, resumed.id]), ["queued", "queued"]);
}

#[test]
fn a_task_paused_before_its_brief_arrived_keeps_the_brief_and_its_window_is_given_nothing() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    assert_eq!(
        w.states(&[brief]),
        ["queued"],
        "the human takes back nothing of the chief's"
    );
    assert_eq!(
        w.next("zeus"),
        None,
        "a task message is no exception: a paused task's window is given nothing"
    );

    let words = w.resume(task, "Go on").message.expect("its words");
    assert_eq!(w.next("zeus"), Some(words.id));
    let begun = w.deliver(words.id);
    assert_eq!(
        ids(&begun.carried),
        [brief],
        "the brief goes in the paste of the words, once"
    );
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_confirm_of_what_was_on_its_way_when_the_task_was_paused_leaves_it_queued_behind_the_words() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.begin(brief);
    // Paused and resumed while the brief is still being pasted.
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    let words = w.resume(task, "Go on").message.expect("its words");
    w.confirm(brief);
    assert_eq!(
        w.state(task),
        "queued",
        "the words that resume it have not arrived"
    );
    assert_eq!(w.next("zeus"), Some(words.id));
    w.deliver(words.id);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_pause_keeps_the_workers_question_still_queued_and_the_one_still_held_for_the_human() {
    let mut w = crate::fixture::gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger
        .approve_message(brief, "human")
        .expect("the brief passes");
    w.deliver(brief);
    let held = w.ask("zeus", task, "Which format?");
    w.ledger.set_gate(w.project, false).expect("the gate opens");
    let queued = w.ask("zeus", task, "And which version?");
    assert_eq!(w.states(&[held.id, queued.id]), ["gated", "queued"]);

    w.pause(task);
    assert_eq!(
        w.states(&[held.id, queued.id]),
        ["gated", "queued"],
        "neither is cancelled: the chief still has both to read"
    );
    assert_eq!(w.next("chief"), Some(queued.id));
    w.ledger
        .approve_message(held.id, "human")
        .expect("the human passes it on");
    assert_eq!(w.states(&[held.id]), ["queued"]);
}

#[test]
fn a_tell_that_was_never_answered_does_not_keep_the_task_from_working_once_it_goes_on() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let tell = w.tell("zeus", task, "Where are you?");
    assert_eq!(w.state(task), "paused", "a tell pauses its task");
    assert_eq!(
        w.next("zeus"),
        Some(tell.id),
        "and goes to its window, paused or not"
    );
    w.deliver(tell.id);

    let carrier = w.resume(task, "Carry on").message.expect("its words");
    let begun = w.deliver(carrier.id);
    assert!(begun.carried.is_empty(), "a tell is never carried");
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_second_hold_and_resume_carries_each_payload_once_and_the_daemons_fixed_words_once() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    let answer = w.answer(question.id, "JSON");

    w.hold(task);
    let first = w.daemon_resumes(task).message.expect("its words");
    // Held again before it was delivered, and resumed again.
    w.hold(task);
    let second = w.daemon_resumes(task).message.expect("its words");
    assert_eq!(
        w.states(&[first.id, answer.id]),
        ["cancelled", "queued"],
        "the fixed words say what one does"
    );
    assert_eq!(
        w.message(first.id).reason,
        Some(format!("superseded by m-{}", second.id))
    );
    let begun = w.deliver(second.id);
    assert_eq!(ids(&begun.carried), [answer.id], "the answer once");
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_second_resume_carries_the_chiefs_own_earlier_words_as_a_row_among_the_rest() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    let answer = w.answer(question.id, "JSON");

    w.pause(task);
    let chiefs = w.resume(task, "Use JSON").message.expect("its words");
    w.hold(task);
    let daemons = w.daemon_resumes(task).message.expect("its words");
    assert_eq!(
        w.states(&[chiefs.id]),
        ["queued"],
        "words of the chief are not the daemon's to drop"
    );
    let begun = w.deliver(daemons.id);
    assert_eq!(
        ids(&begun.carried),
        [answer.id, chiefs.id],
        "a flat set: the answer the chief's words carried is carried by the newer"
    );
    assert_eq!(
        w.states(&[chiefs.id, answer.id]),
        ["delivered", "delivered"]
    );
}

/// The words that resume a held task fail as they are pasted (their window closed): the
/// task is paused again, never failed (T-77), and what they carried is kept, for the next
/// resume to carry. Before, the task failed, and a reopening carried it.
#[test]
fn a_resume_whose_words_fail_lets_go_of_what_they_carried_and_its_task_waits_paused_for_the_next() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    let answer = w.answer(question.id, "JSON");
    w.hold(task);
    let carrier = w.daemon_resumes(task).message.expect("its words");
    w.begin(carrier.id);
    let (_, given_up) = w
        .ledger
        .give_up_delivery(carrier.id, "the window closed")
        .expect("the delivery is given up");
    assert_eq!(given_up, GivenUp::Paused);
    assert_eq!(w.state(task), "paused", "paused again, never failed");
    assert_eq!(
        w.states(&[carrier.id, answer.id]),
        ["failed", "queued"],
        "its words never went in; what they carried stays kept"
    );

    let resumed = w.resume(task, "Try again").message.expect("its words");
    let begun = w.deliver(resumed.id);
    assert_eq!(ids(&begun.carried), [answer.id]);
    assert_eq!(
        w.state(task),
        "working",
        "its question has the answer it was sent"
    );
}

/// A brief that fails as it is pasted fails its task, and lets go of the words told to
/// its window that rode in it: they stay kept, and the reopening carries them.
#[test]
fn a_carrier_that_fails_lets_go_of_what_it_carried_and_the_reopening_carries_it_again() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    let told = w
        .ledger
        .tell_queued(w.project, task, "chief", "Use the v2 grammar")
        .expect("told")
        .id;
    let begun = w.begin(brief);
    assert_eq!(ids(&begun.carried), [told]);
    let (_, given_up) = w
        .ledger
        .give_up_delivery(brief, "the window closed")
        .expect("the delivery is given up");
    assert_eq!(given_up, GivenUp::Failed);
    assert_eq!(w.state(task), "failed", "the task fails as it always did");
    assert_eq!(w.states(&[told]), ["queued"], "what it carried stays kept");

    let reopened = w
        .ledger
        .reopen_task(w.project, task, "chief", "Try again")
        .expect("reopened")
        .message
        .expect("its words");
    let begun = w.deliver(reopened.id);
    assert_eq!(ids(&begun.carried), [told]);
    assert_eq!(w.state(task), "working");
}

/// T-202's shape, cut by a stop of the daemon: the words that resume the task are
/// being pasted, with the answer and the note they carry, when the daemon stops, and
/// it starts again (the ledger is opened again). The carrier, its answer and its note.
fn stopped_while_pasting_a_carrier() -> (crate::fixture::World, i64, i64, i64, i64) {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    let answer = w.answer(question.id, "JSON").id;
    let note = w.note("chief", "zeus", task, "Mind the tests").id;
    w.hold(task);
    let carrier = w.daemon_resumes(task).message.expect("its words").id;
    let begun = w.begin(carrier);
    assert_eq!(
        ids(&begun.carried),
        [answer, note],
        "both ride in its paste"
    );
    w.edit(|_| {});
    assert_eq!(ids(&w.ledger.in_flight().unwrap()), [carrier]);
    (w, task, carrier, answer, note)
}

/// The window's record shows the carrier's marker: the start confirms it, and the
/// rows that rode in its paste were received with it. Let go of it at the start,
/// they were pasted again on their own (a review of 2026-10-16).
#[test]
fn rows_that_rode_in_a_paste_the_daemon_stopped_in_are_received_with_it_when_the_record_shows_it() {
    let (mut w, task, carrier, answer, note) = stopped_while_pasting_a_carrier();
    w.confirm(carrier);
    assert_eq!(
        w.states(&[carrier, answer, note]),
        ["delivered", "delivered", "delivered"]
    );
    assert_eq!(w.receipt(answer), json!({ "carrier": carrier }));
    assert_eq!(
        w.state(task),
        "working",
        "its question has an answer received"
    );
    assert_eq!(w.next("zeus"), None, "nothing is pasted again");
}

/// The window's record does not show it: the start queues the carrier again, and the
/// rows go again in its paste, once.
#[test]
fn rows_that_rode_in_a_paste_the_daemon_stopped_in_go_again_in_its_paste_when_the_record_does_not_show_it(
) {
    let (mut w, task, carrier, answer, note) = stopped_while_pasting_a_carrier();
    w.ledger
        .retry_delivery(carrier, "the daemon stopped before it arrived", true)
        .expect("queued again");
    assert_eq!(w.states(&[answer, note]), ["queued", "queued"]);
    assert_eq!(w.next("zeus"), Some(carrier), "one paste, the carrier's");
    let begun = w.deliver(carrier);
    assert_eq!(ids(&begun.carried), [answer, note], "in its paste again");
    assert_eq!(
        w.states(&[carrier, answer, note]),
        ["delivered", "delivered", "delivered"]
    );
    assert_eq!(w.state(task), "working");
    assert_eq!(w.next("zeus"), None);
}

/// The carrier's paste reached the window, and the start could not read the window's
/// record, so it queued the carrier again; a note for the window came while it waited,
/// and the daemon stopped again as the carrier began to go. The next start finds the
/// first paste's marker: what that paste held was received, and the note, in no
/// paste, goes on its own. It joined the carrier, it was taken for received with it,
/// and never pasted (a review of 2026-10-16).
#[test]
fn a_row_that_comes_after_a_carriers_first_paste_is_never_taken_for_received_with_it() {
    let (mut w, task, carrier, answer, note) = stopped_while_pasting_a_carrier();
    w.ledger
        .retry_delivery(carrier, "the daemon stopped before it arrived", true)
        .expect("queued again");
    let later = w.note("chief", "zeus", task, "And the docs").id;
    let begun = w.begin(carrier);
    assert_eq!(
        ids(&begun.carried),
        [answer, note],
        "the set of its first paste"
    );
    w.edit(|_| {});
    w.confirm(carrier);
    assert_eq!(
        w.states(&[carrier, answer, note, later]),
        ["delivered", "delivered", "delivered", "queued"]
    );
    assert_eq!(w.next("zeus"), Some(later), "it goes on its own");
}

#[test]
fn two_pauses_on_a_clock_that_never_moves_are_two_stops() {
    let mut w = frozen_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let zeus = w.id("zeus");
    assert_eq!(
        w.ledger.stop_of(zeus).expect("a read").map(|stop| stop.seq),
        Some(0)
    );
    w.pause(task);
    let first = w.ledger.stop_of(zeus).expect("a read").expect("a stop");
    w.resume(task, "Go on");
    w.pause(task);
    let second = w.ledger.stop_of(zeus).expect("a read").expect("a stop");
    assert_eq!(
        [first.seq, second.seq, second.number],
        [1, 2, task],
        "each pause counts, whatever the time"
    );
    let counted: Vec<i64> = w
        .ledger
        .events(w.project, 0, 500)
        .expect("the log")
        .iter()
        .filter(|event| event.kind == "task.state" && event.data["to"] == "paused")
        .map(|event| event.data["stop"].as_i64().expect("a stop"))
        .collect();
    assert_eq!(counted, [1, 2], "and the log says which");
}
