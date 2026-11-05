//! Which window holds a process the system's log of privacy dialogs names by its number
//! ([`ProcessTree::holds`], T-1194), on a machine made up (`testing`): the window's program,
//! the tool shell it started and the jobs below it, one a tool shell detached, are the
//! window's; a build init started, a shell of the user's and its job, and a job that left the
//! window's session with no shell to show it, are not.

use super::testing::{
    a_window_among_strangers, identity_of, Machine, BUILD, DEEP, DETACHED, JOB, LEFT, PROGRAM,
    SHELL, USERS_JOB, USERS_SHELL,
};
use super::*;

/// The tree of the window: its program leads its session.
fn tree_of(table: &[Process]) -> ProcessTree {
    let mut tree = ProcessTree::new();
    tree.root = Some(identity_of(table, PROGRAM));
    tree.session = Some(PROGRAM);
    tree.bound = Bound::new(tree.root, tree.session);
    tree
}

fn held(tree: &ProcessTree, machine: &Machine, pid: i32) -> bool {
    tree.holds_by(machine, pid.unsigned_abs()).expect("a look")
}

#[test]
fn a_window_holds_its_program_and_what_runs_below_it() {
    let table = a_window_among_strangers();
    let machine = Machine::new(table.clone());
    let tree = tree_of(&table);

    for pid in [PROGRAM, SHELL, JOB, DEEP, DETACHED] {
        assert!(held(&tree, &machine, pid), "{pid} is the window's");
    }
}

#[test]
fn a_window_holds_no_process_of_the_users_nor_one_that_left_its_session_unseen() {
    let table = a_window_among_strangers();
    let machine = Machine::new(table.clone());
    let tree = tree_of(&table);

    for pid in [BUILD, USERS_SHELL, USERS_JOB, LEFT] {
        assert!(!held(&tree, &machine, pid), "{pid} is no window's");
    }
    // A number no process has.
    assert!(!held(&tree, &machine, 4_999_999));
}
