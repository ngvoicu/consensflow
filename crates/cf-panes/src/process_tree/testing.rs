//! A machine for the tests of the tree that signals nothing: a table of processes made
//! up, and a record of the signals the tree would have sent. What the tests prove of
//! what the tree signals they prove on it, with no signal sent to any process.

use std::cell::RefCell;
use std::io;

use super::bound::System;
use super::{Identity, Process};

/// A process of a table, started by the process whose identifier is `parent`, in a
/// group and a session of its own.
pub(super) fn listed(pid: i32, unique: u64, parent: u64) -> Process {
    Process {
        identity: Identity {
            pid,
            born: (1, 0),
            unique,
        },
        parent,
        group: pid,
        session: None,
        name: format!("p{pid}"),
    }
}

/// The window of the tables below: its program, a shell the program started, the job of
/// the shell's, and a job of the job's. Their numbers are above any a Mac gives, so that
/// no process there is has one.
pub(super) const PROGRAM: i32 = 4_000_010;
pub(super) const SHELL: i32 = 4_000_011;
pub(super) const JOB: i32 = 4_000_012;
pub(super) const DEEP: i32 = 4_000_013;
/// What is outside the window: a build, which the system's init started; a shell of the
/// user's and what it started; and a job nothing saw under the window's program, which is
/// in the window's session and whose parent has gone.
pub(super) const BUILD: i32 = 4_000_020;
pub(super) const USERS_SHELL: i32 = 4_000_021;
pub(super) const USERS_JOB: i32 = 4_000_022;
pub(super) const LOST: i32 = 4_000_023;

/// The number of the process that started the process this test runs in: a process
/// that is none of the window's, whatever selects it.
pub(super) fn the_tests_own_parent() -> i32 {
    i32::try_from(std::os::unix::process::parent_id()).expect("a process number")
}

/// The window's own processes, in the order of the table below.
pub(super) fn the_windows() -> [i32; 4] {
    [PROGRAM, SHELL, JOB, DEEP]
}

/// What the system's table holds: the window, which the pane host (this process, which a
/// table never holds) started, and around it the test's own parent, a child of the
/// system's init that was never seen in the window, a user's shell with a job, and a job
/// in the window's session that lost its parent.
pub(super) fn a_window_among_strangers() -> Vec<Process> {
    vec![
        Process {
            session: Some(PROGRAM),
            ..listed(PROGRAM, 1000, 700)
        },
        listed(SHELL, 1100, 1000),
        listed(JOB, 1200, 1100),
        listed(DEEP, 1300, 1200),
        listed(the_tests_own_parent(), 800, 1),
        listed(BUILD, 2000, 1),
        listed(USERS_SHELL, 2100, 1),
        listed(USERS_JOB, 2200, 2100),
        Process {
            session: Some(PROGRAM),
            ..listed(LOST, 2300, 1)
        },
    ]
}

/// The identity of the process `pid` in `table`.
pub(super) fn identity_of(table: &[Process], pid: i32) -> Identity {
    table
        .iter()
        .find(|process| process.identity.pid == pid)
        .map(|process| process.identity)
        .expect("a process of the table")
}

/// A made-up machine: the processes it has, and the signals sent to them.
pub(super) struct Machine {
    processes: Vec<Process>,
    sent: RefCell<Vec<(i32, i32)>>,
}

impl Machine {
    pub fn new(processes: Vec<Process>) -> Self {
        Self {
            processes,
            sent: RefCell::default(),
        }
    }

    /// The numbers of the processes sent `signal`, each once, in order of number.
    pub fn sent(&self, signal: i32) -> Vec<i32> {
        let mut pids: Vec<i32> = self
            .sent
            .borrow()
            .iter()
            .filter(|(_, said)| *said == signal)
            .map(|(pid, _)| *pid)
            .collect();
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// Whether no signal was sent at all.
    pub fn signalled_none(&self) -> bool {
        self.sent.borrow().is_empty()
    }
}

impl System for Machine {
    fn processes(&self) -> io::Result<Vec<Process>> {
        Ok(self.processes.clone())
    }

    fn send(&self, to: Identity, signal: i32) -> io::Result<()> {
        self.sent.borrow_mut().push((to.pid, signal));
        Ok(())
    }
}
