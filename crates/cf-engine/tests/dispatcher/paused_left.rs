//! A paused task's window that stops at rest stays open, and what its turn left
//! running runs on: the work in its tree, and what may be the window's outside it, a
//! loop a tool shell detached (T-160's zeewin retry loop, paused for disk on 2026-10-22,
//! ran on 37 minutes with nobody told). ConsensFlow names it and ends none of it (the
//! chief's decision of 2026-10-24, a detached job is reported, never ended; T-487).

use cf_engine::testing::Context;
use cf_engine::Unstopped;
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
        ["T-1's window stays open while T-1 is paused, with background work its turn left running, which the pause does not end: process 4711; pid 4242 (sleep) may be this window's and was not ended"]
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

/// The note that a paused task's window stays open asks the chief nothing: it is shown on the
/// task's line, never typed into the chief's window (RS-25), and the chief's inbox lists it.
#[test]
fn the_note_that_a_paused_window_stays_open_is_shown_on_its_line_and_never_typed() {
    let context = Context::new();
    context.background.leaves(Some(Background {
        total: 1,
        processes: vec![BackgroundProcess {
            pid: 4711,
            command: "bash retry-zeewin.sh".to_owned(),
        }],
        possibly_left: Vec::new(),
    }));
    let project = paused_at_rest(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let note = context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .find(|message| message.body.starts_with("T-1's window stays open"))
        .expect("the chief's inbox lists it");
    assert_eq!(note.state, "read", "shown, not queued");
    assert!(
        crate::fixtures::given(&context, "chief")
            .iter()
            .all(|text| !text.contains("window stays open")),
        "never typed"
    );
    assert_eq!(
        context.task(project, 1).task.aside.as_deref(),
        Some("window open, with background work its turn left running, which the pause does not end: process 4711")
    );
    context.close();
}

/// What a pause's note says of the work its turn left running is the program the list names and
/// nothing after it (T-1178, alpha.3's known issue): no argument, no path, no token.
#[test]
fn a_pause_names_the_programs_of_what_its_turn_left_running_and_never_an_argument() {
    let context = Context::new();
    context.background.leaves(Some(Background {
        total: 3,
        processes: vec![
            BackgroundProcess {
                pid: 4711,
                command: "python3 lock.py record --run 7 --token redgreenblue".to_owned(),
            },
            BackgroundProcess {
                pid: 4712,
                command: "curl -u alice:swordfish https://example.test/upload".to_owned(),
            },
            BackgroundProcess {
                pid: 4713,
                command: "/Users/me/secret-project/bin/node server.mjs --password hunter22"
                    .to_owned(),
            },
        ],
        possibly_left: Vec::new(),
    }));
    let project = paused_at_rest(&context);
    assert_eq!(
        told_left(&context, project),
        ["T-1's window stays open while T-1 is paused, with background work its turn left running, which the pause does not end: python3 …; process 4712; node …"]
    );
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
        ["T-1's window stays open while T-1 is paused; pid 4242 may be this window's and was not ended"]
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
        ["T-1's window stays open while T-1 is paused, with background work its turn left running, which the pause does not end: process 4711; pid 4242 may be this window's and was not ended"]
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

/// Zeus at work on T-1 while `left` runs (what the host lists of its turn; none where it gives
/// no answer) and its record says `pending` runs, paused, and ignoring every round of the
/// interrupt (its turn is on a tool call that holds it): the three rounds, and the look after.
fn ignored_while(context: &Context, left: Option<Background>, pending: Pending) -> i64 {
    context.background.leaves(left);
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context
        .adapter
        .with("zeus", |agent| agent.pending = pending);
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

/// [`ignored_while`] its tests run in WSL.
fn ignored_while_its_work_runs(context: &Context) -> i64 {
    ignored_while(context, Some(running_its_tests()), Pending::default())
}

/// The notes of `who` that say T-1's window did not stop.
fn did_not_stop(context: &Context, project: i64, who: &str) -> Vec<String> {
    notes(context, context.id(project, who))
        .into_iter()
        .filter(|note| note.contains("did not stop"))
        .collect()
}

/// What the chief is told of a window kept open for `runs`.
fn kept_for(runs: &str) -> String {
    format!(
        "T-1's window (@zeus) did not stop: its turn went on through the interrupt, and it runs work that closing it would end: {runs}. ConsensFlow keeps the window open for it and T-1 paused, ends none of it, and interrupts the turn again once that work has ended; until then what it writes is not taken as T-1's result. `cf task resume T-1 \"…\"` gives the window your words once its turn ends; only the human can end the work sooner, by deleting its session."
    )
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
    assert_eq!(
        context.dispatcher.unstopped(zeus),
        Some(Unstopped { task: 1, rounds: 3 }),
        "the board shows the stop it owes"
    );
    // The chief is told, once, what runs and how it ends; the human too, on the board.
    assert_eq!(
        did_not_stop(&context, project, "chief"),
        [kept_for("process 4711")]
    );
    assert_eq!(
        did_not_stop(&context, project, "human"),
        ["@zeus did not stop for T-1: its turn went on through the interrupt 3 times, and its window runs work that closing it would end: process 4711. ConsensFlow keeps the window open for that work and T-1 paused, and ends none of it; once the work has ended, it interrupts the turn again. To end the work now, delete @zeus's session."]
    );
    // Still so ten minutes on, with no key: nothing closes it, and nothing is said again.
    for _ in 0..20 {
        context.advance(30_000);
        context.pass().unwrap();
    }
    assert!(context.host.killed().is_empty());
    assert_eq!(context.host.inputs().len(), 3, "no key while its work runs");
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(did_not_stop(&context, project, "chief").len(), 1);
    assert_eq!(did_not_stop(&context, project, "human").len(), 1);
    context.close();
}

#[test]
fn a_window_kept_open_for_its_work_pays_the_pause_once_its_turn_ends_and_stays_open() {
    let context = Context::new();
    let project = ignored_while_its_work_runs(&context);
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
    assert_eq!(
        context.host.inputs().len(),
        3,
        "no key into a window at rest"
    );
    context.close();
}

#[test]
fn once_the_work_a_window_was_kept_for_ends_fresh_rounds_go_in_and_ignored_with_nothing_running_it_is_kept(
) {
    let context = Context::new();
    let project = ignored_while_its_work_runs(&context);
    let zeus = context.id(project, "zeus");
    // The tests end and the turn is still at work: at the next look, three fresh rounds.
    context.background.leaves(Some(Background::default()));
    context.advance(10_000);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        4,
        "the first of the fresh rounds"
    );
    for _ in 0..2 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(context.host.inputs().len(), 6);
    // Ignored too, with nothing of its own running: a hung turn, kept, never closed.
    // Nothing more is said: the window that did not stop was said once.
    context.advance(3_100);
    context.pass().unwrap();
    assert!(context.host.killed().is_empty(), "a pause closes nothing");
    assert!(context.dispatcher.unstopped(zeus).is_some());
    assert_eq!(context.task(project, 1).task.state, "paused");
    let told = did_not_stop(&context, project, "chief");
    assert_eq!(told.len(), 1, "{told:?}");
    // A minute on, asked again: three fresh rounds, and still nothing said.
    context.advance(60_000);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 7);
    assert!(context.host.killed().is_empty());
    assert_eq!(did_not_stop(&context, project, "chief").len(), 1);
    context.close();
}

#[test]
fn a_window_the_host_cannot_say_the_work_of_is_kept_open_as_one_that_runs_some() {
    let context = Context::new();
    let project = ignored_while(&context, None, Pending::default());
    assert!(context.host.killed().is_empty());
    assert_eq!(
        did_not_stop(&context, project, "chief"),
        [kept_for("what the host could not list (it gave no answer)")]
    );
    context.close();
}

#[test]
fn a_window_whose_harness_says_its_jobs_and_agents_run_is_kept_open_for_them_with_an_empty_tree() {
    // Claude's background agents are no process of the window's tree; its record counts them.
    let context = Context::new();
    let pending = Pending {
        jobs: 1,
        agents: 2,
        ..Pending::default()
    };
    let project = ignored_while(&context, Some(Background::default()), pending);
    assert!(context.host.killed().is_empty());
    assert_eq!(
        did_not_stop(&context, project, "chief"),
        [kept_for(
            "1 job its harness started in the background; 2 agents of its harness's own"
        )]
    );
    context.close();
}

#[test]
fn a_window_whose_harness_keeps_commands_running_that_no_tree_shows_is_kept_open_for_them() {
    // Codex 0.166 runs a command with no shell, a program of its own child, and keeps one a
    // pause interrupted as a background terminal (T-1137): the host lists neither as the
    // turn's work, and only Codex's own word names them.
    let context = Context::new();
    context.background.leaves(Some(Background::default()));
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.adapter.with("zeus", |agent| {
        agent.kept_commands = Some(vec!["node ./ignore-signals-cf-stops-live.cjs".to_owned()]);
    });
    context.pause_task(project.id, 1);
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert!(
        context.host.killed().is_empty(),
        "a pause ends none of its work"
    );
    assert_eq!(
        did_not_stop(&context, project.id, "chief"),
        [kept_for("the commands its harness keeps running: node …")]
    );
    context.close();
}

#[test]
fn a_window_whose_harness_cannot_say_what_it_keeps_running_is_kept_open() {
    let context = Context::new();
    context.background.leaves(Some(Background::default()));
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context
        .adapter
        .with("zeus", |agent| agent.kept_commands = None);
    context.pause_task(project.id, 1);
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert!(context.host.killed().is_empty());
    assert_eq!(
        did_not_stop(&context, project.id, "chief"),
        [kept_for(
            "what its harness could not list (it gave no answer)"
        )]
    );
    context.close();
}
