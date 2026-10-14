//! The bound on a machine made up, which signals nothing: what is the window's is
//! signalled and what is not is refused, whatever selected it.

use super::*;
use crate::process_tree::testing::{
    a_window_among_strangers, identity_of, listed, the_tests_own_parent, the_windows, Machine,
    BUILD, DEEP, JOB, LOST, PROGRAM, SHELL, USERS_JOB, USERS_SHELL,
};

/// The bound of the tree that starts at the window's program in `processes`.
fn bound_of(processes: &[Process]) -> Bound {
    Bound::new(Some(identity_of(processes, PROGRAM)))
}

fn allows(bound: &Bound, processes: &[Process], target: Identity) -> bool {
    bound.allows(&Table::of(processes), target)
}

fn observe(bound: &Bound, processes: &[Process]) {
    bound.observe(&Table::of(processes));
}

/// Whether `bound` let the signal to `target` through `machine`.
fn signal(
    bound: &Bound,
    machine: &Machine,
    processes: &[Process],
    target: Identity,
    signal: i32,
) -> bool {
    bound
        .signal(machine, &Table::of(processes), target, signal)
        .expect("no error")
}

/// The machine of the window among strangers, and its processes.
fn machine() -> (Machine, Vec<Process>) {
    let machine = Machine::new(a_window_among_strangers());
    let processes = machine.processes().expect("the table");
    (machine, processes)
}

#[test]
fn what_runs_under_the_panes_program_by_the_processes_that_started_it_is_the_windows() {
    let (machine, processes) = machine();
    let bound = bound_of(&processes);

    for pid in the_windows() {
        let target = identity_of(&processes, pid);
        assert!(allows(&bound, &processes, target), "{pid}");
        assert!(
            signal(&bound, &machine, &processes, target, libc::SIGTERM),
            "{pid}"
        );
    }
    assert_eq!(machine.sent(libc::SIGTERM), the_windows());
}

#[test]
fn a_selection_that_names_what_is_outside_the_window_signals_none_of_it() {
    let (machine, processes) = machine();
    let bound = bound_of(&processes);
    observe(&bound, &processes);

    // Every process of the machine, as a selection gone wrong names them: the test's own
    // parent, a child of pid 1 that was never in the window, a user's shell and its job,
    // and a job in the window's session that nothing saw under the window's program.
    let mut refused = Vec::new();
    for process in &processes {
        if !signal(
            &bound,
            &machine,
            &processes,
            process.identity,
            libc::SIGKILL,
        ) {
            refused.push(process.identity.pid);
        }
    }

    assert_eq!(
        refused,
        [the_tests_own_parent(), BUILD, USERS_SHELL, USERS_JOB, LOST]
    );
    assert_eq!(
        machine.sent(libc::SIGKILL),
        the_windows(),
        "only the window's own was sent anything"
    );
}

#[test]
fn a_refusal_sends_nothing_and_says_so() {
    let (machine, processes) = machine();
    let bound = bound_of(&processes);

    for pid in [the_tests_own_parent(), BUILD, LOST] {
        let target = identity_of(&processes, pid);
        assert!(!allows(&bound, &processes, target), "{pid}");
        assert!(!signal(&bound, &machine, &processes, target, libc::SIGKILL));
    }
    assert!(machine.signalled_none());
}

#[test]
fn what_was_seen_under_the_program_stays_the_windows_when_what_started_it_has_gone() {
    let (_, processes) = machine();
    let bound = bound_of(&processes);
    observe(&bound, &processes);

    // The shell has gone and its job runs on, which the system names as started by the
    // shell still; a process the shell started before it went is found by that alone;
    // what a stranger started is none of it.
    let later = vec![
        processes[0].clone(),
        processes[2].clone(),
        processes[3].clone(),
        listed(4_000_030, 1500, 1100),
        listed(4_000_031, 2500, 2000),
        listed(the_tests_own_parent(), 800, 1),
    ];

    for pid in [PROGRAM, JOB, DEEP, 4_000_030] {
        assert!(allows(&bound, &later, identity_of(&later, pid)), "{pid}");
    }
    for pid in [4_000_031, the_tests_own_parent()] {
        assert!(!allows(&bound, &later, identity_of(&later, pid)), "{pid}");
    }
}

