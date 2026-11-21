//! Devin's real wire reader beside the dispatcher: a released quota refusal
//! must not turn later I/O failures into new automatic resumes (T-1878).

use std::fs;

use cf_engine::testing::Context;
use cf_harness::contract::Window;
use cf_harness::records::{Reading, Role, Settlement};
use cf_harness::seams::Time;
use serde_json::{json, Value};

use super::devin_fixture as devin;
use super::fixtures::{given, Tiers};

const SESSION: &str = "review-session";

fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
    }
}

fn lines(values: &[Value]) -> String {
    values.iter().map(|value| format!("{value}\n")).collect()
}

fn user(index: usize) -> Value {
    json!({"message_id": format!("u-{index}"), "role": "user", "content": "Go on",
        "metadata": {"extensions": {"chisel/client-message-id": format!("request-{index}")}}})
}

fn chunk(index: usize) -> Value {
    json!({"sessionId": SESSION, "turnClientMessageId": format!("request-{index}"),
        "update": {"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Looking"},
            "_meta": {"cognition.ai/streamingMessageId": format!("stream-{index}")}}})
}

fn end(index: usize, cause: &str, message: &str) -> Value {
    json!({"sessionId": SESSION, "turnClientMessageId": format!("request-{index}"),
        "cause": cause, "errorMessage": message})
}

fn store(home: &devin::Home) -> rusqlite::Connection {
    home.store().begins(SESSION);
    let path = if cfg!(windows) {
        "home/AppData/Roaming/devin/cli/sessions.db"
    } else {
        "home/.local/share/devin/cli/sessions.db"
    };
    rusqlite::Connection::open(std::path::Path::new(&home.root).join(path)).unwrap()
}

fn node(store: &rusqlite::Connection, id: &str, parent: Option<&str>, message: Value) {
    store.execute(
        "INSERT INTO message_nodes (session_id,node_id,parent_node_id,chat_message,created_at) VALUES (?1,?2,?3,?4,'2026-09-19T12:00:00Z')",
        (SESSION, id, parent, message.to_string()),
    ).unwrap();
    store
        .execute(
            "UPDATE sessions SET main_chain_id=?1 WHERE id=?2",
            (id, SESSION),
        )
        .unwrap();
}

// Feed the actual Devin window's reading into the dispatcher's test window,
// as the review did, while the dispatcher owns the task and its deliveries.
fn observe(context: &Context, session: &str, window: &dyn Window) {
    let observed = cf_harness::testing::finished(window.observe()).unwrap();
    let Some(Reading::Known(record)) = observed.reading.as_deref() else {
        panic!("Devin's record was not readable");
    };
    context.adapter.with(session, |agent| {
        for item in record
            .items
            .iter()
            .filter(|item| item.role == Role::Assistant)
        {
            if !agent.items.iter().any(|old| old.id == item.id) {
                agent.items.push(item.clone());
            }
        }
        agent.settled = record.settlement == Settlement::Settled;
        agent.failed = record.failed;
        agent.failure = record.failure.clone();
        agent.failure_kind = record.failure_kind.clone();
        agent.settled = observed.settled;
        agent.failed = observed.failed;
        agent.quota = observed.quota.clone();
    });
}

fn resumes(context: &Context, session: &str) -> usize {
    given(context, session)
        .iter()
        .filter(|message| message.contains("Resumed:"))
        .count()
}

fn old_quota_followed_by_io_errors(rewrite: bool) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    passes(&context, 2);
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);

    let home = devin::Home::new();
    let store = store(&home);
    home.fakes.time.settle_at(context.time.wall_ms());
    let window = devin::window(&home.adapter(), &devin::Request::resumed(SESSION));
    home.selects(SESSION);
    node(&store, "n1", None, user(1));
    home.appends(&lines(&[end(
        1,
        "quota_exhausted",
        "Your daily usage quota has been exhausted.",
    )]));
    observe(&context, &session, &*window);
    passes(&context, 2);
    assert_eq!(tiers.task(1).task.state, "paused");
    assert!(tiers.out_until("zeus").is_some());
    assert_eq!(resumes(&context, &session), 0);

    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    context.advance(30_001);
    home.fakes.time.settle_at(context.time.wall_ms());
    observe(&context, &session, &*window);
    passes(&context, 4);
    assert_eq!(resumes(&context, &session), 1);
    assert_eq!(tiers.task(1).task.state, "working");

    for index in [2, 3] {
        let user_node = format!("n{index}u");
        let answer_node = format!("n{index}a");
        let parent = if index == 2 { "n1" } else { "n2a" };
        node(&store, &user_node, Some(parent), user(index));
        node(
            &store,
            &answer_node,
            Some(&user_node),
            json!({
                "message_id": format!("a-{index}"), "role": "assistant", "content": "Looking"
            }),
        );
        home.appends(&lines(&[
            chunk(index),
            end(index, "error", "The agent crashed: I/O error"),
        ]));
        if rewrite {
            // Replace the file, retaining the original quota line and every later line.
            let wire = std::path::PathBuf::from(home.wire());
            let replacement = wire.with_extension("replacement");
            fs::copy(&wire, &replacement).unwrap();
            fs::rename(&replacement, &wire).unwrap();
        }
        home.fakes.time.settle_at(context.time.wall_ms());
        observe(&context, &session, &*window);
        passes(&context, 2);
        context.advance(120_001);
        home.fakes.time.settle_at(context.time.wall_ms());
        observe(&context, &session, &*window);
        passes(&context, 4);
    }
    assert_eq!(
        resumes(&context, &session),
        1,
        "two I/O errors must not deliver another resume"
    );
    assert_eq!(tiers.task(1).task.state, "failed");
    assert_eq!(tiers.out_until("zeus"), None);
}

