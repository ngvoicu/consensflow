//! Programs a test scripts, the twin of the Node recorder's stand-ins and
//! of its scripted `spawn`: a run answered by the program's name and its
//! arguments (`codex --version`), a spawned child scripted by its program's
//! name (the lines it writes, and how it ends), and what was asked of them
//! kept, the same on every platform, as a stand-in sees itself.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::future::poll_fn;
use std::rc::Rc;
use std::task::Poll;

use crate::contract::Work;
use crate::seams::processes::{Child, Ending, Failed, Limits, Processes, Program, Streams};

/// A program's name as a test names it: its executable's file name, the
/// extension a Windows stand-in has taken off (`codex.cmd` is `codex`).
pub fn name(program: &Program) -> String {
    let path = &program.executable;
    let script = path.extension().is_some_and(|extension| {
        ["cmd", "bat", "exe", "mjs"]
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    });
    let name = if script {
        path.file_stem()
    } else {
        path.file_name()
    };
    name.map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
        .into_owned()
}

/// A program as a test names it: its name, then its arguments, a space
/// between each (`codex --version`).
pub fn named(program: &Program) -> String {
    std::iter::once(name(program))
        .chain(program.args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// How a scripted child ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ends {
    /// By itself, once its output is read to its end, or at once when it
    /// writes nothing.
    Itself,
    /// When it is asked to end, or forced.
    Asked,
    /// Only when it is forced; asked, it goes on.
    Forced,
    /// Never: it goes on asked or forced (on Windows, where an end is
    /// always forced, it ends).
    Never,
}

/// A child a test scripts: the lines it writes, and how it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildScript {
    pub lines: Vec<String>,
    pub ends: Ends,
}

/// Programs a test scripts. One asked for with no answer left does not
/// start, as a stand-in that is not there.
#[derive(Default)]
pub struct ScriptedProcesses {
    runs: RefCell<HashMap<String, VecDeque<Result<String, Failed>>>>,
    every: RefCell<HashMap<String, Result<String, Failed>>>,
    children: RefCell<HashMap<String, VecDeque<ChildScript>>>,
    ran: RefCell<Vec<Program>>,
    spawned: RefCell<Vec<(Program, Streams)>>,
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

    /// Scripts what every run of the executable `name` answers whatever it
    /// is asked, as a stand-in that says one thing (`fakeExecutable`'s
    /// `output`): when no answer to the exact arguments is left.
    pub fn every_answer(&self, name: &str, answer: Result<String, Failed>) {
        self.every.borrow_mut().insert(name.to_owned(), answer);
    }

    /// Scripts the next child started of the executable `name`.
    pub fn child(&self, name: &str, script: ChildScript) {
        self.children
            .borrow_mut()
            .entry(name.to_owned())
            .or_default()
            .push_back(script);
    }

    /// Every program run to its end since the last time they were taken,
    /// as it was given.
    pub fn take_ran(&self) -> Vec<Program> {
        std::mem::take(&mut self.ran.borrow_mut())
    }

    /// Every child started since the last time they were taken, as it was
    /// given, with its streams.
    pub fn take_spawned(&self) -> Vec<(Program, Streams)> {
        std::mem::take(&mut self.spawned.borrow_mut())
    }

    /// Every line written to a child since the last time they were taken.
    pub fn take_written(&self) -> Vec<String> {
        std::mem::take(&mut self.written.borrow_mut())
    }

    /// The programs with children scripted and not yet started.
    pub fn unused(&self) -> Vec<String> {
        let mut unused: Vec<String> = self
            .children
            .borrow()
            .iter()
            .filter(|(_, left)| !left.is_empty())
            .map(|(name, _)| name.clone())
            .collect();
        unused.sort();
        unused
    }
}

/// What a stand-in that is not there says when started.
fn missing(named: &str) -> String {
    format!("spawn {named} ENOENT")
}

impl Processes for ScriptedProcesses {
    fn run(&self, program: Program, _limits: Limits) -> Work<'_, Result<String, Failed>> {
        let (named, name) = (named(&program), name(&program));
        self.ran.borrow_mut().push(program);
        let answer = self
            .runs
            .borrow_mut()
            .get_mut(&named)
            .and_then(VecDeque::pop_front)
            .or_else(|| self.every.borrow().get(&name).cloned())
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

    fn spawn(&self, program: Program, streams: Streams) -> Result<Box<dyn Child>, String> {
        let named = named(&program);
        let script = self
            .children
            .borrow_mut()
            .get_mut(&name(&program))
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| missing(&named))?;
        self.spawned.borrow_mut().push((program, streams));
        // One that ends by itself and says nothing has ended once started.
        let ended = script.ends == Ends::Itself && script.lines.is_empty();
        Ok(Box::new(ScriptedChild {
            lines: RefCell::new(script.lines.into()),
            ends: script.ends,
            written: Rc::clone(&self.written),
            ended: Cell::new(ended),
        }))
    }
}

/// A child whose lines out are scripted, ending as its script says.
struct ScriptedChild {
    lines: RefCell<VecDeque<String>>,
    ends: Ends,
    written: Rc<RefCell<Vec<String>>>,
    ended: Cell<bool>,
}

impl Child for ScriptedChild {
    fn write_line<'a>(&'a self, line: &'a str) -> Work<'a, Result<(), String>> {
        self.written.borrow_mut().push(line.to_owned());
        Box::pin(async { Ok(()) })
    }

    /// Its next line; past its last, the end of its output once it has
    /// ended, as a live child's output stays open.
    fn read_line(&self, limit: usize) -> Work<'_, Result<Option<String>, String>> {
        let line = self.lines.borrow_mut().pop_front();
        if line.is_none() && self.ends == Ends::Itself {
            self.ended.set(true);
        }
        Box::pin(async move {
            match line {
                Some(line) if line.len() > limit => {
                    Err(format!("a line of more than {limit} bytes"))
                }
                Some(line) => Ok(Some(line)),
                None => {
                    self.closed().await;
                    Ok(None)
                }
            }
        })
    }

    fn exited(&self) -> bool {
        self.ended.get()
    }

    fn closed(&self) -> Work<'_, ()> {
        Box::pin(poll_fn(|_| {
            if self.ended.get() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
    }

    fn terminate(&self, how: Ending) {
        let ends = match self.ends {
            _ if cfg!(windows) => true,
            Ends::Itself | Ends::Asked => true,
            Ends::Forced => how == Ending::Forced,
            Ends::Never => false,
        };
        if ends {
            self.ended.set(true);
        }
    }
}
