//! The human's Resume of a project whose chief and member run on one harness brings
//! both windows back, whichever harness it is (T-1707: on Pi none came back).

use cf_engine::testing::{Context, Made};
use cf_harness::contract::FirstMessage;
use serde_json::json;

use crate::resume::{opened, quit_and_start};

const HARNESSES: [&str; 5] = ["claude-code", "codex", "pi", "devin", "muse"];

fn resumed_panes(harness: &str) -> (Vec<String>, i64, String) {
    let context = Context::made(Made {
        harnesses: HARNESSES.to_vec(),
        ..Made::default()
    });
    if harness == "pi" {
        context.adapter.first.set(FirstMessage::Delivered);
    }
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": harness, "model": "apollo" },
            "staff": [{ "agent": "zeus", "harness": harness, "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project");
    context.pool_task(project.id, "worker", "Write the lexer");
    for _ in 0..8 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 1).task.state, "working");
    let after = quit_and_start(&context);
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(
        opened(&context).len(),
        2,
        "only the two windows before the quit"
    );
    let before = opened(&context);
    let resumed = after.resume_project(project.id);
    for _ in 0..5 {
        after.pass().unwrap();
    }
    assert!(resumed.is_ok(), "{resumed:?}");
    let now = opened(&context);
    (now[before.len()..].to_vec(), project.id, harness.to_owned())
}

#[test]
fn a_resume_brings_back_the_chief_and_the_member_on_every_harness() {
    for harness in HARNESSES {
        let (panes, id, _) = resumed_panes(harness);
        assert!(
            panes.contains(&format!("p{id}-chief"))
                && panes.iter().any(|pane| pane.contains("zeus")),
            "{harness}: opened {panes:?}"
        );
    }
}

fn at_work(harness: &str) -> (Context, i64) {
    let context = Context::made(Made {
        harnesses: HARNESSES.to_vec(),
        ..Made::default()
    });
    if harness == "pi" {
        context.adapter.first.set(FirstMessage::Delivered);
    }
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": harness, "model": "apollo" },
            "staff": [{ "agent": "zeus", "harness": harness, "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project");
    context.pool_task(project.id, "worker", "Write the lexer");
    for _ in 0..8 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 1).task.state, "working");
    (context, project.id)
}

/// T-1707: a Resume that brought nothing back left no line in the daemon's log, so nobody could
/// tell whether it asked for the windows or never ran.
#[test]
fn a_resume_says_in_the_log_for_which_participants_it_asks_a_window() {
    for harness in HARNESSES {
        let (context, project) = at_work(harness);
        let after = quit_and_start(&context);
        let asked = |context: &Context| {
            context
                .log
                .infos
                .borrow()
                .iter()
                .filter(|line| line.starts_with("the Resume asks a window for participant"))
                .count()
        };
        assert_eq!(asked(&context), 0, "{harness}: nothing before the Resume");
        after.resume_project(project).unwrap();
        for _ in 0..5 {
            after.pass().unwrap();
        }
        assert_eq!(asked(&context), 2, "{harness}: the chief and the member");
    }
}

/// T-1707: a window the Resume does not open says why. Here a stop was asked of the member
/// while its record was read.
#[test]
fn a_resume_that_opens_no_window_says_why_in_the_log() {
    for harness in HARNESSES {
        let (context, project) = at_work(harness);
        let after = quit_and_start(&context);
        let reading = context.adapter.cut_holds.hold(|_| true);
        let resuming = after.begin_resume_project(project);
        context.settle();
        context
            .ledger
            .borrow_mut()
            .pause_task(project, 1, Some("chief"), None)
            .unwrap();
        reading.open();
        context.settle();
        resuming.take().unwrap().unwrap();
        for _ in 0..5 {
            after.pass().unwrap();
        }
        let infos = context.log.infos.borrow().clone();
        assert!(
            infos.iter().any(
                |line| line.starts_with("the Resume opened no window for @zeus")
                    && line.ends_with(": a stop was asked of it meanwhile")
            ),
            "{harness}: {infos:?}"
        );
        assert!(
            context.log.warnings.borrow().is_empty(),
            "{harness}: a plain stop is no warning"
        );
    }
}
