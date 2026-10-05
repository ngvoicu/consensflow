//! What a look at a fresh window finds: its screen drawn or not, the process
//! its adapter is told of, and a window that has not named its conversation
//! (`describe('the dispatcher')`).

use cf_engine::testing::Context;
use cf_engine::{Activity, ActivityState};
use cf_harness::records::Role;
use cf_ledger::NewNote;
use serde_json::json;

use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher"];

#[test]
fn keeps_a_fresh_window_starting_until_its_screen_is_drawn() {
    let context = Context::new();
    context.host.set_snapshot(json!({ "outputQuietMs": null }));
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.pass().unwrap();
    let state = || context.dispatcher.activity(chief).state;
    assert_eq!(state(), ActivityState::Starting, "nothing printed yet");
    context.host.set_snapshot(json!({ "outputQuietMs": 300 }));
    context.pass().unwrap();
    assert_eq!(state(), ActivityState::Starting, "still drawing");
    context.host.set_snapshot(json!({ "outputQuietMs": 2_000 }));
    context.pass().unwrap();
    assert_eq!(state(), ActivityState::Idle);
    held_to(
        context.close(),
        SUITES,
        "keeps a fresh window starting until its screen is drawn: printed, then still a moment",
    );
}

#[test]
fn tells_the_adapter_the_windows_process_when_the_pane_host_names_it_before_it_starts() {
    let context = Context::new();
    context.host.pid.set(Some(4242));
    context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let seen: Vec<(String, serde_json::Value)> = context
        .recorder
        .calls("adapter:claude-code", &["started", "observe"])
        .into_iter()
        .map(|(method, given)| (method, given[0]["launch"]["pid"].clone()))
        .collect();
    assert_eq!(
        seen,
        [
            ("started".to_owned(), json!(4242)),
            ("observe".to_owned(), json!(4242))
        ]
    );
    held_to(
        context.close(),
        SUITES,
        "tells the adapter the window's process, when the pane host names it, before it starts",
    );
}

#[test]
fn reads_a_window_that_has_not_named_its_first_conversation_as_starting_its_messages_held() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    // A resumed window has its record from the start; its harness has not
    // said yet which conversation the window shows.
    let earlier = [
        context.adapter.item(Role::User, "earlier"),
        context.adapter.item(Role::Assistant, "earlier answer"),
    ];
    context.adapter.with("chief", |agent| {
        agent.items.extend(earlier);
        agent.unnamed = Some("the window has not said yet which conversation it shows".to_owned());
    });
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: None,
                to: "chief".to_owned(),
                body: "A result came".to_owned(),
                task: None,
            },
        )
        .unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.activity(chief),
        Activity::of(ActivityState::Starting)
    );
    assert_eq!(
        context.message(note.id).state,
        "queued",
        "its message waits"
    );

    context.adapter.with("chief", |agent| agent.unnamed = None);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");
    // Once the window has named one, showing none is a wait the board names.
    let reason = "the window shows no conversation: its session list is open";
    context
        .adapter
        .with("chief", |agent| agent.unnamed = Some(reason.to_owned()));
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.activity(chief),
        Activity::because(ActivityState::Waiting, reason)
    );
    held_to(
        context.close(),
        SUITES,
        "reads a window that has not named its first conversation as starting, its messages held",
    );
}

#[test]
fn reads_a_window_that_names_no_conversation_as_starting_while_it_still_draws_its_screen() {
    let context = Context::new();
    context.host.set_snapshot(json!({ "outputQuietMs": 300 }));
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    let earlier = [
        context.adapter.item(Role::User, "earlier"),
        context.adapter.item(Role::Assistant, "earlier answer"),
    ];
    context
        .adapter
        .with("chief", |agent| agent.items.extend(earlier));
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.activity(chief).state,
        ActivityState::Idle,
        "it named its own"
    );
    context.adapter.with("chief", |agent| {
        agent.unnamed = Some("the window is reconnecting".to_owned());
    });
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.activity(chief),
        Activity::of(ActivityState::Starting)
    );
    context.host.set_snapshot(json!({ "outputQuietMs": 2_000 }));
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.activity(chief),
        Activity::because(ActivityState::Waiting, "the window is reconnecting")
    );
    held_to(
        context.close(),
        SUITES,
        "reads a window that names no conversation as starting while it still draws its screen",
    );
}
