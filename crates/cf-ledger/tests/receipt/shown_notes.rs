//! Notes of ConsensFlow's own that ask a chief no decision (RS-25): to a chief they are
//! shown, not typed. Each is still a row of the ledger, the chief's inbox lists it, and
//! no delivery ever takes it; what it says is its task's line (`aside`) until the task
//! moves on. To anyone but a chief it is queued as any note.

use cf_ledger::{MessageView, NewNote, NewTask, SHOWN};
use serde_json::json;

use crate::fixture::{frozen_world, world, World};

/// The end of the hold the tests name.
const UNTIL: &str = "2026-10-10T15:00:00.000Z";

/// T-1 for the standard workers, in zeus's session, its brief delivered.
fn with_zeus(w: &mut World) -> (i64, String) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    (task, session)
}

/// What task `number`'s line says of it instead of a note, while it holds.
fn aside(w: &World, number: i64) -> Option<String> {
    w.ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .aside
}

/// ConsensFlow's note about T-`number` to `to`, shown where it asks no decision.
fn shown(w: &mut World, to: &str, number: i64, body: &str, said: &str) -> MessageView {
    w.ledger
        .show(
            w.project,
            &NewNote {
                from: None,
                to: to.into(),
                body: body.into(),
                task: Some(number),
            },
            said,
        )
        .expect("a note")
}

/// A hold of the kinds a chief is told of once (quota, an unavailable account, a moved
/// model): the note of T-`number` with `session`, out of quota until `until` where it is one.
#[derive(Clone, Copy)]
enum Hold {
    Quota,
    Unavailable,
    Moved,
}

/// ConsensFlow's note of `hold` of T-`number` to the chief, as the engine writes it.
fn note_of(w: &mut World, hold: Hold, number: i64, session: &str, until: &str) -> MessageView {
    let ledger = &mut w.ledger;
    match hold {
        Hold::Quota => ledger.note_hold(w.project, "chief", number, session, until),
        Hold::Unavailable => {
            ledger.note_hold_unavailable(w.project, "chief", number, session, "signed out")
        }
        Hold::Moved => ledger.note_hold_moved(
            w.project,
            "chief",
            number,
            session,
            "its harness moved it from Opus to Sonnet",
        ),
    }
    .expect("the chief is told")
}

/// What T-1's line says of `hold` with `session`.
fn said_of(hold: Hold, session: &str) -> String {
    match hold {
        Hold::Quota => format!("waits with @{session}: out of quota until {UNTIL}"),
        Hold::Unavailable => format!(
            "waits with @{session}: its account is unavailable (signed out) until the human says it works"
        ),
        Hold::Moved => format!(
            "waits with @{session}: its harness moved it from Opus to Sonnet, until the human says it may go on"
        ),
    }
}

const HOLDS: [Hold; 3] = [Hold::Quota, Hold::Unavailable, Hold::Moved];

/// A tell whose window has received it; the task is paused for this message.
fn delivered_tell(w: &mut World) -> (i64, String, i64) {
    let (task, session) = with_zeus(w);
    let tell = w.tell(&session, task, "Where did you stop?");
    w.deliver(tell.id);
    (task, session, tell.id)
}

fn answer_tell(w: &mut World, session: &str, tell: i64) -> MessageView {
    let member = w.id(session);
    w.ledger
        .answer(tell, member, Some(&json!("The parser is done.")), None)
        .expect("the window answers the tell on the board")
}

fn note_of_tell(w: &mut World, task: i64, session: &str, tell: i64) -> MessageView {
    w.ledger
        .note_paused_answer(w.project, task, session, tell, None, "The parser is done.")
        .expect("the ledger")
        .expect("the turn's note")
}

fn assert_shown_tell_note(w: &World, task: i64, tell: i64, id: i64) {
    let note = w.message(id);
    assert_eq!(note.state, "read", "an answered tell's note is shown");
    assert!(
        note.receipt.get(SHOWN).is_some(),
        "not typed: {:?}",
        note.receipt
    );
    assert!(note.body.contains(&format!("(m-{tell})")));
    assert!(note.body.contains("The parser is done."));
    assert!(w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("inbox")
        .iter()
        .any(|m| m.id == id));
    assert!(w
        .ledger
        .task(w.project, task)
        .expect("read")
        .expect("task")
        .messages
        .iter()
        .any(|m| m.id == id && m.body == note.body));
    assert_ne!(
        w.next("chief"),
        Some(id),
        "the note is not queued for the chief"
    );
}

#[test]
fn an_answered_tells_note_is_shown_before_and_after_the_chief_resumes() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    w.deliver(answer.id);
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_shown_tell_note(&w, task, tell, note.id);
    w.resume(task, "Go on");
    assert_shown_tell_note(&w, task, tell, note.id);
    assert_eq!(w.next("chief"), None);
}

#[test]
fn an_answered_tells_note_is_shown_when_the_chief_resumed_before_the_turn_ended() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    w.deliver(answer.id);
    w.resume(task, "Go on");
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_shown_tell_note(&w, task, tell, note.id);
    assert!(note.body.contains("has moved on since"));
    assert_eq!(w.next("chief"), None);
}

#[test]
fn an_answered_tells_note_is_shown_while_the_task_stays_paused() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    w.deliver(answer.id);
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_eq!(w.state(task), "paused");
    assert!(note.body.contains("is paused"));
    assert_shown_tell_note(&w, task, tell, note.id);
    assert_eq!(w.next("chief"), None);
}

#[test]
fn an_answer_on_its_way_to_the_chief_already_makes_the_tells_note_shown() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    assert_eq!(answer.state, "queued");
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_shown_tell_note(&w, task, tell, note.id);
    assert_eq!(w.next("chief"), Some(answer.id));
}

