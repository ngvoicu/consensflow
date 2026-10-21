//! What the pane host is told of the windows a turn begins in, for a test to read:
//! the engine marks each window where a turn begins (with how its harness keeps its
//! servers, and which harness it is), so that what a close says the window left
//! running is the turns'. It writes nothing of its calls in the Node traces' shape:
//! Node's dispatcher never asked, and the traces hold the engine to what Node's asked.
//! Nothing is ever asked to end (`turn_end`): the seam has no such call.

use std::cell::RefCell;
use std::rc::Rc;

use cf_harness::contract::{Pane, Work};
use cf_proto::panes::Servers;

use crate::seams::BackgroundWork;

/// What a mark told the host of a window.
struct Marked {
    pane: Pane,
    servers: Servers,
    harness: String,
}

/// The pane host's side of the marks, which a test reads.
#[derive(Default)]
pub struct FakeBackground {
    /// What the engine told of each window a turn begins in, in order.
    marked: RefCell<Vec<Marked>>,
}

impl FakeBackground {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
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
        Box::pin(async {})
    }
}
