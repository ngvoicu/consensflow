//! The bound on a machine made up, which signals nothing: what is the window's is
//! signalled and what is not is refused, whatever selected it.

use super::*;
use crate::process_tree::testing::{
    a_window_among_strangers, identity_of, listed, the_tests_own_parent, the_windows, Machine,
    BUILD, DEEP, DETACHED, JOB, LEFT, PROGRAM, SHELL, USERS_JOB, USERS_SHELL,
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

/// The program of the window, a session leader.
fn program() -> Process {
    Process {
        session: Some(PROGRAM),
        ..listed(PROGRAM, 1000, 700)
    }
}

#[test]
fn what_runs_under_the_panes_program_by_the_processes_that_started_it_is_the_windows() {
    let (machine, processes) = machine();
    let bound = bound_of(&processes);

    // The shells of a tool are in sessions of their own: only the parents say they are
    // the window's.
    for pid in [PROGRAM, SHELL, JOB, DEEP] {
        let target = identity_of(&processes, pid);
        assert!(allows(&bound, &processes, target), "{pid}");
        assert!(
            signal(&bound, &machine, &processes, target, libc::SIGTERM),
            "{pid}"
        );
    }
    assert_eq!(machine.sent(libc::SIGTERM), [PROGRAM, SHELL, JOB, DEEP]);
}

#[test]
fn a_selection_that_names_what_is_outside_the_window_signals_none_of_it() {
    let (machine, processes) = machine();
    let bound = bound_of(&processes);
    observe(&bound, &processes);

    // Every process of the machine, as a selection gone wrong names them: the test's own
    // parent, a child of pid 1 that was never in the window, a user's shell and its job,
    // and a job that left the window's session and whose shell is gone.
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
        [the_tests_own_parent(), BUILD, USERS_SHELL, USERS_JOB, LEFT]
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

    for pid in [the_tests_own_parent(), BUILD, LEFT] {
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

    // No look: the job runs under a shell that has gone, in the session of that shell, and
    // nothing saw it under the program.
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
    assert_eq!(
        held(&bound),
        5,
        "the program, what runs under it and what is in its session"
    );

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

// The session the program leads.

#[test]
fn what_is_in_the_session_the_program_leads_is_the_windows_whoever_started_it() {
    let (machine, processes) = machine();
    let bound = bound_of(&processes);

    // A job detached from its shell, whose parent has gone and which no look saw: the
    // program leads its session, and it is in it.
    let detached = identity_of(&processes, DETACHED);
    assert!(allows(&bound, &processes, detached));
    assert!(signal(
        &bound,
        &machine,
        &processes,
        detached,
        libc::SIGTERM
    ));
    // What it started, in a session of its own, is under it.
    let mut with_child = processes.clone();
    with_child.push(Process {
        session: Some(4_000_050),
        ..listed(4_000_050, 1500, 1400)
    });
    assert!(allows(
        &bound,
        &with_child,
        identity_of(&with_child, 4_000_050)
    ));
    // What left the session (`setsid`) and has no parent is not.
    assert!(!allows(&bound, &processes, identity_of(&processes, LEFT)));
    assert!(!allows(&bound, &processes, identity_of(&processes, BUILD)));
}

#[test]
fn a_session_whose_leader_has_gone_allows_nothing() {
    let (_, processes) = machine();
    let bound = bound_of(&processes);
    let detached = identity_of(&processes, DETACHED);

    // The program is not in the table: nothing says whose the session's number is.
    let later: Vec<Process> = processes
        .iter()
        .filter(|process| process.identity.pid != PROGRAM)
        .cloned()
        .collect();
    assert!(!allows(&bound, &later, detached));
    // A look then sees nothing in it either, and a job that begins in it later is no one's.
    observe(&bound, &later);
    let mut newer = later.clone();
    newer.push(Process {
        session: Some(PROGRAM),
        ..listed(4_000_051, 1600, 1)
    });
    assert!(!allows(&bound, &newer, identity_of(&newer, 4_000_051)));
    assert!(!allows(&bound, &newer, detached));
}

#[test]
fn what_a_look_saw_in_the_session_while_its_leader_ran_stays_the_windows_after() {
    let (_, processes) = machine();
    let bound = bound_of(&processes);
    let detached = identity_of(&processes, DETACHED);
    observe(&bound, &processes);

    let later: Vec<Process> = processes
        .iter()
        .filter(|process| process.identity.pid != PROGRAM)
        .cloned()
        .collect();
    assert!(allows(&bound, &later, detached), "seen: it is that process");
    // Not what began in the session's number since: that was never seen.
    let mut newer = later.clone();
    newer.push(Process {
        session: Some(PROGRAM),
        ..listed(4_000_051, 1600, 1)
    });
    assert!(!allows(&bound, &newer, identity_of(&newer, 4_000_051)));
}

#[test]
fn a_session_whose_number_another_process_has_allows_nothing() {
    let (_, processes) = machine();
    let bound = bound_of(&processes);
    let detached = identity_of(&processes, DETACHED);

    // The program has gone and its number is another process's now (another identifier),
    // which leads a session of that number, and so does what it started.
    let mut later: Vec<Process> = processes
        .iter()
        .filter(|process| process.identity.pid != PROGRAM)
        .cloned()
        .collect();
    later.push(Process {
        session: Some(PROGRAM),
        ..listed(PROGRAM, 9000, 1)
    });
    later.push(Process {
        session: Some(PROGRAM),
        ..listed(4_000_052, 9100, 9000)
    });
    assert!(!allows(&bound, &later, detached));
    assert!(!allows(&bound, &later, identity_of(&later, PROGRAM)));
    assert!(!allows(&bound, &later, identity_of(&later, 4_000_052)));
}

#[test]
fn a_program_that_leads_no_session_allows_none_of_the_session_it_is_in() {
    let (_, processes) = machine();
    // The program is in a session some other process leads: a shell of the user's, say, as a
    // program started from a terminal is. Everything of that session is not the window's.
    let mut table = processes.clone();
    table[0].session = Some(4_000_060);
    table.push(Process {
        session: Some(4_000_060),
        ..listed(4_000_061, 9200, 1)
    });
    let bound = bound_of(&table);
    assert!(!allows(&bound, &table, identity_of(&table, 4_000_061)));
    assert!(!allows(&bound, &table, identity_of(&table, DETACHED)));
    // And the program is its own still.
    assert!(allows(&bound, &table, identity_of(&table, PROGRAM)));
}

// The process group a close signals.

/// A process of the group `group`, started by the process whose identifier is `parent`.
fn of_group(group: i32, pid: i32, unique: u64, parent: u64) -> Process {
    Process {
        group,
        ..listed(pid, unique, parent)
    }
}

#[test]
fn a_group_the_program_leads_is_signalled_whole_while_the_program_runs() {
    let machine = Machine::new(Vec::new());
    let table = vec![
        program(),
        of_group(PROGRAM, 4_000_011, 1100, 1000),
        // One in the group whose parent has gone: the group is the program's, whole.
        of_group(PROGRAM, 4_000_012, 1200, 1),
    ];
    let bound = Bound::new(Some(table[0].identity));

    bound
        .signal_group(&machine, &Table::of(&table), PROGRAM, libc::SIGTERM)
        .expect("signalled");

    assert_eq!(machine.sent_to_groups(libc::SIGTERM), [PROGRAM]);
    assert!(
        machine.sent(libc::SIGTERM).is_empty(),
        "no member by itself"
    );
}

#[test]
fn a_group_whose_program_has_gone_is_signalled_member_by_member_through_the_bound() {
    let machine = Machine::new(Vec::new());
    // The program is gone. The group's members: one the program started, a stranger that
    // leads a group of that number now (a process of another identifier), and what that
    // stranger started.
    let table = vec![
        of_group(PROGRAM, 4_000_011, 1100, 1000),
        of_group(PROGRAM, 4_000_070, 9300, 1),
        of_group(PROGRAM, 4_000_071, 9400, 9300),
        // Not in the group at all.
        listed(4_000_072, 9500, 1000),
    ];
    let bound = Bound::new(Some(program().identity));

    bound
        .signal_group(&machine, &Table::of(&table), PROGRAM, libc::SIGKILL)
        .expect("signalled");

    assert!(
        machine.sent_to_groups(libc::SIGKILL).is_empty(),
        "no group is signalled whole"
    );
    assert_eq!(
        machine.sent(libc::SIGKILL),
        [4_000_011],
        "what the program started, and none of the strangers"
    );
}

#[test]
fn a_group_another_process_leads_now_is_not_signalled_whole() {
    let machine = Machine::new(Vec::new());
    // The program's number belongs to another process now, which leads a group by it.
    let table = vec![
        Process {
            session: Some(PROGRAM),
            ..listed(PROGRAM, 9000, 1)
        },
        of_group(PROGRAM, 4_000_073, 9600, 9000),
    ];
    let bound = Bound::new(Some(program().identity));

    bound
        .signal_group(&machine, &Table::of(&table), PROGRAM, libc::SIGTERM)
        .expect("nothing to signal");

    assert!(machine.signalled_none());
}

#[test]
fn a_group_the_program_does_not_lead_is_signalled_member_by_member() {
    let machine = Machine::new(Vec::new());
    let group = 4_000_099;
    let table = vec![
        program(),
        of_group(group, 4_000_080, 9700, 1000),
        of_group(group, 4_000_081, 9800, 1),
    ];
    let bound = Bound::new(Some(table[0].identity));

    bound
        .signal_group(&machine, &Table::of(&table), group, libc::SIGTERM)
        .expect("signalled");

    assert!(machine.sent_to_groups(libc::SIGTERM).is_empty());
    assert_eq!(machine.sent(libc::SIGTERM), [4_000_080]);
}

#[test]
fn a_pane_with_no_program_signals_no_group() {
    let machine = Machine::new(Vec::new());
    let table = vec![of_group(PROGRAM, 4_000_011, 1100, 1000)];
    let bound = Bound::new(None);

    bound
        .signal_group(&machine, &Table::of(&table), PROGRAM, libc::SIGKILL)
        .expect("nothing to signal");

    assert!(machine.signalled_none());
}

#[test]
fn a_number_that_names_more_than_a_group_is_refused() {
    let machine = Machine::new(Vec::new());
    let bound = Bound::new(Some(program().identity));
    let table = vec![program()];
    for number in [0, 1, -1, -4_000_010] {
        let refused = bound.signal_group(&machine, &Table::of(&table), number, libc::SIGKILL);
        assert!(
            refused.is_err_and(|error| error.kind() == io::ErrorKind::InvalidInput),
            "{number}"
        );
    }
    assert!(machine.signalled_none());
}

#[test]
fn nothing_but_the_bound_sends_a_signal() {
    // The selection reads the whole table and may be wrong: it picks who, and never signals.
    for (file, source) in [
        ("process_tree.rs", include_str!("../../process_tree.rs")),
        ("selection.rs", include_str!("../selection.rs")),
    ] {
        for call in [".send(", ".send_group(", "native::signal", "libc::kill"] {
            assert!(
                !source.contains(call),
                "{file} sends a signal itself ({call}): only the bound does"
            );
        }
    }
}
