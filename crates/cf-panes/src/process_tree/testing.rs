//! A machine for the tests of the tree that signals nothing: a table of processes made
//! up, and a record of the signals the tree would have sent. What the tests prove of
//! what the tree signals they prove on it, with no signal sent to any process.

use std::cell::RefCell;
use std::io;

use super::bound::System;
use super::{key_of, Identity, Process};
use crate::background::ProcessKey;

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

/// The window of the tables below: its program, which leads a session; a shell the program
/// started in a session of its own (as Claude Code's tool shells are), the job of the
/// shell's, and a job of the job's; and a job an agent detached from its shell, which is in
/// the program's session and whose parent has gone. Their numbers are above any a Mac gives,
/// so that no process there is has one.
pub(super) const PROGRAM: i32 = 4_000_010;
pub(super) const SHELL: i32 = 4_000_011;
pub(super) const JOB: i32 = 4_000_012;
pub(super) const DEEP: i32 = 4_000_013;
pub(super) const DETACHED: i32 = 4_000_014;
/// What is outside the window: a build, which the system's init started; a shell of the
/// user's and what it started; and a job that left the window's session (`setsid`), whose
/// shell is gone and which no look saw under the window's program.
pub(super) const BUILD: i32 = 4_000_020;
pub(super) const USERS_SHELL: i32 = 4_000_021;
pub(super) const USERS_JOB: i32 = 4_000_022;
pub(super) const LEFT: i32 = 4_000_023;

/// The number of the process that started the process this test runs in: a process
/// that is none of the window's, whatever selects it.
pub(super) fn the_tests_own_parent() -> i32 {
    i32::try_from(std::os::unix::process::parent_id()).expect("a process number")
}

/// The window's own processes, in the order of their numbers.
pub(super) fn the_windows() -> [i32; 5] {
    [PROGRAM, SHELL, JOB, DEEP, DETACHED]
}

/// What the system's table holds: the window, which the pane host (this process, which a
/// table never holds) started, and around it the test's own parent, a child of the
/// system's init that was never seen in the window, a user's shell with a job, and a job
/// that left the window's session.
pub(super) fn a_window_among_strangers() -> Vec<Process> {
    vec![
        Process {
            session: Some(PROGRAM),
            ..listed(PROGRAM, 1000, 700)
        },
        Process {
            session: Some(SHELL),
            ..listed(SHELL, 1100, 1000)
        },
        Process {
            session: Some(SHELL),
            ..listed(JOB, 1200, 1100)
        },
        Process {
            session: Some(SHELL),
            ..listed(DEEP, 1300, 1200)
        },
        Process {
            session: Some(PROGRAM),
            ..listed(DETACHED, 1400, 1)
        },
        listed(the_tests_own_parent(), 800, 1),
        listed(BUILD, 2000, 1),
        listed(USERS_SHELL, 2100, 1),
        listed(USERS_JOB, 2200, 2100),
        Process {
            session: Some(LEFT),
            ..listed(LEFT, 2300, 1)
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

/// A made-up machine: the processes it has, and the signals sent to them and to groups.
pub(super) struct Machine {
    processes: Vec<Process>,
    sent: RefCell<Vec<(i32, i32)>>,
    sent_to_groups: RefCell<Vec<(i32, i32)>>,
    /// The processes that carry the window's marker, and the marker.
    marked: Vec<i32>,
    marker: String,
    /// Numbers held by processes the table does not hold: another user's, or a zombie.
    held: Vec<i32>,
    /// Processes of the table whose numbers another process has taken since it was read.
    taken: Vec<i32>,
}

impl Machine {
    pub fn new(processes: Vec<Process>) -> Self {
        Self {
            processes,
            sent: RefCell::default(),
            sent_to_groups: RefCell::default(),
            marked: Vec::new(),
            marker: String::new(),
            held: Vec::new(),
            taken: Vec::new(),
        }
    }

    /// The machine, where the processes `pids` carry `marker` in their environment.
    pub fn marking(mut self, marker: &str, pids: &[i32]) -> Self {
        marker.clone_into(&mut self.marker);
        self.marked = pids.to_vec();
        self
    }

    /// The machine, where processes the table does not hold have the numbers `pids`.
    pub fn holding(mut self, pids: &[i32]) -> Self {
        self.held = pids.to_vec();
        self
    }

    /// The machine, where the processes `pids` of the table have ended since it was read and
    /// others have their numbers: none of them runs.
    pub fn taking(mut self, pids: &[i32]) -> Self {
        self.taken = pids.to_vec();
        self
    }

    /// The numbers of the groups sent `signal` whole, each once, in order of number.
    pub fn sent_to_groups(&self, signal: i32) -> Vec<i32> {
        let mut groups: Vec<i32> = self
            .sent_to_groups
            .borrow()
            .iter()
            .filter(|(_, said)| *said == signal)
            .map(|(group, _)| *group)
            .collect();
        groups.sort_unstable();
        groups.dedup();
        groups
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

    /// Whether no signal was sent at all, to a process or to a group.
    pub fn signalled_none(&self) -> bool {
        self.sent.borrow().is_empty() && self.sent_to_groups.borrow().is_empty()
    }
}

impl System for Machine {
    fn processes(&self) -> io::Result<Vec<Process>> {
        Ok(self.processes.clone())
    }

    /// A process of the table runs until it is sent SIGKILL, unless its number was taken.
    fn runs(&self, key: ProcessKey) -> bool {
        self.processes.iter().any(|process| {
            key_of(process.identity) == key && !self.taken.contains(&process.identity.pid)
        }) && !self
            .sent
            .borrow()
            .iter()
            .any(|(pid, signal)| u32::try_from(*pid) == Ok(key.pid) && *signal == libc::SIGKILL)
    }

    fn send(&self, to: Identity, signal: i32) -> io::Result<()> {
        self.sent.borrow_mut().push((to.pid, signal));
        Ok(())
    }

    fn send_group(&self, group: i32, signal: i32) -> io::Result<()> {
        self.sent_to_groups.borrow_mut().push((group, signal));
        Ok(())
    }

    fn carries(&self, pid: i32, marker: &str) -> bool {
        !marker.is_empty() && marker == self.marker && self.marked.contains(&pid)
    }

    fn nobody_holds(&self, pid: i32) -> bool {
        pid > 1
            && !self.held.contains(&pid)
            && !self
                .processes
                .iter()
                .any(|process| process.identity.pid == pid)
    }
}
