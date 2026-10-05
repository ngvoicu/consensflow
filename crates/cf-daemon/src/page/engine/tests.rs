//! The engine trait is the dispatcher's: each call reaches the dispatcher
//! and answers as it answers, through the engine's own test kit.

use std::rc::Rc;

use cf_engine::testing::Context;
use cf_ledger::{NewChief, NewMember, NewProject};

use super::*;

fn request() -> NewProject {
    NewProject {
        directory: "/work/app".to_owned(),
        name: "app".to_owned(),
        chief: NewChief {
            harness: "claude-code".to_owned(),
            agent: Some("apollo".to_owned()),
        },
        staff: vec![NewMember {
            agent: "zeus".to_owned(),
            harness: "claude-code".to_owned(),
            designer: false,
            roles: vec!["worker".to_owned()],
            tier: "standard".to_owned(),
        }],
        gate: false,
    }
}

fn engine(kit: &Context) -> Rc<dyn Engine> {
    Rc::new(Rc::clone(&kit.dispatcher))
}

#[test]
fn the_projections_of_a_participant_nobody_knows_are_the_closed_window_of_nothing() {
    let kit = Context::new();
    let engine = engine(&kit);
    assert_eq!(engine.activity(999), json!({ "state": "closed" }));
    assert!(!engine.hidden(999));
    assert_eq!(engine.pending_switch(999), None);
    assert_eq!(engine.pane(999), None);
    assert!(!engine.holding(999).unwrap());
}

#[test]
fn an_activity_is_written_as_the_dispatcher_writes_its_object() {
    let said = |activity: &Activity| activity_value(activity).to_string();
    for state in [
        ActivityState::Starting,
        ActivityState::Working,
        ActivityState::Idle,
        ActivityState::Closed,
    ] {
        assert_eq!(
            said(&Activity::of(state)),
            format!(r#"{{"state":"{}"}}"#, state.as_str()),
            "no reason to give"
        );
    }
    assert_eq!(
        said(&Activity::because(
            ActivityState::Out,
            "out of quota until 2099-01-01T00:00:00.000Z"
        )),
        r#"{"state":"out","reason":"out of quota until 2099-01-01T00:00:00.000Z"}"#
    );
    assert_eq!(
        said(&Activity::because(
            ActivityState::Unknown,
            "the look failed"
        )),
        r#"{"state":"unknown","reason":"the look failed"}"#
    );
    // `observed.waiting.reason ?? null`: a window that waits names its reason
    // or says `null`, and no other window says `null`.
    assert_eq!(
        said(&Activity::of(ActivityState::Waiting)),
        r#"{"state":"waiting","reason":null}"#
    );
    assert_eq!(
        said(&Activity::because(ActivityState::Waiting, "a question")),
        r#"{"state":"waiting","reason":"a question"}"#
    );
}

#[test]
fn a_pane_and_a_waiting_switch_are_written_as_the_dispatcher_writes_them() {
    let pane = Pane {
        id: "p1-chief".to_owned(),
        generation: 1_791_194_400_123,
    };
    assert_eq!(
        pane_value(&pane).to_string(),
        r#"{"id":"p1-chief","generation":1791194400123}"#
    );
    let to = SwitchTo {
        harness: "codex".to_owned(),
        agent: "diana".to_owned(),
    };
    assert_eq!(
        switch_value(&to).to_string(),
        r#"{"harness":"codex","agent":"diana"}"#
    );
}

#[test]
fn a_harness_with_no_adapter_is_refused_and_the_ones_with_one_are_not() {
    let kit = Context::new();
    let engine = engine(&kit);
    engine.require_adapter("claude-code").unwrap();
    let refused = engine.require_adapter("mystery").unwrap_err();
    assert!(!refused.to_string().is_empty());
}

#[test]
fn a_member_that_is_not_in_the_project_is_not_back_from_quota() {
    let kit = Context::new();
    let project = kit.with_staff(&["zeus"]);
    let engine = engine(&kit);
    let refused = engine.back_from_quota(project.id, "nobody").unwrap_err();
    assert_eq!(
        refused.refusal().message,
        format!("no @nobody in project {}", project.id)
    );
}

#[test]
fn a_project_opens_its_chief_window_and_closes_it_through_the_trait() {
    let kit = Context::new();
    let engine = engine(&kit);
    let opened = {
        let engine = Rc::clone(&engine);
        kit.run(async move { engine.open_project(request()).await })
            .unwrap()
            .expect("the project, not deleted")
    };
    let chief = opened
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .unwrap();
    let pane = engine.pane(chief.id).expect("the chief's window is open");
    let keys: Vec<&str> = pane
        .as_object()
        .map(|pane| pane.keys().map(String::as_str).collect())
        .unwrap_or_default();
    assert_eq!(keys, ["id", "generation"], "{pane}");
    assert_ne!(engine.activity(chief.id), json!({ "state": "closed" }));

    let closed = {
        let engine = Rc::clone(&engine);
        let project = opened.id;
        kit.run(async move { engine.close_project(project).await })
            .unwrap()
            .expect("the project is still there")
    };
    assert_eq!(closed.state, "suspended");
    assert_eq!(engine.pane(chief.id), None, "its window went with it");
}

#[test]
fn a_session_is_opened_hidden_and_ended_through_the_trait_with_the_engine_s_refusals() {
    let kit = Context::new();
    let project = kit.with_staff(&["zeus"]);
    let engine = engine(&kit);
    let missing = {
        let engine = Rc::clone(&engine);
        let id = project.id;
        kit.run(async move { engine.open_window(id, "nobody").await })
            .unwrap_err()
    };
    assert!(missing.to_string().contains("nobody"), "{missing}");
    let ended = {
        let engine = Rc::clone(&engine);
        let id = project.id;
        kit.run(async move { engine.end_session(id, "nobody").await })
            .unwrap_err()
    };
    assert!(ended.to_string().contains("nobody"), "{ended}");
}

#[test]
fn deleting_an_open_project_is_refused_and_a_closed_one_goes_with_its_counts() {
    let kit = Context::new();
    let project = kit.with_staff(&["zeus"]);
    let engine = engine(&kit);
    let id = project.id;
    let refused = {
        let engine = Rc::clone(&engine);
        kit.run(async move { engine.delete_project(id).await })
            .unwrap_err()
    };
    assert!(!refused.to_string().is_empty());
    {
        let engine = Rc::clone(&engine);
        kit.run(async move { engine.close_project(id).await })
            .unwrap();
    }
    let gone = {
        let engine = Rc::clone(&engine);
        kit.run(async move { engine.delete_project(id).await })
            .unwrap()
    };
    assert_eq!((gone.id, gone.name.as_str()), (id, "app"));
}
