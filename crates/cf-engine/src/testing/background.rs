//! What the pane host is told of the windows a turn begins in, for a test to read:
//! the engine marks each window where a turn begins (with how its harness keeps its
//! servers, and which harness it is), so that what a close says the window left
//! running is the turns'. It writes nothing of its calls in the Node traces' shape:
//! Node's dispatcher never asked, and the traces hold the engine to what Node's asked.
//! Nothing is ever asked to end (`turn_end`): the seam has no such call.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cf_harness::contract::{Pane, Work};
use cf_proto::panes::Servers;

use crate::seams::{BackgroundWork, CommandsSeen};

/// What a mark told the host of a window.
struct Marked {
    pane: Pane,
    servers: Servers,
    harness: String,
}

/// A look at a window's commands: the pane, the rest it named and the time.
type Looked = (Pane, Option<u64>, Option<i64>);

/// The pane host's side of the marks, which a test reads, and of its looks at the
/// commands of a window's harness, which a test sets.
#[derive(Default)]
pub struct FakeBackground {
    /// What the engine told of each window a turn begins in, in order.
    marked: RefCell<Vec<Marked>>,
    /// What a look at a window's commands answers: that the host cannot tell, unless a
    /// test says.
    commands: Cell<Option<CommandsSeen>>,
    /// The panes the engine looked at the commands of, in order, and the rest each named.
    looked: RefCell<Vec<Looked>>,
    /// The rests the engine asked the host to keep, in order, with the earlier rests each
    /// asked kept: each is given the number of its place, from 0.
    rests: RefCell<Vec<(Pane, Vec<u64>)>>,
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

    /// A look at a window's commands answers that `commands` run from now on, or that the
    /// host cannot tell them (none).
    pub fn commands_run(&self, commands: Option<usize>) {
        self.commands.set(Some(
            commands.map_or(CommandsSeen::Untold, CommandsSeen::Running),
        ));
    }

    /// A look at a window's commands is answered that a process of it cannot be read.
    pub fn commands_unreadable(&self) {
        self.commands.set(Some(CommandsSeen::Unreadable));
    }

    /// A look at a window's commands is answered that its processes kept changing.
    pub fn commands_unsettled(&self) {
        self.commands.set(Some(CommandsSeen::Unsettled));
    }

    /// The earlier rests each rest asked the host to keep, in order.
    pub fn kept(&self) -> Vec<Vec<u64>> {
        self.rests
            .borrow()
            .iter()
            .map(|(_, keep)| keep.clone())
            .collect()
    }

    /// A look at a window's commands is answered with nothing from now on.
    pub fn commands_unanswered(&self) {
        self.commands.set(Some(CommandsSeen::Unanswered));
    }

    /// How many looks at a window's commands the engine made.
    pub fn looks(&self) -> usize {
        self.looked.borrow().len()
    }

    /// The rest each look at a window's commands named, in order.
    pub fn looked_since(&self) -> Vec<Option<u64>> {
        self.looked
            .borrow()
            .iter()
            .map(|(_, since, _)| *since)
            .collect()
    }

    /// The time each look at a window's commands named, before which a process began is
    /// no command, in order.
    pub fn looked_after(&self) -> Vec<Option<i64>> {
        self.looked
            .borrow()
            .iter()
            .map(|(_, _, after)| *after)
            .collect()
    }

    /// How many rests the engine asked the host to keep.
    pub fn rests(&self) -> usize {
        self.rests.borrow().len()
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

    fn commands<'a>(
        &'a self,
        pane: &'a Pane,
        since: Option<u64>,
        after: Option<i64>,
    ) -> Work<'a, CommandsSeen> {
        self.looked.borrow_mut().push((pane.clone(), since, after));
        let commands = self.commands.get().unwrap_or(CommandsSeen::Untold);
        Box::pin(async move { commands })
    }

    fn rest<'a>(&'a self, pane: &'a Pane, keep: &'a [u64]) -> Work<'a, Option<u64>> {
        let mut rests = self.rests.borrow_mut();
        rests.push((pane.clone(), keep.to_vec()));
        let number = u64::try_from(rests.len() - 1).ok();
        Box::pin(async move { number })
    }
}
