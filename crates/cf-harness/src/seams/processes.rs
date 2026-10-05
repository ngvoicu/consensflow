//! Where an adapter runs a program: asks a CLI what it answers (`probe`,
//! once per executable as it is on disk), runs one to its end (`run`, as
//! `execFile`), or starts one to speak with a line at a time (`spawn`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_base::env::Env;
use cf_base::file::{stat, FileError};
use cf_process::{runnable, Run};
pub use cf_process::{Ending, Failed, Limits, Streams};
use futures_util::future::{FutureExt, LocalBoxFuture, Shared};

use crate::contract::Work;

/// A program as `runnable` starts it, given everything: its folder and its
/// whole environment. Nothing is inherited.
#[derive(Debug, Clone)]
pub struct Program {
    pub run: Run,
    pub cwd: Option<PathBuf>,
    pub env: Env,
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
    /// which JavaScript threw where it was asked (`realpathSync`).
    Unread(String),
    /// It ran and did not answer.
    Failed(Failed),
}

/// Where an adapter's programs run.
pub trait Processes {
    /// `probeExecutable`: what `executable` answers to `args`, asked once
    /// per executable as it is on disk and per `args`, two callers sharing
    /// one run; a run that failed is not remembered.
    fn probe<'a>(
        &'a self,
        executable: &'a Path,
        args: &'a [&'a str],
        env: &'a Env,
    ) -> Work<'a, Result<Probed, Unanswered>>;

    /// `execFile`, uncached: its output, an exit other than 0 a failure.
    fn run(&self, program: Program, limits: Limits) -> Work<'_, Result<String, Failed>>;

    /// `spawn`: the child, or why it did not start.
    fn spawn(&self, program: Program, streams: Streams) -> Result<Box<dyn Child>, String>;
}

/// A program running beside the adapter.
pub trait Child {
    /// Writes `line` and a newline to its input.
    fn write_line<'a>(&'a mut self, line: &'a str) -> Work<'a, Result<(), String>>;
    /// The next line of its output: none once it ended, a failure past
    /// `limit` bytes without a newline.
    fn read_line(&mut self, limit: usize) -> Work<'_, Result<Option<String>, String>>;
    /// Whether it has exited.
    fn exited(&mut self) -> bool;
    /// Waits until it has exited.
    fn closed(&mut self) -> Work<'_, ()>;
    /// Asks or forces it to end, not waiting (Windows: its whole tree).
    fn terminate(&mut self, how: Ending);
}

/// How long a probe may run and how much it may say (`probeExecutable`'s
/// 30 s and 128 KiB).
const PROBE: Limits = Limits {
    timeout: Duration::from_secs(30),
    max_buffer: 128 * 1024,
};

/// A probe's answer, shared by every caller that asked while it ran.
type Asked = Shared<LocalBoxFuture<'static, Result<Probed, Failed>>>;

/// Which executable a probe asked, as it is on disk, and what.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    file: PathBuf,
    identity: cf_base::file::Identity,
    size: u64,
    modified: u64,
    args: Vec<String>,
}

/// The system's programs.
#[derive(Default)]
pub struct SystemProcesses {
    probes: RefCell<HashMap<Key, Asked>>,
}

impl Processes for SystemProcesses {
    fn probe<'a>(
        &'a self,
        executable: &'a Path,
        args: &'a [&'a str],
        env: &'a Env,
    ) -> Work<'a, Result<Probed, Unanswered>> {
        Box::pin(async move {
            let key = key(executable, args).map_err(Unanswered::Unread)?;
            let asked = self
                .probes
                .borrow_mut()
                .entry(key.clone())
                .or_insert_with(|| ask(executable, args, env))
                .clone();
            let answer = asked.clone().await;
            if answer.is_err() {
                let mut probes = self.probes.borrow_mut();
                if probes.get(&key).is_some_and(|stored| stored.ptr_eq(&asked)) {
                    probes.remove(&key);
                }
            }
            answer.map_err(Unanswered::Failed)
        })
    }

    fn run(&self, program: Program, limits: Limits) -> Work<'_, Result<String, Failed>> {
        Box::pin(async move {
            cf_process::execute(&program.run, program.cwd.as_deref(), &program.env, limits).await
        })
    }

    fn spawn(&self, program: Program, streams: Streams) -> Result<Box<dyn Child>, String> {
        let child = cf_process::spawn(&program.run, program.cwd.as_deref(), &program.env, streams)?;
        Ok(Box::new(child))
    }
}

/// The executable as it is on disk: its real path, which file it is, its
/// size and when it was written, and what it is asked. Node also counted
/// its change time; a file changed in place keeps its size and time only
/// when its contents did not change. A path that cannot be resolved says
/// `realpath` in its sentence, where Node's walk says the `lstat` of the
/// first part missing (a difference kept: the CLI was found a moment ago).
fn key(executable: &Path, args: &[&str]) -> Result<Key, String> {
    let file = std::fs::canonicalize(executable)
        .map_err(|failed| FileError::call(failed, "realpath", Some(executable)).to_string())?;
    let found =
        stat(&file).map_err(|failed| FileError::call(failed, "stat", Some(&file)).to_string())?;
    Ok(Key {
        file,
        identity: found.identity,
        size: found.size,
        modified: found.mtime_ms.to_bits(),
        args: args.iter().map(|&arg| arg.to_owned()).collect(),
    })
}

