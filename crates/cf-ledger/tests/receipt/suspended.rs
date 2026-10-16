//! The pauses a project's suspension makes, and the human's Resume that ends them
//! (`tasks/pausing.rs`): the work whose window closed with the project goes on
//! when it opens again, at work as a quit leaves it, and every other pause stays,
//! however it was made; so does a pause a ledger made before the mark.

use cf_ledger::{migrate, open_ledger, Options, MIGRATIONS};
use serde_json::json;

use crate::fixture::{world, World};

/// What the daemon says of a window that closed with its project.
const CLOSED: &str = "@zeus's window closed";

/// Task `number`, given to a new session of `member` and at work there: its
/// brief pasted and proved.
fn at_work(w: &mut World, member: &str) -> i64 {
    let number = w.open("Work").task.number;
    let brief = w.assign(number, member).message.expect("its brief").id;
    w.deliver(brief);
    assert_eq!(w.state(number), "working");
    number
}

#[test]
fn the_resume_of_a_project_has_the_work_its_suspension_paused_at_work_again_and_no_other_pause() {
    let mut w = world();
    let closed = at_work(&mut w, "zeus");
    let human = at_work(&mut w, "zeus");
    let chief = at_work(&mut w, "diana");
    let crashed = at_work(&mut w, "diana");
    let held = at_work(&mut w, "zeus");
    let told = at_work(&mut w, "diana");
    let open = at_work(&mut w, "zeus");
    // While the project is open: the human's pause, the chief's, a window that
    // closed by itself, a hold, the chief's tell, and a pause with the project
    // that is no suspension's.
    w.ledger
        .pause_task(w.project, human, Some("human"), None)
        .expect("paused by the human");
    w.ledger
        .pause_task(w.project, chief, Some("chief"), None)
        .expect("paused by the chief");
    w.ledger
        .pause_task(w.project, crashed, None, Some(CLOSED))
        .expect("paused by ConsensFlow");
    w.hold(held);
    let session = w
        .ledger
        .task(w.project, told)
        .expect("a read")
        .expect("the task")
        .task
        .assignee
        .expect("its session");
    w.tell(&session, told, "Use the new style");
    w.ledger
        .pause_with_project(w.project, open, CLOSED)
        .expect("paused, the project open");
    // The human's Close: the project is suspended, and its window closes.
    w.ledger
        .set_project_state(w.project, "suspended")
        .expect("closed");
    w.ledger
        .pause_with_project(w.project, closed, CLOSED)
        .expect("paused with the project");
    let note = w
        .ledger
        .note_pause(w.project, "chief", closed, CLOSED)
        .expect("the chief is told");
    let others = [human, chief, crashed, held, told, open];
    assert!(others
        .iter()
        .chain([&closed])
        .all(|number| w.state(*number) == "paused"));

    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");

    assert_eq!(w.state(closed), "working", "the Close's pause ends");
    for number in others {
        assert_eq!(w.state(number), "paused", "T-{number} stays paused");
    }
    let hold = w
        .ledger
        .task(w.project, held)
        .expect("a read")
        .expect("the task")
        .task;
    assert_eq!(
        hold.held_until.as_deref(),
        Some("2026-10-10T15:00:00.000Z"),
        "the hold is the daemon's to end"
    );
    let withdrawn = w.message(note.id);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some(format!("T-{closed} resumed").as_str())),
        "the news of the pause goes with it, never given"
    );
    let events = w.ledger.events(w.project, 0, 500).expect("the log");
    let went_on: Vec<_> = events
        .iter()
        .filter(|event| event.kind == "task.state" && event.data["from"] == "paused")
        .map(|event| event.data.clone())
        .collect();
    assert_eq!(
        went_on,
        [json!({
            "task": closed, "from": "paused", "to": "working",
            "by": null, "because": "the project was resumed",
        })]
    );
    // A Close and a Resume again leave every other pause as it is.
    w.ledger
        .set_project_state(w.project, "suspended")
        .expect("closed");
    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");
    for number in others {
        assert_eq!(w.state(number), "paused", "T-{number}, again");
    }
}

