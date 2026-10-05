//! Switching the chief to another agent, where it is refused or goes on
//! as it was: a project it does not know, a harness's own default model, an
//! image agent, a closed project, an agent that was deleted, and a daemon
//! that stopped with a message on its way.

use cf_engine::SwitchWhen;
use cf_ledger::NewProject;
use serde_json::json;

use crate::chiefs::{assert_match, chief_of, to, to_human, with_codex};
use crate::traces::held_to;

const SUITES: &[&str] = &["switching the chief to another agent"];

#[test]
fn switches_no_chief_of_a_project_it_does_not_know_and_says_so() {
    let context = with_codex();
    let refused = context.switch_chief(42, to("codex", "astraeus"), SwitchWhen::Now, false);
    assert_eq!(refused.unwrap_err().to_string(), "no project 42");
    held_to(
        context.close(),
        SUITES,
        "switches no chief of a project it does not know, and says so",
    );
}

#[test]
fn switches_no_chief_to_a_harnesss_own_default_model_it_names_a_saved_agent() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.busy("chief");
    // An agent the request does not name is the empty one, which the engine refuses.
    for when in [SwitchWhen::Now, SwitchWhen::Turn] {
        let refused = context
            .switch_chief(project.id, to("codex", ""), when, false)
            .unwrap_err();
        assert_match(
            &refused.to_string(),
            "pick one of your saved agents for the chief",
        );
    }
    let now = chief_of(&context, project.id);
    assert_eq!(
        (now.harness.as_deref(), now.agent.as_deref()),
        (Some("claude-code"), Some("apollo"))
    );
    assert_eq!(
        context.dispatcher.pending_switch(chief),
        None,
        "nothing waits to switch"
    );
    assert!(context.host.killed().is_empty());
    assert!(context.codex.prepared().is_empty());
    held_to(
        context.close(),
        SUITES,
        "switches no chief to a harness's own default model: it names a saved agent",
    );
}

#[test]
fn opens_no_project_on_an_image_agent_nor_switches_a_chief_to_one() {
    let context = with_codex();
    context
        .roster
        .designers
        .borrow_mut()
        .insert("pygmalion".to_owned());
    let designing = "pygmalion is an image agent, which can only be an image designer";
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.busy("chief");
    let refused = context
        .open_project(json!({
            "directory": "/work/api",
            "name": "api",
            "chief": { "harness": "codex", "agent": "pygmalion" },
        }))
        .unwrap_err();
    assert_eq!(refused.to_string(), designing);
    for when in [SwitchWhen::Now, SwitchWhen::Turn] {
        let refused = context
            .switch_chief(project.id, to("codex", "pygmalion"), when, false)
            .unwrap_err();
        assert_eq!(refused.to_string(), designing);
    }
    let names: Vec<String> = context
        .ledger
        .borrow()
        .projects()
        .unwrap()
        .into_iter()
        .map(|project| project.name)
        .collect();
    assert_eq!(names, ["app"], "nothing was opened");
    let now = chief_of(&context, project.id);
    assert_eq!(
        (now.harness.as_deref(), now.agent.as_deref()),
        (Some("claude-code"), Some("apollo"))
    );
    assert_eq!(
        context.dispatcher.pending_switch(chief),
        None,
        "nothing waits to switch"
    );
    assert!(context.host.killed().is_empty());
    assert!(context.codex.prepared().is_empty());
    held_to(
        context.close(),
        SUITES,
        "opens no project on an image agent, nor switches a chief to one: an image agent only designs",
    );
}

