//! What a pane host says of the work a window's turn left running, for a test
//! to give: the commands a window's tree has running that began after its
//! turn did, and the windows the engine told a turn begins in. It writes
//! nothing of its calls in the Node traces' shape: Node's dispatcher never
//! asked, and the traces hold the engine to what Node's asked.
//!
//! It keeps the rule of the real host that the engine's marks are for: a
//! window's tree has the jobs its agent started, each in a group of its own as a
//! harness runs a command ([`FakeBackground::run`]), the servers its harness
//! started after the mark, which stay in the group of the window's program, and
//! what they started ([`FakeBackground::serve`]), and what a server the window
//! opened with started after the mark, from a group of its own
//! ([`FakeBackground::serve_from_launch`]). Where the last mark said how the
//! harness keeps its servers, those of that kind are not the turn's and are not
//! listed.

use std::cell::RefCell;
use std::rc::Rc;

use cf_harness::contract::{Pane, Work};
use cf_proto::panes::{Background, BackgroundProcess, Servers};

use super::host::window_of;
use crate::seams::BackgroundWork;

/// What a window's tree runs, one list a window.
type Processes = RefCell<Vec<(String, Vec<BackgroundProcess>)>>;

/// The pane host's answers about background work, which a test sets.
#[derive(Default)]
pub struct FakeBackground {
    /// What each window's tree runs that its turn started, each in a group of
    /// its own, by the handle of its participant.
    jobs: Processes,
    /// What each window's tree runs that its harness started after the mark, in
    /// the group of the window's program, and what those started.
    servers: Processes,
    /// What each window's tree runs that a server the window opened with started
    /// after the mark, from a group of its own.
    launch_children: Processes,
    /// The panes the engine said a turn begins in, and what it said of how the
    /// window's harness keeps its servers.
    marked: RefCell<Vec<(Pane, Servers)>>,
    asked: RefCell<Vec<Pane>>,
}

/// `commands` as the processes a host lists, numbered from `first`.
fn listed(commands: &[&str], first: u32) -> Vec<BackgroundProcess> {
    commands
        .iter()
        .zip(first..)
        .map(|(command, pid)| BackgroundProcess {
            pid,
            command: (*command).to_owned(),
        })
        .collect()
}

/// `list` with the entry of `handle` made `processes`.
fn set(list: &Processes, handle: &str, processes: Vec<BackgroundProcess>) {
    let mut list = list.borrow_mut();
    list.retain(|(named, _)| named != handle);
    list.push((handle.to_owned(), processes));
}

impl FakeBackground {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// The window of `handle` (its own, or one of its sessions') has these
    /// commands running that its turn started, each in a group of its own as a
    /// harness runs a command its agent starts, instead of what it had.
    pub fn run(&self, handle: &str, commands: &[&str]) {
        set(&self.jobs, handle, listed(commands, 1001));
    }

    /// The window of `handle` has these servers running that its harness started
    /// after the mark, in the group of the window's program, and what they
    /// started, instead of what it had: the host lists them unless the mark said
    /// the harness keeps its servers in that group.
    pub fn serve(&self, handle: &str, commands: &[&str]) {
        set(&self.servers, handle, listed(commands, 2001));
    }

    /// The window of `handle` has these processes running that a server it was
    /// opened with started after the mark, from a group of its own, instead of
    /// what it had: the host lists them unless the mark said the harness opens
    /// its servers with the window.
    pub fn serve_from_launch(&self, handle: &str, commands: &[&str]) {
        set(&self.launch_children, handle, listed(commands, 3001));
    }

    /// What the window of `handle` ran has ended: its jobs. Its servers live as
    /// long as the window.
    pub fn end(&self, handle: &str) {
        self.jobs.borrow_mut().retain(|(named, _)| named != handle);
    }

    /// The panes the engine said a turn begins in, in order.
    pub fn marked(&self) -> Vec<Pane> {
        self.marked
            .borrow()
            .iter()
            .map(|(pane, _)| pane.clone())
            .collect()
    }

    /// What each of those marks said of how the window's harness keeps its
    /// servers.
    pub fn servers_said(&self) -> Vec<Servers> {
        self.marked.borrow().iter().map(|(_, said)| *said).collect()
    }

    /// How many times the engine asked what the window of `handle` ran.
    pub fn asked(&self, handle: &str) -> usize {
        self.asked
            .borrow()
            .iter()
            .filter(|pane| window_of(&pane.id, handle))
            .count()
    }

    /// What of the window's list `pane` the host lists: the processes of the
    /// kind `list` holds for it.
    fn of(list: &Processes, pane: &Pane) -> Vec<BackgroundProcess> {
        list.borrow()
            .iter()
            .find(|(handle, _)| window_of(&pane.id, handle))
            .map(|(_, processes)| processes.clone())
            .unwrap_or_default()
    }
}

impl BackgroundWork for FakeBackground {
    fn mark<'a>(&'a self, pane: &'a Pane, servers: Servers) -> Work<'a, ()> {
        self.marked.borrow_mut().push((pane.clone(), servers));
        Box::pin(async {})
    }

    fn running<'a>(&'a self, pane: &'a Pane) -> Work<'a, Background> {
        self.asked.borrow_mut().push(pane.clone());
        let said = self
            .marked
            .borrow()
            .iter()
            .rev()
            .find(|(marked, _)| marked == pane)
            .map_or(Servers::Unknown, |(_, said)| *said);
        // The harness's servers began first: the host lists the oldest first.
        let mut processes = Vec::new();
        if said != Servers::InGroup {
            processes.extend(Self::of(&self.servers, pane));
        }
        if said != Servers::AtLaunch {
            processes.extend(Self::of(&self.launch_children, pane));
        }
        processes.extend(Self::of(&self.jobs, pane));
        Box::pin(async move {
            Background {
                total: processes.len(),
                processes,
            }
        })
    }
}
