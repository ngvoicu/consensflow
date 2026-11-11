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

/// The size the project's row holds, as an earlier build left it (3.0.7-alpha.4 took any
/// size from 100,000): the ledger itself refuses today a size under its least, so the file
/// is written while no ledger holds it.
fn hold(context: &Context, project: i64, size: u32) {
    context.edit_ledger(|file| {
        let db = rusqlite::Connection::open(file).unwrap();
        db.execute(
            "UPDATE project SET compaction = ?1 WHERE id = ?2",
            rusqlite::params![size, project],
        )
        .unwrap();
    });
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

/// What a stored size is given to a launch as: a size under the least the ledger takes is
/// given as the least, one in the range as it is, and none as none.
const AT_LAUNCH: [(Option<u32>, Option<u32>); 8] = [
    (Some(100_000), Some(250_000)),
    (Some(150_000), Some(250_000)),
    (Some(249_999), Some(250_000)),
    (Some(250_000), Some(250_000)),
    (Some(300_000), Some(300_000)),
    (Some(400_000), Some(400_000)),
    (Some(1_000_000), Some(1_000_000)),
    (None, None),
];

#[test]
fn a_size_the_project_holds_under_the_least_is_given_to_every_harness_as_the_least_and_none_to_the_chief(
) {
    for harness in HARNESSES {
        for (stored, given) in AT_LAUNCH {
            let context = Context::made(Made {
                harnesses: HARNESSES.to_vec(),
                ..Made::default()
            });
            let project = staffed_on(&context, harness);
            if let Some(size) = stored {
                hold(&context, project, size);
            }
            for (role, _) in MEMBERS {
                context.pool_task(project, role, &format!("A task for the {role}"));
            }
            context.pass().unwrap();
            context.pass().unwrap();
            for (role, _) in MEMBERS {
                assert_eq!(
                    member_launch(&context, role).get("compaction"),
                    given.map(|size| json!(size)).as_ref(),
                    "{harness}: {role} of a project holding {stored:?}"
                );
            }
            relaunch(&context, project);
            let chief = chief_launches(&context);
            assert_eq!(
                chief.last().unwrap().get("compaction"),
                None,
                "{harness}: the chief of a project holding {stored:?}"
            );
            context.close();
        }
    }
}

#[test]
fn a_member_opened_again_on_its_conversation_by_a_project_holding_a_size_under_the_least_is_given_the_least_and_the_row_is_left_as_it_was(
) {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    hold(&context, project, 150_000);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    assert_eq!(
        launches_of(&context, &session)[0]["compaction"],
        json!(250_000),
        "at its first launch"
    );
    context.adapter.answer(&session, "Done");
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Follow-up 2".to_owned(),
            ..NewTask::default()
        },
    );
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let launches = launches_of(&context, &session);
    let launch = launches.last().unwrap();
    assert!(launch["resume"].is_string(), "resumed");
    assert_eq!(launch["compaction"], json!(250_000), "on resume");
    // The row is what the human chose and is not rewritten: the page still says so.
    let view = context.ledger.borrow().project(project).unwrap().unwrap();
    assert_eq!(view.compaction, Some(150_000), "the stored size is kept");
    context.close();
}
