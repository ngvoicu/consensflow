//! A program started to run beside this one, as Node's `spawn` starts it:
//! its whole environment given, lines written to its input and read from
//! its output, asked or forced to end, and waited for until it has exited
//! and its streams have closed (Node's `close`). One let go that was never
//! asked to end is forced to end; one asked is left to end as it was asked.

use std::cell::{Cell, RefCell};
use std::future::{poll_fn, Future};
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};

use cf_base::env::Env;
use cf_base::file::error_code;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};

use crate::terminate::{end, Reach};
use crate::{terminate, Ending, Run};

/// What a child's streams are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Streams {
    /// Its input and output are this side's, a line at a time; its error
    /// stream is let go (`['pipe', 'pipe', 'ignore']`).
    Lines,
    /// Its input and output are let go; its error stream is read and let
    /// go, and it has closed only once that stream has
    /// (`['ignore', 'ignore', 'pipe']`, drained).
    Quiet,
}

/// A program running beside this one.
pub struct Child {
    pid: Option<u32>,
    input: RefCell<Option<ChildStdin>>,
    output: RefCell<Option<BufReader<ChildStdout>>>,
    /// Waits until it has exited and its error stream has closed; none once
    /// it has.
    closing: RefCell<Option<Pin<Box<dyn Future<Output = ()>>>>>,
    /// Whether it has exited and been waited for: its pid is no longer its.
    exited: Rc<Cell<bool>>,
    /// Whether it was asked or forced to end.
    asked: Cell<bool>,
}

/// Starts `run` in `cwd` with the environment `env`, all of it and nothing
/// inherited: the child, or Node's words for why it did not start
/// (`spawn <file> ENOENT`).
pub fn spawn(run: &Run, cwd: Option<&Path>, env: &Env, streams: Streams) -> Result<Child, String> {
    let mut command = tokio::process::Command::from(run.command());
    command.env_clear().envs(env.iter());
    match streams {
        Streams::Lines => command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
        Streams::Quiet => command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped()),
    };
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    crate::execute::hide_window(&mut command);
    let mut process = command.spawn().map_err(|failed| {
        format!(
            "spawn {} {}",
            run.program.to_string_lossy(),
            error_code(&failed)
        )
    })?;
    crate::job::adopt(&process);
    // Read and let go while the child lives, whoever waits on it: one that
    // fills its error stream would otherwise stop, as Node's never does.
    let drained = process
        .stderr
        .take()
        .map(|errors| tokio::spawn(drain(errors)));
    let exited = Rc::new(Cell::new(false));
    Ok(Child {
        pid: process.id(),
        input: RefCell::new(process.stdin.take()),
        output: RefCell::new(process.stdout.take().map(BufReader::new)),
        closing: RefCell::new(Some(Box::pin(closing(
            drained,
            process,
            Rc::clone(&exited),
        )))),
        exited,
        asked: Cell::new(false),
    })
}

/// Reads `errors` to its end, letting each byte go.
async fn drain(mut errors: ChildStderr) {
    let mut sink = [0; 4096];
    while matches!(errors.read(&mut sink).await, Ok(count) if count > 0) {}
}

/// Until `process` has exited (then `exited` is set) and its error stream,
/// drained apart, has closed.
async fn closing(
    drained: Option<tokio::task::JoinHandle<()>>,
    mut process: tokio::process::Child,
    exited: Rc<Cell<bool>>,
) {
    let drain = async {
        if let Some(drained) = drained {
            let _ = drained.await;
        }
    };
    let wait = async {
        let _ = process.wait().await;
        exited.set(true);
    };
    tokio::join!(drain, wait);
}

impl Child {
    /// Writes `line` and a newline to its input, waiting while its pipe is
    /// full.
    pub async fn write_line(&self, line: &str) -> Result<(), String> {
        let mut input = self
            .input
            .borrow_mut()
            .take()
            .ok_or_else(|| "the child takes no input now".to_owned())?;
        let written = async {
            input.write_all(format!("{line}\n").as_bytes()).await?;
            input.flush().await
        }
        .await;
        *self.input.borrow_mut() = Some(input);
        written.map_err(|failed| failed.to_string())
    }

    /// The next line of its output, its newline taken off; none once its
    /// output ended, what followed the last newline being no line, as Node
    /// never read it; a failure past `limit` bytes without a newline. A line
    /// is read as UTF-8 whole, where Node decoded each chunk alone and broke
    /// a character two chunks shared, and is bounded alone, where Node's
    /// bound took in the rest of the chunk too: neither can be held to, as
    /// both hang on where the system cut the chunks.
    pub async fn read_line(&self, limit: usize) -> Result<Option<String>, String> {
        let mut output = self
            .output
            .borrow_mut()
            .take()
            .ok_or_else(|| "the child gives no output now".to_owned())?;
        let line = next_line(&mut output, limit).await;
        *self.output.borrow_mut() = Some(output);
        line
    }

