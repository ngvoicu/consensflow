//! Where an adapter runs a program: to its end (`run`, as `execFile`), or
//! beside it a line at a time (`spawn`). What a CLI answers to its version
//! or its help (`probe`, `probeExecutable`) is asked through `run`, once per
//! executable as it is on disk, the answers kept in [`Probes`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::file::{stat, FileError, Identity};
use cf_process::{runnable, with_required, Ender};
pub use cf_process::{Ending, Failed, Limits, Streams};
use futures_util::future::{FutureExt, LocalBoxFuture, Shared};

use crate::contract::Work;

/// A program and all it is given: its executable as the adapter found it
/// and its arguments (`runnable` makes of them what starts here), its
/// folder, and its whole environment. Nothing is inherited but what
/// Windows needs, as libuv gives it.
#[derive(Debug, Clone)]
pub struct Program {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Env,
}

/// Where an adapter's programs run.
pub trait Processes {
    /// `execFile`: its output, or how it failed, an exit other than 0
    /// among the failures, with its code.
    fn run(&self, program: Program, limits: Limits) -> Work<'_, Result<String, Failed>>;

    /// `spawn`: the child, or why it did not start.
    fn spawn(&self, program: Program, streams: Streams) -> Result<Box<dyn Child>, String>;
}

/// A program running beside the adapter. Each call takes it shared: a
/// stop forces it while its close is awaited.
pub trait Child {
    /// Writes `line` and a newline to its input, waiting while its pipe is
    /// full.
    fn write_line<'a>(&'a self, line: &'a str) -> Work<'a, Result<(), String>>;
    /// The next line of its output: none once it ended (what followed the
    /// last newline is no line), a failure past `limit` bytes without one.
    fn read_line(&self, limit: usize) -> Work<'_, Result<Option<String>, String>>;
    /// Whether it has exited (Node's `exitCode`).
    fn exited(&self) -> bool;
    /// Waits until it has exited and its streams have closed (`close`).
    fn closed(&self) -> Work<'_, ()>;
    /// Asks or forces it to end, not waiting (Windows: its whole tree).
    fn terminate(&self, how: Ending);
}

/// The system's programs, started as this process's children: on Windows
/// given what Windows needs of this process's environment where theirs
/// lacks it, and in a job that ends them with this process.
pub struct SystemProcesses {
    this: Env,
    started: RefCell<Vec<Ender>>,
}

impl SystemProcesses {
    /// Programs started from a process whose environment is `this`.
    pub fn new(this: Env) -> Self {
        Self {
            this,
            started: RefCell::new(Vec::new()),
        }
    }

    /// Forces every child still running to end: what this process does on
    /// its way out, as Node's exit hook did for OpenCode's server.
    pub fn end_all(&self) {
        for ender in self.started.borrow_mut().drain(..) {
            ender.force();
        }
    }

    fn start(&self, program: &Program) -> (cf_process::Run, Env) {
        let env = with_required(&program.env, &self.this);
        let args: Vec<OsString> = program.args.iter().map(OsString::from).collect();
        (runnable(&program.executable, &args, &env), env)
    }
}

impl Processes for SystemProcesses {
    fn run(&self, program: Program, limits: Limits) -> Work<'_, Result<String, Failed>> {
        let (run, env) = self.start(&program);
        Box::pin(
            async move { cf_process::execute(&run, program.cwd.as_deref(), &env, limits).await },
        )
    }

    fn spawn(&self, program: Program, streams: Streams) -> Result<Box<dyn Child>, String> {
        let (run, env) = self.start(&program);
        let child = cf_process::spawn(&run, program.cwd.as_deref(), &env, streams)?;
        let mut started = self.started.borrow_mut();
        started.retain(Ender::running);
        started.push(child.ender());
        Ok(Box::new(child))
    }
}

impl Child for cf_process::Child {
    fn write_line<'a>(&'a self, line: &'a str) -> Work<'a, Result<(), String>> {
        Box::pin(cf_process::Child::write_line(self, line))
    }

