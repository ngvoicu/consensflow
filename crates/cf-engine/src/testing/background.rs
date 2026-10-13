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
//!
//! Asked to end what it listed, it ends what it tells for the turn's with no doubt,
//! as the real host does: where the mark said how the harness keeps its servers,
//! all it lists; where it did not, the jobs its agent's shells ran, and none of the
//! servers, which it cannot tell from them by when each began, and leaves those
//! running ([`FakeBackground::ended`] says what it ended).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cf_harness::contract::{Pane, Work};
use cf_proto::panes::{Background, BackgroundProcess, Ending, Servers};

use super::executor::Gate;
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
    /// What the engine asked to end, by the pane it asked it of, and what the
    /// host ended of it, in order.
    endings: RefCell<Vec<(Pane, Vec<BackgroundProcess>)>>,
    /// The host answers no ending: one that cannot, or that did not answer.
    refusing: Cell<bool>,
    /// An ending waits for this gate before it ends anything.
    hold: RefCell<Option<Gate>>,
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

    /// What the window of `handle` ran has ended by itself: its jobs. Its servers
    /// live as long as the window.
    pub fn finish(&self, handle: &str) {
        self.jobs.borrow_mut().retain(|(named, _)| named != handle);
    }

    /// The host ends nothing it is asked to, and answers all of it left running:
    /// one that cannot (an older one, which does not know the operation) or that
    /// did not answer.
    pub fn end_nothing(&self) {
        self.refusing.set(true);
    }

    /// The host takes its time over an ending, as it does over the grace it gives
    /// the work: it ends nothing until the gate is opened.
    pub fn hold_endings(&self) -> Gate {
        let gate = Gate::default();
        *self.hold.borrow_mut() = Some(gate.clone());
        gate
    }

    /// The commands the host ended of the window of `handle`'s work, in order.
    pub fn ended(&self, handle: &str) -> Vec<String> {
        self.endings
            .borrow()
            .iter()
            .filter(|(pane, _)| window_of(&pane.id, handle))
            .flat_map(|(_, ended)| ended.iter().map(|process| process.command.clone()))
            .collect()
    }

    /// How many times the engine asked the host to end the window of `handle`'s work.
    pub fn endings_asked(&self, handle: &str) -> usize {
        self.endings
            .borrow()
            .iter()
            .filter(|(pane, _)| window_of(&pane.id, handle))
            .count()
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

    /// What the last mark of `pane` said of how its harness keeps its servers.
    fn said(&self, pane: &Pane) -> Servers {
        self.marked
            .borrow()
            .iter()
            .rev()
            .find(|(marked, _)| marked == pane)
            .map_or(Servers::Unknown, |(_, said)| *said)
    }

    /// What the host lists of `pane`'s tree, by kind: what it tells for the turn's
    /// with no doubt (the jobs, and where the mark said how the harness keeps its
    /// servers, whatever of the servers it lists) and what it cannot tell from the
    /// harness's own (the servers it lists by time alone), the oldest first within
    /// each.
    fn listing(&self, pane: &Pane) -> (Vec<BackgroundProcess>, Vec<BackgroundProcess>) {
        let said = self.said(pane);
        // The harness's servers began first.
        let mut servers = Vec::new();
        if said != Servers::InGroup {
            servers.extend(Self::of(&self.servers, pane));
        }
        if said != Servers::AtLaunch {
            servers.extend(Self::of(&self.launch_children, pane));
        }
        let jobs = Self::of(&self.jobs, pane);
        if said == Servers::Unknown {
            (jobs, servers)
        } else {
            (servers.into_iter().chain(jobs).collect(), Vec::new())
        }
    }

    /// `processes` as the host lists them.
    fn background_of(processes: Vec<BackgroundProcess>) -> Background {
        Background {
            total: processes.len(),
            processes,
        }
    }
}

impl BackgroundWork for FakeBackground {
    fn mark<'a>(&'a self, pane: &'a Pane, servers: Servers) -> Work<'a, ()> {
        self.marked.borrow_mut().push((pane.clone(), servers));
        Box::pin(async {})
    }

    fn running<'a>(&'a self, pane: &'a Pane) -> Work<'a, Background> {
        self.asked.borrow_mut().push(pane.clone());
        let (turns, unsure) = self.listing(pane);
        // The harness's servers began first: the host lists the oldest first.
        let processes: Vec<_> = unsure.into_iter().chain(turns).collect();
        Box::pin(async move {
            Background {
                total: processes.len(),
                processes,
            }
        })
    }

    fn end<'a>(&'a self, pane: &'a Pane, work: &'a [BackgroundProcess]) -> Work<'a, Ending> {
        // What is asked is written down when it is asked; what is ended, when the
        // host does it.
        self.endings.borrow_mut().push((pane.clone(), Vec::new()));
        let at = self.endings.borrow().len() - 1;
        let held = self.hold.borrow().clone();
        Box::pin(async move {
            if let Some(gate) = held {
                gate.wait().await;
            }
            let asked =
                |process: &BackgroundProcess| work.iter().any(|named| named.pid == process.pid);
            let (turns, unsure) = if self.refusing.get() {
                (Vec::new(), work.to_vec())
            } else {
                self.listing(pane)
            };
            let (ended, left): (Vec<_>, Vec<_>) = (
                turns.into_iter().filter(asked).collect(),
                unsure.into_iter().filter(asked).collect(),
            );
            // A process that is ended is no more among what the window's tree runs.
            for list in [&self.jobs, &self.servers, &self.launch_children] {
                for (handle, processes) in list.borrow_mut().iter_mut() {
                    if window_of(&pane.id, handle) {
                        processes
                            .retain(|process| !ended.iter().any(|gone| gone.pid == process.pid));
                    }
                }
            }
            self.endings.borrow_mut()[at].1.clone_from(&ended);
            Ending {
                ended: Self::background_of(ended),
                left: Self::background_of(left),
            }
        })
    }
}
