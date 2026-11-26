//! A question a restart or a project Close closed is reminded while its task
//! waits for new words (Q2, design `adapter-contract-questions.md` §2). Main
//! closes it and gives the chief one note; the wait is kept in `question_wait`
//! with the close's place in the ledger's own message order, the note, and the
//! reminders' cursor of their own. The clock starts at the chief's earliest
//! receipt of the note, and a reminder goes only while the wait is eligible.

use cf_base::time::{Clock as _, SystemClock};
use cf_ledger::{HandOver, NewNote, QuestionWait, Read};

use crate::fixture::{gated_world, world, World};

const RESTARTED: &str = "ConsensFlow restarted";

fn assigned(w: &mut World, gated: bool) -> (i64, String, i64) {
    let task = w.open("Review fixture").task.number;
    let given = w.assign(task, "zeus");
    let brief = given.message.unwrap().id;
    if gated {
        w.ledger.approve_message(brief, "human").unwrap();
    }
    (task, given.task.assignee.unwrap(), brief)
}

/// T-n with its brief received and a question of its received by the chief.
fn asked(w: &mut World) -> (i64, String, i64) {
    let (task, session, brief) = assigned(w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which format?").id;
    w.deliver(question);
    (task, session, question)
}

fn restart(w: &mut World) {
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
}

/// The newest note holding `words`, wherever it is.
fn note_with(w: &World, words: &str) -> i64 {
    w.ledger
        .sqlite()
        .query_row(
            "SELECT MAX(id) FROM message WHERE kind='note' AND instr(body, ?)>0",
            [words],
            |row| row.get::<_, Option<i64>>(0),
        )
        .unwrap()
        .unwrap_or_else(|| panic!("no note with {words:?}"))
}

/// The note the chief was given for `question`'s close.
fn close_note(w: &World, question: i64) -> i64 {
    note_with(w, &format!("question m-{question} on T-"))
}

fn wait_of(w: &World, question: i64) -> Option<QuestionWait> {
    w.ledger
        .waiting_questions(w.project)
        .unwrap()
        .into_iter()
        .find(|wait| wait.id == question)
}

fn closed_value(w: &World, question: i64) -> i64 {
    w.ledger
        .sqlite()
        .query_row(
            "SELECT closed FROM question_binding WHERE question_id=?",
            [question],
            |row| row.get(0),
        )
        .unwrap()
}

fn reminder_note(task: i64, words: &str) -> NewNote {
    NewNote {
        from: None,
        to: "chief".into(),
        task: Some(task),
        body: words.into(),
    }
}

fn read_in_the_inbox(w: &mut World, id: i64) {
    let chief = w.id("chief");
    w.ledger
        .receive_read(
            chief,
            &[id],
            Read::Inbox,
            &cf_proto::cf_word::CfWord::bare(),
        )
        .unwrap();
}

#[test]
fn a_restarted_question_is_a_wait_once_its_note_is_delivered() {
    let mut w = world();
    let (task, session, question) = asked(&mut w);
    restart(&mut w);
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(closed_value(&w, question), 2);
    let note = close_note(&w, question);
    assert!(
        wait_of(&w, question).is_none(),
        "a note that never reached the chief starts no clock"
    );
    w.deliver(note);
    let wait = wait_of(&w, question).expect("its note was delivered");
    assert_eq!(wait.closed.as_deref(), Some(RESTARTED));
    assert_eq!(wait.task, Some(task));
    assert_eq!(wait.held_task, Some(task));
    assert_eq!(wait.sender, session);
    assert_eq!(
        Some(wait.received_at),
        w.message(note).delivered_at,
        "the clock starts at the note's receipt"
    );
}

#[test]
fn the_clock_starts_at_the_first_inbox_read_of_a_note_still_on_its_way() {
    let mut w = world();
    let (_task, _session, question) = asked(&mut w);
    restart(&mut w);
    let note = close_note(&w, question);
    w.begin(note);
    read_in_the_inbox(&mut w, note);
    let first = wait_of(&w, question)
        .expect("the chief read the note while its paste was on its way")
        .received_at;
    read_in_the_inbox(&mut w, note);
    w.confirm(note);
    let delivered = w.message(note).delivered_at.unwrap();
    let wait = wait_of(&w, question).unwrap();
    assert!(first < delivered, "{first} before {delivered}");
    assert_eq!(wait.received_at, first, "the earliest receipt is kept");
}

#[test]
fn a_returned_answers_note_starts_the_same_clock() {
    let mut w = world();
    let (_task, _session, question) = asked(&mut w);
    let answer = w.answer(question, "Use tabs").id;
    assert_eq!(w.message(answer).state, "queued");
    restart(&mut w);
    let reason = w.message(answer).reason.unwrap();
    let note: i64 = reason
        .strip_prefix("returned in m-")
        .unwrap()
        .parse()
        .unwrap();
    assert!(wait_of(&w, question).is_none());
    w.deliver(note);
    let wait = wait_of(&w, question).expect("the returned answer's note was delivered");
    assert_eq!(wait.closed.as_deref(), Some(RESTARTED));
    assert_eq!(Some(wait.received_at), w.message(note).delivered_at);
}

/// T-1911 #16: a gated answer survives the restart, the chief queues a tell on
/// the task, and the approval then writes the returned-answer note: the chief
/// acted after the close, though not after the note.
#[test]
fn a_tell_queued_before_the_returned_answers_note_is_the_chiefs_action() {
    let mut w = gated_world();
    let (task, session, brief) = assigned(&mut w, true);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which output?").id;
    w.ledger.approve_message(question, "human").unwrap();
    w.deliver(question);
    let answer = w.answer(question, "Return these words").id;
    assert_eq!(w.message(answer).state, "gated");
    restart(&mut w);
    assert_eq!(closed_value(&w, question), 2);
    w.ledger.set_gate(w.project, false).unwrap();
    let tell = w
        .ledger
        .tell_queued(w.project, task, "chief", "Use the revised output")
        .unwrap();
    w.ledger.approve_message(answer, "human").unwrap();
    let note = note_with(&w, "Return these words");
    assert!(tell.id < note, "the tell was queued before the note");
    w.deliver(note);
    assert!(
        wait_of(&w, question).is_none(),
        "the chief has told the member: nothing is reminded"
    );
    // A tell that was taken back is no action.
    w.ledger
        .sqlite()
        .execute("UPDATE message SET state='cancelled' WHERE id=?", [tell.id])
        .unwrap();
    assert!(
        wait_of(&w, question).is_some(),
        "a tell taken back lets the reminders go on"
    );
}

#[test]
fn a_closed_questions_reminder_has_a_cursor_of_its_own_and_the_open_ones_link_stays() {
    for handed in [false, true] {
        let mut w = world();
        let (task, _session, question) = asked(&mut w);
        let open = w
            .ledger
            .remind_question(w.project, question, None, &reminder_note(task, "Answer it"))
            .unwrap()
            .expect("the open question's reminder");
        if handed {
            w.begin(open.id);
            assert_eq!(w.ledger.admit(open.id, &[]).unwrap(), HandOver::Admitted);
        }
        restart(&mut w);
        let note = close_note(&w, question);
        if !handed {
            // main's own check retires the "Answer it" not yet handed.
            w.begin(open.id);
            assert_eq!(w.ledger.admit(open.id, &[]).unwrap(), HandOver::Withdrawn);
        } else {
            // handed, then given back after the close.
            w.ledger
                .retry_delivery(open.id, "returned after close", true)
                .unwrap();
            assert_eq!(w.message(open.id).state, "cancelled");
        }
        w.deliver(note);
        let linked: Option<i64> = w
            .ledger
            .sqlite()
            .query_row(
                "SELECT reminder_id FROM question_reminder WHERE question_id=?",
                [question],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked, Some(open.id), "the open question's link is kept");
        let wait = wait_of(&w, question).expect("the closed wait");
        assert!(
            wait.reminder.is_none(),
            "the closed cursor is empty: handed={handed}"
        );
        let closed = w
            .ledger
            .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
            .unwrap()
            .expect("the closed question's own reminder");
        let after: Option<i64> = w
            .ledger
            .sqlite()
            .query_row(
                "SELECT reminder_id FROM question_reminder WHERE question_id=?",
                [question],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after, Some(open.id), "the open cursor was not reused");
        assert_eq!(
            wait_of(&w, question).unwrap().reminder.map(|m| m.id),
            Some(closed.id)
        );
        assert_ne!(
            w.message(closed.id).state,
            "cancelled",
            "the closed reminder stands"
        );
    }
}

#[test]
fn none_is_written_while_one_is_queued_and_the_next_follows_its_delivery() {
    let mut w = world();
    let (task, _session, question) = asked(&mut w);
    restart(&mut w);
    w.deliver(close_note(&w, question));
    let first = w
        .ledger
        .remind_question(w.project, question, None, &reminder_note(task, "First"))
        .unwrap()
        .unwrap();
    assert!(w
        .ledger
        .remind_question(
            w.project,
            question,
            Some(first.id),
            &reminder_note(task, "Again")
        )
        .unwrap()
        .is_none());
    assert!(w
        .ledger
        .remind_question(
            w.project,
            question,
            None,
            &reminder_note(task, "Stale cursor")
        )
        .unwrap()
        .is_none());
    w.deliver(first.id);
    let second = w
        .ledger
        .remind_question(
            w.project,
            question,
            Some(first.id),
            &reminder_note(task, "Second"),
        )
        .unwrap();
    assert!(
        second.is_some(),
        "a delivered reminder gives way to the next"
    );
}

/// What ends the wait, as each is done to the task or the member.
fn end_it(w: &mut World, end: &str, task: i64, session: &str) {
    match end {
        "a queued tell" => {
            w.ledger
                .tell_queued(w.project, task, "chief", "New words")
                .unwrap();
        }
        "a delivered tell" => {
            let tell = w
                .ledger
                .tell_queued(w.project, task, "chief", "New words")
                .unwrap();
            w.deliver(tell.id);
        }
        "a finish" => {
            w.ledger
                .record_finished(w.project, task, "Done", (0, 0))
                .unwrap();
        }
        "a pause" => {
            w.ledger
                .pause_task(w.project, task, Some("human"), None)
                .unwrap();
        }
        "a cancel" => {
            w.ledger.cancel_task(w.project, task, "human").unwrap();
        }
        "a deletion" => {
            w.ledger.cancel_task(w.project, task, "human").unwrap();
            w.ledger.delete_tasks(w.project, &[task]).unwrap();
        }
        "the member's removal" => {
            let member = session
                .rsplit_once("-session-")
                .map_or(session, |(handle, _)| handle);
            w.ledger.remove_member(w.project, member).unwrap();
        }
        other => panic!("{other}"),
    }
}

#[test]
fn each_end_stops_the_reminders_and_withdraws_a_queued_one() {
    for end in [
        "a queued tell",
        "a delivered tell",
        "a finish",
        "a pause",
        "a cancel",
        "a deletion",
        "the member's removal",
    ] {
        let mut w = world();
        let (task, session, question) = asked(&mut w);
        restart(&mut w);
        w.deliver(close_note(&w, question));
        let reminder = w
            .ledger
            .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
            .unwrap()
            .unwrap_or_else(|| panic!("a reminder before {end}"));
        end_it(&mut w, end, task, &session);
        assert!(
            wait_of(&w, question).is_none(),
            "{end}: the wait is no longer listed"
        );
        assert!(
            w.ledger
                .remind_question(
                    w.project,
                    question,
                    Some(reminder.id),
                    &reminder_note(task, "Late")
                )
                .unwrap()
                .is_none(),
            "{end}: no further reminder is written"
        );
        let state = w.message(reminder.id).state;
        if state == "queued" {
            w.begin(reminder.id);
            assert_eq!(
                w.ledger.admit(reminder.id, &[]).unwrap(),
                HandOver::Withdrawn,
                "{end}: the queued reminder is withdrawn at its admission"
            );
        }
    }
}

#[test]
fn a_failed_tell_lets_the_reminders_go_on() {
    let mut w = world();
    let (task, _session, question) = asked(&mut w);
    restart(&mut w);
    w.deliver(close_note(&w, question));
    let tell = w
        .ledger
        .tell_queued(w.project, task, "chief", "New words")
        .unwrap();
    assert!(wait_of(&w, question).is_none());
    w.ledger
        .sqlite()
        .execute("UPDATE message SET state='failed' WHERE id=?", [tell.id])
        .unwrap();
    assert!(wait_of(&w, question).is_some());
}

#[test]
fn no_wait_where_main_makes_none() {
    // A task a person paused before the restart.
    let mut w = world();
    let (task, _session, question) = asked(&mut w);
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .unwrap();
    restart(&mut w);
    if let Some(note) = w
        .ledger
        .sqlite()
        .query_row(
            "SELECT MAX(id) FROM message WHERE kind='note' AND instr(body, 'closed when')>0",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .unwrap()
    {
        w.deliver(note);
    }
    assert!(wait_of(&w, question).is_none(), "a paused task is no wait");

    // A note that failed.
    let mut w = world();
    let (_task, _session, question) = asked(&mut w);
    restart(&mut w);
    let note = close_note(&w, question);
    w.ledger
        .sqlite()
        .execute("UPDATE message SET state='failed' WHERE id=?", [note])
        .unwrap();
    assert!(
        wait_of(&w, question).is_none(),
        "a failed note starts no clock"
    );
}

#[test]
fn a_second_restart_keeps_the_wait_and_writes_no_second_note() {
    let mut w = world();
    let (_task, _session, question) = asked(&mut w);
    restart(&mut w);
    w.deliver(close_note(&w, question));
    let before = wait_of(&w, question).unwrap().received_at;
    restart(&mut w);
    let notes: i64 = w
        .ledger
        .sqlite()
        .query_row(
            "SELECT count(*) FROM message WHERE kind='note' AND instr(body, 'closed when')>0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(notes, 1);
    assert_eq!(wait_of(&w, question).unwrap().received_at, before);
}

/// A wait an earlier build left: its note is linked where the ledger names it,
/// as a returned answer's reason does, and its clock is that note's receipt;
/// else its clock starts at the update.
#[test]
fn the_migration_links_only_the_note_the_ledger_names() {
    for case in [
        "closed note",
        "returned answer",
        "returned answer queued",
        "a tell since",
    ] {
        let mut w = world();
        let (task, _session, question) = asked(&mut w);
        let answer = case
            .starts_with("returned answer")
            .then(|| w.answer(question, "Use tabs").id);
        if case == "a tell since" {
            w.ledger
                .tell_queued(w.project, task, "chief", "New words")
                .unwrap();
        }
        restart(&mut w);
        let note = match answer {
            Some(answer) => w
                .message(answer)
                .reason
                .unwrap()
                .strip_prefix("returned in m-")
                .unwrap()
                .parse::<i64>()
                .unwrap(),
            None => close_note(&w, question),
        };
        if case != "returned answer queued" {
            w.deliver(note);
        }
        let arrived = w.message(note).delivered_at;
        let updated = now_ms();
        as_alpha_13(&mut w);
        match case {
            "closed note" => {
                assert_eq!(note_of_wait(&w, question), None, "{case}: no text is read");
                from_the_update(&w, question, updated);
            }
            "returned answer" => {
                assert_eq!(note_of_wait(&w, question), Some(note), "{case}");
                let wait = wait_of(&w, question).expect("timed from the note");
                assert_eq!(Some(wait.received_at), arrived, "{case}");
            }
            "returned answer queued" => {
                assert!(
                    wait_of(&w, question).is_none(),
                    "{case}: its note has not arrived"
                );
                w.deliver(note);
                let wait = wait_of(&w, question).expect("timed when it arrives");
                assert_eq!(Some(wait.received_at), w.message(note).delivered_at);
            }
            _ => assert!(wait_of(&w, question).is_none(), "{case}: never reminded"),
        }
    }
}

/// A gated answer with no note yet at the migration: its wait starts at the
/// update, and the note its approval writes later is linked without moving it.
#[test]
fn a_note_written_after_the_update_leaves_the_clock_at_the_update() {
    let mut w = gated_world();
    let (task, session, brief) = assigned(&mut w, true);
    w.deliver(brief);
    let question = w.ask(&session, task, "What next?").id;
    w.ledger.approve_message(question, "human").unwrap();
    w.deliver(question);
    let answer = w.answer(question, "Return this answer after approval").id;
    restart(&mut w);
    assert_eq!(closed_value(&w, question), 2);
    let updated = now_ms();
    as_alpha_13(&mut w);
    from_the_update(&w, question, updated);
    let clock = wait_of(&w, question).unwrap().received_at;
    let after: i64 = w
        .ledger
        .sqlite()
        .query_row(
            "SELECT after_id FROM question_wait WHERE question_id=?",
            [question],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        after, question,
        "the conservative boundary of an old question"
    );
    w.ledger.set_gate(w.project, false).unwrap();
    w.ledger.approve_message(answer, "human").unwrap();
    let note = note_with(&w, "Return this answer after approval");
    assert_eq!(
        w.message(answer).reason,
        Some(format!("returned in m-{note}"))
    );
    w.deliver(note);
    assert_eq!(note_of_wait(&w, question), Some(note));
    assert_eq!(wait_of(&w, question).unwrap().received_at, clock);
    // Any words of the chief to that member since the question are its action.
    w.ledger
        .tell_queued(w.project, task, "chief", "New words")
        .unwrap();
    assert!(wait_of(&w, question).is_none());
}

#[test]
fn a_project_close_writes_none_while_it_lasts_and_its_resume_keeps_the_clock() {
    let mut w = world();
    let (task, _session, question) = asked(&mut w);
    restart(&mut w);
    w.deliver(close_note(&w, question));
    let clock = wait_of(&w, question).unwrap().received_at;
    let reminder = w
        .ledger
        .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
        .unwrap()
        .unwrap();
    w.ledger
        .sqlite()
        .execute(
            "UPDATE project SET state='suspended' WHERE id=?",
            [w.project],
        )
        .unwrap();
    assert!(
        wait_of(&w, question).is_none(),
        "nothing is reminded while the project is closed"
    );
    w.begin(reminder.id);
    assert_ne!(
        w.ledger.admit(reminder.id, &[]).unwrap(),
        HandOver::Admitted
    );
    w.ledger
        .sqlite()
        .execute("UPDATE project SET state='open' WHERE id=?", [w.project])
        .unwrap();
    assert_eq!(
        wait_of(&w, question).unwrap().received_at,
        clock,
        "the Resume goes on with the same clock"
    );
}

/// Round 2 (review T-2017), 1: a recorded wait leaves Delete Project working.
#[test]
fn a_project_with_a_recorded_wait_is_deleted_with_it() {
    let mut w = world();
    let (task, _session, question) = asked(&mut w);
    restart(&mut w);
    let note = close_note(&w, question);
    w.deliver(note);
    let reminder = w
        .ledger
        .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
        .unwrap()
        .unwrap();
    w.deliver(reminder.id);
    w.ledger
        .record_finished(w.project, task, "Done", (0, 0))
        .unwrap();
    w.ledger
        .sqlite()
        .execute(
            "UPDATE project SET state='suspended' WHERE id=?",
            [w.project],
        )
        .unwrap();
    let rows: i64 = w
        .ledger
        .sqlite()
        .query_row("SELECT count(*) FROM question_wait", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1, "the wait is recorded");
    let project = w.project;
    w.ledger
        .delete_project(project)
        .expect("a project with a recorded wait is deleted");
    let rows: i64 = w
        .ledger
        .sqlite()
        .query_row("SELECT count(*) FROM question_wait", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0, "the wait goes with its question");
}

/// The ledger as alpha.13 left it: no table of waits, opened again by this build,
/// which applies this migration alone.
fn as_alpha_13(w: &mut World) {
    w.edit(|db| {
        cf_ledger::testing::undo_migration_0048(db).expect("0048 undone");
        cf_ledger::testing::undo_migration_0047(db).unwrap();
        cf_ledger::testing::undo_migration_0046(db).unwrap();
        db.pragma_update(None, "user_version", 45).unwrap();
    });
}

fn quoting(question: i64, task: i64) -> String {
    format!("Re your notice: question m-{question} on T-{task} closed when the project closed, so what now?")
}

/// A member's quotation of the notice's words is not read: no note is linked, and
/// the clock starts at the update.
#[test]
fn the_migration_takes_no_members_quotation_for_the_closure_notice() {
    let mut w = world();
    let (task, session, question) = asked(&mut w);
    let quote = w
        .ledger
        .note(
            w.project,
            &NewNote {
                from: Some(session.clone()),
                to: "chief".into(),
                task: None,
                body: quoting(question, task),
            },
        )
        .unwrap()
        .id;
    w.deliver(quote);
    restart(&mut w);
    w.deliver(close_note(&w, question));
    let updated = now_ms();
    as_alpha_13(&mut w);
    assert_eq!(note_of_wait(&w, question), None);
    from_the_update(&w, question, updated);
}

/// An old wait's clock, where the ledger names no note: the update's own time.
fn from_the_update(w: &World, question: i64, since_ms: i64) {
    let wait = wait_of(w, question).expect("reminded from the update");
    let at = cf_base::time::parse(&wait.received_at).expect("a time");
    // The migration reads SQLite's clock, which keeps milliseconds.
    assert!(
        at >= since_ms - 1,
        "{at} from {since_ms}: {}",
        wait.received_at
    );
}

fn now_ms() -> i64 {
    SystemClock.now_ms()
}

fn note_of_wait(w: &World, question: i64) -> Option<i64> {
    w.ledger
        .sqlite()
        .query_row(
            "SELECT note_id FROM question_wait WHERE question_id=?",
            [question],
            |r| r.get(0),
        )
        .unwrap()
}

/// Round 3 (review T-2019): a quotation that begins with the writer's own
/// sentence is no notice. The migration matches no text.
#[test]
fn the_migration_takes_no_quotation_that_begins_with_the_writers_sentence() {
    let mut w = gated_world();
    let (task, session, brief) = assigned(&mut w, true);
    w.deliver(brief);
    let question = w.ask(&session, task, "What next?").id;
    w.ledger.approve_message(question, "human").unwrap();
    w.deliver(question);
    let quote = w
        .ledger
        .note(w.project, &NewNote {
            from: Some(session.clone()),
            to: "chief".into(),
            task: None,
            body: format!("@{session}'s question m-{question} on T-{task} closed when the project closed.\nThat is an example from the draft."),
        })
        .unwrap()
        .id;
    w.ledger.approve_message(quote, "human").unwrap();
    w.deliver(quote);
    w.answer(question, "Return this");
    restart(&mut w);
    let updated = now_ms();
    as_alpha_13(&mut w);
    assert_ne!(
        note_of_wait(&w, question),
        Some(quote),
        "the quotation is no notice"
    );
    from_the_update(&w, question, updated);
}

/// Round 3 (review T-2019): a renamed member's real notice names the old handle;
/// the migration reads no handle, so the wait is kept.
#[test]
fn the_migration_keeps_the_wait_of_a_renamed_member() {
    let mut w = world();
    let (_task, session, question) = asked(&mut w);
    restart(&mut w);
    w.deliver(close_note(&w, question));
    let window = w.id(&session);
    w.edit(|db| {
        db.execute(
            "UPDATE participant SET handle='hermes-session-1', former_handles=?1 WHERE id=?2",
            rusqlite::params![format!("[\"{session}\"]"), window],
        )
        .unwrap();
    });
    let updated = now_ms();
    as_alpha_13(&mut w);
    from_the_update(&w, question, updated);
}

/// What the chief's inbox lists of `reminder`.
fn listed(w: &World, reminder: i64) -> bool {
    w.ledger
        .readable_inbox(w.id("chief"), 100)
        .unwrap()
        .iter()
        .any(|m| m.id == reminder)
}

/// Round 2 and 3: the inbox lists and takes the receipt of no reminder the wait
/// no longer allows, and reading changes nothing.
#[test]
fn the_inbox_shows_no_stale_reminder_and_takes_no_receipt_of_it() {
    for (end, in_progress) in [
        ("a tell", false),
        ("a tell", true),
        ("a pause", false),
        ("a pause", true),
    ] {
        let mut w = world();
        let (task, _session, question) = asked(&mut w);
        restart(&mut w);
        w.deliver(close_note(&w, question));
        let reminder = w
            .ledger
            .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
            .unwrap()
            .unwrap();
        if in_progress {
            w.begin(reminder.id);
        }
        assert!(
            listed(&w, reminder.id),
            "{end}, in progress {in_progress}: it stands"
        );
        if end == "a tell" {
            w.ledger
                .tell_queued(w.project, task, "chief", "New words")
                .unwrap();
        } else {
            w.ledger
                .pause_task(w.project, task, Some("human"), None)
                .unwrap();
        }
        let before = w.message(reminder.id);
        assert!(
            w.ledger.reminder_withheld(reminder.id).unwrap().is_some(),
            "{end}, in progress {in_progress}"
        );
        assert!(!listed(&w, reminder.id), "{end}, in progress {in_progress}");
        read_in_the_inbox(&mut w, reminder.id);
        let after = w.message(reminder.id);
        assert_eq!(
            after.state, before.state,
            "{end}, in progress {in_progress}: a read changes nothing"
        );
        assert!(after.receipt.get("readEarly").is_none());
    }
}

/// A reminder that failed and was replaced is shown to no reader, though no
/// cursor names it any more.
#[test]
fn a_reminder_that_failed_and_was_replaced_is_not_listed() {
    let mut w = world();
    let (task, _session, question) = asked(&mut w);
    restart(&mut w);
    w.deliver(close_note(&w, question));
    let first = w
        .ledger
        .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
        .unwrap()
        .unwrap();
    w.begin(first.id);
    w.ledger
        .fail_delivery(first.id, "the window refused it")
        .unwrap();
    let second = w
        .ledger
        .remind_question(
            w.project,
            question,
            Some(first.id),
            &reminder_note(task, "Still closed"),
        )
        .unwrap()
        .expect("the next takes the cursor");
    assert!(!listed(&w, first.id), "the failed one is nobody's");
    assert!(listed(&w, second.id));
}

/// A reminder the chief had stays its own, whatever the wait does after.
#[test]
fn a_delivered_reminder_stays_readable() {
    let mut w = world();
    let (task, _session, question) = asked(&mut w);
    restart(&mut w);
    w.deliver(close_note(&w, question));
    let reminder = w
        .ledger
        .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
        .unwrap()
        .unwrap();
    w.deliver(reminder.id);
    w.ledger.cancel_task(w.project, task, "human").unwrap();
    assert!(listed(&w, reminder.id));
}

/// A reminder a delivery gave back while the project was closed is held, not
/// withdrawn, and goes after the Resume.
#[test]
fn a_reminder_given_back_during_a_close_is_kept() {
    let mut w = world();
    let (task, _session, question) = asked(&mut w);
    restart(&mut w);
    w.deliver(close_note(&w, question));
    let reminder = w
        .ledger
        .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
        .unwrap()
        .unwrap();
    w.begin(reminder.id);
    w.ledger.set_project_state(w.project, "suspended").unwrap();
    w.ledger
        .pause_with_project(w.project, task, "the project closed")
        .unwrap();
    w.ledger
        .retry_delivery(reminder.id, "its window ended", true)
        .unwrap();
    assert_eq!(
        w.message(reminder.id).state,
        "queued",
        "held for the Resume"
    );
    assert!(!listed(&w, reminder.id), "hidden while closed");
    w.ledger.set_project_state(w.project, "open").unwrap();
    assert!(listed(&w, reminder.id), "back at the Resume");
}

#[test]
fn a_reminder_that_still_stands_is_read_in_the_inbox_as_before() {
    let mut w = world();
    let (task, _session, question) = asked(&mut w);
    restart(&mut w);
    w.deliver(close_note(&w, question));
    let reminder = w
        .ledger
        .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
        .unwrap()
        .unwrap();
    let chief = w.id("chief");
    assert!(w.ledger.reminder_withheld(reminder.id).unwrap().is_none());
    let read = w
        .ledger
        .receive_read(
            chief,
            &[reminder.id],
            Read::Inbox,
            &cf_proto::cf_word::CfWord::bare(),
        )
        .unwrap();
    assert_eq!(read, 1);
}

/// The receipt itself checks the wait: no listing came first.
#[test]
fn a_receipt_is_taken_of_no_reminder_the_wait_no_longer_allows_even_unlisted() {
    for in_progress in [false, true] {
        let mut w = world();
        let (task, _session, question) = asked(&mut w);
        restart(&mut w);
        w.deliver(close_note(&w, question));
        let reminder = w
            .ledger
            .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
            .unwrap()
            .unwrap();
        if in_progress {
            w.begin(reminder.id);
        }
        w.ledger
            .tell_queued(w.project, task, "chief", "New words")
            .unwrap();
        let chief = w.id("chief");
        let read = w
            .ledger
            .receive_read(
                chief,
                &[reminder.id],
                Read::Inbox,
                &cf_proto::cf_word::CfWord::bare(),
            )
            .unwrap();
        assert_eq!(read, 0, "in progress {in_progress}");
        let after = w.message(reminder.id);
        let pending = if in_progress { "delivering" } else { "queued" };
        assert_eq!(
            after.state, pending,
            "in progress {in_progress}: a read changes nothing"
        );
        assert!(after.receipt.get("readEarly").is_none());
    }
}

/// The reminder `closed_and_reminded` makes: a restart's closed question, its note
/// had by the chief, and the closed question's reminder written.
fn reminded(w: &mut World) -> (i64, i64) {
    let (task, _session, question) = asked(w);
    restart(w);
    w.deliver(close_note(w, question));
    let reminder = w
        .ledger
        .remind_question(w.project, question, None, &reminder_note(task, "It closed"))
        .unwrap()
        .unwrap();
    (task, reminder.id)
}

/// What takes a reminder's wait from it, as each is done to the task.
fn end_the_wait(w: &mut World, task: i64, end: &str) {
    match end {
        "a tell" => {
            w.ledger
                .tell_queued(w.project, task, "chief", "New words")
                .unwrap();
        }
        "a cancel" => {
            w.ledger.cancel_task(w.project, task, "human").unwrap();
        }
        "a deletion" => {
            w.ledger.cancel_task(w.project, task, "human").unwrap();
            w.ledger.delete_tasks(w.project, &[task]).unwrap();
        }
        other => panic!("{other}"),
    }
}

/// Round 4 (review T-2276, 1): a reminder the chief has had stays the chief's to
/// look at again, whatever then happens to its task. Only one it has not had is
/// hidden.
#[test]
fn a_reminder_the_chief_has_had_stays_readable_when_its_wait_ends() {
    for end in ["a tell", "a cancel", "a deletion"] {
        for had in [
            "read while its paste was on its way",
            "handed to its window",
            "delivered",
            "read in the inbox",
        ] {
            let mut w = world();
            let (task, reminder) = reminded(&mut w);
            match had {
                "read while its paste was on its way" => {
                    w.begin(reminder);
                    read_in_the_inbox(&mut w, reminder);
                }
                "handed to its window" => {
                    w.begin(reminder);
                    assert_eq!(w.ledger.admit(reminder, &[]).unwrap(), HandOver::Admitted);
                }
                "delivered" => {
                    w.deliver(reminder);
                }
                _ => read_in_the_inbox(&mut w, reminder),
            }
            end_the_wait(&mut w, task, end);
            // Transport's next look at it withdraws what the window has not had.
            if w.message(reminder).state == "delivering" {
                let _ = w.ledger.admit(reminder, &[]);
            }
            assert!(
                w.ledger.reminder_withheld(reminder).unwrap().is_none(),
                "{end}, {had}: m-{reminder} is {:?}",
                w.message(reminder).state
            );
            assert!(listed(&w, reminder), "{end}, {had}: listed");
        }
    }
}

/// What ends a meeting with the chief: the project's meeting on, held reminders.
fn meeting_on(w: &mut World) {
    w.ledger.set_meeting(w.project, true).unwrap();
}

/// Round 4 (review T-2276, 3): a reminder a chief's meeting holds is not
/// deliverable now: not listed, no place in the page, no receipt, and once the
/// meeting ends, listed and delivered once.
#[test]
fn a_reminder_a_meeting_holds_is_not_listed_and_takes_no_receipt() {
    let mut w = world();
    let (_task, reminder) = reminded(&mut w);
    let chief = w.id("chief");
    meeting_on(&mut w);
    assert!(
        w.ledger
            .next_delivery_of(chief, reminder)
            .unwrap()
            .is_none(),
        "transport holds it"
    );
    assert_eq!(
        w.ledger.reminder_withheld(reminder).unwrap(),
        Some(cf_ledger::Withheld::Meeting),
        "held"
    );
    assert!(!listed(&w, reminder));
    let before = w.message(reminder);
    read_in_the_inbox(&mut w, reminder);
    let after = w.message(reminder);
    assert_eq!(
        after.state, before.state,
        "a read of a held reminder changes nothing"
    );
    assert!(after.receipt.get("readEarly").is_none());
    w.ledger.set_meeting(w.project, false).unwrap();
    assert!(
        w.ledger.reminder_withheld(reminder).unwrap().is_none(),
        "back with the meeting's end"
    );
    assert!(listed(&w, reminder));
    let next = w.ledger.next_delivery_of(chief, reminder).unwrap();
    assert_eq!(next.map(|m| m.id), Some(reminder), "and delivered");
    w.deliver(reminder);
    assert_eq!(w.message(reminder).state, "delivered", "once");
}

/// Round 4 (review T-2276, 3): a hundred held reminders take no place in the
/// inbox's page, and so hide no note that can be delivered.
#[test]
fn a_hundred_held_reminders_hide_no_note_that_can_be_delivered() {
    let mut w = world();
    let task = w.open("Review fixture").task.number;
    let given = w.assign(task, "zeus");
    let zeus = given.task.assignee.clone().unwrap();
    w.deliver(given.message.unwrap().id);
    let questions: Vec<i64> = (0..100)
        .map(|n| {
            let question = w.ask(&zeus, task, &format!("Question {n}?")).id;
            w.deliver(question);
            question
        })
        .collect();
    restart(&mut w);
    let notices: Vec<i64> = w
        .ledger
        .sqlite()
        .prepare("SELECT id FROM message WHERE kind='note' AND instr(body,'closed when')>0 AND state='queued'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for notice in notices {
        w.deliver(notice);
    }
    let older = w
        .ledger
        .note(
            w.project,
            &NewNote {
                from: None,
                to: "chief".into(),
                task: None,
                body: "An older note".into(),
            },
        )
        .unwrap()
        .id;
    w.deliver(older);
    meeting_on(&mut w);
    for question in &questions {
        w.ledger
            .remind_question(
                w.project,
                *question,
                None,
                &reminder_note(task, "It closed"),
            )
            .unwrap();
    }
    assert!(
        listed(&w, older),
        "the older note is in the page of {} it is not displaced from",
        100
    );
}

/// Review T-2276, 3: the other holds take no receipt of a reminder either: a
/// project's Close, a pause, and a reminder no one has yet approved.
#[test]
fn the_other_holds_take_no_receipt_of_a_reminder_either() {
    for hold in ["a close", "a pause"] {
        let mut w = world();
        let (task, reminder) = reminded(&mut w);
        match hold {
            "a close" => {
                w.ledger.set_project_state(w.project, "suspended").unwrap();
                w.ledger
                    .pause_with_project(w.project, task, "the project closed")
                    .unwrap();
            }
            _ => {
                w.ledger
                    .pause_task(w.project, task, Some("human"), None)
                    .unwrap();
            }
        }
        let before = w.message(reminder);
        assert_eq!(
            w.ledger.reminder_withheld(reminder).unwrap(),
            Some(cf_ledger::Withheld::Gone),
            "{hold}"
        );
        assert!(!listed(&w, reminder), "{hold}");
        read_in_the_inbox(&mut w, reminder);
        let after = w.message(reminder);
        assert_eq!(after.state, before.state, "{hold}: a read changes nothing");
        assert!(after.receipt.get("readEarly").is_none(), "{hold}");
    }
}

/// Review T-2276, 3: what a hold keeps from the chief is not received by a read of
/// it, whatever its kind: an ordinary note a meeting holds, and one a closed
/// project holds, are as they were after the receipt.
#[test]
fn an_ordinary_note_a_hold_keeps_takes_no_receipt() {
    for hold in ["a meeting", "a close"] {
        let mut w = world();
        let chief = w.id("chief");
        match hold {
            "a meeting" => meeting_on(&mut w),
            _ => {
                w.ledger.set_project_state(w.project, "suspended").unwrap();
            }
        }
        let note = w
            .ledger
            .note(
                w.project,
                &NewNote {
                    from: None,
                    to: "chief".into(),
                    task: None,
                    body: "A note".into(),
                },
            )
            .unwrap()
            .id;
        read_in_the_inbox(&mut w, note);
        assert_eq!(
            w.message(note).state,
            "queued",
            "{hold}: a read of what it holds changes nothing"
        );
        match hold {
            "a meeting" => {
                w.ledger.set_meeting(w.project, false).unwrap();
            }
            _ => {
                w.ledger.set_project_state(w.project, "open").unwrap();
            }
        }
        assert!(
            w.ledger.next_delivery_of(chief, note).unwrap().is_some(),
            "{hold}: then it goes"
        );
    }
}
