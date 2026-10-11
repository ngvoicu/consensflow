//! What a pane host says of the work a window's turn left running, for a test
//! to give: the commands a window's tree has running that began after its
//! turn did, and the windows the engine told a turn begins in. It writes
//! nothing of its calls in the Node traces' shape: Node's dispatcher never
//! asked, and the traces hold the engine to what Node's asked.

use std::cell::RefCell;
use std::rc::Rc;

use cf_harness::contract::{Pane, Work};
use cf_proto::panes::{Background, BackgroundProcess};

use super::host::window_of;
use crate::seams::BackgroundWork;

/// The pane host's answers about background work, which a test sets.
#[derive(Default)]
pub struct FakeBackground {
    /// What each window's tree runs, by the handle of its participant.
    running: RefCell<Vec<(String, Vec<BackgroundProcess>)>>,
    marked: RefCell<Vec<Pane>>,
    asked: RefCell<Vec<Pane>>,
}

impl FakeBackground {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// The window of `handle` (its own, or one of its sessions') has these
    /// commands running that its turn started, instead of what it had.
    pub fn run(&self, handle: &str, commands: &[&str]) {
        let processes = commands
            .iter()
            .zip(1u32..)
            .map(|(command, pid)| BackgroundProcess {
                pid: 1000 + pid,
                command: (*command).to_owned(),
            })
            .collect();
        let mut running = self.running.borrow_mut();
        running.retain(|(named, _)| named != handle);
        running.push((handle.to_owned(), processes));
    }

    /// What the window of `handle` ran has ended.
    pub fn end(&self, handle: &str) {
        self.running
            .borrow_mut()
            .retain(|(named, _)| named != handle);
    }

    /// The panes the engine said a turn begins in, in order.
    pub fn marked(&self) -> Vec<Pane> {
        self.marked.borrow().clone()
    }

    /// How many times the engine asked what the window of `handle` ran.
    pub fn asked(&self, handle: &str) -> usize {
        self.asked
            .borrow()
            .iter()
            .filter(|pane| window_of(&pane.id, handle))
            .count()
    }
}

impl BackgroundWork for FakeBackground {
    fn mark<'a>(&'a self, pane: &'a Pane) -> Work<'a, ()> {
        self.marked.borrow_mut().push(pane.clone());
        Box::pin(async {})
    }

    fn running<'a>(&'a self, pane: &'a Pane) -> Work<'a, Background> {
        self.asked.borrow_mut().push(pane.clone());
        let processes = self
            .running
            .borrow()
            .iter()
            .find(|(handle, _)| window_of(&pane.id, handle))
            .map(|(_, processes)| processes.clone())
            .unwrap_or_default();
        Box::pin(async move {
            Background {
                total: processes.len(),
                processes,
            }
        })
    }
}
