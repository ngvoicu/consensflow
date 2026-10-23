//! What a window's exit names as possibly the window's and not ended
//! ([`ProcessTree::possibly_left`]), on a machine made up (`testing`): a job a tool shell
//! detached, which carries the window's marker, began after its program and is in a session
//! whose leader has gone, and each of those taken away. And the bound refuses what is
//! named: no close nor ending signals it, whatever a selection names (T-406).

use super::testing::{a_window_among_strangers, identity_of, listed, Machine, PROGRAM};
use super::*;

/// The window's marker on the machines below.
const MARKER: &str = "0123456789abcdef0123456789abcdef";

/// A server a tool shell of the window detached: in the session of the tool shell, which has
/// gone, with init named as the process that started it, begun after the window's program.
const SERVER: i32 = 4_000_030;
/// The tool shell that started [`SERVER`], which has gone: its number is the session's.
const TOOL_SHELL: i32 = 4_000_031;

/// [`a_window_among_strangers`] with [`SERVER`], begun `born`.
fn a_window_with_a_detached_server(born: (u64, u64)) -> Vec<Process> {
    let mut table = a_window_among_strangers();
    table.push(Process {
        identity: Identity {
            born,
            ..listed(SERVER, 3000, 1).identity
        },
        session: Some(TOOL_SHELL),
        ..listed(SERVER, 3000, 1)
    });
    table
}

/// The tree of the window: its program, begun at (1, 0), leads its session.
fn tree_of(table: &[Process]) -> ProcessTree {
    let mut tree = ProcessTree::new();
    MARKER.clone_into(&mut tree.marker);
    tree.root = Some(identity_of(table, PROGRAM));
    tree.session = Some(PROGRAM);
    tree.bound = Bound::new(tree.root, tree.session);
    tree
}

/// The numbers of what `tree` names on `machine`.
fn named(tree: &ProcessTree, machine: &Machine) -> Vec<u32> {
    tree.possibly_left_by(machine)
        .iter()
        .map(|left| left.pid)
        .collect()
}

#[test]
fn a_marked_job_in_a_session_whose_leader_has_gone_is_named_by_its_number_and_program() {
    let table = a_window_with_a_detached_server((2, 0));
    let machine = Machine::new(table.clone()).marking(MARKER, &[SERVER]);
    let tree = tree_of(&table);

    assert_eq!(
        tree.possibly_left_by(&machine),
        [PossiblyLeft {
            pid: SERVER.unsigned_abs(),
            program: format!("p{SERVER}"),
        }]
    );
    assert!(machine.signalled_none());
}

#[test]
fn a_job_that_carries_another_marker_or_none_is_not_named() {
    let table = a_window_with_a_detached_server((2, 0));
    let tree = tree_of(&table);

    let unmarked = Machine::new(table.clone());
    assert_eq!(named(&tree, &unmarked), Vec::<u32>::new());
    let anothers = Machine::new(table).marking("another window's", &[SERVER]);
    assert_eq!(named(&tree, &anothers), Vec::<u32>::new());
}

#[test]
fn a_marked_job_that_began_before_the_windows_program_is_not_named() {
    let table = a_window_with_a_detached_server((0, 999_999));
    let machine = Machine::new(table.clone()).marking(MARKER, &[SERVER]);

    assert_eq!(named(&tree_of(&table), &machine), Vec::<u32>::new());
}

#[test]
fn a_marked_job_whose_sessions_number_a_process_holds_is_not_named() {
    // The leader runs: a process of ours has the session's number.
    let mut table = a_window_with_a_detached_server((2, 0));
    table.push(Process {
        session: Some(TOOL_SHELL),
        ..listed(TOOL_SHELL, 2900, 1)
    });
    let machine = Machine::new(table.clone()).marking(MARKER, &[SERVER]);
    assert_eq!(named(&tree_of(&table), &machine), Vec::<u32>::new());

    // Another user's process has it (the system answers EPERM), or a zombie (it answers).
    let table = a_window_with_a_detached_server((2, 0));
    let machine = Machine::new(table.clone())
        .marking(MARKER, &[SERVER])
        .holding(&[TOOL_SHELL]);
    assert_eq!(named(&tree_of(&table), &machine), Vec::<u32>::new());
}

#[test]
fn a_window_whose_program_was_never_read_names_nothing() {
    let table = a_window_with_a_detached_server((2, 0));
    let machine = Machine::new(table.clone()).marking(MARKER, &[SERVER]);
    let mut tree = tree_of(&table);
    tree.root = None;
    tree.bound = Bound::new(None, None);

    assert_eq!(named(&tree, &machine), Vec::<u32>::new());
}

#[test]
fn what_the_bound_allows_is_not_named_the_close_ends_it() {
    // A process the program started, in a session of a shell that has gone: under the
    // program by the process that started it, which the close ends.
    let mut table = a_window_among_strangers();
    table.push(Process {
        identity: Identity {
            born: (2, 0),
            ..listed(SERVER, 3000, 1000).identity
        },
        session: Some(TOOL_SHELL),
        ..listed(SERVER, 3000, 1000)
    });
    let machine = Machine::new(table.clone()).marking(MARKER, &[SERVER]);

    assert_eq!(named(&tree_of(&table), &machine), Vec::<u32>::new());
}

#[test]
fn the_bound_refuses_what_is_named_and_no_close_or_ending_signals_it() {
    let table = a_window_with_a_detached_server((2, 0));
    let machine = Machine::new(table.clone()).marking(MARKER, &[SERVER]);
    let tree = tree_of(&table);
    let server = identity_of(&table, SERVER);
    assert_eq!(named(&tree, &machine), [SERVER.unsigned_abs()]);

    // Looks at the tree, then a close and an ending whose selection names every process.
    tree.members_by(&machine).expect("a look");
    assert!(!tree.bound.allows(&Table::of(&table), server));
    let every = Lineage {
        identities: table.iter().map(|process| process.identity).collect(),
        members: Vec::new(),
        unverified: Vec::new(),
    };
    tree.ask_by(&machine, Subject::Whole, None).expect("asked");
    tree.ask_by(&machine, Subject::Lineage(&every), None)
        .expect("asked");
    tree.force_by(&machine, Subject::Whole, &[server]);
    tree.force_by(&machine, Subject::Lineage(&every), &[server]);

    for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGSTOP, libc::SIGKILL] {
        assert!(
            !machine.sent(signal).contains(&SERVER),
            "signal {signal} reached what was only named"
        );
    }
    assert!(!tree.bound.allows(&Table::of(&table), server));
    assert_eq!(
        named(&tree, &machine),
        [SERVER.unsigned_abs()],
        "it is named again: nothing made it the window's"
    );
}
