//! What a pane host says of the work a window's turn left running, for a test
//! to give: the commands a window's tree has running that its turn started, which
//! the host proves are the turn's, and the windows the engine told a turn begins in.
//! It writes nothing of its calls in the Node traces' shape: Node's dispatcher never
//! asked, and the traces hold the engine to what Node's asked.
//!
//! It keeps the rule of the real host that the engine's asks are for: the host
//! chooses what it ends, from what it can prove the turn's, and says so; what it
//! cannot tell from the harness's own (a server the harness started late) it leaves
//! alone, and says nothing of, so a test gives the engine none of it. Asked to end
//! what its window's turn left, it ends the commands the test said ran
//! ([`FakeBackground::run`]), says those it could not end ([`FakeBackground::stubborn`])
//! left, says nothing was ended where it is made to ([`FakeBackground::refuse`]), and
//! gives no answer where it is made to ([`FakeBackground::leave_unanswered`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cf_harness::contract::{Pane, Work};
use cf_proto::panes::{Background, BackgroundProcess, Ending, Servers, BACKGROUND_PROCESSES};

use super::executor::Gate;
use super::host::window_of;
use crate::seams::BackgroundWork;

/// What a window's tree runs, one list a window.
type Processes = RefCell<Vec<(String, Vec<BackgroundProcess>)>>;

/// What a mark told the host of a window.
struct Marked {
    pane: Pane,
    servers: Servers,
    harness: String,
}

/// The pane host's answers about background work, which a test sets.
#[derive(Default)]
pub struct FakeBackground {
    /// What each window's tree runs that its turn started, which an ending ends,
    /// by the handle of its participant.
    jobs: Processes,
    /// What each window's tree runs that its turn started and the host cannot end.
    stubborn: Processes,
    /// What the engine told of each window a turn begins in, in order.
    marked: RefCell<Vec<Marked>>,
    /// What the engine asked to end, by the pane it asked it of, and what the host
    /// ended of it, in order.
    endings: RefCell<Vec<(Pane, Vec<BackgroundProcess>)>>,
    /// The host refuses an ending: it says nothing was ended, as one that does not
    /// know the operation does.
    refusing: Cell<bool>,
    /// How many of the next endings the host gives no answer to.
    unanswered: Cell<usize>,
    /// An ending waits for this gate before it ends anything.
    hold: RefCell<Option<Gate>>,
    /// The word that a turn begins waits for this gate before it is taken.
    mark_hold: RefCell<Option<Gate>>,
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

/// What `list` holds of the window of `pane`.
fn of(list: &Processes, pane: &Pane) -> Vec<BackgroundProcess> {
    list.borrow()
        .iter()
        .find(|(handle, _)| window_of(&pane.id, handle))
        .map(|(_, processes)| processes.clone())
        .unwrap_or_default()
}

/// `processes` as the host lists them: how many, and the first of them.
fn background_of(processes: Vec<BackgroundProcess>) -> Background {
    Background {
        total: processes.len(),
        processes: processes.into_iter().take(BACKGROUND_PROCESSES).collect(),
    }
}

impl FakeBackground {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// The window of `handle` (its own, or one of its sessions') has these
    /// commands running that its turn started, instead of what it had: what an
    /// ending of it ends.
    pub fn run(&self, handle: &str, commands: &[&str]) {
        set(&self.jobs, handle, listed(commands, 1001));
    }

    /// The window of `handle` has these commands running that its turn started and
    /// that the host cannot end: it says they are still running.
    pub fn stubborn(&self, handle: &str, commands: &[&str]) {
        set(&self.stubborn, handle, listed(commands, 2001));
    }

    /// What the window of `handle` ran has ended by itself.
    pub fn finish(&self, handle: &str) {
        self.jobs.borrow_mut().retain(|(named, _)| named != handle);
    }

    /// The host refuses to end anything: an older one, which does not know the
    /// operation, answers that it ended nothing.
    pub fn refuse(&self) {
        self.refusing.set(true);
    }

    /// The host gives no answer to the next `times` endings it is asked: it may be
    /// ending still, or it went.
    pub fn leave_unanswered(&self, times: usize) {
        self.unanswered.set(times);
    }

    /// The host takes its time over the word that a turn begins: it is not taken
    /// until the gate is opened.
    pub fn hold_marks(&self) -> Gate {
        let gate = Gate::default();
        *self.mark_hold.borrow_mut() = Some(gate.clone());
        gate
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
            .map(|marked| marked.pane.clone())
            .collect()
    }

    /// What each of those marks said of how the window's harness keeps its
    /// servers.
    pub fn servers_said(&self) -> Vec<Servers> {
        self.marked
            .borrow()
            .iter()
            .map(|marked| marked.servers)
            .collect()
    }

    /// What each of those marks said of which harness the window is.
    pub fn harness_said(&self) -> Vec<String> {
        self.marked
            .borrow()
            .iter()
            .map(|marked| marked.harness.clone())
            .collect()
    }
}

impl BackgroundWork for FakeBackground {
    fn mark<'a>(&'a self, pane: &'a Pane, servers: Servers, harness: &'a str) -> Work<'a, ()> {
        self.marked.borrow_mut().push(Marked {
            pane: pane.clone(),
            servers,
            harness: harness.to_owned(),
        });
        let held = self.mark_hold.borrow().clone();
        Box::pin(async move {
            if let Some(gate) = held {
                gate.wait().await;
            }
        })
    }

    fn end<'a>(&'a self, pane: &'a Pane) -> Work<'a, Option<Ending>> {
        // What is asked is written down when it is asked; what is ended, when the
        // host does it.
        self.endings.borrow_mut().push((pane.clone(), Vec::new()));
        let at = self.endings.borrow().len() - 1;
        let held = self.hold.borrow().clone();
        Box::pin(async move {
            if let Some(gate) = held {
                gate.wait().await;
            }
            let unanswered = self.unanswered.get();
            if unanswered > 0 {
                self.unanswered.set(unanswered - 1);
                return None;
            }
            if self.refusing.get() {
                return Some(Ending::default());
            }
            let ended = of(&self.jobs, pane);
            let left = of(&self.stubborn, pane);
            // A process that is ended is no more among what the window's tree runs.
            for (handle, processes) in self.jobs.borrow_mut().iter_mut() {
                if window_of(&pane.id, handle) {
                    processes.clear();
                }
            }
            self.endings.borrow_mut()[at].1.clone_from(&ended);
            Some(Ending {
                ended: background_of(ended),
                left: background_of(left),
            })
        })
    }
}
