//! A machine of the tests' own, for the checks that find and end what a window runs: the
//! processes it lists, which process has a number now, and what holds a process to end
//! it, as a test says. It ends none, and keeps the numbers it was asked to end.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use super::{Held, Holds, Identity, Link, Machine, Running};
use crate::Result;

/// What a number is now, where a test says it is not the process listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Now {
    /// No process has it.
    Nobody,
    /// The process that has it began then.
    Began(u64),
    /// It cannot be reached (another user's, say).
    Unreachable,
    /// When it began cannot be read.
    Unreadable,
    /// It is the process listed, and its end fails.
    Unending,
}

/// The machine: what it lists, what some numbers are now, and what it was asked to end.
pub(crate) struct Listed {
    listed: Vec<Running>,
    now: Vec<(u32, Now)>,
    uniques: Vec<(u32, u64)>,
    unreadable: Vec<u32>,
    ended: Rc<RefCell<Vec<u32>>>,
}

impl Listed {
    /// A machine that lists `listed`, each number its process's.
    pub(crate) fn new(listed: Vec<Running>) -> Self {
        Self {
            listed,
            now: Vec::new(),
            uniques: Vec::new(),
            unreadable: Vec::new(),
            ended: Rc::default(),
        }
    }

    /// The machine, the number `pid` being as `now` says.
    pub(crate) fn with(mut self, pid: u32, now: Now) -> Self {
        self.now.push((pid, now));
        self
    }

    /// The machine, the process with the number `pid` having the unique identifier `unique` now.
    pub(crate) fn with_unique(mut self, pid: u32, unique: u64) -> Self {
        self.uniques.push((pid, unique));
        self
    }

    /// The machine, the identifier of the process with the number `pid` unreadable now.
    pub(crate) fn without_unique(mut self, pid: u32) -> Self {
        self.unreadable.push(pid);
        self
    }

    /// The unique identifier the number has now: the one a test says, else the listed row's.
    fn unique_of(&self, pid: u32) -> Option<u64> {
        if self.unreadable.contains(&pid) {
            return None;
        }
        self.uniques
            .iter()
            .find(|(number, _)| *number == pid)
            .map(|(_, unique)| *unique)
            .or_else(|| {
                self.listed
                    .iter()
                    .find(|process| process.identity.pid == pid)
                    .and_then(|process| match process.link {
                        Link::Forked { own, .. } => Some(own),
                        _ => None,
                    })
            })
    }

    /// The numbers it was asked to end, in order.
    pub(crate) fn ended(&self) -> Vec<u32> {
        self.ended.borrow().clone()
    }

    fn now_of(&self, pid: u32) -> Option<Now> {
        self.now
            .iter()
            .find(|(number, _)| *number == pid)
            .map(|(_, now)| *now)
    }

    fn listed_of(&self, pid: u32) -> Option<Identity> {
        self.listed
            .iter()
            .map(|process| process.identity)
            .find(|identity| identity.pid == pid)
    }
}

impl Machine for Listed {
    fn processes(&self) -> Result<Vec<Running>> {
        Ok(self.listed.clone())
    }

    fn identity(&self, pid: u32) -> Result<Option<Identity>> {
        Ok(match self.now_of(pid) {
            Some(Now::Began(started)) => Some(Identity { pid, started }),
            Some(Now::Nobody) => None,
            _ => self.listed_of(pid),
        })
    }
}

impl Holds for Listed {
    fn hold(&self, pid: u32) -> io::Result<Option<Box<dyn Held>>> {
        let listed = self.listed_of(pid).map(|identity| identity.started);
        match (self.now_of(pid), listed) {
            (Some(Now::Unreachable), _) => Err(io::Error::other("access denied")),
            (Some(Now::Nobody), _) | (None, None) => Ok(None),
            (now, listed) => Ok(Some(Box::new(Fake {
                pid,
                now,
                listed,
                unique: self.unique_of(pid),
                ended: Rc::clone(&self.ended),
            }))),
        }
    }
}

/// A process the machine holds.
struct Fake {
    pid: u32,
    now: Option<Now>,
    listed: Option<u64>,
    unique: Option<u64>,
    ended: Rc<RefCell<Vec<u32>>>,
}

impl Held for Fake {
    fn started(&self) -> io::Result<Option<u64>> {
        match self.now {
            Some(Now::Began(started)) => Ok(Some(started)),
            Some(Now::Unreadable) => Err(io::Error::other("unreadable")),
            _ => Ok(self.listed),
        }
    }

    fn unique(&self) -> io::Result<Option<u64>> {
        Ok(self.unique)
    }

    fn end(&self) -> io::Result<()> {
        if self.now == Some(Now::Unending) {
            return Err(io::Error::other("refused"));
        }
        self.ended.borrow_mut().push(self.pid);
        Ok(())
    }

    fn pause(&self, _pause: super::Pause) -> io::Result<()> {
        if self.now == Some(Now::Unending) {
            return Err(io::Error::other("refused"));
        }
        self.ended.borrow_mut().push(self.pid);
        Ok(())
    }
}
