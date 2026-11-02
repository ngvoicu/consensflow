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

/// What T-1's window runs when the pause comes: the field's worker's database tests,
/// started in WSL from its Git Bash (T-1117).
fn running_its_tests() -> Background {
    Background {
        total: 1,
        processes: vec![BackgroundProcess {
            pid: 4711,
            command: "wsl.exe -d Ubuntu -- bash cf-t3-check.sh".to_owned(),
        }],
        possibly_left: Vec::new(),
    }
}

/// Zeus at work on T-1 while its tests run, paused, and ignoring every round of the
/// interrupt (its turn is on a tool call that holds it): the three rounds, and the look after.
fn ignored_while_its_work_runs(context: &Context) -> i64 {
    context.background.leaves(Some(running_its_tests()));
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.pause_task(project.id, 1);
    for _ in 0..3 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        3,
        "three rounds of the interrupt"
    );
    project.id
}

#[test]
fn a_window_that_ignores_every_round_while_its_work_runs_stays_open_and_its_work_runs_on() {
    // T-1117 (m-374, m-375): "T-3's window did not stop: it ignored the interrupt, so
    // ConsensFlow closed it", and its close ended the worker's database tests in WSL. A
    // pause never ends background work: the window stays open and the task paused.
    let context = Context::new();
    let project = ignored_while_its_work_runs(&context);
    assert!(
        context.host.killed().is_empty(),
        "a pause ends none of its work"
    );
    let zeus = context.id(project, "zeus");
    assert!(context.dispatcher.pane(zeus).is_some());
    assert_eq!(context.task(project, 1).task.state, "paused");
    // The chief is told, once, that it did not stop, and what runs that nothing ended.
    let told: Vec<String> = notes(&context, context.id(project, "chief"))
        .into_iter()
        .filter(|note| note.contains("did not stop"))
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains("wsl.exe -d Ubuntu -- bash cf-t3-check.sh"),
        "{told:?}"
    );
    assert!(!told[0].contains("closed"), "{told:?}");
    // Still so ten minutes on: nothing closes it, and nothing is said again.
    for _ in 0..20 {
        context.advance(30_000);
        context.pass().unwrap();
    }
    assert!(context.host.killed().is_empty());
    assert_eq!(context.task(project, 1).task.state, "paused");
    context.close();
}

#[test]
fn a_window_kept_open_for_its_work_pays_the_pause_once_its_turn_ends_and_stays_open() {
    let context = Context::new();
    let project = ignored_while_its_work_runs(&context);
    let pressed = context.host.inputs().len();
    // Its tool call returns and the turn ends: the pause is paid at rest, with no key.
    context
        .adapter
        .answer("zeus", "The tests are still running in WSL");
    context.pass().unwrap();
    context.pass().unwrap();
    context.advance(10_000);
    context.pass().unwrap();
    assert!(context.host.killed().is_empty());
    assert_eq!(context.task(project, 1).task.state, "paused");
    let zeus = context.id(project, "zeus");
    assert_eq!(
        context.dispatcher.unstopped(zeus),
        None,
        "paid: the board shows no stop owed"
    );
    assert!(
        context.host.inputs().len() <= pressed + 1,
        "no key into a window at rest"
    );
    context.close();
}