    fn read_line(&self, limit: usize) -> Work<'_, Result<Option<String>, String>> {
        Box::pin(cf_process::Child::read_line(self, limit))
    }

    fn exited(&self) -> bool {
        cf_process::Child::exited(self)
    }

    fn closed(&self) -> Work<'_, ()> {
        Box::pin(cf_process::Child::closed(self))
    }

    fn terminate(&self, how: Ending) {
        cf_process::Child::terminate(self, how);
    }
}

/// What a CLI answered: its output and its exit code, any exit an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    pub stdout: String,
    pub code: i32,
}

/// Why a CLI could not be asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unanswered {
    /// Its file could not be looked at, before any run: Node's sentence,
    /// which JavaScript threw where it asked (`realpathSync`, `statSync`).
    Unread(String),
    /// It ran and did not answer.
    Failed(Failed),
}

/// How long a probe may run and how much it may say (`probeExecutable`'s
/// 30 s and 128 KiB).
const PROBE: Limits = Limits {
    timeout: Duration::from_secs(30),
    max_buffer: 128 * 1024,
};

/// A probe's answer, shared by every caller that asked while it ran.
type Asked = Shared<LocalBoxFuture<'static, Result<Probed, Failed>>>;

/// The probes asked so far, one map for the engine, as Node's was one per
/// process (`src/harnesses.js`).
#[derive(Default)]
pub struct Probes {
    asked: RefCell<HashMap<Key, Asked>>,
}

/// Which executable a probe asked, as it is on disk, and what.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    file: PathBuf,
    identity: Identity,
    size: u64,
    modified: u64,
    changed: Option<(i64, i64)>,
    args: Vec<String>,
}

/// `probeExecutable(executable, args, env)`: what the CLI answers, its
/// output and exit code. It is asked once per executable as it is on disk
/// and per `args`: an update gives the file another identity, so an updated
/// CLI is asked again and an unchanged one never twice, and two callers
/// asking while it runs share the run. A run that did not answer (could
/// not start, out of time, too much said, ended by a signal) is not kept.
pub async fn probe(
    probes: &Probes,
    processes: &Rc<dyn Processes>,
    executable: &Path,
    args: &[&str],
    env: &Env,
) -> Result<Probed, Unanswered> {
    let key = key(executable, args).map_err(Unanswered::Unread)?;
    let asked = probes
        .asked
        .borrow_mut()
        .entry(key.clone())
        .or_insert_with(|| ask(processes, executable, args, env))
        .clone();
    let answer = asked.clone().await;
    if answer.is_err() {
        let mut kept = probes.asked.borrow_mut();
        if kept.get(&key).is_some_and(|stored| stored.ptr_eq(&asked)) {
            kept.remove(&key);
        }
    }
    answer.map_err(Unanswered::Failed)
}

/// The executable as it is on disk: its real path, which file it is, its
/// size, when it was written, when it was changed (on Unix; Windows' change
/// time is not read here), and what it is asked. A path that cannot be
/// resolved says `realpath` in its sentence, where Node's walk names the
/// `lstat` of its first part missing (a difference kept: the CLI was found
/// a moment before).
fn key(executable: &Path, args: &[&str]) -> Result<Key, String> {
    let file = std::fs::canonicalize(executable)
        .map_err(|failed| FileError::call(failed, "realpath", Some(executable)).to_string())?;
    let found =
        stat(&file).map_err(|failed| FileError::call(failed, "stat", Some(&file)).to_string())?;
    Ok(Key {
        changed: changed(&file),
        identity: found.identity,
        size: found.size,
        modified: found.mtime_ms.to_bits(),
        args: args.iter().map(|&arg| arg.to_owned()).collect(),
        file,
    })
}

/// When the file was last changed, in seconds and nanoseconds.
#[cfg(unix)]
fn changed(file: &Path) -> Option<(i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(file)
        .ok()
        .map(|found| (found.ctime(), found.ctime_nsec()))
}

/// Windows' change time is not read here: an update writes the file anew.
#[cfg(not(unix))]
fn changed(_file: &Path) -> Option<(i64, i64)> {
    None
}

