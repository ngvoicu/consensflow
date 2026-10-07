//! What the tests of the Codex modules watch of the programs they start: what
//! each child was given (its folder, its environment, its streams), and
//! whether it was ended and how.

use std::cell::RefCell;
use std::rc::Rc;

use crate::contract::Work;
use crate::seams::processes::{Child, Ending, Failed, Limits, Processes, Program, Streams};
use crate::testing::ScriptedProcesses;

/// Scripted programs that keep a record of how their children were started
/// and ended.
#[derive(Default)]
pub(super) struct Watching {
    pub(super) scripted: ScriptedProcesses,
    pub(super) spawned: RefCell<Vec<(Program, Streams)>>,
    pub(super) ended: Rc<RefCell<Vec<Ending>>>,
}

impl Processes for Watching {
    fn run(&self, program: Program, limits: Limits) -> Work<'_, Result<String, Failed>> {
        self.scripted.run(program, limits)
    }

    fn spawn(&self, program: Program, streams: Streams) -> Result<Box<dyn Child>, String> {
        self.spawned.borrow_mut().push((program.clone(), streams));
        let child = self.scripted.spawn(program, streams)?;
        Ok(Box::new(Watched {
            child,
            ended: Rc::clone(&self.ended),
        }))
    }
}

/// A child that says how it was ended.
struct Watched {
    child: Box<dyn Child>,
    ended: Rc<RefCell<Vec<Ending>>>,
}

impl Child for Watched {
    fn write_line<'a>(&'a self, line: &'a str) -> Work<'a, Result<(), String>> {
        self.child.write_line(line)
    }

    fn read_line(&self, limit: usize) -> Work<'_, Result<Option<String>, String>> {
        self.child.read_line(limit)
    }

    fn exited(&self) -> bool {
        self.child.exited()
    }

    fn closed(&self) -> Work<'_, ()> {
        self.child.closed()
    }

    fn terminate(&self, how: Ending) {
        self.ended.borrow_mut().push(how);
        self.child.terminate(how);
    }
}