#[test]
fn an_older_delivered_answer_with_no_proof_of_ways_on_keeps_the_note_typed_until_a_move() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    w.deliver(answer.id);
    // An older build's event did not say whether the answer had the ways-on footer.
    w.edit(|db| {
        db.execute(
            "UPDATE event SET data = json_remove(data, '$.tellPauseWaysOn')
             WHERE kind = 'delivery.begun' AND json_extract(data, '$.message') = ?",
            [answer.id],
        )
        .expect("the older event has no footer proof");
    });
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_eq!(note.state, "queued", "the chief still needs the ways on");
    w.resume(task, "Go on");
    assert_shown_tell_note(&w, task, tell, note.id);
}

#[test]
fn an_older_carrier_already_pasting_an_answer_without_ways_on_keeps_the_note_typed() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    let carrier = w.note(&session, "chief", task, "More for the chief");
    w.edit(|db| {
        db.execute(
            "UPDATE message SET carried_by = ?1 WHERE id = ?2",
            [carrier.id, answer.id],
        )
        .expect("the answer rides in the carrier");
    });
    w.begin(carrier.id);
    assert_eq!(w.message(answer.id).state, "queued");
    w.edit(|db| {
        db.execute(
            "UPDATE event SET data = json_remove(data, '$.tellPauseWaysOn')
             WHERE kind = 'delivery.begun' AND json_extract(data, '$.message') = ?",
            [carrier.id],
        )
        .expect("the older carrier has no footer proof");
    });
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_eq!(note.state, "queued", "the chief still needs the ways on");
    w.resume(task, "Go on");
    assert_shown_tell_note(&w, task, tell, note.id);
}

#[test]
fn an_answer_in_a_queued_carrier_makes_the_note_shown() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    let carrier = w.note(&session, "chief", task, "More for the chief");
    w.edit(|db| {
        db.execute(
            "UPDATE message SET carried_by = ?1 WHERE id = ?2",
            [carrier.id, answer.id],
        )
        .expect("the answer rides in the carrier");
    });
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_shown_tell_note(&w, task, tell, note.id);
}

#[test]
fn an_answer_in_a_begun_carrier_with_ways_on_makes_the_note_shown() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    let carrier = w.note(&session, "chief", task, "More for the chief");
    w.edit(|db| {
        db.execute(
            "UPDATE message SET carried_by = ?1 WHERE id = ?2",
            [carrier.id, answer.id],
        )
        .expect("the answer rides in the carrier");
    });
    w.begin(carrier.id);
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_shown_tell_note(&w, task, tell, note.id);
}

#[test]
fn an_answer_the_chief_read_in_the_inbox_still_makes_its_tells_note_shown() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    let chief = w.id("chief");
    w.ledger
        .receive_read(
            chief,
            &[answer.id],
            cf_ledger::Read::Inbox,
            &cf_proto::cf_word::CfWord::bare(),
        )
        .expect("the chief read the answer");
    assert_eq!(w.message(answer.id).state, "read");
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_shown_tell_note(&w, task, tell, note.id);
}

#[test]
fn the_ways_on_of_an_answer_in_a_carrier_stay_with_that_paste_after_resume() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    let carrier = w.note(&session, "chief", task, "More for the chief");
    w.edit(|db| {
        db.execute(
            "UPDATE message SET carried_by = ?1 WHERE id = ?2",
            [carrier.id, answer.id],
        )
        .expect("the answer rides in the carrier");
    });
    let begun = w.begin(carrier.id);
    assert!(begun.carried.iter().any(|row| row.id == answer.id));
    assert_eq!(
        w.ledger.tell_pause_ways_on(carrier.id).expect("the paste"),
        [task]
    );
    w.resume(task, "Go on");
    assert_eq!(
        w.ledger.tell_pause_ways_on(carrier.id).expect("the paste"),
        [task],
        "the already-begun paste does not change with the task"
    );
}

#[test]
fn a_retry_records_the_ways_on_of_the_new_attempt_not_the_earlier_one() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    w.begin(answer.id);
    assert_eq!(
        w.ledger.tell_pause_ways_on(answer.id).expect("the paste"),
        [task]
    );
    w.ledger
        .retry_delivery(answer.id, "the first attempt went back", true)
        .expect("queued again");
    w.resume(task, "Go on");
    w.begin(answer.id);
    assert!(w
        .ledger
        .tell_pause_ways_on(answer.id)
        .expect("the new paste")
        .is_empty());
}

#[test]
fn an_answer_withdrawn_before_the_note_does_not_make_it_shown() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    w.ledger
        .cancel_message(answer.id, "withdrawn")
        .expect("withdrawn");
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_eq!(note.state, "queued");
    assert_eq!(w.next("chief"), Some(note.id));
}

#[test]
fn a_shown_note_has_a_typed_fallback_if_its_board_answers_delivery_fails() {
    for moved in [false, true] {
        let mut w = world();
        let (task, session, tell) = delivered_tell(&mut w);
        let answer = answer_tell(&mut w, &session, tell);
        let note = note_of_tell(&mut w, task, &session, tell);
        assert_shown_tell_note(&w, task, tell, note.id);
        if moved {
            w.resume(task, "Go on");
        }
        w.begin(answer.id);
        w.ledger
            .give_up_delivery(answer.id, "the chief's window could not receive it")
            .expect("the answer failed");
        assert_eq!(w.message(answer.id).state, "failed");
        assert_shown_tell_note(&w, task, tell, note.id);
        let fallback = w.next("chief").expect("the chief gets the turn's words");
        assert_ne!(fallback, note.id, "the shown note stays shown");
        let fallback = w.message(fallback);
        assert!(fallback.body.contains("The parser is done."));
        if moved {
            assert!(fallback.body.contains("has moved on since"));
        } else {
            assert!(fallback.body.contains("cf task finish T-1"));
            assert!(fallback.body.contains("cf task resume T-1"));
        }
    }
}

