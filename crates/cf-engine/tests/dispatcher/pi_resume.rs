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
