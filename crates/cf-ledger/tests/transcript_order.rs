//! The copy of a task's window is in the order the items came, whatever the record
//! the harness shows does (T-1118, from Gabriel's other board: at 18:42 the
//! transcript of T-3 ended at the message m-382 sent to the window, while the board
//! listed m-383, a result from the worker, and m-384, a task from the chief,
//! delivered, and showed T-3 working).
//!
//! A look copies the record from where the last one stopped, each item at its place
//! in the record (`from` + its index). A record that shrank (a resumed window
//! rewrote it, or two of its items became one) is copied from its start, and the
//! places of its new items were taken by older ones the copy kept: the new items
//! sorted among them, and the newest-last order every reader counts on was lost, so
//! `--last` showed old items while the window was well past them.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView,
};
use serde_json::{json, Value};

fn project(ledger: &mut Ledger) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                agent: Some("apollo".into()),
            },
            staff: vec![NewMember {
                agent: "zeus".into(),
                harness: "claude-code".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project")
}

/// T-1, given to a session of zeus whose window took the brief in, and the
/// conversation the window runs in.
fn window(ledger: &mut Ledger, view: &ProjectView) -> i64 {
    ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Parser".into(),
                ..NewTask::default()
            },
        )
        .expect("a task");
    let member = view
        .participants
        .iter()
        .find(|participant| participant.handle == "zeus")
        .expect("zeus")
        .id;
    let moved = ledger.assign_task(1, 1, member).expect("assigned");
    let brief = moved.message.expect("its brief").id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "u-1" })))
        .expect("its delivery is confirmed");
    let session = moved.task.assignee.expect("a session");
    let participant = ledger
        .project(1)
        .expect("a read")
        .expect("the project")
        .participants
        .iter()
        .find(|participant| participant.handle == session)
        .expect("the session")
        .id;
    ledger
        .start_conversation(participant, "claude-code")
        .expect("a conversation")
        .id
}

fn item(id: &str, role: &str, text: &str) -> Value {
    json!({ "id": id, "role": role, "text": text, "complete": true })
}

/// The task's transcript, the last `limit` items (all of them without one), as ids.
fn ids(ledger: &Ledger, limit: Option<usize>) -> Vec<String> {
    ledger
        .transcript(1, 1, limit)
        .expect("its transcript")
        .items
        .into_iter()
        .map(|item| item.id)
        .collect()
}

#[test]
fn the_items_of_a_record_that_shrank_come_after_the_ones_the_copy_kept() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("ledger");
    let view = project(&mut ledger);
    let conversation = window(&mut ledger, &view);

    // The window works: the brief, and two answers.
    let brief = item(
        "u-1",
        "user",
        "[ConsensFlow m-1 · T-1 · task from @chief]\nParser",
    );
    ledger
        .copy_transcript(
            conversation,
            &[
                brief.clone(),
                item("a-1", "assistant", "One"),
                item("a-2", "assistant", "Two"),
            ],
            0,
        )
        .expect("copied");
    assert_eq!(ids(&ledger, None), ["u-1", "a-1", "a-2"]);

    // A resumed window rewrote its record: shorter, with a new item where the old ones
    // were. The look copies it from its start.
    ledger
        .copy_transcript(
            conversation,
            &[brief.clone(), item("a-9", "assistant", "Again")],
            0,
        )
        .expect("copied");
    assert_eq!(
        ids(&ledger, None),
        ["u-1", "a-1", "a-2", "a-9"],
        "the new item is the newest"
    );
    assert_eq!(
        ids(&ledger, Some(1)),
        ["a-9"],
        "and `--last 1` shows it, not an older one"
    );

    // The record grows from there: each look copies it from its last item.
    ledger
        .copy_transcript(
            conversation,
            &[
                item("a-9", "assistant", "Again"),
                item("a-10", "assistant", "And again"),
            ],
            1,
        )
        .expect("copied");
    ledger
        .copy_transcript(
            conversation,
            &[
                item("a-10", "assistant", "And again"),
                item("a-11", "assistant", "Done"),
            ],
            2,
        )
        .expect("copied");
    assert_eq!(
        ids(&ledger, None),
        ["u-1", "a-1", "a-2", "a-9", "a-10", "a-11"],
        "in the order they came"
    );
    assert_eq!(ids(&ledger, Some(2)), ["a-10", "a-11"]);

    // An item the copy has, written again with its text changed, stays where it is: the
    // record shows it at an earlier place than the one the copy gave it.
    ledger
        .copy_transcript(
            conversation,
            &[
                item("a-9", "assistant", "Again, in full"),
                item("a-10", "assistant", "And again"),
                item("a-11", "assistant", "Done"),
            ],
            1,
        )
        .expect("copied");
    assert_eq!(
        ids(&ledger, None),
        ["u-1", "a-1", "a-2", "a-9", "a-10", "a-11"],
        "in the order they came"
    );
}

#[test]
fn an_item_the_record_says_again_later_stays_after_the_ones_that_came_between() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("ledger");
    let view = project(&mut ledger);
    let conversation = window(&mut ledger, &view);
    ledger
        .copy_transcript(
            conversation,
            &[
                item(
                    "u-1",
                    "user",
                    "[ConsensFlow m-1 · T-1 · task from @chief]\nParser",
                ),
                item("a-1", "assistant", "Running"),
                item("t-1", "tool", "first"),
            ],
            0,
        )
        .expect("copied");

    // A user's words come in, and the tool's output is said again, now after them: the
    // record has it later, and its text changed.
    ledger
        .copy_transcript(
            conversation,
            &[item("u-3", "user", "More"), item("t-1", "tool", "second")],
            2,
        )
        .expect("copied");
    assert_eq!(ids(&ledger, None), ["u-1", "a-1", "u-3", "t-1"]);
}