#[test]
fn a_shown_note_has_a_typed_fallback_if_its_queued_board_answer_is_withdrawn() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    let note = note_of_tell(&mut w, task, &session, tell);
    w.ledger
        .cancel_message(answer.id, "the answer was withdrawn")
        .expect("withdrawn");
    assert_shown_tell_note(&w, task, tell, note.id);
    let fallback = w.next("chief").expect("the chief gets the turn's words");
    assert_ne!(fallback, note.id);
    assert!(w.message(fallback).body.contains("The parser is done."));
}

#[test]
fn a_task_failing_after_its_answer_was_shown_keeps_a_typed_fallback() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    let note = note_of_tell(&mut w, task, &session, tell);
    w.resume(task, "Go on");
    w.ledger
        .fail_task(w.project, task, "the resumed turn failed")
        .expect("the task failed");
    assert_eq!(w.message(answer.id).state, "cancelled");
    assert_shown_tell_note(&w, task, tell, note.id);
    let fallback = w.next("chief").expect("the chief gets the turn's words");
    assert_ne!(fallback, note.id);
    let fallback = w.message(fallback);
    assert!(fallback.body.contains("The parser is done."));
    assert!(fallback.body.contains("has moved on since"));
}

#[test]
fn another_board_answer_shows_an_unpasted_fallback_and_another_failure_recovers_it() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let first = answer_tell(&mut w, &session, tell);
    let note = note_of_tell(&mut w, task, &session, tell);
    w.ledger
        .cancel_message(first.id, "the answer was withdrawn")
        .expect("withdrawn");
    let fallback = w.next("chief").expect("the first fallback");
    let second = answer_tell(&mut w, &session, tell);
    assert_shown_tell_note(&w, task, tell, note.id);
    assert_shown_tell_note(&w, task, tell, fallback);
    assert_eq!(w.next("chief"), Some(second.id));
    w.ledger
        .cancel_message(second.id, "the other answer was withdrawn")
        .expect("withdrawn");
    let next = w.next("chief").expect("another typed fallback");
    assert_ne!(next, fallback);
    assert!(w.message(next).body.contains("The parser is done."));
    assert_shown_tell_note(&w, task, tell, fallback);
}

#[test]
fn a_finish_that_took_the_shown_words_needs_no_fallback_for_a_lost_answer() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    let note = note_of_tell(&mut w, task, &session, tell);
    w.ledger
        .record_finished(w.project, task, "The parser is done.", (0, 0))
        .expect("the chief took its words");
    w.ledger
        .cancel_message(answer.id, "the answer was withdrawn")
        .expect("withdrawn");
    assert_shown_tell_note(&w, task, tell, note.id);
    let thread = w.ledger.task(w.project, task).expect("read").expect("task");
    assert_eq!(
        thread
            .messages
            .iter()
            .filter(|message| message.kind == "note" && message.body.contains("The parser is done."))
            .count(),
        1,
        "the finished words need no new note"
    );
}

#[test]
fn an_unanswered_tells_note_stays_queued_when_the_chief_resumes_after_the_turn_ended() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_eq!(w.next("chief"), Some(note.id));
    w.resume(task, "Go on");
    let note = w.message(note.id);
    assert_eq!(note.state, "queued");
    assert!(note.body.contains("has moved on since"));
    assert_eq!(w.next("chief"), Some(note.id));
}

#[test]
fn an_unanswered_tells_note_is_queued_when_the_chief_resumed_before_the_turn_ended() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    w.resume(task, "Go on");
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_eq!(note.state, "queued");
    assert!(note.body.contains("has moved on since"));
    assert_eq!(w.next("chief"), Some(note.id));
}

#[test]
fn an_unanswered_tells_note_is_queued_with_both_ways_on_while_the_task_stays_paused() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_eq!(note.state, "queued");
    assert!(note.body.contains("cf task finish T-1"));
    assert!(note.body.contains("cf task resume T-1"));
    assert_eq!(w.next("chief"), Some(note.id));
}

#[test]
fn a_queued_note_is_shown_when_its_tell_is_answered_after_the_turn_ended_and_before_resume() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_eq!(note.state, "queued");
    let answer = answer_tell(&mut w, &session, tell);
    assert_eq!(answer.state, "queued");
    w.resume(task, "Go on");
    assert_shown_tell_note(&w, task, tell, note.id);
    assert_eq!(w.next("chief"), Some(answer.id));
}

#[test]
fn a_late_board_answer_shows_the_queued_note_even_if_the_task_stays_paused() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let note = note_of_tell(&mut w, task, &session, tell);
    let answer = answer_tell(&mut w, &session, tell);
    assert_shown_tell_note(&w, task, tell, note.id);
    assert_eq!(w.state(task), "paused");
    assert_eq!(w.next("chief"), Some(answer.id));
}

#[test]
fn a_late_board_answer_shows_a_note_written_and_rewritten_before_it_arrived() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let note = note_of_tell(&mut w, task, &session, tell);
    w.resume(task, "Go on");
    assert_eq!(w.message(note.id).state, "queued");
    let answer = answer_tell(&mut w, &session, tell);
    assert_shown_tell_note(&w, task, tell, note.id);
    assert_eq!(w.next("chief"), Some(answer.id));
}

#[test]
fn an_answer_after_resume_but_before_the_turns_end_shows_the_moved_note() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    w.resume(task, "Go on");
    let answer = answer_tell(&mut w, &session, tell);
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_shown_tell_note(&w, task, tell, note.id);
    assert!(note.body.contains("has moved on since"));
    assert_eq!(w.next("chief"), Some(answer.id));
}

#[test]
fn a_gated_answer_does_not_hide_the_note_until_the_human_passes_it_on() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    w.ledger.set_gate(w.project, true).expect("the gate");
    let answer = answer_tell(&mut w, &session, tell);
    assert_eq!(answer.state, "gated");
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_eq!(note.state, "queued");
    w.ledger
        .approve_message(answer.id, "human")
        .expect("passed on");
    assert_shown_tell_note(&w, task, tell, note.id);
    assert_eq!(w.next("chief"), Some(answer.id));
}