    /// Whether it has exited, as Node's `exitCode` says once it has.
    pub fn exited(&self) -> bool {
        let _ = self.poll_closing(&mut Context::from_waker(Waker::noop()));
        self.exited.get()
    }

    /// Waits until it has exited and its streams have closed.
    pub async fn closed(&self) {
        poll_fn(|context| self.poll_closing(context)).await;
    }

    /// Asks or forces it to end, not waiting: on Unix the signal `how`
    /// names, on Windows its whole tree at once whatever `how` asks
    /// (`terminate`, `src/harnesses.js`). Nothing is sent to one that has
    /// exited and been waited for: its pid may be another's now.
    pub fn terminate(&self, how: Ending) {
        self.asked.set(true);
        if let (Some(pid), false) = (self.pid, self.exited()) {
            terminate(pid, how);
        }
    }

    /// What ends it later, should this process be ending: the forcing of it
    /// while it runs.
    pub fn ender(&self) -> Ender {
        Ender::new(self.pid, Reach::Process, &self.exited)
    }

    fn poll_closing(&self, context: &mut Context<'_>) -> Poll<()> {
        let mut closing = self.closing.borrow_mut();
        let Some(running) = closing.as_mut() else {
            return Poll::Ready(());
        };
        if running.as_mut().poll(context).is_ready() {
            *closing = None;
            return Poll::Ready(());
        }
        Poll::Pending
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if !self.asked.get() {
            self.terminate(Ending::Forced);
        }
    }
}

/// A child's end, kept apart from the child: this process's exit path
/// forces every child still running. A program run to its end (`capture`,
/// `execute`) hands its own over as it starts.
#[derive(Debug, Clone)]
pub struct Ender {
    pid: Option<u32>,
    reach: Reach,
    exited: Weak<Cell<bool>>,
}

impl Ender {
    /// The end of the program `pid`, which is still there while `exited` is
    /// held and not set: once it is set (the program has been waited for, and
    /// its pid, and the id of the group it led, may be another's) or dropped
    /// (the program is no one's to end), nothing is sent. `reach` is what
    /// the end is sent to: the program alone, or the group it leads.
    pub(crate) fn new(pid: Option<u32>, reach: Reach, exited: &Rc<Cell<bool>>) -> Self {
        Self {
            pid,
            reach,
            exited: Rc::downgrade(exited),
        }
    }

    /// Whether the child is still there to be ended.
    pub fn running(&self) -> bool {
        self.exited.upgrade().is_some_and(|exited| !exited.get())
    }

    /// Forces the child to end, and the group it leads with it, if it is
    /// still there, and waits for nothing (on Windows `taskkill` is started
    /// and let go): this is the end of a process on its way out.
    pub fn force(&self) {
        if let (Some(pid), true) = (self.pid, self.running()) {
            end(pid, Ending::Forced, self.reach, false);
        }
    }
}