#[test]
fn old_devin_quota_does_not_resume_later_io_errors() {
    old_quota_followed_by_io_errors(false);
}

#[test]
fn rewritten_devin_wire_does_not_reanimate_the_old_quota() {
    old_quota_followed_by_io_errors(true);
}

#[test]
fn rewriting_the_old_line_before_the_next_turn_does_not_hold_again() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    passes(&context, 2);
    let session = tiers.assignee(1);
    let home = devin::Home::new();
    let store = store(&home);
    home.fakes.time.settle_at(context.time.wall_ms());
    let window = devin::window(&home.adapter(), &devin::Request::resumed(SESSION));
    home.selects(SESSION);
    node(&store, "n1", None, user(1));
    home.appends(&lines(&[end(
        1,
        "quota_exhausted",
        "Your daily usage quota has been exhausted.",
    )]));
    observe(&context, &session, &*window);
    passes(&context, 2);
    assert!(tiers.out_until("zeus").is_some());
    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    context.advance(30_001);
    home.fakes.time.settle_at(context.time.wall_ms());
    observe(&context, &session, &*window);
    passes(&context, 4);
    assert_eq!(resumes(&context, &session), 1);
    assert_eq!(tiers.task(1).task.state, "working");

    let wire = std::path::PathBuf::from(home.wire());
    let replacement = wire.with_extension("replacement");
    fs::copy(&wire, &replacement).unwrap();
    fs::rename(&replacement, &wire).unwrap();
    context.advance(1_000);
    home.fakes.time.settle_at(context.time.wall_ms());
    observe(&context, &session, &*window);
    passes(&context, 3);
    assert_eq!(
        tiers.out_until("zeus"),
        None,
        "a rewrite is not a new refusal"
    );
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(resumes(&context, &session), 1);
}

#[test]
fn a_second_devin_quota_refusal_still_holds_the_member() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    passes(&context, 2);
    let session = tiers.assignee(1);
    let home = devin::Home::new();
    let store = store(&home);
    home.fakes.time.settle_at(context.time.wall_ms());
    let window = devin::window(&home.adapter(), &devin::Request::resumed(SESSION));
    home.selects(SESSION);
    node(&store, "n1", None, user(1));
    home.appends(&lines(&[end(
        1,
        "quota_exhausted",
        "Your daily usage quota has been exhausted.",
    )]));
    observe(&context, &session, &*window);
    passes(&context, 2);
    assert!(tiers.out_until("zeus").is_some());
    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    context.advance(30_001);
    home.fakes.time.settle_at(context.time.wall_ms());
    observe(&context, &session, &*window);
    passes(&context, 4);
    assert_eq!(resumes(&context, &session), 1);
    let wire = std::path::PathBuf::from(home.wire());
    let replacement = wire.with_extension("replacement");
    fs::copy(&wire, &replacement).unwrap();
    fs::rename(&replacement, &wire).unwrap();
    context.advance(1_000);
    home.fakes.time.settle_at(context.time.wall_ms());
    observe(&context, &session, &*window);
    passes(&context, 2);
    assert_eq!(tiers.out_until("zeus"), None);
    node(&store, "n2u", Some("n1"), user(2));
    home.appends(&lines(&[
        chunk(2),
        end(
            2,
            "quota_exhausted",
            "Your daily usage quota has been exhausted.",
        ),
    ]));
    home.fakes.time.settle_at(context.time.wall_ms());
    observe(&context, &session, &*window);
    passes(&context, 2);
    assert_eq!(tiers.task(1).task.state, "paused");
    assert!(
        tiers.out_until("zeus").is_some(),
        "a fresh refusal must hold immediately"
    );
    assert_eq!(resumes(&context, &session), 1);
}