#[test]
fn an_answer_to_another_tell_does_not_hide_the_unanswered_tells_note() {
    let mut w = world();
    let (task, session, first) = delivered_tell(&mut w);
    let next = w.tell(&session, task, "And the lexer?");
    w.deliver(next.id);
    let answer = answer_tell(&mut w, &session, next.id);
    w.deliver(answer.id);
    let note = note_of_tell(&mut w, task, &session, first);
    assert_eq!(note.state, "queued");
    assert_eq!(w.next("chief"), Some(note.id));
}

#[test]
fn an_earlier_tell_answered_during_the_same_pause_is_still_shown() {
    let mut w = world();
    let (task, session, first) = delivered_tell(&mut w);
    let next = w.tell(&session, task, "And the lexer?");
    w.deliver(next.id);
    let answer = answer_tell(&mut w, &session, first);
    assert_eq!(
        w.ledger.answer_to_current_pause(answer.id).expect("read"),
        Some(task)
    );
    w.deliver(answer.id);
    let note = note_of_tell(&mut w, task, &session, first);
    assert_shown_tell_note(&w, task, first, note.id);
}

#[test]
fn a_note_already_being_pasted_stays_typed_when_its_tell_is_answered() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let note = note_of_tell(&mut w, task, &session, tell);
    w.begin(note.id);
    let answer = answer_tell(&mut w, &session, tell);
    assert_eq!(w.message(note.id).state, "delivering");
    w.resume(task, "Go on");
    let pasted = w.confirm(note.id);
    assert_eq!(pasted.state, "delivered");
    assert_eq!(pasted.body, note.body);
    assert!(pasted.receipt.get(SHOWN).is_none());
    assert_eq!(w.next("chief"), Some(answer.id));
}

#[test]
fn a_note_already_pasted_stays_typed_when_its_tell_is_answered() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let note = note_of_tell(&mut w, task, &session, tell);
    w.deliver(note.id);
    answer_tell(&mut w, &session, tell);
    w.resume(task, "Go on");
    let pasted = w.message(note.id);
    assert_eq!(pasted.state, "delivered");
    assert_eq!(pasted.body, note.body);
    assert!(pasted.receipt.get(SHOWN).is_none());
}

#[test]
fn a_note_queued_again_after_an_attempt_stays_typed_when_its_tell_is_answered() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let note = note_of_tell(&mut w, task, &session, tell);
    w.begin(note.id);
    w.ledger
        .retry_delivery(note.id, "the window went before confirmation", true)
        .expect("the note is queued again");
    assert_eq!(w.message(note.id).state, "queued");
    answer_tell(&mut w, &session, tell);
    assert_eq!(w.message(note.id).state, "queued");
    assert!(w.message(note.id).receipt.get(SHOWN).is_none());
    w.resume(task, "Go on");
    assert_eq!(w.message(note.id).state, "queued");
    assert!(w.message(note.id).body.contains("has moved on since"));
}

#[test]
fn a_note_in_a_carrier_is_shown_only_if_the_carrier_never_began() {
    for began in [false, true] {
        let mut w = world();
        let (task, session, tell) = delivered_tell(&mut w);
        let note = note_of_tell(&mut w, task, &session, tell);
        let carrier = w.note(&session, "chief", task, "More for the chief");
        // Model a note riding in a carrier, both still queued for the chief.
        w.edit(|db| {
            db.execute(
                "UPDATE message SET carried_by = ?1 WHERE id = ?2",
                [carrier.id, note.id],
            )
            .expect("it rides in the carrier");
        });
        if began {
            let begun = w.begin(carrier.id);
            assert!(begun.carried.iter().any(|row| row.id == note.id));
            w.ledger
                .retry_delivery(carrier.id, "the carrier lost confirmation", true)
                .expect("the carrier is queued again");
        }
        answer_tell(&mut w, &session, tell);
        let note = w.message(note.id);
        if began {
            assert_eq!(note.state, "queued");
            assert!(note.receipt.get(SHOWN).is_none());
        } else {
            assert_eq!(note.state, "read");
            assert!(note.receipt.get(SHOWN).is_some());
        }
    }
}

#[test]
fn an_answered_tells_note_to_a_human_requester_is_still_queued() {
    let mut w = world();
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
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.expect("its session");
    w.deliver(moved.message.expect("its brief").id);
    let tell = w.tell(&session, task, "Where did you stop?");
    w.deliver(tell.id);
    let answer = answer_tell(&mut w, &session, tell.id);
    w.deliver(answer.id);
    let note = note_of_tell(&mut w, task, &session, tell.id);
    assert_eq!(
        (note.recipient.as_str(), note.state.as_str()),
        ("human", "queued")
    );
    assert!(note.receipt.get(SHOWN).is_none());
    assert!(note.body.contains("Finish it in the task's drawer"));
}

#[test]
fn a_delivered_tell_answer_identifies_only_its_current_pause() {
    let mut w = world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    assert_eq!(
        w.ledger.answer_to_current_pause(answer.id).expect("read"),
        Some(task)
    );
    w.resume(task, "Go on");
    assert_eq!(
        w.ledger.answer_to_current_pause(answer.id).expect("read"),
        None
    );
    w.pause(task);
    assert_eq!(
        w.ledger.answer_to_current_pause(answer.id).expect("read"),
        None
    );
}

#[test]
fn another_pause_in_the_same_millisecond_is_not_the_pause_of_an_earlier_tell() {
    let mut w = frozen_world();
    let (task, session, tell) = delivered_tell(&mut w);
    let answer = answer_tell(&mut w, &session, tell);
    assert_eq!(
        w.ledger.answer_to_current_pause(answer.id).expect("read"),
        Some(task)
    );
    w.resume(task, "Go on");
    w.pause(task);
    assert_eq!(
        w.ledger.answer_to_current_pause(answer.id).expect("read"),
        None
    );
}