/// The next line of `output` within `limit`, none at its end.
async fn next_line(
    output: &mut BufReader<ChildStdout>,
    limit: usize,
) -> Result<Option<String>, String> {
    let mut line = Vec::new();
    loop {
        let available = output
            .fill_buf()
            .await
            .map_err(|failed| failed.to_string())?;
        if available.is_empty() {
            return Ok(None);
        }
        let (taken, ended) = match available.iter().position(|&byte| byte == b'\n') {
            Some(at) => (at + 1, true),
            None => (available.len(), false),
        };
        line.extend_from_slice(&available[..taken]);
        output.consume(taken);
        if ended {
            line.pop();
        }
        if line.len() > limit {
            return Err(format!("a line of more than {limit} bytes"));
        }
        if ended {
            return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Duration;

    fn shell(script: &str) -> Run {
        #[cfg(unix)]
        let (program, flags) = (PathBuf::from("/bin/sh"), vec!["-c"]);
        #[cfg(windows)]
        let (program, flags) = (
            PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            vec!["/d", "/c"],
        );
        Run {
            program,
            args: flags
                .into_iter()
                .chain([script])
                .map(OsString::from)
                .collect(),
            verbatim: false,
        }
    }

    fn system_env() -> Env {
        #[cfg(unix)]
        return Env::from_vars([("PATH", "/usr/bin:/bin")]);
        #[cfg(windows)]
        return Env::from_vars([
            ("SystemRoot", r"C:\Windows"),
            ("PATH", r"C:\Windows\System32"),
        ]);
    }

    fn block_on<T>(work: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(work)
    }

    #[cfg(unix)]
    #[test]
    fn lines_go_both_ways_and_what_follows_the_last_newline_is_no_line() {
        block_on(async {
            let child = spawn(
                &shell("read line; echo \"got $line\"; printf 'caf\\303\\251\\nunended'"),
                None,
                &system_env(),
                Streams::Lines,
            )
            .unwrap();
            child.write_line("hello").await.unwrap();
            assert_eq!(
                child.read_line(64).await.unwrap().as_deref(),
                Some("got hello")
            );
            assert_eq!(child.read_line(64).await.unwrap().as_deref(), Some("café"));
            assert_eq!(child.read_line(64).await.unwrap(), None);
            child.closed().await;
            assert!(child.exited());
        });
    }

    #[test]
    fn a_line_past_its_limit_is_a_failure() {
        block_on(async {
            let child = spawn(
                &shell("echo 0123456789abcdef"),
                None,
                &system_env(),
                Streams::Lines,
            )
            .unwrap();
            assert_eq!(
                child.read_line(8).await.unwrap_err(),
                "a line of more than 8 bytes"
            );
        });
    }

    #[test]
    fn a_child_asked_to_end_ends_and_is_waited_for_and_says_so() {
        block_on(async {
            let script = if cfg!(windows) {
                "ping -n 30 127.0.0.1 >NUL"
            } else {
                "exec sleep 30"
            };
            let child = spawn(&shell(script), None, &system_env(), Streams::Quiet).unwrap();
            let ender = child.ender();
            assert!(!child.exited());
            assert!(ender.running());
            child.terminate(Ending::Asked);
            tokio::time::timeout(Duration::from_secs(10), child.closed())
                .await
                .unwrap();
            assert!(child.exited());
            assert!(!ender.running());
        });
    }

    #[test]
    fn an_ender_forces_a_running_child_to_end_and_a_gone_one_is_left_alone() {
        block_on(async {
            let script = if cfg!(windows) {
                "ping -n 30 127.0.0.1 >NUL"
            } else {
                "exec sleep 30"
            };
            let child = spawn(&shell(script), None, &system_env(), Streams::Quiet).unwrap();
            let ender = child.ender();
            assert!(ender.running());
            ender.force();
            tokio::time::timeout(Duration::from_secs(10), child.closed())
                .await
                .unwrap();
            assert!(child.exited());
            assert!(!ender.running());
            // Nothing is left to end, and nothing is sent to a pid that may be another's.
            ender.force();
        });
    }

    #[cfg(unix)]
    #[test]
    fn a_quiet_child_has_closed_only_once_what_it_started_lets_its_errors_go() {
        block_on(async {
            // The shell exits at once; the sleep it left holds its error stream.
            let child = spawn(
                &shell("sleep 1 & echo started 1>&2"),
                None,
                &system_env(),
                Streams::Quiet,
            )
            .unwrap();
            let closing = tokio::time::timeout(Duration::from_millis(300), child.closed()).await;
            assert!(closing.is_err(), "not closed while the stream is held");
            assert!(child.exited(), "the shell itself has exited");
            child.closed().await;
        });
    }

    #[cfg(unix)]
    #[test]
    fn a_quiet_child_s_errors_are_drained_while_nobody_waits_on_it() {
        block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let done = dir.path().join("done");
            // A megabyte to its error stream, far past what a pipe holds.
            let script = format!(
                "head -c 1000000 /dev/zero 1>&2; : > '{}'; exec sleep 30",
                done.display()
            );
            let child = spawn(&shell(&script), None, &system_env(), Streams::Quiet).unwrap();
            tokio::time::sleep(Duration::from_millis(1500)).await;
            assert!(done.exists(), "it wrote its errors and went on");
            child.terminate(Ending::Forced);
            child.closed().await;
        });
    }

    #[test]
    fn a_program_that_is_not_there_says_spawn_and_enoent() {
        let run = Run {
            program: PathBuf::from("/nonexistent/cli"),
            args: Vec::new(),
            verbatim: false,
        };
        let failed = block_on(async { spawn(&run, None, &system_env(), Streams::Quiet).err() });
        assert_eq!(failed.as_deref(), Some("spawn /nonexistent/cli ENOENT"));
    }

    #[cfg(unix)]
    #[test]
    fn one_let_go_unasked_is_forced_and_one_asked_is_left_to_end_as_asked() {
        block_on(async {
            let child =
                spawn(&shell("exec sleep 30"), None, &system_env(), Streams::Quiet).unwrap();
            let pid = child.pid.unwrap();
            drop(child);
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert!(!crate::alive(pid), "forced when let go");

            // Asked to end, it may take its time: it is not forced.
            let child = spawn(
                &shell("trap 'sleep 1; exit 0' TERM; while true; do sleep 0.05; done"),
                None,
                &system_env(),
                Streams::Quiet,
            )
            .unwrap();
            let pid = child.pid.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            child.terminate(Ending::Asked);
            drop(child);
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert!(crate::alive(pid), "still ending as it was asked");
            tokio::time::sleep(Duration::from_millis(1500)).await;
        });
    }
}
