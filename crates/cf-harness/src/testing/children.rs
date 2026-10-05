//! Programs a test scripts, the twin of the Node recorder's stand-ins and
//! of its scripted `spawn`: a run answered by the program's name and its
//! arguments (`codex --version`), a spawned child scripted by its program's
//! name (the lines it writes, and how it ends), and what was asked of them
//! kept, the same on every platform, as a stand-in sees itself.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::future::poll_fn;
use std::path::Path;
use std::rc::Rc;

use super::Latch;
use crate::contract::Work;
use crate::seams::processes::{Child, Ending, Failed, Limits, Processes, Program, Streams};

/// A program's name as a test names it: its executable's file name, the
/// extension a Windows stand-in has taken off (`codex.cmd` is `codex`).
pub fn name(program: &Program) -> String {
    called(&program.executable)
}

/// What the program at `path` is called: its file's name, the extension a
/// Windows stand-in has taken off.
pub fn called(path: &Path) -> String {
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
    always: RefCell<HashMap<String, Result<String, Failed>>>,
    every: RefCell<HashMap<String, Result<String, Failed>>>,
    children: RefCell<HashMap<String, VecDeque<ChildScript>>>,
    ran: RefCell<Vec<(Program, Limits)>>,
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

    /// Scripts what `named` answers every time it is run, as a stand-in
    /// answers by its arguments (`standIn`): once the answers scripted for
    /// one run each are used up.
    pub fn always_answer(&self, named: &str, answer: Result<String, Failed>) {
        self.always.borrow_mut().insert(named.to_owned(), answer);
    }

    /// Scripts what every run of the executable `name` answers whatever it
    /// is asked, as a stand-in that says one thing (`standIn`'s `*`): when
    /// no answer to the exact arguments is scripted.
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
    /// as it was given, with its limits.
    pub fn take_ran(&self) -> Vec<(Program, Limits)> {
        std::mem::take(&mut self.ran.borrow_mut())
    }

    /// Every child asked for since the last time they were taken, started
    /// or not, as it was given, with its streams.
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
    fn run(&self, program: Program, limits: Limits) -> Work<'_, Result<String, Failed>> {
        let (named, name) = (named(&program), name(&program));
        self.ran.borrow_mut().push((program, limits));
        let answer = self
            .runs
            .borrow_mut()
            .get_mut(&named)
            .and_then(VecDeque::pop_front)
            .or_else(|| self.always.borrow().get(&named).cloned())
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
        let (named, name) = (named(&program), name(&program));
        // Written down whether it starts or not, as Node's recorder writes
        // down every `spawn`.
        self.spawned.borrow_mut().push((program, streams));
        let script = self
            .children
            .borrow_mut()
            .get_mut(&name)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| missing(&named))?;
        // One that ends by itself and says nothing has ended once started.
        let ended = Latch::default();
        if script.ends == Ends::Itself && script.lines.is_empty() {
            ended.open();
        }
        Ok(Box::new(ScriptedChild {
            lines: RefCell::new(script.lines.into()),
            ends: script.ends,
            written: Rc::clone(&self.written),
            ended,
        }))
    }
}

/// A child whose lines out are scripted, ending as its script says.
struct ScriptedChild {
    lines: RefCell<VecDeque<String>>,
    ends: Ends,
    written: Rc<RefCell<Vec<String>>>,
    ended: Latch,
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
            self.ended.open();
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
        self.ended.is_open()
    }

    fn closed(&self) -> Work<'_, ()> {
        Box::pin(poll_fn(|context| self.ended.poll_open(context)))
    }

    fn terminate(&self, how: Ending) {
        let ends = match self.ends {
            _ if cfg!(windows) => true,
            Ends::Itself | Ends::Asked => true,
            Ends::Forced => how == Ending::Forced,
            Ends::Never => false,
        };
        if ends {
            self.ended.open();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use cf_base::env::Env;

    use super::*;
    use crate::testing::finished;

    fn program(executable: &str, args: &[&str]) -> Program {
        Program {
            executable: PathBuf::from(executable),
            args: args.iter().map(|&arg| arg.to_owned()).collect(),
            cwd: None,
            env: Env::default(),
        }
    }

    const LIMITS: Limits = Limits {
        timeout: Duration::from_secs(1),
        max_buffer: 1024,
    };

    #[test]
    fn a_stand_in_answers_by_its_arguments_every_time_after_its_answers_for_one_run() {
        let scripted = ScriptedProcesses::default();
        scripted.run_answer("codex --version", Ok("once".to_owned()));
        scripted.always_answer("codex --version", Ok("always".to_owned()));
        scripted.every_answer("codex", Ok("anything".to_owned()));
        let ask = |args: &[&str]| finished(scripted.run(program("/bin/codex.cmd", args), LIMITS));
        assert_eq!(ask(&["--version"]).as_deref(), Ok("once"));
        assert_eq!(ask(&["--version"]).as_deref(), Ok("always"));
        assert_eq!(ask(&["--version"]).as_deref(), Ok("always"));
        assert_eq!(ask(&["mcp", "list"]).as_deref(), Ok("anything"));
        let ran: Vec<(String, Limits)> = scripted
            .take_ran()
            .iter()
            .map(|(program, limits)| (named(program), *limits))
            .collect();
        assert_eq!(ran.len(), 4);
        assert_eq!(ran[3], ("codex mcp list".to_owned(), LIMITS));
    }

    #[test]
    fn a_child_ended_when_asked_wakes_the_work_waiting_for_it_to_close() {
        let scripted = ScriptedProcesses::default();
        scripted.child(
            "opencode",
            ChildScript {
                lines: Vec::new(),
                ends: Ends::Asked,
            },
        );
        let child = scripted
            .spawn(program("/bin/opencode", &["serve"]), Streams::Quiet)
            .unwrap();
        crate::testing::woken_by(child.closed(), || child.terminate(Ending::Asked));
        assert!(child.exited());
    }

    #[test]
    fn a_child_asked_for_is_written_down_whether_it_starts_or_not() {
        let scripted = ScriptedProcesses::default();
        scripted.child(
            "opencode",
            ChildScript {
                lines: Vec::new(),
                ends: Ends::Asked,
            },
        );
        assert!(scripted
            .spawn(program("/bin/opencode", &["serve"]), Streams::Quiet)
            .is_ok());
        assert_eq!(
            scripted
                .spawn(program("/bin/opencode", &["serve"]), Streams::Quiet)
                .err()
                .as_deref(),
            Some("spawn opencode serve ENOENT"),
            "no child is left scripted"
        );
        let spawned: Vec<String> = scripted
            .take_spawned()
            .iter()
            .map(|(program, _)| named(program))
            .collect();
        assert_eq!(spawned, ["opencode serve", "opencode serve"]);
    }
}