#[test]
fn an_answered_tell_from_a_previous_pause_is_shown_even_if_the_task_paused_again() {
    for answer_first in [false, true] {
        let mut w = frozen_world();
        let (task, session, tell) = delivered_tell(&mut w);
        w.resume(task, "Go on");
        w.pause(task);
        let earlier_answer = answer_first.then(|| answer_tell(&mut w, &session, tell));
        let note = note_of_tell(&mut w, task, &session, tell);
        if earlier_answer.is_none() {
            answer_tell(&mut w, &session, tell);
        }
        assert_shown_tell_note(&w, task, tell, note.id);
        let shown = w.message(note.id);
        assert!(shown.body.contains("has moved on since"), "{shown:?}");
        assert!(shown.receipt.get("over").is_some(), "{shown:?}");
    }
}

#[test]
fn an_unanswered_tell_from_a_previous_pause_is_still_queued_when_the_task_paused_again() {
    let mut w = frozen_world();
    let (task, session, tell) = delivered_tell(&mut w);
    w.resume(task, "Go on");
    w.pause(task);
    let note = note_of_tell(&mut w, task, &session, tell);
    assert_eq!(note.state, "queued");
    assert!(note.receipt.get(SHOWN).is_none());
}

#[test]
fn an_answer_whose_tells_pause_cannot_be_proved_keeps_the_note_typed_until_the_task_moves() {
    for note_first in [false, true] {
        let mut w = world();
        let (task, session, tell) = delivered_tell(&mut w);
        // An old or incomplete event log does not say whether this tell came
        // before the current pause. The answer's footer cannot claim it did.
        w.edit(|db| {
            db.execute(
                "DELETE FROM event WHERE kind = 'message.sent' AND json_extract(data, '$.message') = ?",
                [tell],
            )
            .expect("the old event is absent");
        });
        let earlier = note_first.then(|| note_of_tell(&mut w, task, &session, tell));
        let answer = answer_tell(&mut w, &session, tell);
        assert_eq!(
            w.ledger.answer_to_current_pause(answer.id).expect("read"),
            None
        );
        let note = earlier.unwrap_or_else(|| note_of_tell(&mut w, task, &session, tell));
        assert_eq!(
            w.message(note.id).state,
            "queued",
            "note first: {note_first}"
        );
        w.resume(task, "Go on");
        assert_shown_tell_note(&w, task, tell, note.id);
    }
}

#[test]
fn a_shown_tells_note_stays_shown_when_the_task_is_released_failed_finished_or_cancelled() {
    for ending in ["release", "failed", "finished", "cancelled"] {
        let mut w = world();
        let (task, session, tell) = delivered_tell(&mut w);
        let answer = answer_tell(&mut w, &session, tell);
        w.deliver(answer.id);
        let note = note_of_tell(&mut w, task, &session, tell);
        match ending {
            "release" => {
                w.ledger
                    .release_task(w.project, task, "another worker")
                    .expect("released");
            }
            "failed" => {
                w.resume(task, "Go on");
                w.ledger
                    .fail_task(w.project, task, "the window failed")
                    .expect("failed");
            }
            "finished" => {
                w.ledger
                    .record_finished(w.project, task, "Done", (0, 0))
                    .expect("finished");
            }
            "cancelled" => {
                w.ledger
                    .cancel_task(w.project, task, "chief")
                    .expect("cancelled");
            }
            _ => unreachable!(),
        }
        assert_shown_tell_note(&w, task, tell, note.id);
        assert!(w.message(note.id).receipt.get("over").is_some(), "{ending}");
    }
}

#[test]
fn the_first_note_of_a_hold_is_typed_to_the_chief_and_the_same_hold_said_again_is_shown() {
    for hold in HOLDS {
        let mut w = world();
        let (task, session) = with_zeus(&mut w);
        w.hold(task);
        // The first word of a hold is typed: a chief waits for nothing else, and only the
        // chief asks the human for an account or a model, or gives the work to another.
        let first = note_of(&mut w, hold, task, &session, UNTIL);
        assert_eq!(first.state, "queued");
        assert_eq!(w.next("chief"), Some(first.id));
        // Its task's line says it all the same.
        assert_eq!(aside(&w, task), Some(said_of(hold, &session)));
        // The same hold said again, before or after the first is typed: shown, not typed.
        let again = note_of(&mut w, hold, task, &session, UNTIL);
        assert_eq!(again.state, "read");
        assert_eq!(again.receipt, json!({ SHOWN: said_of(hold, &session) }));
        w.deliver(first.id);
        let and_again = note_of(&mut w, hold, task, &session, UNTIL);
        assert_eq!(and_again.state, "read");
        assert_eq!(w.next("chief"), None);
        assert_eq!(aside(&w, task), Some(said_of(hold, &session)));
        // The chief's inbox lists them all.
        let listed: Vec<i64> = w
            .ledger
            .inbox(w.id("chief"), 100)
            .expect("the inbox")
            .iter()
            .map(|message| message.id)
            .collect();
        assert!([first.id, again.id, and_again.id]
            .iter()
            .all(|id| listed.contains(id)));
    }
}

#[test]
fn a_quota_hold_with_another_end_is_another_hold_and_is_typed() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let first = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    w.deliver(first.id);
    let later = note_of(
        &mut w,
        Hold::Quota,
        task,
        &session,
        "2026-10-17T15:00:00.000Z",
    );
    assert_eq!(later.state, "queued", "a new end is news");
}

#[test]
fn a_hold_whose_note_was_withdrawn_before_it_was_typed_is_typed_when_it_comes_again() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let first = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    // The daemon resumes it before the chief's window takes the note: it is withdrawn.
    w.daemon_resumes(task);
    assert_eq!(w.message(first.id).state, "cancelled");
    w.hold(task);
    let again = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    assert_eq!(again.state, "queued", "the chief was never told");
}

