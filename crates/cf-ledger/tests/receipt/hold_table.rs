//! A whole read is accepted wherever transport says its reader already has the
//! message, and refused only where a hold keeps one the reader does not have yet
//! (Q2, review T-2293, finding 4). One table: every kind of message a window can
//! read, every state it can be in on its way, every hold. The receipt is taken in
//! a cell if and only if it is taken where nothing holds, and the hold either
//! keeps the message from a reader that does not have it yet or does not apply.

use cf_ledger::{NewNote, Read};

use crate::fixture::{world, World};

const KINDS: [&str; 7] = [
    "task", "question", "answer", "tell", "result", "note", "reminder",
];
const STATES: [&str; 5] = ["queued", "claimed", "begun", "handed", "delivered"];
const HOLDS: [&str; 4] = ["none", "a meeting", "a close", "a pause"];

/// A message of `kind`, and the task it is about.
fn made(w: &mut World, kind: &str) -> (i64, i64) {
    let task = w.open("Table fixture").task.number;
    let given = w.assign(task, "zeus");
    let zeus = given.task.assignee.clone().unwrap();
    let brief = given.message.unwrap().id;
    if kind != "task" {
        w.deliver(brief);
    }
    let id = match kind {
        "task" => brief,
        "question" => w.ask(&zeus, task, "Which format?").id,
        "answer" => {
            let question = w.ask(&zeus, task, "Which format?").id;
            w.deliver(question);
            w.answer(question, "Plain").id
        }
        "tell" => w.tell(&zeus, task, "Use ANSI").id,
        "note" | "result" => {
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
            if kind == "result" {
                w.edit(|db| {
                    db.execute("UPDATE message SET kind='result' WHERE id=?", [note])
                        .unwrap();
                });
            }
            note
        }
        "reminder" => {
            let question = w.ask(&zeus, task, "Which format?").id;
            w.deliver(question);
            w.ledger.begin_question_engine();
            w.ledger.close_questions_of_ended_engine().unwrap();
            let notice: i64 = w
                .ledger
                .sqlite()
                .query_row(
                    "SELECT MAX(id) FROM message WHERE kind='note' AND instr(body, ?)>0",
                    [format!("question m-{question} on T-")],
                    |row| row.get(0),
                )
                .unwrap();
            w.deliver(notice);
            w.ledger
                .remind_question(
                    w.project,
                    question,
                    None,
                    &NewNote {
                        from: None,
                        to: "chief".into(),
                        task: Some(task),
                        body: "It closed".into(),
                    },
                )
                .unwrap()
                .unwrap()
                .id
        }
        other => panic!("{other}"),
    };
    (task, id)
}

fn put_in(w: &mut World, id: i64, state: &str) {
    let sql = match state {
        "queued" => return,
        "claimed" => "UPDATE message SET claimed_at='2026-11-25T10:00:00.000Z' WHERE id=?",
        "begun" => "UPDATE message SET state='delivering', handed_at=NULL WHERE id=?",
        "handed" => {
            "UPDATE message SET state='delivering', handed_at='2026-11-25T10:00:00.000Z' WHERE id=?"
        }
        "delivered" => {
            "UPDATE message SET state='delivered', delivered_at='2026-11-25T10:00:00.000Z' WHERE id=?"
        }
        other => panic!("{other}"),
    };
    w.edit(|db| {
        db.execute(sql, [id]).unwrap();
    });
}

fn hold(w: &mut World, hold: &str, task: i64) {
    match hold {
        "none" => {}
        "a meeting" => {
            w.ledger.set_meeting(w.project, true).unwrap();
        }
        "a close" => {
            w.ledger.set_project_state(w.project, "suspended").unwrap();
        }
        "a pause" => {
            // A tell has paused its task already.
            let _ = w.ledger.pause_task(w.project, task, Some("human"), None);
        }
        other => panic!("{other}"),
    }
}

/// Whether a whole read in the inbox took a receipt of `id`.
fn taken_by_a_read(kind: &str, state: &str, held: &str) -> bool {
    let mut w = world();
    let (task, id) = made(&mut w, kind);
    put_in(&mut w, id, state);
    hold(&mut w, held, task);
    let reader = w.message(id).recipient_id;
    w.ledger
        .receive_read(
            reader,
            &[id],
            Read::Inbox,
            &cf_proto::cf_word::CfWord::bare(),
        )
        .unwrap();
    let after = w.message(id);
    after.state == "read" || after.receipt.get("readEarly").is_some()
}

/// Where a hold applies to a message the reader does not have yet. The reader has
/// it where transport has handed it over (begun and handed, or an answer's door
/// claimed it) or it was delivered.
fn kept(kind: &str, state: &str, held: &str) -> bool {
    let had = matches!(state, "handed" | "claimed" | "delivered");
    let to_chief = matches!(kind, "question" | "result" | "note" | "reminder");
    if had {
        return false;
    }
    match held {
        "none" => false,
        "a meeting" => to_chief && state == "queued",
        "a close" => matches!(state, "queued" | "begun"),
        "a pause" => kind == "reminder" && matches!(state, "queued" | "begun"),
        other => panic!("{other}"),
    }
}

#[test]
fn a_whole_read_is_received_wherever_its_reader_has_the_message_and_nothing_else_holds() {
    let mut wrong = Vec::new();
    for kind in KINDS {
        for state in STATES {
            // A claim is an answer's door's: no other message has one.
            if state == "claimed" && kind != "answer" {
                continue;
            }
            let baseline = taken_by_a_read(kind, state, "none");
            for held in HOLDS {
                let got = taken_by_a_read(kind, state, held);
                let want = baseline && !kept(kind, state, held);
                println!("{kind:9} {state:9} {held:10} taken={got} want={want}");
                if got != want {
                    wrong.push(format!(
                        "{kind} {state} under {held}: taken {got}, expected {want}"
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
