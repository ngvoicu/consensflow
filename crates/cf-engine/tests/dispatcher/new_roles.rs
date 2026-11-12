//! The four roles of spec new-roles (FD-04): the frontend designer, the architect, the
//! researcher and the integrator. A task for each, with no tier, goes to the member who holds
//! the role, in a window opened with that role's text, and to no worker beside it; its
//! result goes to the chief. A task of one whose role nobody holds any more waits, and says
//! so by the role's name.

use cf_engine::testing::Context;
use cf_ledger::{NewTask, ProjectView};
use serde_json::json;

/// The roles, their members' agents and the names the notes say.
const ROLES: [(&str, &str, &str); 4] = [
    ("frontend", "iris", "frontend designer"),
    ("architect", "daedalus", "architect"),
    ("researcher", "argus", "researcher"),
    ("integrator", "janus", "integrator"),
];

/// A project at /work/app: apollo its chief, zeus a light worker, and a member of each of
/// `roles`, given no tier.
fn project_of(context: &Context, roles: &[(&str, &str, &str)]) -> ProjectView {
    let mut staff = vec![
        json!({ "agent": "zeus", "harness": "claude-code", "roles": ["worker"], "tier": "light" }),
    ];
    staff.extend(roles.iter().map(
        |(role, agent, _)| json!({ "agent": agent, "harness": "claude-code", "roles": [role] }),
    ));
    context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "model": "apollo" },
            "staff": staff,
        }))
        .unwrap()
        .expect("the project")
}

fn open(context: &Context, project: &ProjectView, pool: &str, body: &str) -> i64 {
    context
        .create_task(
            project.id,
            NewTask {
                from: "chief".to_owned(),
                pool: Some(pool.to_owned()),
                body: body.to_owned(),
                ..NewTask::default()
            },
        )
        .task
        .number
}

#[test]
fn a_task_of_each_new_role_goes_to_its_member_in_a_window_of_its_role_and_its_result_to_the_chief()
{
    let context = Context::new();
    let project = project_of(&context, &ROLES);
    for (role, agent, _) in ROLES {
        let number = open(&context, &project, role, &format!("Work for the {role}"));
        context.pass().unwrap();
        let task = context.task(project.id, number).task;
        assert_eq!(
            (task.pool.as_deref(), task.tier.as_deref()),
            (Some(role), None),
            "{role}"
        );
        assert!(
            task.assignee
                .as_deref()
                .is_some_and(|assignee| assignee.starts_with(&format!("{agent}-"))),
            "{role}: taken by {:?}, not {agent}",
            task.assignee
        );
        let launch = context.adapter.prepared().last().cloned().unwrap();
        assert_eq!(launch["role"], role);
        assert_eq!(launch["instructions"], format!("instructions for {role}"));
        context.pass().unwrap();
        context.adapter.answer(agent, &format!("{role} done"));
        context.pass().unwrap();
        let thread = context.task(project.id, number);
        assert_eq!(thread.task.state, "done", "{role}");
        let result = thread.messages.last().unwrap();
        assert_eq!(
            (
                result.kind.as_str(),
                result.recipient.as_str(),
                result.body.as_str()
            ),
            ("result", "chief", format!("{role} done").as_str())
        );
    }
    // The worker took none of them: no window of a worker was opened.
    let zeus_windows = context
        .adapter
        .prepared()
        .iter()
        .filter(|launch| launch["role"] == "worker")
        .count();
    assert_eq!(zeus_windows, 0);
}

#[test]
fn a_task_of_a_new_role_nobody_holds_any_more_waits_and_says_it_by_the_roles_name() {
    for (role, agent, named) in ROLES {
        let context = Context::new();
        let project = project_of(&context, &ROLES);
        let number = open(&context, &project, role, "Do it");
        context
            .ledger
            .borrow_mut()
            .remove_member(project.id, agent)
            .unwrap();
        for _ in 0..3 {
            context.pass().unwrap();
        }
        let task = context.task(project.id, number).task;
        assert_eq!(
            (task.state.as_str(), task.assignee),
            ("open", None),
            "{role}"
        );
        let chief = context.id(project.id, "chief");
        let waits: Vec<String> = context
            .inbox(chief)
            .into_iter()
            .filter(|message| message.kind == "note" && message.body.contains(" waits for "))
            .map(|message| message.body)
            .collect();
        assert_eq!(
            waits,
            [format!(
                "T-{number} waits for a free {named}: no {named} is on the staff (ask the human for one, in your terminal)."
            )],
            "{role}"
        );
    }
}