#[test]
fn a_hold_after_the_task_went_on_is_typed_again() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let first = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    w.deliver(first.id);
    w.daemon_resumes(task);
    assert_eq!(aside(&w, task), None, "a move ends what the line said");
    w.went_on(task);
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("goes on: its account has quota again."),
        "the news is shown"
    );
    w.hold(task);
    let again = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    assert_eq!(
        again.state, "queued",
        "a wait that ended and came back is news"
    );
    assert_eq!(aside(&w, task), Some(said_of(Hold::Quota, &session)));
}

#[test]
fn a_hold_of_a_task_for_anyone_but_a_chief_is_queued_each_time_and_says_nothing_on_its_line() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    for _ in 0..2 {
        let note = w
            .ledger
            .note_hold(w.project, "human", task, &session, UNTIL)
            .expect("the human is told");
        assert_eq!(note.state, "queued");
    }
    assert_eq!(aside(&w, task), None);
}

#[test]
fn a_note_shown_to_anyone_but_a_chief_is_queued_as_any_note() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    let for_the_human = shown(&mut w, "human", task, "T-1 goes on.", "goes on");
    assert_eq!(for_the_human.state, "queued");
    assert_eq!(for_the_human.receipt, serde_json::Value::Null);
    assert_eq!(aside(&w, task), None);
}

#[test]
fn the_newest_aside_holds_until_the_task_moves_and_is_said_anew_in_place() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    let first = shown(
        &mut w,
        "chief",
        task,
        "T-1 is still working: 1 command.",
        "in the background: 1 command",
    );
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("in the background: 1 command")
    );
    // Said anew: the line, not the note, and no row more.
    assert!(w
        .ledger
        .renew_aside(w.project, first.id, "in the background: 2 commands")
        .expect("renewed"));
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("in the background: 2 commands")
    );
    assert_eq!(w.message(first.id).body, "T-1 is still working: 1 command.");
    let note = |_: &World| NewNote {
        from: None,
        to: "chief".into(),
        body: "T-1 has been working in the background for 60 minutes.".into(),
        task: Some(task),
    };
    // One reminder, until the note is said anew.
    let sent = w
        .ledger
        .remind_of_aside(w.project, first.id, &note(&w))
        .expect("a write");
    assert_eq!(
        sent.as_ref().map(|sent| sent.state.as_str()),
        Some("queued")
    );
    assert_eq!(
        w.ledger
            .remind_of_aside(w.project, first.id, &note(&w))
            .expect("a write"),
        None
    );
    assert!(w
        .ledger
        .renew_aside(w.project, first.id, "in the background: 1 command")
        .expect("renewed"));
    assert!(w
        .ledger
        .remind_of_aside(w.project, first.id, &note(&w))
        .expect("a write")
        .is_some());
    // A move ends it, and it is neither said anew nor reminded of after.
    w.pause(task);
    assert_eq!(aside(&w, task), None, "a pause is a move");
    assert!(!w
        .ledger
        .renew_aside(w.project, first.id, "in the background")
        .expect("a write"));
    assert_eq!(aside(&w, task), None);
    w.ledger
        .renew_aside(w.project, first.id, "x")
        .expect("a write");
    assert!(w
        .ledger
        .remind_of_aside(w.project, first.id, &note(&w))
        .expect("a write")
        .is_none());
}

#[test]
fn a_note_whose_line_ended_with_a_move_is_shown_again_with_no_row_more() {
    // A busy model's wait: one note for the whole wait, each try a move of its own.
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    w.hold(task);
    let note = shown(
        &mut w,
        "chief",
        task,
        "T-1 waits with @zeus: its model is busy.",
        "waits with @zeus: its model is busy",
    );
    w.resume(task, "Go on.");
    assert_eq!(aside(&w, task), None, "the try is a move");
    w.hold(task);
    assert!(w.ledger.show_again(w.project, note.id).expect("a write"));
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("waits with @zeus: its model is busy")
    );
    assert_eq!(w.message(note.id).state, "read", "still never typed");
    // Once shown, it is shown; and no note of anyone else's is.
    assert!(!w.ledger.show_again(w.project, note.id).expect("a write"));
}

#[test]
fn a_task_taken_back_with_nothing_bound_to_it_is_shown_to_the_chief_not_typed() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("released");
    let note = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the inbox")
        .into_iter()
        .find(|message| message.body.starts_with("T-1 was taken back from"))
        .expect("the chief's inbox lists the note");
    assert_eq!(
        note.body,
        format!(
            "T-1 was taken back from @{session} (by @human) and waits for another standard worker."
        )
    );
    // It goes on by itself: the chief has nothing to decide, and nothing is typed.
    assert_eq!(note.state, "read");
    assert_eq!(
        w.next("chief"),
        None,
        "nothing is typed into the chief's window"
    );
    assert_eq!(
        aside(&w, task).as_deref(),
        Some(
            format!(
                "was taken back from @{session} (by @human) and waits for another standard worker."
            )
            .as_str()
        )
    );
}

#[test]
fn a_task_taken_back_with_a_follow_up_bound_to_its_window_is_typed_to_the_chief() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    let after = w
        .follow_up(task, "Part 2", &[])
        .expect("a follow-up")
        .task
        .number;
    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("released");
    let note = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the inbox")
        .into_iter()
        .find(|message| message.body.starts_with("T-1 was taken back from"))
        .expect("the note");
    // The follow-up stays bound to a window that no longer has T-1: the chief decides.
    assert!(
        note.body.contains(&format!("T-{after} stays bound to @")),
        "{}",
        note.body
    );
    assert_eq!(note.state, "queued");
}

/// What task `number`'s row says it is held until.
fn held_until(w: &World, number: i64) -> Option<String> {
    w.ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .held_until
}