#[test]
fn keeps_a_chief_on_its_harnesss_own_default_model_as_it_is_until_switch_chief_moves_it_to_an_agent(
) {
    let context = with_codex();
    // Opened before a chief was always a saved agent: its record names none.
    let created = NewProject::from_json(&json!({
        "directory": "/work/app",
        "name": "app",
        "chief": { "harness": "claude-code" },
    }))
    .unwrap();
    let project = context
        .ledger
        .borrow_mut()
        .create_project(&created)
        .unwrap();
    context.driver().resume_project(project.id).unwrap();
    let prepared = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(
        prepared["agent"],
        serde_json::Value::Null,
        "on its harness's own default"
    );
    context.pass().unwrap();
    let said = context
        .adapter
        .item(cf_harness::records::Role::User, "The codeword is tern");
    context
        .adapter
        .with("chief", |agent| agent.items.push(said));
    context.adapter.answer("chief", "Noted: tern");
    context.pass().unwrap();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    let now = chief_of(&context, project.id);
    assert_eq!(
        (now.harness.as_deref(), now.agent.as_deref()),
        (Some("codex"), Some("astraeus"))
    );
    let launched = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launched["message"].as_str().unwrap(),
        r"The human switched this project's chief from Claude Code to you, Codex \(astraeus\)\.",
    );
    held_to(
        context.close(),
        SUITES,
        "keeps a chief on its harness's own default model as it is, until Switch chief moves it to an agent",
    );
}

#[test]
fn switches_no_chief_of_a_closed_project() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.driver().close_project(project.id).unwrap();
    let refused = context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap_err();
    assert_match(&refused.to_string(), "app is closed: resume it first");
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("claude-code")
    );
    assert_eq!(context.codex.prepared().len(), 0);
    held_to(
        context.close(),
        SUITES,
        "switches no chief of a closed project",
    );
}

#[test]
fn keeps_a_chief_whose_agent_was_deleted_closed_tells_the_human_and_holds_what_it_was_to_receive() {
    let context = with_codex();
    context.roster.gone.borrow_mut().insert("nobody".to_owned());
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let refused = context
        .switch_chief(project.id, to("codex", "nobody"), SwitchWhen::Now, false)
        .unwrap_err();
    assert_match(&refused.to_string(), "nobody is not among your agents");
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    context.driver().close_project(project.id).unwrap();
    context
        .roster
        .gone
        .borrow_mut()
        .insert("astraeus".to_owned());
    let opened = context.host.opened().len();
    context.driver().resume_project(project.id).unwrap();
    assert_eq!(
        context.host.opened().len(),
        opened,
        "no window on a model that is gone"
    );
    let told = to_human(&context, project.id)
        .into_iter()
        .find(|body| body.contains("no longer among your agents"))
        .unwrap();
    assert_match(
        &told,
        r"The chief runs on astraeus, which is no longer among your agents: add it back under Agents, or switch the chief\.",
    );
    context.pass().unwrap();
    assert_eq!(
        context.host.opened().len(),
        opened,
        "and no pass tries again"
    );
    held_to(
        context.close(),
        SUITES,
        "keeps a chief whose agent was deleted closed, tells the human, and holds what it was to receive",
    );
}

#[test]
fn never_closes_the_new_chiefs_window_for_taking_long_to_show_its_handoff() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    context.codex.with("chief", |agent| agent.items.clear());
    context.advance(10 * 120_000);
    context.pass().unwrap();
    assert_eq!(
        context.host.killed().len(),
        1,
        "only the old chief's window was closed"
    );
    assert_eq!(context.project(project.id).state, "open");
    held_to(
        context.close(),
        SUITES,
        "never closes the new chief's window for taking long to show its handoff",
    );
}

#[test]
fn switches_a_chief_that_had_a_message_on_its_way_when_the_daemon_stopped() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let one = context.note(project.id, Some("zeus"), "chief", "One");
    context.pass().unwrap();
    // The daemon starts again a few seconds later.
    context.advance(5_000);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.resume_after_restart().unwrap();

    after
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("codex")
    );
    let launched = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launched["message"].as_str().unwrap(),
        r"You are the chief now\.",
    );
    after.pass().unwrap();
    context.codex.answer("chief", "Taken over.");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(
        context.message(one.id).state,
        "delivered",
        "it follows the new chief"
    );
    drop(after);
    held_to(
        context.close(),
        SUITES,
        "switches a chief that had a message on its way when the daemon stopped",
    );
}
