//! A paused task's window that stops at rest stays open, and what its turn left
//! running runs on: the work in its tree, and what may be the window's outside it, a
//! loop a tool shell detached (T-160's zeewin retry loop, paused for disk on 2026-10-22,
//! ran on 37 minutes with nobody told). ConsensFlow names it and ends none of it (the
//! chief's decision of 2026-10-24, a detached job is reported, never ended; T-487).

use cf_engine::testing::Context;
use cf_harness::records::Pending;
use cf_proto::panes::{Background, BackgroundProcess, PossiblyLeft};

use crate::fixtures::notes;

/// Zeus at work on T-1, paused, its turn then over: the pause is paid at rest.
fn paused_at_rest(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    context.adapter.answer("zeus", "Stopped where I was");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    project.id
}

/// The notes the chief was sent that say what T-1's window left running while it is paused.
fn told_left(context: &Context, project: i64) -> Vec<String> {
    notes(context, context.id(project, "chief"))
        .into_iter()
        .filter(|note| note.starts_with("T-1's window stays open"))
        .collect()
}

#[test]
fn a_pause_whose_window_stops_names_what_its_turn_left_running_once_and_ends_none_of_it() {
    let context = Context::new();
    context.background.leaves(Some(Background {
        total: 1,
        processes: vec![BackgroundProcess {
            pid: 4711,
            command: "bash retry-zeewin.sh".to_owned(),
        }],
        possibly_left: vec![PossiblyLeft {
            pid: 4242,
            program: "sleep".to_owned(),
        }],
    }));
    let project = paused_at_rest(&context);
    assert_eq!(
        told_left(&context, project),
        ["T-1's window stays open while T-1 is paused, with background work its turn left running, which the pause does not end: bash retry-zeewin.sh; pid 4242 (sleep) may be this window's and was not ended"]
    );
    // Never signalled: the window stays, and nothing is asked to end.
    assert!(context.host.killed().is_empty());
    let zeus = context.id(project, "zeus");
    assert!(context.dispatcher.pane(zeus).is_some());
    // Told once: the pause is paid, and the passes after it ask nothing more.
    context.advance(10_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.background.left_asked().len(), 1);
    assert_eq!(told_left(&context, project).len(), 1);
    context.close();
}

/// A detached job alone, with nothing of the turn's left in the window's tree, is named too.
#[test]
fn a_pause_names_a_job_that_may_be_its_windows_though_its_tree_runs_nothing_of_the_turn() {
    let context = Context::new();
    context.background.leaves(Some(Background {
        possibly_left: vec![PossiblyLeft {
            pid: 4242,
            program: "bash".to_owned(),
        }],
        ..Background::default()
    }));
    let project = paused_at_rest(&context);
    assert_eq!(
        told_left(&context, project),
        ["T-1's window stays open while T-1 is paused; pid 4242 (bash) may be this window's and was not ended"]
    );
    context.close();
}

#[test]
fn a_pause_whose_window_left_nothing_running_says_nothing_of_it() {
    let context = Context::new();
    context.background.leaves(Some(Background::default()));
    let project = paused_at_rest(&context);
    assert_eq!(context.background.left_asked().len(), 1);
    assert!(told_left(&context, project).is_empty());
    context.close();
}

/// A pause resumed before its window came to rest: the window goes on with the task, and
/// nothing says it is paused.
#[test]
fn a_pause_resumed_before_its_window_stopped_says_nothing_of_what_it_left() {
    let context = Context::new();
    context.background.leaves(Some(Background {
        total: 1,
        processes: vec![BackgroundProcess {
            pid: 4711,
            command: "bash retry-zeewin.sh".to_owned(),
        }],
        ..Background::default()
    }));
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    context.resume_task(project.id, 1, "Carry on");
    context.adapter.answer("zeus", "Stopped where I was");
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_ne!(context.task(project.id, 1).task.state, "paused");
    assert!(told_left(&context, project.id).is_empty());
    context.close();
}

/// T-160's shape: the window's turn is over already, held for the job it left (turn-end), when
/// the pause comes; no key is pressed into a window at rest, and the pause is paid at the next
/// look, which names what it left.
#[test]
fn a_pause_of_a_task_held_for_its_job_names_the_job_and_the_loop_it_left() {
    let context = Context::new();
    context.background.leaves(Some(Background {
        total: 1,
        processes: vec![BackgroundProcess {
            pid: 4711,
            command: "bash retry-zeewin.sh".to_owned(),
        }],
        possibly_left: vec![PossiblyLeft {
            pid: 4242,
            program: "bash".to_owned(),
        }],
    }));
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer_leaving(
        "zeus",
        "The zeewin run retries until it gets the queue",
        Pending {
            agents: 0,
            jobs: 1,
            retired: 0,
            noticed: 0,
        },
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working", "held");
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        context.host.inputs().is_empty(),
        "no key into a window at rest"
    );
    assert!(context.host.killed().is_empty());
    assert_eq!(
        told_left(&context, project.id),
        ["T-1's window stays open while T-1 is paused, with background work its turn left running, which the pause does not end: bash retry-zeewin.sh; pid 4242 (bash) may be this window's and was not ended"]
    );
    context.close();
}