#[test]
fn a_held_task_taken_back_to_the_board_holds_with_nobody_and_its_line_says_no_hold() {
    // A quota-held task with a follow-up bound to its window, taken back: its note is typed
    // (a decision), so no shown note says its line; the hold went with the member it was taken
    // from, and its line must not say it still waits with that member.
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.follow_up(task, "Part 2", &[]).expect("a follow-up");
    w.hold(task);
    let typed = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    w.deliver(typed.id);
    assert_eq!(aside(&w, task), Some(said_of(Hold::Quota, &session)));
    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("released");
    let view = w
        .ledger
        .task(w.project, task)
        .expect("a read")
        .expect("the task")
        .task;
    assert_eq!(view.state, "open");
    assert_eq!(view.aside, None, "an open task says no hold");
    assert_eq!(view.held_until, None, "it holds with nobody");
}

#[test]
fn a_held_task_resumed_after_its_session_ended_holds_no_more_and_its_line_says_no_hold() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let typed = note_of(&mut w, Hold::Unavailable, task, &session, UNTIL);
    w.deliver(typed.id);
    assert_eq!(aside(&w, task), Some(said_of(Hold::Unavailable, &session)));
    w.ledger
        .end_session(w.project, &session, "human")
        .expect("a session that holds only a paused task ends");
    w.resume(task, "Go on");
    assert_eq!(
        w.ledger
            .task(w.project, task)
            .expect("a read")
            .expect("the task")
            .task
            .state,
        "open"
    );
    assert_eq!(aside(&w, task), None, "an open task says no hold");
    assert_eq!(held_until(&w, task), None, "it holds with nobody");
}

#[test]
fn what_waits_on_a_chief_that_takes_over_names_each_task_it_gave_that_is_held_and_its_hold() {
    // A switched chief is a new conversation: the first note of a hold was typed to the old
    // one, and the same hold said again is only shown. Its handoff names each held task with
    // what its line says (T-1500's finding 2); a busy model goes on by itself and is not one.
    for hold in HOLDS {
        let mut w = world();
        let (task, session) = with_zeus(&mut w);
        w.hold(task);
        let first = note_of(&mut w, hold, task, &session, UNTIL);
        w.deliver(first.id);
        note_of(&mut w, hold, task, &session, UNTIL);
        let open = w.ledger.chief_open_work(w.project).expect("the open work");
        let held: Vec<(i64, Option<String>)> = open
            .held
            .iter()
            .map(|task| (task.number, task.aside.clone()))
            .collect();
        assert_eq!(held, [(task, Some(said_of(hold, &session)))]);
    }
    // A hold that became a busy model's wait is one no more.
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let first = note_of(&mut w, HOLDS[0], task, &session, UNTIL);
    w.deliver(first.id);
    w.ledger
        .note_hold_busy(w.project, "chief", task, &session, "overloaded", true)
        .expect("the chief is told");
    let open = w.ledger.chief_open_work(w.project).expect("the open work");
    assert!(open.held.is_empty(), "{:?}", open.held);
}

/// What a chief that takes over is told of each task it gave that is held or whose member left.
fn told_held(w: &World) -> Vec<(i64, Option<String>)> {
    w.ledger
        .chief_open_work(w.project)
        .expect("the open work")
        .held
        .iter()
        .map(|task| (task.number, task.aside.clone()))
        .collect()
}

/// What a chief that takes over is told of a paused task whose member left the staff.
const MEMBER_GONE: &str = "is paused, and its member @zeus has left the staff";

#[test]
fn a_held_task_whose_member_left_the_staff_is_told_to_a_new_chief_as_paused_and_not_held() {
    // T-1514's finding A: removal leaves a paused task for its resumption, its hold kept: at
    // its end the daemon resumes it in a fresh window, or tells the chief it stays paused
    // (`out_of_quota`'s test of a member given a task by name). A chief that takes over is
    // told the task is paused with its member gone, not that it waits with that member.
    for hold in HOLDS {
        let mut w = world();
        let (task, session) = with_zeus(&mut w);
        w.hold(task);
        let first = note_of(&mut w, hold, task, &session, UNTIL);
        w.deliver(first.id);
        w.ledger
            .remove_member(w.project, "zeus")
            .expect("zeus leaves the staff");
        assert_eq!(w.state(task), "paused", "left for its resumption");
        assert_eq!(
            held_until(&w, task).as_deref(),
            Some(UNTIL),
            "its hold ends as it would"
        );
        assert_eq!(told_held(&w), [(task, Some(MEMBER_GONE.to_owned()))]);
    }
}

#[test]
fn a_paused_task_that_never_held_whose_member_left_the_staff_is_told_to_a_new_chief() {
    // The handoff lists every paused task the chief gave whose member has left the staff, held
    // or not: nothing else wakes a new chief for it (T-1517's note).
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    w.pause(task);
    w.ledger
        .remove_member(w.project, "zeus")
        .expect("zeus leaves the staff");
    assert_eq!(w.state(task), "paused", "left for its resumption");
    assert_eq!(held_until(&w, task), None, "it never held");
    assert_eq!(told_held(&w), [(task, Some(MEMBER_GONE.to_owned()))]);
}

/// What the line of a paused task whose member left the staff says beside its state.
const MEMBER_GONE_LINE: &str = "its member @zeus has left the staff";

