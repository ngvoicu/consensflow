//! The human's Resume of a project whose member the quit cut at work, whichever harness it
//! runs on (T-1707: on Pi the member came back on "Operation aborted" and was never told to
//! go on).

use cf_engine::testing::{Context, Made};
use cf_harness::contract::FirstMessage;
use serde_json::json;

use crate::resume::{cut_notes, quit_and_start};

const HARNESSES: [&str; 5] = ["claude-code", "codex", "pi", "devin", "muse"];

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

/// T-1707: a member whose record ends in an abort (a quit and an Escape leave the same step)
/// is told to go on at its Resume, on every harness, and a chief with the same record is not.
#[test]
fn a_member_whose_record_ends_in_an_abort_is_told_to_go_on_and_a_chief_is_told_nothing() {
    for harness in HARNESSES {
        let (context, project) = at_work(harness);
        for handle in ["zeus", "chief"] {
            context.adapter.answer(handle, "Working on it");
            context.adapter.ended_in_abort(handle);
        }
        let after = quit_and_start(&context);
        after.resume_project(project).unwrap();
        for _ in 0..5 {
            after.pass().unwrap();
        }
        let zeus = context.id(project, "zeus-amber-pine");
        assert_eq!(cut_notes(&context, zeus).len(), 1, "{harness}");
        let note = &cut_notes(&context, zeus)[0];
        assert_eq!(
            note.body,
            "ConsensFlow stopped while you were working on T-1. Go on where you stopped.",
            "{harness}"
        );
        assert!(
            ["delivering", "delivered", "read"].contains(&note.state.as_str()),
            "{harness}: {}",
            note.state
        );
        let chief = context.id(project, "chief");
        assert!(cut_notes(&context, chief).is_empty(), "{harness}");
    }
}

/// The control: a turn that ended at rest, with no abort, is told nothing.
#[test]
fn a_member_whose_turn_ended_at_rest_is_told_nothing() {
    for harness in HARNESSES {
        let (context, project) = at_work(harness);
        context.adapter.answer("zeus", "Working on it");
        let after = quit_and_start(&context);
        after.resume_project(project).unwrap();
        for _ in 0..5 {
            after.pass().unwrap();
        }
        let zeus = context.id(project, "zeus-amber-pine");
        assert!(cut_notes(&context, zeus).is_empty(), "{harness}");
    }
}
