//! Whether the project leaves Claude Code's prompt suggestion on in its windows (T-1444,
//! Project settings, "Prompt suggestion"): off for every project until the human ticks it.
//! Every window's launch is told to switch it off (`suggestion_off`), the chief's as each
//! member's, whatever the role, at its first launch and each time its conversation is opened
//! again, and a chief switched to another agent is told as the one before it was. Which harness
//! acts on it is the adapter's (`cf-harness`'s launch tests: only Claude Code's settings file);
//! the engine says it the same to each. A project that ticked it launches every window as it
//! always did. A change reaches the window's next launch, and the window open keeps what it
//! began with. The test is not held to a Node recording: it is this build's own setting.

use cf_engine::testing::{Context, Made};
use cf_ledger::NewTask;
use serde_json::{json, Value};

use cf_engine::SwitchWhen;

use crate::chiefs::{to, with_codex};
use crate::fixtures::{launches_of, Tiers};

/// Every harness a window of a project runs on.
const HARNESSES: [&str; 5] = ["claude-code", "codex", "pi", "devin", "muse"];

/// The members' roles that take a task by tier, with the agent that holds each.
const MEMBERS: [(&str, &str); 4] = [
    ("worker", "zeus"),
    ("advisor", "athena"),
    ("reviewer", "argus"),
    ("tester", "tess"),
];

/// The human ticks "Prompt suggestion" for the project, or unticks it.
fn tick(context: &Context, project: i64, on: bool) {
    context
        .ledger
        .borrow_mut()
        .set_prompt_suggestion(project, on)
        .unwrap();
}

/// A project on `harness`, its chief and one member of each role in [`MEMBERS`].
fn staffed_on(context: &Context, harness: &str) -> i64 {
    let staff: Vec<Value> = MEMBERS
        .iter()
        .map(|(role, agent)| {
            json!({ "agent": agent, "harness": harness, "roles": [role], "tier": "standard" })
        })
        .collect();
    context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": harness, "model": "apollo" },
            "staff": staff,
        }))
        .unwrap()
        .expect("the project")
        .id
}

/// The chief's window is opened again: the project is closed and resumed.
fn relaunch(context: &Context, project: i64) {
    context.close_project(project).unwrap();
    context.resume_project(project).unwrap();
}

/// The launches the adapter was asked for of `role`, oldest first.
fn launches_of_role(context: &Context, role: &str) -> Vec<Value> {
    context
        .adapter
        .prepared()
        .into_iter()
        .filter(|launch| launch["role"] == role)
        .collect()
}

#[test]
fn every_launch_of_a_project_with_the_suggestion_off_is_told_to_switch_it_off_the_chiefs_among_them(
) {
    for harness in HARNESSES {
        let context = Context::made(Made {
            harnesses: HARNESSES.to_vec(),
            ..Made::default()
        });
        let project = staffed_on(&context, harness);
        assert_eq!(
            launches_of_role(&context, "chief")[0].get("suggestionOff"),
            Some(&json!(true)),
            "{harness}: the chief at its first launch"
        );
        for (role, _) in MEMBERS {
            context.pool_task(project, role, &format!("A task for the {role}"));
        }
        context.pass().unwrap();
        context.pass().unwrap();
        for (role, _) in MEMBERS {
            let launches = launches_of_role(&context, role);
            assert_eq!(launches.len(), 1, "{harness} {role}");
            assert_eq!(
                launches[0]["suggestionOff"],
                json!(true),
                "{harness} {role} at its launch"
            );
        }
        // The chief's window, opened again, is told it as well.
        relaunch(&context, project);
        let chief = launches_of_role(&context, "chief");
        assert_eq!(chief.len(), 2, "{harness}");
        assert!(
            chief[1]["resume"].is_string(),
            "{harness}: the chief is resumed"
        );
        assert_eq!(
            chief[1].get("suggestionOff"),
            Some(&json!(true)),
            "{harness}: the chief on resume"
        );
        context.close();
    }
}

