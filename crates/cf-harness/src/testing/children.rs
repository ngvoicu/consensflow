//! Programs a test scripts, the twin of the Node recorder's stand-ins: a
//! run answered by the program's name and its arguments (`codex
//! --version`), a spawned child whose lines out are scripted, and every
//! program asked for kept as its name and arguments, the same on every
//! platform, as a stand-in sees itself.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::contract::Work;
use crate::seams::processes::{Child, Ending, Failed, Limits, Processes, Program, Streams};

/// A program as a test names it: its executable's name, then its
/// arguments, a space between each.
fn named(program: &Program) -> String {
    let name = program.executable.file_name().map_or_else(
        || program.executable.to_string_lossy(),
        |name| name.to_string_lossy(),
    );
    std::iter::once(name.into_owned())
        .chain(program.args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Programs a test scripts. One asked for with no answer left does not
/// start, as a stand-in that is not there.
#[derive(Default)]
pub struct ScriptedProcesses {
    runs: RefCell<HashMap<String, VecDeque<Result<String, Failed>>>>,
    children: RefCell<HashMap<String, VecDeque<Vec<String>>>>,
    ran: RefCell<Vec<String>>,
    written: Rc<RefCell<Vec<String>>>,
}

impl ScriptedProcesses {
    /// Scripts what `named` (`codex --version`) answers when run.
    pub fn run_answer(&self, named: &str, answer: Result<String, Failed>) {
        self.runs
            .borrow_mut()
            .entry(named.to_owned())
            .or_default()
            .push_back(answer);
    }

    /// Scripts the lines a child started as `named` writes, in order, its
    /// output ending after them.
    pub fn child_lines(&self, named: &str, lines: impl IntoIterator<Item = String>) {
        self.children
            .borrow_mut()
            .entry(named.to_owned())
            .or_default()
            .push_back(lines.into_iter().collect());
    }

    /// Every program asked for since the last time they were taken.
    pub fn take_ran(&self) -> Vec<String> {
        std::mem::take(&mut self.ran.borrow_mut())
    }

    /// Every line written to a child since the last time they were taken.
    pub fn take_written(&self) -> Vec<String> {
        std::mem::take(&mut self.written.borrow_mut())
    }
}

/// What a stand-in that is not there says when started.
fn missing(named: &str) -> String {
    format!("spawn {named} ENOENT")
}

impl Processes for ScriptedProcesses {
    fn run(&self, program: Program, _limits: Limits) -> Work<'_, Result<String, Failed>> {
        let named = named(&program);
        self.ran.borrow_mut().push(named.clone());
        let answer = self
            .runs
            .borrow_mut()
            .get_mut(&named)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| {
                Err(Failed {
                    message: missing(&named),
                    code: None,
                    killed: false,
                    stdout: String::new(),
                })
            });
        Box::pin(async move { answer })
    }

    fn spawn(&self, program: Program, _streams: Streams) -> Result<Box<dyn Child>, String> {
        let named = named(&program);
        self.ran.borrow_mut().push(named.clone());
        let lines = self
            .children
            .borrow_mut()
            .get_mut(&named)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| missing(&named))?;
        Ok(Box::new(ScriptedChild {
            lines: RefCell::new(lines.into()),
            written: Rc::clone(&self.written),
            ended: Cell::new(false),
        }))
    }
}

/// A child whose lines out are scripted, and which has ended once asked
/// to or once its output is read to its end.
struct ScriptedChild {
    lines: RefCell<VecDeque<String>>,
    written: Rc<RefCell<Vec<String>>>,
    ended: Cell<bool>,
}

impl Child for ScriptedChild {
    fn write_line<'a>(&'a self, line: &'a str) -> Work<'a, Result<(), String>> {
        self.written.borrow_mut().push(line.to_owned());
        Box::pin(async { Ok(()) })
    }

    fn read_line(&self, limit: usize) -> Work<'_, Result<Option<String>, String>> {
        let line = self.lines.borrow_mut().pop_front();
        if line.is_none() {
            self.ended.set(true);
        }
        Box::pin(async move {
            match line {
                Some(line) if line.len() > limit => {
                    Err(format!("a line of more than {limit} bytes"))
                }
                line => Ok(line),
            }
        })
    }

    fn exited(&self) -> bool {
        self.ended.get()
    }

    fn closed(&self) -> Work<'_, ()> {
        self.ended.set(true);
        Box::pin(async {})
    }

    fn terminate(&self, _how: Ending) {
        self.ended.set(true);
    }
}
