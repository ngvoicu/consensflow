//! Whether the project has its members run with Claude Code's own advisor tool (A5-20,
//! Project settings, "Advisor tool"): off for every project until the human ticks it. Every
//! member's launch is told to switch it off (`advisor_off`), whatever the member's role,
//! at its first launch and each time its conversation is opened again, and the chief's never is
//! (the chief keeps its harness's own setting). Which harness acts on it is the adapter's
//! (`cf-harness`'s launch tests: only Claude Code's settings file); the engine says it the same
//! to each. A project that ticked it launches every window as it always did. A change reaches
//! the member's next launch, and the window open keeps what it began with. The test is not
//! held to a Node recording: it is this build's own setting.

use cf_engine::testing::{Context, Made};
use cf_ledger::NewTask;
use serde_json::{json, Value};

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

/// The human ticks "Advisor tool" for the project, or unticks it.
fn tick(context: &Context, project: i64, on: bool) {
    context
        .ledger
        .borrow_mut()
        .set_advisor_tool(project, on)
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
fn every_members_launch_of_a_project_with_the_tool_off_is_told_to_switch_it_off_and_the_chiefs_never_is(
) {
    for harness in HARNESSES {
        let context = Context::made(Made {
            harnesses: HARNESSES.to_vec(),
            ..Made::default()
        });
        let project = staffed_on(&context, harness);
        assert_eq!(
            launches_of_role(&context, "chief")[0].get("advisorOff"),
            None,
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
                launches[0]["advisorOff"],
                json!(true),
                "{harness} {role} at its launch"
            );
        }
        // The chief's window, opened again, is not told it either.
        relaunch(&context, project);
        let chief = launches_of_role(&context, "chief");
        assert_eq!(chief.len(), 2, "{harness}");
        assert!(
            chief[1]["resume"].is_string(),
            "{harness}: the chief is resumed"
        );
        assert_eq!(
            chief[1].get("advisorOff"),
            None,
            "{harness}: the chief on resume"
        );
        context.close();
    }
}

#[test]
fn a_project_that_ticked_the_tool_launches_every_member_as_it_always_did_and_a_change_reaches_the_next_launch(
) {
    let context = Context::new();
    let project = staffed_on(&context, "claude-code");
    tick(&context, project, true);
    for (role, _) in MEMBERS {
        context.pool_task(project, role, &format!("A task for the {role}"));
    }
    context.pass().unwrap();
    context.pass().unwrap();
    for (role, _) in MEMBERS {
        assert_eq!(
            launches_of_role(&context, role)[0].get("advisorOff"),
            None,
            "{role}: the owner's own setting"
        );
    }
    // Unticked, the window that began without keeps what it began with: no launch is
    // asked for again by the change alone.
    tick(&context, project, false);
    context.pass().unwrap();
    for (role, _) in MEMBERS {
        assert_eq!(
            launches_of_role(&context, role).len(),
            1,
            "{role}: no new window"
        );
    }
    context.close();
}

#[test]
fn a_member_opened_again_on_its_conversation_is_told_as_the_project_says_it_by_then() {
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
        launches_of(&context, &session)[0]["advisorOff"],
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
            launch.get("advisorOff"),
            (!ticked).then(|| json!(true)).as_ref(),
            "T-{follow_up}: the word of the project now"
        );
    }
    context.close();
}