#[test]
fn a_project_that_ticked_the_suggestion_launches_every_window_as_it_always_did_and_a_change_reaches_the_next_launch(
) {
    let context = Context::new();
    let project = staffed_on(&context, "claude-code");
    tick(&context, project, true);
    // Closed and resumed, the chief takes the word of the project as it is then.
    relaunch(&context, project);
    for (role, _) in MEMBERS {
        context.pool_task(project, role, &format!("A task for the {role}"));
    }
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = launches_of_role(&context, "chief");
    assert_eq!(chief.len(), 2);
    assert_eq!(
        chief[0].get("suggestionOff"),
        Some(&json!(true)),
        "at its first launch, before it was ticked"
    );
    assert_eq!(
        chief[1].get("suggestionOff"),
        None,
        "the owner's own setting, once it is"
    );
    for (role, _) in MEMBERS {
        assert_eq!(
            launches_of_role(&context, role)[0].get("suggestionOff"),
            None,
            "{role}: the owner's own setting"
        );
    }
    // Unticked, the windows that began without keep what they began with: no launch is
    // asked for again by the change alone.
    tick(&context, project, false);
    context.pass().unwrap();
    assert_eq!(launches_of_role(&context, "chief").len(), 2);
    for (role, _) in MEMBERS {
        assert_eq!(
            launches_of_role(&context, role).len(),
            1,
            "{role}: no new window"
        );
    }
    // And the chief's next launch is told again.
    relaunch(&context, project);
    assert_eq!(
        launches_of_role(&context, "chief")[2].get("suggestionOff"),
        Some(&json!(true))
    );
    context.close();
}

#[test]
fn a_chief_switched_to_another_agent_is_told_as_the_project_says_it_then() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let first = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(first["role"], "chief");
    assert_eq!(
        first["suggestionOff"],
        json!(true),
        "the chief it began with"
    );
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    let switched = context.codex.prepared().last().cloned().unwrap();
    assert_eq!(switched["role"], "chief");
    assert_eq!(
        switched["suggestionOff"],
        json!(true),
        "the chief switched in is told, on whatever harness it runs"
    );
    // With the suggestion ticked, the chief switched back is not.
    tick(&context, project.id, true);
    context
        .switch_chief(
            project.id,
            to("claude-code", "apollo"),
            SwitchWhen::Now,
            false,
        )
        .unwrap();
    let back = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(back["role"], "chief");
    assert_eq!(back.get("suggestionOff"), None, "{back}");
    context.close();
}

#[test]
fn a_member_opened_again_on_its_conversation_is_told_the_suggestion_as_the_project_says_it_by_then()
{
    let context = Context::new();
    // The host proves each window's program gone, as the app's does: a follow-up,
    // another task, goes on with the session's conversation (`ownership`).
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    assert_eq!(
        launches_of(&context, &session)[0]["suggestionOff"],
        json!(true),
        "at its first launch: off, as every project is"
    );
    // The human ticks and unticks it between the follow-ups that open the session's
    // conversation again: each takes the word of the project as it is then.
    for (follow_up, ticked) in [(2, true), (3, false), (4, true)] {
        tick(&context, project, ticked);
        context.adapter.answer(&session, "Done");
        context.pass().unwrap();
        context
            .ledger
            .borrow_mut()
            .accept_task(project, follow_up - 1, "chief")
            .unwrap();
        context.create_task(
            project,
            NewTask {
                from: "chief".to_owned(),
                after: Some(follow_up - 1),
                body: format!("Follow-up {follow_up}"),
                ..NewTask::default()
            },
        );
        for _ in 0..4 {
            context.pass().unwrap();
        }
        let launches = launches_of(&context, &session);
        let launch = launches.last().unwrap();
        assert!(launch["resume"].is_string(), "T-{follow_up}: resumed");
        assert_eq!(
            launch.get("suggestionOff"),
            (!ticked).then(|| json!(true)).as_ref(),
            "T-{follow_up}: the word of the project now"
        );
    }
    context.close();
}