/// A run of `executable` with `args` to be shared: an exit with a code is
/// an answer, its output with it; any other failure is not.
fn ask(processes: &Rc<dyn Processes>, executable: &Path, args: &[&str], env: &Env) -> Asked {
    let processes = Rc::clone(processes);
    let program = Program {
        executable: executable.to_path_buf(),
        args: args.iter().map(|&arg| arg.to_owned()).collect(),
        cwd: None,
        env: env.clone(),
    };
    async move {
        match processes.run(program, PROBE).await {
            Ok(stdout) => Ok(Probed { stdout, code: 0 }),
            Err(Failed {
                code: Some(code),
                stdout,
                ..
            }) => Ok(Probed { stdout, code }),
            Err(failed) => Err(failed),
        }
    }
    .boxed_local()
    .shared()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Driver, ScriptedProcesses};

    fn stand_in(dir: &Path, name: &str) -> PathBuf {
        let cli = dir.join(name);
        std::fs::write(&cli, "a stand-in").unwrap();
        cli
    }

    #[test]
    fn two_callers_share_one_run_and_the_answer_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let cli = stand_in(dir.path(), "codex");
        let scripted = Rc::new(ScriptedProcesses::default());
        scripted.run_answer("codex --version", Ok("codex 0.159.2".to_owned()));
        let processes: Rc<dyn Processes> = Rc::clone(&scripted) as Rc<dyn Processes>;
        let probes = Rc::new(Probes::default());
        let mut driver = Driver::default();
        for id in 0..3 {
            let (probes, processes, cli) = (Rc::clone(&probes), Rc::clone(&processes), cli.clone());
            driver.begin(id, async move {
                probe(&probes, &processes, &cli, &["--version"], &Env::default())
                    .await
                    .map(|probed| probed.stdout)
            });
        }
        let answer = Ok("codex 0.159.2".to_owned());
        assert_eq!(
            driver.run(),
            [(0, answer.clone()), (1, answer.clone()), (2, answer)]
        );
        assert_eq!(scripted.take_ran(), ["codex --version"], "asked once");
    }

    #[test]
    fn an_exit_with_a_code_is_an_answer_and_a_failure_is_asked_again() {
        let dir = tempfile::tempdir().unwrap();
        let cli = stand_in(dir.path(), "devin");
        let scripted = Rc::new(ScriptedProcesses::default());
        let failed = |code| Failed {
            message: "Command failed: devin --version\n".to_owned(),
            code,
            killed: code.is_none(),
            stdout: "usage".to_owned(),
        };
        scripted.run_answer("devin --version", Err(failed(None)));
        scripted.run_answer("devin --version", Err(failed(Some(2))));
        let processes: Rc<dyn Processes> = Rc::clone(&scripted) as Rc<dyn Processes>;
        let probes = Rc::new(Probes::default());
        let mut driver = Driver::default();
        let asked = Probed {
            stdout: "usage".to_owned(),
            code: 2,
        };
        for (id, expected) in [
            (0, Err(Unanswered::Failed(failed(None)))),
            (1, Ok(asked.clone())),
            (2, Ok(asked)),
        ] {
            let (probes, processes, cli) = (Rc::clone(&probes), Rc::clone(&processes), cli.clone());
            driver.begin(id, async move {
                probe(&probes, &processes, &cli, &["--version"], &Env::default()).await
            });
            assert_eq!(driver.run(), [(id, expected)]);
        }
        assert_eq!(
            scripted.take_ran().len(),
            2,
            "the failure asked again, the answer kept"
        );
    }

    #[test]
    fn a_cli_that_is_not_there_is_unread_before_any_run() {
        let processes: Rc<dyn Processes> = Rc::new(ScriptedProcesses::default());
        let probes = Rc::new(Probes::default());
        let mut driver = Driver::default();
        driver.begin(0, async move {
            probe(
                &probes,
                &processes,
                Path::new("/nonexistent/cli"),
                &["--version"],
                &Env::default(),
            )
            .await
        });
        let settled = driver.run();
        let [(0, Err(Unanswered::Unread(sentence)))] = &settled[..] else {
            panic!("not unread: {settled:?}");
        };
        assert!(
            sentence.starts_with("ENOENT: no such file or directory, realpath"),
            "{sentence}"
        );
    }
}
