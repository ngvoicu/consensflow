//! Programs a test scripts, the twin of the Node recorder's stand-in CLIs:
//! a probe and a run answered by the program's name and its arguments, a
//! spawned child whose lines out are scripted, and every program asked for
//! kept, as its stand-in writes down how it was run.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::rc::Rc;

use cf_base::env::Env;

use crate::contract::Work;
use crate::seams::processes::{
    Child, Ending, Failed, Limits, Probed, Processes, Program, Streams, Unanswered,
};

/// A program's name and arguments, as a test scripts an answer to them:
/// `codex --version`.
fn asked(name: &Path, args: &[String]) -> String {
    let name = name
        .file_name()
        .map_or_else(|| name.to_string_lossy(), |name| name.to_string_lossy());
    std::iter::once(name.into_owned())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Programs a test scripts. One asked for with no answer left fails to
/// start, as a stand-in that is not there.
#[derive(Default)]
pub struct ScriptedProcesses {
    probes: RefCell<HashMap<String, VecDeque<Result<Probed, Unanswered>>>>,
    runs: RefCell<HashMap<String, VecDeque<Result<String, Failed>>>>,
    children: RefCell<HashMap<String, VecDeque<Vec<String>>>>,
    ran: RefCell<Vec<String>>,
    written: Rc<RefCell<Vec<String>>>,
}

impl ScriptedProcesses {
    /// Scripts what `asked` (`codex --version`) answers when probed.
    pub fn probe_answer(&self, asked: &str, answer: Result<Probed, Unanswered>) {
        self.probes
            .borrow_mut()
            .entry(asked.to_owned())
            .or_default()
            .push_back(answer);
    }

    /// Scripts what `asked` answers when run to its end.
    pub fn run_answer(&self, asked: &str, answer: Result<String, Failed>) {
        self.runs
            .borrow_mut()
            .entry(asked.to_owned())
            .or_default()
            .push_back(answer);
    }

    /// Scripts the lines a child started as `asked` writes, in order, its
    /// output ending after them.
    pub fn child_lines(&self, asked: &str, lines: impl IntoIterator<Item = String>) {
        self.children
            .borrow_mut()
            .entry(asked.to_owned())
            .or_default()
            .push_back(lines.into_iter().collect());
    }

    /// Every program asked for since the last time they were taken, as
    /// `asked` writes it.
    pub fn take_ran(&self) -> Vec<String> {
        std::mem::take(&mut self.ran.borrow_mut())
    }

    /// Every line written to a child since the last time they were taken.
    pub fn take_written(&self) -> Vec<String> {
        std::mem::take(&mut self.written.borrow_mut())
    }
}

/// The next answer scripted for `asked`, if one is left.
fn next<T>(answers: &RefCell<HashMap<String, VecDeque<T>>>, asked: &str) -> Option<T> {
    answers
        .borrow_mut()
        .get_mut(asked)
        .and_then(VecDeque::pop_front)
}

/// What a stand-in that is not there says when started.
fn missing(asked: &str) -> String {
    format!("spawn {asked} ENOENT")
}

impl Processes for ScriptedProcesses {
    fn probe<'a>(
        &'a self,
        executable: &'a Path,
        args: &'a [&'a str],
        _env: &'a Env,
    ) -> Work<'a, Result<Probed, Unanswered>> {
        let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
        let asked = asked(executable, &args);
        self.ran.borrow_mut().push(asked.clone());
        let answer = next(&self.probes, &asked).unwrap_or_else(|| {
            Err(Unanswered::Failed(Failed {
                message: missing(&asked),
                code: None,
                killed: false,
                stdout: String::new(),
            }))
        });
        Box::pin(async move { answer })
    }

    fn run(&self, program: Program, _limits: Limits) -> Work<'_, Result<String, Failed>> {
        let args: Vec<String> = program
            .run
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let asked = asked(&program.run.program, &args);
        self.ran.borrow_mut().push(asked.clone());
        let answer = next(&self.runs, &asked).unwrap_or_else(|| {
            Err(Failed {
                message: missing(&asked),
                code: None,
                killed: false,
                stdout: String::new(),
            })
        });
        Box::pin(async move { answer })
    }

    fn spawn(&self, program: Program, _streams: Streams) -> Result<Box<dyn Child>, String> {
        let args: Vec<String> = program
            .run
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let asked = asked(&program.run.program, &args);
        self.ran.borrow_mut().push(asked.clone());
        let lines = next(&self.children, &asked).ok_or_else(|| missing(&asked))?;
        Ok(Box::new(ScriptedChild {
            lines: lines.into(),
            written: Rc::clone(&self.written),
            ended: false,
        }))
    }
}

/// A child whose lines out are scripted.
struct ScriptedChild {
    lines: VecDeque<String>,
    written: Rc<RefCell<Vec<String>>>,
    ended: bool,
}

impl Child for ScriptedChild {
    fn write_line<'a>(&'a mut self, line: &'a str) -> Work<'a, Result<(), String>> {
        self.written.borrow_mut().push(line.to_owned());
        Box::pin(async { Ok(()) })
    }

    fn read_line(&mut self, limit: usize) -> Work<'_, Result<Option<String>, String>> {
        let line = self.lines.pop_front();
        Box::pin(async move {
            match line {
                Some(line) if line.len() > limit => {
                    Err(format!("a line of more than {limit} bytes"))
                }
                line => Ok(line),
            }
        })
    }

    fn exited(&mut self) -> bool {
        self.ended
    }

    fn closed(&mut self) -> Work<'_, ()> {
        self.ended = true;
        Box::pin(async {})
    }

    fn terminate(&mut self, _how: Ending) {
        self.ended = true;
    }
}
