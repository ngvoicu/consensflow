//! The size a project chose for its members' windows to compact at (spec
//! smaller-contexts, SCX-25, widened by SCX-30): the launch of every member is given
//! it, whatever the member's role and whatever its harness, at its first launch and
//! each time its conversation is opened again, and the chief's never is. Each
//! harness's adapter gives it by its own route (`cf-harness`'s launch tests). A
//! project that chose none launches every window as it always did. A size changed or
//! taken away reaches the member's next launch. The test is not held to a Node
//! recording: it is this build's own setting.

use cf_engine::testing::{Context, Made};
use cf_ledger::NewTask;
use serde_json::{json, Value};

use crate::fixtures::{launches_of, Tiers};

/// The size the human chose.
const SIZE: u32 = 400_000;

/// Every harness a window of a project runs on.
const HARNESSES: [&str; 5] = ["claude-code", "codex", "pi", "devin", "muse"];

/// The members' roles that take a task by tier, with the agent that holds each.
const MEMBERS: [(&str, &str); 4] = [
    ("worker", "zeus"),
    ("advisor", "athena"),
    ("reviewer", "argus"),
    ("tester", "tess"),
];

/// The human chooses the project's size, or takes the choice away.
fn choose(context: &Context, project: i64, size: Option<u32>) {
    context
        .ledger
        .borrow_mut()
        .set_compaction(project, size)
        .unwrap();
}

/// The request for a project on `harness`, its chief and one member of each role in
/// [`MEMBERS`], that says nothing of the compaction.
fn request_on(harness: &str) -> Value {
    let staff: Vec<Value> = MEMBERS
        .iter()
        .map(|(role, agent)| {
            json!({ "agent": agent, "harness": harness, "roles": [role], "tier": "standard" })
        })
        .collect();
    json!({
        "directory": "/work/app",
        "name": "app",
        "chief": { "harness": harness, "model": "apollo" },
        "staff": staff,
    })
}

/// A project on `harness`, its chief and one member of each role in [`MEMBERS`], on each
/// harness's own point as the engine's kit opens every project (T-1400).
fn staffed_on(context: &Context, harness: &str) -> i64 {
    context
        .open_project(request_on(harness))
        .unwrap()
        .expect("the project")
        .id
}

/// The chief's window is opened again: the project is closed and resumed.
fn relaunch(context: &Context, project: i64) {
    context.close_project(project).unwrap();
    context.resume_project(project).unwrap();
}

/// The launches the adapter was asked for the project's chief, oldest first.
fn chief_launches(context: &Context) -> Vec<Value> {
    context
        .adapter
        .prepared()
        .into_iter()
        .filter(|launch| launch["role"] == "chief")
        .collect()
}

/// The launch of the project's member of `role`, which it must have had once.
fn member_launch(context: &Context, role: &str) -> Value {
    let launches: Vec<Value> = context
        .adapter
        .prepared()
        .into_iter()
        .filter(|launch| launch["role"] == role)
        .collect();
    assert_eq!(launches.len(), 1, "{role}");
    launches[0].clone()
}

#[test]
fn every_members_launch_of_a_claude_project_is_given_its_size_and_the_chiefs_never_is() {
    let context = Context::new();
    let project = staffed_on(&context, "claude-code");
    assert_eq!(
        chief_launches(&context)[0].get("compaction"),
        None,
        "a project that chose none launches its chief as it always did"
    );
    choose(&context, project, Some(SIZE));
    for (role, _) in MEMBERS {
        context.pool_task(project, role, &format!("A task for the {role}"));
    }
    context.pass().unwrap();
    context.pass().unwrap();
    for (role, _) in MEMBERS {
        assert_eq!(
            member_launch(&context, role)["compaction"],
            json!(SIZE),
            "{role} at its launch"
        );
    }
    // The chief's window, opened again with the size chosen, is given none.
    relaunch(&context, project);
    let chief = chief_launches(&context);
    assert_eq!(chief.len(), 2);
    assert!(chief[1]["resume"].is_string(), "the chief is resumed");
    assert_eq!(chief[1].get("compaction"), None, "the chief on resume");
    context.close();
}