#[test]
fn what_nothing_saw_under_the_program_is_not_the_windows_once_its_parent_has_gone() {
    let (_, processes) = machine();
    let bound = bound_of(&processes);

    // No look: the job runs under a shell that has gone, and nothing saw it under the
    // program. It is in the window's session, but that is no proof here.
    let later = vec![processes[0].clone(), processes[2].clone()];
    assert!(!allows(&bound, &later, identity_of(&later, JOB)));
}

#[test]
fn a_process_that_took_the_number_of_one_that_was_seen_is_not_the_windows() {
    let (_, processes) = machine();
    let bound = bound_of(&processes);
    observe(&bound, &processes);

    // The job has gone, and a stranger took its number: another identifier, which nothing
    // saw. (The job that was, if asked of, is the window's by the identity it had; the
    // signal itself goes to none that has taken its number.)
    let later = vec![processes[0].clone(), listed(JOB, 9999, 1)];
    assert!(!allows(&bound, &later, identity_of(&later, JOB)));
}

#[test]
fn the_program_is_the_windows_when_it_has_gone_and_what_it_started_still_is() {
    let (_, processes) = machine();
    let program = identity_of(&processes, PROGRAM);
    let bound = Bound::new(Some(program));

    // The program is not in the table, which names it as the parent of what it started.
    let later = vec![processes[1].clone(), listed(the_tests_own_parent(), 800, 1)];
    assert!(allows(&bound, &later, identity_of(&later, SHELL)));
    assert!(!allows(
        &bound,
        &later,
        identity_of(&later, the_tests_own_parent())
    ));
    // Itself, which the system's own check says is gone or is still it.
    assert!(allows(&bound, &[], program));
}

#[test]
fn with_no_program_nothing_is_the_windows_but_what_was_seen() {
    let (machine, processes) = machine();
    let bound = Bound::new(None);
    for pid in the_windows() {
        assert!(
            !allows(&bound, &processes, identity_of(&processes, pid)),
            "{pid}"
        );
    }
    assert!(!signal(
        &bound,
        &machine,
        &processes,
        identity_of(&processes, SHELL),
        libc::SIGKILL
    ));
    assert!(machine.signalled_none());
}

#[test]
fn a_target_the_table_does_not_hold_is_the_windows_only_if_it_was_seen() {
    let (_, processes) = machine();
    let bound = bound_of(&processes);
    let (job, build) = (identity_of(&processes, JOB), identity_of(&processes, BUILD));
    observe(&bound, &processes);

    let later = vec![processes[0].clone()];
    assert!(allows(&bound, &later, job), "it was seen, and may be gone");
    assert!(!allows(&bound, &later, build), "it never was");
}

#[test]
fn a_look_lets_go_of_what_has_gone_and_keeps_what_runs() {
    let (_, processes) = machine();
    let bound = bound_of(&processes);
    observe(&bound, &processes);
    let held = |bound: &Bound| bound.seen.lock().expect("the memory").len();
    assert_eq!(held(&bound), 4, "the program and what runs under it");

    observe(&bound, &[processes[0].clone(), processes[2].clone()]);
    assert_eq!(held(&bound), 2, "what has gone is let go of");

    observe(&bound, &[]);
    assert_eq!(held(&bound), 0);
}

#[test]
fn a_table_that_is_no_tree_ends_the_walk() {
    // Two processes that name each other as the process that started them, and none of
    // them the program's: the walk ends, and neither is the window's.
    let a = listed(4_000_040, 3000, 3100);
    let b = listed(4_000_041, 3100, 3000);
    let processes = vec![a.clone(), b.clone()];
    let bound = Bound::new(Some(listed(PROGRAM, 1000, 700).identity));
    assert!(!allows(&bound, &processes, a.identity));
    assert!(!allows(&bound, &processes, b.identity));
}

#[test]
fn the_clones_of_a_bound_know_what_one_of_them_saw() {
    let (_, processes) = machine();
    let bound = bound_of(&processes);
    let other = bound.clone();
    observe(&bound, &processes);
    let later = vec![processes[0].clone()];
    assert!(allows(&other, &later, identity_of(&processes, JOB)));
}

#[test]
fn nothing_but_the_bound_sends_a_signal() {
    // The selection reads the whole table and may be wrong: it picks who, and never signals.
    for (file, source) in [
        ("process_tree.rs", include_str!("../../process_tree.rs")),
        ("selection.rs", include_str!("../selection.rs")),
    ] {
        for call in [".send(", "native::signal", "libc::kill"] {
            assert!(
                !source.contains(call),
                "{file} sends a signal itself ({call}): only the bound does"
            );
        }
    }
}
