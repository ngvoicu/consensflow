//! A harness ConsensFlow has no adapter for (Pi stands for one in these
//! tests, on an engine whose fake is made without it): no work for a member on
//! it, no window opened, no project, member or chief on it. A task at work on a
//! member whose harness word is one the build does not run (as an older build
//! left it, `kimi` here) is paused with the way out: no resume can open its window.

use cf_engine::testing::{Context, Made};
use cf_engine::{SwitchTo, SwitchWhen};
use cf_ledger::{NewMember, NewTask, ProjectView};
use serde_json::json;

use crate::fixtures::{assert_match, notes};
use crate::traces::held_to;

const SUITES: &[&str] = &["a harness ConsensFlow has no adapter for"];

/// An engine whose fake answers for Claude Code alone, so Pi has no adapter.
fn without_pi() -> Context {
    Context::made(Made {
        harnesses: vec!["claude-code"],
        codex: false,
    })
}

/// A worker of the standard tier on Pi joins the staff (`onPi`).
fn on_pi(context: &Context, project: &ProjectView) {
    context
        .ledger
        .borrow_mut()
        .add_member(
            project.id,
            &NewMember {
                agent: "hera".to_owned(),
                harness: "pi".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
}

#[test]
fn gives_a_member_on_it_no_work_says_why_and_every_pass_goes_on() {
    let context = without_pi();
    let project = context.with_staff(&[]);
    on_pi(&context, &project);
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Write the parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let task = context.task(project.id, 1).task;
    assert_eq!((task.state.as_str(), task.assignee), ("open", None));
    assert_eq!(
        notes(&context, context.id(project.id, "chief")),
        ["T-1 waits for a free standard worker: @hera runs on pi, whose windows ConsensFlow cannot open (remove @hera from the staff and add a member on another harness)."]
    );
    assert_eq!(context.host.opened().len(), 1, "only the chief window");
    held_to(
        context.close(),
        SUITES,
        "gives a member on it no work, says why, and every pass goes on",
    );
}

#[test]
fn fails_what_was_given_to_a_member_on_it_by_name_and_its_requester_hears_why() {
    let context = without_pi();
    let project = context.with_staff(&[]);
    on_pi(&context, &project);
    context.give(project.id, "hera", "Parser");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "failed");
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_match(
        &note.body,
        r"^T-1 failed: the launch failed: ConsensFlow cannot open pi windows\.",
    );
    held_to(
        context.close(),
        SUITES,
        "fails what was given to a member on it by name, and its requester hears why",
    );
}

#[test]
fn opens_no_project_member_or_chief_on_it() {
    let context = without_pi();
    let refused = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "pi", "agent": "leto" },
        }))
        .unwrap_err()
        .to_string();
    assert_match(&refused, "ConsensFlow cannot open pi windows");
    let refused = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [{ "agent": "hera", "harness": "pi", "role": "worker", "tier": "standard" }],
        }))
        .unwrap_err()
        .to_string();
    assert_match(&refused, "ConsensFlow cannot open pi windows");
    assert!(context.ledger.borrow().projects().unwrap().is_empty());
    let project = context.with_staff(&["zeus"]);
    let refused = context
        .switch_chief(
            project.id,
            SwitchTo {
                harness: "pi".to_owned(),
                agent: "leto".to_owned(),
            },
            SwitchWhen::Now,
            false,
        )
        .unwrap_err()
        .to_string();
    assert_match(&refused, "ConsensFlow cannot open pi windows");
    held_to(
        context.close(),
        SUITES,
        "opens no project, member or chief on it",
    );
}

/// The way out a pause note gives for T-`number`, whose member's harness has no adapter.
fn way_out(number: i64) -> String {
    format!(
        "Its member runs on a harness whose windows ConsensFlow cannot open: cancel it with cf task cancel T-{number} and add it again with cf task add once a member on another harness is on the staff, or finish it with cf task finish T-{number}"
    )
}

/// `handle`'s rows, the member's and its sessions', on `harness` from now on, as an
/// older build that ran it left them.
fn now_on(context: &Context, handle: &str, harness: &str) {
    context.edit_ledger(|file| {
        let db = rusqlite::Connection::open(file).unwrap();
        let moved = db
            .execute(
                "UPDATE participant SET harness = ?1 WHERE handle = ?2 \
                 OR member_id = (SELECT id FROM participant WHERE handle = ?2)",
                [harness, handle],
            )
            .unwrap();
        assert!(moved >= 1, "@{handle} has a row");
    });
}

#[test]
fn a_task_whose_window_closes_on_a_harness_with_no_adapter_is_paused_with_the_way_out_not_a_resume()
{
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    now_on(&context, "zeus", "kimi");

    context.host.exits_showing(Some(3), None, None);
    context.exit("zeus");
    context.pass().unwrap();

    assert_eq!(context.task(project.id, 1).task.state, "paused");
    let paused: Vec<String> = notes(&context, context.id(project.id, "chief"))
        .into_iter()
        .filter(|said| said.starts_with("T-1 is paused: "))
        .collect();
    assert_eq!(
        paused,
        [format!(
            "T-1 is paused: @zeus's window closed (exit code 3). {}.",
            way_out(1)
        )]
    );
}

#[test]
fn tasks_paused_together_on_a_harness_with_and_one_with_no_adapter_each_say_their_own_way_out() {
    let context = Context::new();
    let project = context.with_staff(&["zeus", "diana"]);
    context.give(project.id, "zeus", "Parser");
    context.give(project.id, "diana", "Lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    assert_eq!(context.task(project.id, 2).task.state, "working");
    now_on(&context, "zeus", "kimi");

    context.close_project(project.id).unwrap();

    let paused: Vec<String> = notes(&context, context.id(project.id, "chief"))
        .into_iter()
        .filter(|said| said.starts_with("T-1 is paused: "))
        .collect();
    assert_eq!(paused.len(), 1, "{paused:?}");
    let lines: Vec<&str> = paused[0].lines().collect();
    assert_eq!(lines.len(), 3, "{paused:?}");
    assert!(
        lines[0].ends_with(&format!(". {}", way_out(1))),
        "{paused:?}"
    );
    assert!(
        lines[1].ends_with(". Resume it with: cf task resume T-2 \"…\""),
        "{paused:?}"
    );
    assert_eq!(lines[2], "Each window comes back on its own conversation.");
}