#[test]
fn a_task_the_suspension_paused_while_its_window_waited_for_an_answer_waits_again() {
    let mut w = world();
    let number = at_work(&mut w, "zeus");
    let session = w
        .ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .assignee
        .expect("its session");
    let question = w.ask(&session, number, "Which dialect?");
    w.deliver(question.id);
    assert_eq!(w.state(number), "waiting");
    w.ledger
        .set_project_state(w.project, "suspended")
        .expect("closed");
    w.ledger
        .pause_with_project(w.project, number, CLOSED)
        .expect("paused with the project");

    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");

    assert_eq!(
        w.state(number),
        "waiting",
        "its question is still to be answered"
    );
}

#[test]
fn a_task_the_suspension_paused_that_moved_since_is_no_longer_its_to_resume() {
    let mut w = world();
    let number = at_work(&mut w, "zeus");
    w.ledger
        .set_project_state(w.project, "suspended")
        .expect("closed");
    w.ledger
        .pause_with_project(w.project, number, CLOSED)
        .expect("paused with the project");
    // While it is closed, the chief resumes it, and the human pauses it.
    w.resume(number, "Go on");
    w.ledger
        .pause_task(w.project, number, Some("human"), None)
        .expect("paused by the human");

    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");

    assert_eq!(w.state(number), "paused", "the human's pause is the last");
}

#[test]
fn a_ledger_from_before_the_mark_keeps_every_paused_task_paused_through_a_resume() {
    const AT: &str = "2026-10-15T00:00:00.000Z";
    let dir = tempfile::tempdir().expect("a home");
    let file = dir.path().join("consensflow.db");
    // 3.0.4's ledger: a project its Close suspended, its task paused by ConsensFlow
    // as the window closed, and the chief told in a note not yet given.
    {
        let db = rusqlite::Connection::open(&file).expect("the file");
        migrate(&db, &MIGRATIONS[..18]).expect("a version 18 ledger");
        db.execute_batch(&format!(
            "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at) VALUES
               (1, '/work/app', 'app', 'suspended', 0, '{AT}', '{AT}');
             INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer) VALUES
               (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0),
               (2, 1, 'chief', 'chief', '[]', 'apollo', 'claude-code', NULL, NULL, '{AT}', 0),
               (3, 1, 'zeus', 'worker', '[\"worker\"]', 'zeus', 'claude-code', 'standard', NULL, '{AT}', 0);
             INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at, paused_at, stop_seq)
               VALUES (1, 1, 1, 'Parser', 'Parser', 2, 3, 'paused', '{AT}', '{AT}', '{AT}', 1);
             INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at) VALUES
               (1, 1, 3, 2, 'task', 1, 'Parser', 'delivered', '{AT}'),
               (2, 1, 2, NULL, 'note', 1, 'T-1 is paused: @zeus''s window closed. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation.', 'queued', '{AT}');
             INSERT INTO event (id, project_id, at, kind, data) VALUES
               (1, 1, '{AT}', 'task.state', '{{\"task\":1,\"from\":\"working\",\"to\":\"paused\",\"by\":null,\"because\":\"@zeus''s window closed\",\"stop\":1}}'),
               (2, 1, '{AT}', 'project.state', '{{\"from\":\"open\",\"to\":\"suspended\"}}');"
        ))
        .expect("its rows");
    }

    let mut ledger = open_ledger(&file, Options::default()).expect("this build opens it");
    ledger.set_project_state(1, "open").expect("resumed");

    let task = ledger.task(1, 1).expect("a read").expect("the task").task;
    assert_eq!(task.state, "paused", "a pause with no mark is not resumed");
    assert_eq!(
        ledger.message(2).expect("a read").expect("the note").state,
        "queued",
        "and the chief is still to be told of it"
    );
}
