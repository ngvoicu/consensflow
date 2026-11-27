//! Every view the page and `cf` read of a project, as JSON, to the byte, on a project of
//! one chief, two members and the human, with a task in each way it can be (the leads
//! design, step 0). The migration that adds leads must leave every one of them equal:
//! the golden is `tests/fixtures/no_lead_views.json`. Set `CF_WRITE_GOLDENS=1` to write
//! it again after a change that is meant to move a view.

use super::fixture::{world, World};
use cf_ledger::NewTask;
use serde::Serialize;
use serde_json::{json, Value};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/no_lead_views.json"
);

fn json_of(view: &impl Serialize) -> Value {
    serde_json::to_value(view).expect("a view is JSON")
}

/// The project: T-1 at work in zeus with a note and an open question, T-2 the human's for
/// the chief (queued), T-3 waiting for T-1, T-4 done in diana with its result queued, T-5
/// cancelled, T-6 the human's for a standard worker (open).
fn busy(w: &mut World) {
    let one = w.open("Parser").task.number;
    let session = w.assign(one, "zeus").task.assignee.expect("a session");
    let brief = w.ledger.task(w.project, one).unwrap().unwrap().messages[0].id;
    w.deliver(brief);
    w.note(&session, "chief", one, "Half of it is done");
    w.ask(&session, one, "Which branch?");
    w.ledger
        .create_task(
            w.project,
            &NewTask {
                from: "human".into(),
                to: Some("chief".into()),
                body: "Plan the week".into(),
                ..NewTask::default()
            },
        )
        .expect("the human's task");
    w.open_needing("Docs", &[one]);
    let four = w.open("Tests").task.number;
    let session = w.assign(four, "diana").task.assignee.expect("a session");
    let brief = w.ledger.task(w.project, four).unwrap().unwrap().messages[0].id;
    w.deliver(brief);
    w.ledger
        .record_finished(w.project, four, "Tests pass", (0, 0))
        .expect("finished");
    let five = w.open("Cleanup").task.number;
    w.ledger
        .cancel_task(w.project, five, "chief")
        .expect("cancelled");
    w.ledger
        .create_task(
            w.project,
            &NewTask {
                from: "human".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "From the page".into(),
                ..NewTask::default()
            },
        )
        .expect("the human's task for a pool");
    let _ = session;
}

#[test]
fn every_view_of_a_project_with_no_lead_is_the_same_json_to_the_byte() {
    let mut w = world();
    busy(&mut w);
    let ledger = &w.ledger;
    let tasks: Vec<Value> = (1..=6)
        .map(|number| json_of(&ledger.task(w.project, number).unwrap().unwrap()))
        .collect();
    let fits: Vec<Value> = (1..=6)
        .map(|number| json_of(&ledger.task_that_fits(w.project, number).unwrap().unwrap()))
        .collect();
    let handles = ["human", "chief", "zeus", "diana"];
    let project = ledger.project(w.project).unwrap().unwrap();
    let sessions: Vec<String> = project
        .participants
        .iter()
        .map(|participant| participant.handle.clone())
        .collect();
    let inboxes: Vec<Value> = sessions
        .iter()
        .map(|handle| {
            let id = w.id(handle);
            json!({
                "handle": handle,
                "inbox": json_of(&ledger.inbox(id, 100).unwrap()),
                "latest": json_of(&ledger.latest_messages(id, false).unwrap()),
                "unread": json_of(&ledger.latest_messages(id, true).unwrap()),
            })
        })
        .collect();
    let seen = json!({
        "handles": handles,
        "project": json_of(&project),
        "projects": json_of(&ledger.projects().unwrap()),
        "board": json_of(&ledger.board(w.project).unwrap()),
        "open": json_of(&ledger.open_tasks(w.project).unwrap()),
        "tasks": tasks,
        "fits": fits,
        "transcripts": (1..=6).map(|number| json_of(&ledger.latest_transcript(w.project, number, None).unwrap())).collect::<Vec<_>>(),
        "inboxes": inboxes,
        "members": json_of(&ledger.members(w.project, "worker").unwrap()),
    });
    let text = format!("{}\n", serde_json::to_string_pretty(&seen).unwrap());
    if writing_goldens() {
        std::fs::write(GOLDEN, &text).expect("the golden is written");
    }
    let golden = std::fs::read_to_string(GOLDEN).expect("the golden is there");
    assert!(
        golden == text,
        "a view of a project with no lead moved: if it is meant to, run again with CF_WRITE_GOLDENS=1 and read the diff of {GOLDEN}"
    );
}

/// Whether this run writes the golden again (a test's own switch, read where it is used).
#[allow(clippy::disallowed_methods)]
fn writing_goldens() -> bool {
    std::env::var_os("CF_WRITE_GOLDENS").is_some()
}