#[test]
fn a_project_that_chose_no_size_launches_every_member_as_it_always_did() {
    let context = Context::new();
    let project = staffed_on(&context, "claude-code");
    for (role, _) in MEMBERS {
        context.pool_task(project, role, &format!("A task for the {role}"));
    }
    context.pass().unwrap();
    context.pass().unwrap();
    for (role, _) in MEMBERS {
        assert_eq!(
            member_launch(&context, role).get("compaction"),
            None,
            "{role}"
        );
    }
    // A size chosen and taken away is none again.
    choose(&context, project, Some(SIZE));
    choose(&context, project, None);
    relaunch(&context, project);
    assert_eq!(chief_launches(&context)[1].get("compaction"), None);
    context.close();
}

/// A new project whose request says nothing of the compaction starts on a size of 300,000
/// tokens, which every member's launch is given and the chief's never is; one that says `null`
/// launches every window as it always did (T-1400). The request is read as the API's is.
#[test]
fn a_new_project_that_said_nothing_of_its_compaction_launches_every_member_with_300000_and_never_the_chief(
) {
    for harness in HARNESSES {
        let context = Context::made(Made {
            harnesses: HARNESSES.to_vec(),
            ..Made::default()
        });
        let project = context
            .open_project_as_asked(request_on(harness))
            .unwrap()
            .expect("the project")
            .id;
        assert_eq!(context.project(project).compaction, Some(300_000));
        assert_eq!(
            chief_launches(&context)[0].get("compaction"),
            None,
            "{harness}: the chief"
        );
        for (role, _) in MEMBERS {
            context.pool_task(project, role, &format!("A task for the {role}"));
        }
        context.pass().unwrap();
        context.pass().unwrap();
        for (role, _) in MEMBERS {
            assert_eq!(
                member_launch(&context, role)["compaction"],
                json!(300_000),
                "{harness}: {role}"
            );
        }
        context.close();
    }
    // The harness's own point, said: no size, and the launches as they always were.
    let context = Context::new();
    let mut request = request_on("claude-code");
    request["compaction"] = Value::Null;
    let project = context
        .open_project_as_asked(request)
        .unwrap()
        .expect("the project")
        .id;
    assert_eq!(context.project(project).compaction, None);
    for (role, _) in MEMBERS {
        context.pool_task(project, role, &format!("A task for the {role}"));
    }
    context.pass().unwrap();
    context.pass().unwrap();
    for (role, _) in MEMBERS {
        assert_eq!(
            member_launch(&context, role).get("compaction"),
            None,
            "{role}"
        );
    }
    context.close();
}

#[test]
fn every_member_is_given_the_size_whatever_its_harness_and_no_chief_is() {
    for harness in HARNESSES {
        let context = Context::made(Made {
            harnesses: HARNESSES.to_vec(),
            ..Made::default()
        });
        let project = staffed_on(&context, harness);
        choose(&context, project, Some(SIZE));
        for (role, _) in MEMBERS {
            context.pool_task(project, role, &format!("A task for the {role}"));
        }
        context.pass().unwrap();
        context.pass().unwrap();
        for (role, _) in MEMBERS {
            let launch = member_launch(&context, role);
            assert_eq!(launch["compaction"], json!(SIZE), "{harness}: {role}");
        }
        relaunch(&context, project);
        let chief = chief_launches(&context);
        assert_eq!(chief.last().unwrap().get("compaction"), None, "{harness}");
        context.close();
    }
}

#[test]
fn a_member_opened_again_on_its_conversation_is_given_the_size_as_the_project_has_it_by_then() {
    let context = Context::new();
    // The host proves each window's program gone, as the app's does: a follow-up,
    // another task, goes on with the session's conversation (`ownership`).
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    choose(&context, project, Some(SIZE));
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    assert_eq!(
        launches_of(&context, &session)[0]["compaction"],
        json!(SIZE),
        "at its first launch"
    );
    // The size changes, then is taken away, between the follow-ups that open the
    // session's conversation again.
    for (follow_up, now) in [(2, Some(250_000)), (3, None), (4, Some(1_000_000))] {
        choose(&context, project, now);
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
            launch.get("compaction"),
            now.map(|size| json!(size)).as_ref(),
            "T-{follow_up}: the size as the project has it now"
        );
    }
    context.close();
}