#[test]
fn a_held_task_whose_member_left_the_staff_says_on_its_line_it_is_paused_with_its_member_gone() {
    // The hold is kept to its end (`out_of_quota`'s test of a member given a task by name), but
    // `cf task list` and `cf task get` say what the handoff says, not a hold with a member that
    // is gone (T-1517's note): typed first, then said again and shown.
    for hold in HOLDS {
        let mut w = world();
        let (task, session) = with_zeus(&mut w);
        w.hold(task);
        let first = note_of(&mut w, hold, task, &session, UNTIL);
        w.deliver(first.id);
        let again = note_of(&mut w, hold, task, &session, UNTIL);
        assert_eq!(again.state, "read", "the same hold said again is shown");
        assert_eq!(aside(&w, task), Some(said_of(hold, &session)));
        w.ledger
            .remove_member(w.project, "zeus")
            .expect("zeus leaves the staff");
        assert_eq!(
            held_until(&w, task).as_deref(),
            Some(UNTIL),
            "its hold is kept"
        );
        assert_eq!(aside(&w, task).as_deref(), Some(MEMBER_GONE_LINE));
    }
    // Paused and never held, the same.
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    w.pause(task);
    w.ledger
        .remove_member(w.project, "zeus")
        .expect("zeus leaves the staff");
    assert_eq!(aside(&w, task).as_deref(), Some(MEMBER_GONE_LINE));
}

#[test]
fn a_hold_after_its_task_left_its_member_is_another_hold_and_is_typed() {
    // T-1514's finding B: a hold is its member's. Taken back and given to another member, or
    // to a new window of the same member, the task's first refusal of the same kind and end is
    // typed: the chief's rescue failed, and only a typed note wakes it.
    for hold in HOLDS {
        for to in ["diana", "zeus"] {
            let mut w = world();
            let (task, session) = with_zeus(&mut w);
            w.hold(task);
            let first = note_of(&mut w, hold, task, &session, UNTIL);
            w.deliver(first.id);
            w.ledger
                .release_task(w.project, task, "by @human")
                .expect("released");
            let moved = w.assign(task, to);
            let other = moved.task.assignee.clone().expect("a session");
            w.deliver(moved.message.expect("its brief").id);
            w.hold(task);
            let again = note_of(&mut w, hold, task, &other, UNTIL);
            assert_eq!(
                again.state, "queued",
                "@{other}'s first hold after @{session}'s is typed"
            );
            assert_eq!(w.next("chief"), Some(again.id));
            // Said again by the same window, it is the same hold: shown.
            let same = note_of(&mut w, hold, task, &other, UNTIL);
            assert_eq!(same.state, "read");
        }
    }
}

/// A role read in parts (T-1513) meets a shown note (RS-25, T-1556): a note that says the chief
/// its role is typed whatever else is shown, on every path that writes one (`note_role`: a role
/// changed, a conversation begun without it, a setting's news), and its parts printed by `cf
/// inbox read` make it `read` with no `shown` in its receipt: it is no shown note, nor its
/// task's line, nor marked `not typed` (`cf`'s mark reads the receipt's `shown`).
#[test]
fn a_role_note_to_the_chief_is_typed_and_its_parts_read_make_it_no_shown_note() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    let held = shown(
        &mut w,
        "chief",
        task,
        "T-1 is still working in the background: 1 command.",
        "in the background: 1 command",
    );
    let chief = w.id("chief");
    let conversation = w
        .ledger
        .start_conversation(chief, "codex")
        .expect("a conversation")
        .id;
    let words: String = (1..=600).map(|line| format!("Rule {line}.\n")).collect();
    let role = w
        .ledger
        .note_role(w.project, "chief", &words, "the long role")
        .expect("a role");
    let cf = cf_proto::cf_word::CfWord::bare();
    let parts = cf_base::role_read::role_parts(&cf.in_text(&role.body)).len();
    assert_eq!(parts, 2, "the role is read in parts");
    // Typed: queued for its paste, no receipt, the next thing the chief's window is given.
    let written = w.message(role.id);
    assert_eq!(
        (written.state.as_str(), &written.receipt),
        ("queued", &json!(null))
    );
    assert_eq!(w.next("chief"), Some(role.id));
    // Pasted (its opening), then read part by part in the chief's window.
    w.deliver(role.id);
    for part in 1..=parts {
        w.ledger
            .receive_read_part(chief, role.id, part, parts, cf_ledger::Read::Inbox, &cf)
            .expect("a part read");
    }
    let read = w.message(role.id);
    assert_eq!(
        read.state, "read",
        "the role is read once every part was printed"
    );
    assert!(read.receipt.get(SHOWN).is_none(), "{}", read.receipt);
    assert_eq!(
        w.ledger
            .conversation_role_text(conversation)
            .expect("read")
            .as_deref(),
        Some("the long role")
    );
    // The task's line is still the shown note's, and the shown note is as it was.
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("in the background: 1 command")
    );
    assert_eq!(
        w.ledger
            .aside_note(w.project, task)
            .expect("read")
            .map(|note| note.id),
        Some(held.id)
    );
}

/// The other way: a shown note read in the chief's inbox, whole or as a part, tells the
/// conversation no role and is no part of one: it stays the shown note it was (T-1556).
#[test]
fn a_shown_note_read_in_the_inbox_is_no_role_read() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    let chief = w.id("chief");
    let conversation = w
        .ledger
        .start_conversation(chief, "codex")
        .expect("a conversation")
        .id;
    let held = shown(
        &mut w,
        "chief",
        task,
        "T-1 is still working in the background: 1 command.",
        "in the background: 1 command",
    );
    let before = w.message(held.id);
    let cf = cf_proto::cf_word::CfWord::bare();
    let whole = w
        .ledger
        .receive_read(chief, &[held.id], cf_ledger::Read::Inbox, &cf)
        .expect("read in the inbox");
    let part = w
        .ledger
        .receive_read_part(chief, held.id, 1, 1, cf_ledger::Read::Inbox, &cf)
        .expect("a part read");
    assert_eq!((whole, part), (0, 0), "nothing is received");
    let after = w.message(held.id);
    assert_eq!((after.state, after.receipt), (before.state, before.receipt));
    assert_eq!(
        w.ledger.conversation_role_text(conversation).expect("read"),
        None
    );
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("in the background: 1 command")
    );
}
