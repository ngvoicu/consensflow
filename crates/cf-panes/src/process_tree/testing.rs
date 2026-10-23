//! A machine for the tests of the tree that signals nothing: a table of processes made
//! up, and a record of the signals the tree would have sent. What the tests prove of
//! what the tree signals they prove on it, with no signal sent to any process.

use std::cell::{Cell, RefCell};
use std::io;

use super::bound::System;
use super::{key_of, Identity, Looked, Process};
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

/// The numbers above which the children a churning process starts are.
const CHURNED: i32 = 5_000_000;

/// A made-up machine: the processes it has, and the signals sent to them and to groups.
pub(super) struct Machine {
    processes: Vec<Process>,
    /// Processes it has that no look can read (another user's): each by the number of
    /// the process that started it, and its own.
    unreadable: Vec<(i32, i32)>,
    /// Processes that ended and were not reaped, which their parent still names.
    zombies: Vec<(i32, i32)>,
    /// Processes born after its table was read, named by their parent once: the next look
    /// finds them in it.
    born: RefCell<Vec<(i32, i32)>>,
    /// A process that starts a child at every listing of its children, none of which a
    /// later table has: a tree that changes at every look. Its number, and how many it
    /// started.
    churning: Option<(i32, Cell<i32>)>,
    sent: RefCell<Vec<(i32, i32)>>,
    sent_to_groups: RefCell<Vec<(i32, i32)>>,
}

impl Machine {
    pub fn new(processes: Vec<Process>) -> Self {
        Self {
            processes,
            unreadable: Vec::new(),
            zombies: Vec::new(),
            born: RefCell::default(),
            churning: None,
            sent: RefCell::default(),
            sent_to_groups: RefCell::default(),
        }
    }

    /// The machine, with a process `pid` that `parent` started and no look can read.
    pub fn with_unreadable(mut self, parent: i32, pid: i32) -> Self {
        self.unreadable.push((parent, pid));
        self
    }

    /// The machine, with a process `pid` that `parent` started and that ended unreaped.
    pub fn with_zombie(mut self, parent: i32, pid: i32) -> Self {
        self.zombies.push((parent, pid));
        self
    }

    /// The machine, with a process `pid` that `parent` started while a look read its table.
    pub fn with_born(self, parent: i32, pid: i32) -> Self {
        self.born.borrow_mut().push((parent, pid));
        self
    }

    /// The machine, where `parent` starts a child at every look, which the table read
    /// before it never has.
    pub fn with_churning(mut self, parent: i32) -> Self {
        self.churning = Some((parent, Cell::new(0)));
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

    /// A process of the table runs until it is sent SIGKILL.
    fn runs(&self, key: ProcessKey) -> bool {
        self.processes
            .iter()
            .any(|process| key_of(process.identity) == key)
            && !self
                .sent
                .borrow()
                .iter()
                .any(|(pid, signal)| u32::try_from(*pid) == Ok(key.pid) && *signal == libc::SIGKILL)
    }

    fn children(&self, pid: i32) -> io::Result<Vec<i32>> {
        let unique = self
            .processes
            .iter()
            .find(|process| process.identity.pid == pid)
            .map(|process| process.identity.unique);
        Ok(self
            .processes
            .iter()
            .filter(|process| unique.is_some_and(|unique| process.parent == unique))
            .map(|process| process.identity.pid)
            .chain(
                self.unreadable
                    .iter()
                    .chain(&self.zombies)
                    .chain(self.born.borrow().iter())
                    .filter(|(parent, _)| *parent == pid)
                    .map(|(_, child)| *child),
            )
            .chain(self.churning.as_ref().and_then(|(parent, started)| {
                (*parent == pid).then(|| {
                    started.set(started.get() + 1);
                    CHURNED + started.get()
                })
            }))
            .collect())
    }

    fn looked(&self, pid: i32) -> Looked {
        let born = {
            let mut born = self.born.borrow_mut();
            let before = born.len();
            born.retain(|(_, child)| *child != pid);
            born.len() < before
        };
        if born
            || (self.churning.is_some() && pid > CHURNED)
            || self
                .processes
                .iter()
                .any(|process| process.identity.pid == pid)
        {
            Looked::Readable
        } else if self.unreadable.iter().any(|(_, child)| *child == pid) {
            Looked::Unreadable
        } else {
            Looked::Ended
        }
    }

    fn send(&self, to: Identity, signal: i32) -> io::Result<()> {
        self.sent.borrow_mut().push((to.pid, signal));
        Ok(())
    }

    fn send_group(&self, group: i32, signal: i32) -> io::Result<()> {
        self.sent_to_groups.borrow_mut().push((group, signal));
        Ok(())
    }
}