/// A run of `executable` with `args`, to be shared: an exit other than 0
/// is an answer; a failure to start, to answer in time or within its size,
/// or an end by a signal, is not.
fn ask(executable: &Path, args: &[&str], env: &Env) -> Asked {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let run = runnable(executable, &args, env);
    let env = env.clone();
    async move {
        match cf_process::execute(&run, None, &env, PROBE).await {
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

impl Child for cf_process::Child {
    fn write_line<'a>(&'a mut self, line: &'a str) -> Work<'a, Result<(), String>> {
        Box::pin(cf_process::Child::write_line(self, line))
    }

    fn read_line(&mut self, limit: usize) -> Work<'_, Result<Option<String>, String>> {
        Box::pin(cf_process::Child::read_line(self, limit))
    }

    fn exited(&mut self) -> bool {
        cf_process::Child::exited(self)
    }

    fn closed(&mut self) -> Work<'_, ()> {
        Box::pin(cf_process::Child::closed(self))
    }

    fn terminate(&mut self, how: Ending) {
        cf_process::Child::terminate(self, how);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A stand-in CLI at `file` that counts its runs in `counter` and
    /// answers `answer` with exit `code`.
    #[cfg(unix)]
    fn stand_in(file: &Path, counter: &Path, answer: &str, code: i32) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(
            file,
            format!(
                "#!/bin/sh\necho run >> '{}'\necho '{answer}'\nexit {code}\n",
                counter.display()
            ),
        )
        .unwrap();
        fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn block_on<T>(work: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(work)
    }

    fn runs(counter: &Path) -> usize {
        fs::read_to_string(counter).map_or(0, |text| text.lines().count())
    }

    #[cfg(unix)]
    #[test]
    fn a_cli_is_asked_once_as_it_is_on_disk_and_two_callers_share_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let (cli, counter) = (dir.path().join("codex"), dir.path().join("runs"));
        stand_in(&cli, &counter, "codex 0.159.2", 0);
        let processes = SystemProcesses::default();
        let env = Env::from_vars([("PATH", "/usr/bin:/bin")]);
        let (first, second) = block_on(async {
            tokio::join!(
                processes.probe(&cli, &["--version"], &env),
                processes.probe(&cli, &["--version"], &env)
            )
        });
        let answered = Probed {
            stdout: "codex 0.159.2\n".to_owned(),
            code: 0,
        };
        assert_eq!(
            (first.unwrap(), second.unwrap()),
            (answered.clone(), answered.clone())
        );
        assert_eq!(
            block_on(processes.probe(&cli, &["--version"], &env)).unwrap(),
            answered
        );
        assert_eq!(runs(&counter), 1, "asked once");
        block_on(processes.probe(&cli, &["queue", "--help"], &env)).unwrap();
        assert_eq!(runs(&counter), 2, "other arguments are asked again");
        stand_in(&cli, &counter, "codex 0.160.0 is newer", 0);
        block_on(processes.probe(&cli, &["--version"], &env)).unwrap();
        assert_eq!(runs(&counter), 3, "an updated CLI is asked again");
    }

    #[cfg(unix)]
    #[test]
    fn an_exit_other_than_0_is_an_answer_and_a_failure_is_not_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let (cli, counter) = (dir.path().join("devin"), dir.path().join("runs"));
        stand_in(&cli, &counter, "usage", 2);
        let processes = SystemProcesses::default();
        let env = Env::from_vars([("PATH", "/usr/bin:/bin")]);
        let answered = block_on(processes.probe(&cli, &["--version"], &env)).unwrap();
        assert_eq!(answered.code, 2);
        let silent = dir.path().join("silent");
        fs::write(&silent, "#!/bin/sh\nkill -9 $$\n").unwrap();
        fs::set_permissions(&silent, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let failed = block_on(processes.probe(&silent, &["--version"], &env)).unwrap_err();
        assert!(
            matches!(failed, Unanswered::Failed(Failed { code: None, .. })),
            "{failed:?}"
        );
        assert!(
            !processes
                .probes
                .borrow()
                .keys()
                .any(|key| key.file.ends_with("silent")),
            "the failure is not remembered"
        );
    }

    #[test]
    fn a_cli_that_is_not_there_is_unread_before_any_run() {
        let processes = SystemProcesses::default();
        let unanswered = block_on(processes.probe(
            Path::new("/nonexistent/cli"),
            &["--version"],
            &Env::default(),
        ))
        .unwrap_err();
        let Unanswered::Unread(sentence) = unanswered else {
            panic!("not unread: {unanswered:?}");
        };
        assert!(
            sentence.starts_with("ENOENT: no such file or directory, realpath"),
            "{sentence}"
        );
    }
}
