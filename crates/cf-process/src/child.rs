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
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use cf_base::env::Env;
use cf_base::file::error_code;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};

use crate::job::Job;
use crate::live::Live;
use crate::terminate::{end, Reach};
use crate::{Ending, Run};

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
    /// A conversation with a program that loads code of the human's (Pi, with the
    /// packages and extensions it has): its input and output are this side's, a line at a
    /// time, as with [`Streams::Lines`]; but its error stream is read and its last
    /// [`SAID_KEPT`] bytes kept, for whoever must say why it ended ([`Child::said`]), the
    /// code it exited with is kept ([`Child::code`]), and what it started ends with it,
    /// however it ends: on Unix it leads a process group of its own, which every end of it
    /// reaches and which is forced once the program has gone (as `capture` leads one), on
    /// Windows its job is ended. A child of this kind is a read, never a window: nothing it
    /// started is meant to outlive it.
    Conversation,
}

/// How much of a [`Streams::Conversation`] child's error stream is kept: the last of it.
pub const SAID_KEPT: usize = 4096;

/// A program running beside this one.
pub struct Child {
    pid: Option<u32>,
    input: RefCell<Option<ChildStdin>>,
    output: RefCell<Option<BufReader<ChildStdout>>>,
    /// Waits until it has exited and its error stream has closed; none once
    /// it has.
    closing: RefCell<Option<Pin<Box<dyn Future<Output = ()>>>>>,
    /// Whether it has exited and been waited for (its pid is no longer its), and the
    /// job that holds it and what it started.
    live: Rc<Live>,
    /// Whether it was asked or forced to end.
    asked: Cell<bool>,
    /// What an end of it reaches: itself, or the group it leads.
    reach: Reach,
    /// The last of its error stream, where it is kept ([`Streams::Conversation`]).
    said: Arc<Mutex<Kept>>,
}

/// What is kept of the error stream of a [`Streams::Conversation`] child: its last
/// [`SAID_KEPT`] bytes, and whether the bytes before them were let go.
#[derive(Default)]
struct Kept {
    bytes: Vec<u8>,
    cut: bool,
}

impl Kept {
    /// Adds `chunk` to what is kept, and lets go of the front where that is more than
    /// [`SAID_KEPT`].
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > SAID_KEPT {
            let over = self.bytes.len() - SAID_KEPT;
            self.bytes.drain(..over);
            self.cut = true;
        }
    }
}

/// Starts `run` in `cwd` with the environment `env`, all of it and nothing
/// inherited: the child, or Node's words for why it did not start
/// (`spawn <file> ENOENT`).
pub fn spawn(run: &Run, cwd: Option<&Path>, env: &Env, streams: Streams) -> Result<Child, String> {
    let mut command = tokio::process::Command::from(run.command());
    // It runs for as long as this does, and holds what it is left to its end.
    crate::descriptors::hold_only_the_streams(command.as_std_mut());
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
        Streams::Conversation => command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    };
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let conversation = streams == Streams::Conversation;
    if conversation {
        crate::group::lead(&mut command);
    }
    crate::job::start_in_job(&mut command);
    let mut process = crate::spawning::one_at_a_time(|| command.spawn()).map_err(|failed| {
        format!(
            "spawn {} {}",
            run.program.to_string_lossy(),
            error_code(&failed)
        )
    })?;
    // In a job of its own before it runs, which ends it and all it starts with the child, and
    // with this process. A program that could not be let run did not start.
    let job = match Job::of(&process, false) {
        Ok(job) => job,
        Err(why) => {
            let _ = process.start_kill();
            return Err(format!("spawn {} {why}", run.program.to_string_lossy()));
        }
    };
    // Read and let go while the child lives, whoever waits on it: one that
    // fills its error stream would otherwise stop, as Node's never does.
    let said = Arc::new(Mutex::new(Kept::default()));
    let kept = conversation.then(|| Arc::clone(&said));
    let drained = process
        .stderr
        .take()
        .map(|errors| tokio::spawn(drain(errors, kept)));
    let live = Rc::new(Live::new(job));
    let pid = process.id();
    let reach = if conversation {
        Reach::Group
    } else {
        Reach::Process
    };
    Ok(Child {
        pid,
        input: RefCell::new(process.stdin.take()),
        output: RefCell::new(process.stdout.take().map(BufReader::new)),
        closing: RefCell::new(Some(Box::pin(closing(
            drained,
            process,
            Rc::clone(&live),
            conversation.then_some(pid).flatten(),
        )))),
        live,
        asked: Cell::new(false),
        reach,
        said,
    })
}

/// Reads `errors` to its end, letting each byte go but the last [`SAID_KEPT`], which are
/// kept in `kept` where it is given one.
async fn drain(mut errors: ChildStderr, kept: Option<Arc<Mutex<Kept>>>) {
    let mut sink = [0; 4096];
    while let Ok(count) = errors.read(&mut sink).await {
        if count == 0 {
            break;
        }
        if let Some(kept) = &kept {
            kept.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(&sink[..count]);
        }
    }
}

/// Until `process` has exited (then `live` says so, with the code it exited with, and lets
/// go of what it left running) and its error stream, drained apart, has closed. Where it
/// leads a group (`led`, its pid), what of the group is left once it has gone is forced to
/// end first.
async fn closing(
    drained: Option<tokio::task::JoinHandle<()>>,
    mut process: tokio::process::Child,
    live: Rc<Live>,
    led: Option<u32>,
) {
    let drain = async {
        if let Some(drained) = drained {
            let _ = drained.await;
        }
    };
    let wait = async {
        let status = process.wait().await;
        live.set_code(status.ok().and_then(|status| status.code()));
        if let Some(leader) = led {
            end_what_it_started(leader, &live);
        }
        live.set_exited();
    };
    tokio::join!(drain, wait);
}

/// Forces to end what a program that led a group started and left running once it was
/// waited for: on Unix, if a process of its group that is no zombie is there, the group (a
/// group whose id is in use is its leader's still, however long ago it was reaped, so the
/// number names no other); on Windows its job.
fn end_what_it_started(leader: u32, live: &Live) {
    #[cfg(unix)]
    {
        let _ = live;
        if let Some(group) = i32::try_from(leader).ok().filter(|&leader| leader > 1) {
            crate::terminate::force_stragglers(-group);
        }
    }
    #[cfg(windows)]
    {
        let _ = leader;
        let _ = live.job().terminate();
    }
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
        self.live.exited()
    }

    /// The code it exited with, once it has: none while it runs, and none where a signal
    /// ended it (and on Windows where this side did).
    pub fn code(&self) -> Option<i32> {
        self.exited().then(|| self.live.code()).flatten()
    }

    /// The last [`SAID_KEPT`] bytes of its error stream, as text, where the child was
    /// started to keep them ([`Streams::Conversation`]); nothing otherwise. What it said
    /// before the moment of asking: more may follow while it runs.
    pub fn said(&self) -> String {
        let kept = self
            .said
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&kept.bytes).into_owned()
    }

    /// Whether the front of its error stream was let go: more than [`SAID_KEPT`] bytes were
    /// written, and [`Child::said`] is the end of them. What stands at its start may be the
    /// rest of a line, or of a value, whose beginning is gone.
    pub fn said_cut(&self) -> bool {
        self.said
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cut
    }

    /// Waits until it has exited and its streams have closed.
    pub async fn closed(&self) {
        poll_fn(|context| self.poll_closing(context)).await;
    }

    /// Waits until it has exited, its streams closed or not: a program it started, or one
    /// started beside it that was left a copy of the pipe, can hold the error stream open
    /// for as long as it lives, and what is waited for is the process.
    pub async fn ended(&self) {
        poll_fn(|context| {
            let _ = self.poll_closing(context);
            if self.live.exited() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }

    /// Asks or forces it to end, not waiting: on Unix the signal `how` names,
    /// on Windows its whole job at once, everything in it, whatever `how` asks. Nothing is
    /// sent to one that has exited and been waited for: its pid may be another's now.
    pub fn terminate(&self, how: Ending) {
        self.asked.set(true);
        if let (Some(pid), false) = (self.pid, self.exited()) {
            end(pid, how, self.reach, true, self.live.job());
        }
    }

    /// What ends it later, should this process be ending: the forcing of it
    /// while it runs.
    pub fn ender(&self) -> Ender {
        Ender::new(self.pid, self.reach, &self.live)
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
    live: Weak<Live>,
}

impl Ender {
    /// The end of the program `pid`, which is still there while `live` is
    /// held and does not say it has exited: once it does (the program has been waited for,
    /// and its pid, and the id of the group it led, may be another's) or is dropped
    /// (the program is no one's to end), nothing is sent. `reach` is what
    /// the end is sent to: the program alone, or the group it leads.
    pub(crate) fn new(pid: Option<u32>, reach: Reach, live: &Rc<Live>) -> Self {
        Self {
            pid,
            reach,
            live: Rc::downgrade(live),
        }
    }

    /// Whether the child is still there to be ended.
    pub fn running(&self) -> bool {
        self.live.upgrade().is_some_and(|live| !live.exited())
    }

    /// Forces the child to end, and the group it leads with it, if it is
    /// still there, and waits for nothing: on Windows its job is ended, and a program
    /// with none has `taskkill` started and let go; on Unix the group is looked at once
    /// more, at once, for what it had started as the signal went round, and that is
    /// forced too. This is the end of a process on its way out (the daemon's stop tail
    /// forces every kept child so), which must not wait (Calliope's P2 on T-581, T-672).
    pub fn force(&self) {
        let Some(live) = self.live.upgrade() else {
            return;
        };
        if let (Some(pid), false) = (self.pid, live.exited()) {
            end(pid, Ending::Forced, self.reach, false, live.job());
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

    /// A program that has exited while another it started holds its error stream open for
    /// five seconds is ended, and not closed: the pipe is the other's to close.
    #[cfg(unix)]
    #[test]
    fn a_program_is_ended_when_it_exits_and_closed_when_its_error_stream_has_closed_too() {
        let held = std::time::Instant::now();
        let finished = block_on(async {
            let child = spawn(
                &shell("/bin/sleep 5 & echo $! >&2; exit 0"),
                None,
                &system_env(),
                Streams::Quiet,
            )
            .unwrap();
            tokio::time::timeout(Duration::from_secs(60), child.ended())
                .await
                .expect("it ended");
            assert!(child.exited());
            // The pipe is held by the program it started: closed is not, for a while.
            let closed = tokio::time::timeout(Duration::from_millis(300), child.closed()).await;
            assert!(
                closed.is_err(),
                "the error stream closed before the sleep ended"
            );
            child.ender()
        });
        assert!(held.elapsed() < Duration::from_secs(20));
        drop(finished);
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_exits_with_its_streams_closed_is_ended_and_closed_together() {
        block_on(async {
            let child = spawn(&shell("exit 0"), None, &system_env(), Streams::Quiet).unwrap();
            tokio::time::timeout(Duration::from_secs(60), child.ended())
                .await
                .expect("it ended");
            tokio::time::timeout(Duration::from_secs(60), child.closed())
                .await
                .expect("it closed");
            assert!(child.exited());
        });
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
                &shell(
                    "trap 'sleep 1; exit 0' TERM; while kill -0 $PPID 2>/dev/null; do /bin/sleep 0.05; done",
                ),
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

    /// What is running of a program that keeps starting programs, ended as `end` says once
    /// it has started some: `end` is given the child, and may keep it for the check by
    /// returning it. The programs it started that are running 5 s after, and how many it
    /// wrote down.
    #[cfg(windows)]
    fn left_after(end: impl FnOnce(Child) -> Option<Child>) -> (Vec<u32>, usize) {
        use crate::testing::{
            keeps_starting_programs, left_running, pids_so_far, Recorded, STARTS_WITHIN,
        };
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pids");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _left = Recorded(&file);
        let left = runtime.block_on(async {
            let child = spawn(
                &keeps_starting_programs(dir.path()),
                None,
                &system_env(),
                Streams::Quiet,
            )
            .unwrap();
            let until = std::time::Instant::now() + STARTS_WITHIN;
            while pids_so_far(&file).len() < 4 {
                assert!(std::time::Instant::now() < until, "it started none");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let kept = end(child);
            let left = left_running(&file, Duration::from_secs(5)).await;
            drop(kept);
            left
        });
        // Not waited for: a program that is left holds the streams a read waits on.
        runtime.shutdown_timeout(Duration::from_secs(10));
        (left, pids_so_far(&file).len())
    }

    // Windows ends the tree whatever it is asked; Unix signals the process alone.
    #[cfg(windows)]
    #[test]
    fn a_child_that_is_starting_programs_is_ended_with_everything_it_started() {
        for how in [Ending::Asked, Ending::Forced] {
            let (left, started) = left_after(|child| {
                child.terminate(how);
                Some(child)
            });
            assert!(
                left.is_empty(),
                "{how:?}: {} of the {started} processes it started are still running: {left:?}",
                left.len()
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_child_let_go_while_it_is_starting_programs_is_ended_with_everything_it_started() {
        let (left, started) = left_after(|child| {
            drop(child);
            None
        });
        assert!(
            left.is_empty(),
            "{} of the {started} processes it started are still running: {left:?}",
            left.len()
        );
    }

    #[test]
    fn a_child_that_exits_of_itself_leaves_what_it_started_running() {
        use crate::testing::{leaves_a_writer, Survivors};
        let dir = tempfile::tempdir().unwrap();
        let (file, marked) = (dir.path().join("pids"), dir.path().join("late"));
        block_on(async {
            let child = spawn(
                &leaves_a_writer(&file, &marked, 2),
                None,
                &Env::from_process(),
                Streams::Quiet,
            )
            .unwrap();
            child.closed().await;
        });
        let pids = crate::testing::pids_in(&file, 2).expect("written before it ended");
        let _left = Survivors(pids);
        let until = std::time::Instant::now() + Duration::from_secs(15);
        while !marked.exists() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            marked.exists(),
            "what it left running was let go of, and wrote"
        );
    }

    /// What it said last and the code it exited with are kept for a conversation, and for no
    /// other kind of child.
    #[test]
    fn a_conversation_keeps_the_last_of_its_error_stream_and_the_code_it_exited_with() {
        let script = if cfg!(windows) {
            "echo first 1>&2 & echo last 1>&2 & exit /b 3"
        } else {
            "echo first >&2; echo last >&2; exit 3"
        };
        block_on(async {
            let kept = spawn(&shell(script), None, &system_env(), Streams::Conversation).unwrap();
            assert_eq!(kept.code(), None, "it has not been waited for");
            tokio::time::timeout(Duration::from_secs(60), kept.closed())
                .await
                .expect("it closed");
            let said = kept.said();
            assert!(said.contains("first") && said.contains("last"), "{said:?}");
            assert!(!kept.said_cut(), "all of it is kept: nothing was let go");
            assert_eq!(kept.code(), Some(3));
            // Nothing is kept of the others.
            for streams in [Streams::Lines, Streams::Quiet] {
                let other = spawn(&shell(script), None, &system_env(), streams).unwrap();
                tokio::time::timeout(Duration::from_secs(60), other.closed())
                    .await
                    .expect("it closed");
                assert_eq!(other.said(), "", "{streams:?}");
            }
        });
    }

    #[cfg(unix)]
    #[test]
    fn a_conversation_keeps_no_more_than_the_last_bytes_it_said() {
        block_on(async {
            let kept = spawn(
                &shell("/usr/bin/head -c 10000 /dev/zero | /usr/bin/tr '\\0' x >&2; echo END >&2"),
                None,
                &system_env(),
                Streams::Conversation,
            )
            .unwrap();
            tokio::time::timeout(Duration::from_secs(60), kept.closed())
                .await
                .expect("it closed");
            let said = kept.said();
            assert_eq!(said.len(), SAID_KEPT, "the last of it, no more");
            assert!(
                said.ends_with("xxxxEND\n"),
                "its end is kept: {:?}",
                &said[said.len() - 12..]
            );
            assert!(
                kept.said_cut(),
                "the front of it was let go, and that is said"
            );
        });
    }

    /// A conversation that wrote more than is kept says its front was let go, and one that wrote
    /// less does not, on Windows as well (the loop is `cmd`'s).
    #[test]
    fn a_conversation_that_wrote_more_than_is_kept_says_its_front_was_let_go() {
        for (lines, cut) in [(10, false), (400, true)] {
            let script = if cfg!(windows) {
                format!("(for /L %i in (1,1,{lines}) do @echo xxxxxxxxxxxxxxxxxxxxxxxxx 1>&2) & exit /b 3")
            } else {
                format!("i=0; while [ $i -lt {lines} ]; do echo xxxxxxxxxxxxxxxxxxxxxxxxx >&2; i=$((i+1)); done; exit 3")
            };
            block_on(async {
                let kept =
                    spawn(&shell(&script), None, &system_env(), Streams::Conversation).unwrap();
                tokio::time::timeout(Duration::from_secs(60), kept.closed())
                    .await
                    .expect("it closed");
                assert_eq!(kept.said_cut(), cut, "{lines} lines");
                assert!(kept.said().len() <= SAID_KEPT);
                assert_eq!(kept.code(), Some(3));
            });
        }
    }

    /// What is kept says whether the front was let go: only where more than [`SAID_KEPT`] bytes
    /// were written, however they came in chunks (the label of a value can be in the front, and
    /// its continuation in what is left). Not a test of a child, so that Windows runs it too.
    #[test]
    fn what_is_kept_of_the_error_stream_says_whether_its_front_was_let_go() {
        let mut kept = Kept::default();
        assert!(!kept.cut && kept.bytes.is_empty());
        kept.push(&[b'a'; 1000]);
        kept.push(&[b'b'; SAID_KEPT - 1000]);
        assert_eq!(kept.bytes.len(), SAID_KEPT);
        assert!(!kept.cut, "exactly SAID_KEPT bytes lost none");
        kept.push(b"c");
        assert!(kept.cut, "one byte over lost the first");
        assert_eq!(kept.bytes.len(), SAID_KEPT);
        assert_eq!(kept.bytes.first(), Some(&b'a'));
        assert_eq!(kept.bytes.last(), Some(&b'c'));
        kept.push(b"d");
        assert!(kept.cut, "once cut, cut");
        let mut one_chunk = Kept::default();
        one_chunk.push(&vec![b'x'; SAID_KEPT * 3]);
        assert!(one_chunk.cut);
        assert_eq!(one_chunk.bytes.len(), SAID_KEPT);
    }

    /// A conversation that goes of itself leaves nothing it started running; any other child
    /// does (the test above this one's twin).
    #[test]
    fn a_conversation_that_exits_leaves_nothing_it_started_running() {
        use crate::testing::{leaves_a_writer, Survivors};
        let dir = tempfile::tempdir().unwrap();
        let (file, marked) = (dir.path().join("pids"), dir.path().join("late"));
        block_on(async {
            let child = spawn(
                &leaves_a_writer(&file, &marked, 2),
                None,
                &Env::from_process(),
                Streams::Conversation,
            )
            .unwrap();
            child.ended().await;
        });
        let pids = crate::testing::pids_in(&file, 2).expect("written before it ended");
        let _left = Survivors(pids);
        std::thread::sleep(Duration::from_secs(5));
        assert!(
            !marked.exists(),
            "what the conversation started was ended with it, and did not write"
        );
    }

    /// Asked to end, a conversation ends what it started with it, not the program alone.
    #[cfg(unix)]
    #[test]
    fn a_conversation_asked_to_end_ends_the_group_it_leads() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pids");
        let script = format!("/bin/sleep 30 & echo $! > '{}'; wait", file.display());
        block_on(async {
            let child = spawn(&shell(&script), None, &system_env(), Streams::Conversation).unwrap();
            let sleeper = crate::testing::pids_written_to_soon(&file, 1).await[0];
            child.terminate(Ending::Asked);
            tokio::time::timeout(Duration::from_secs(10), child.ended())
                .await
                .expect("it ended");
            assert!(
                crate::testing::gone_within(sleeper, Duration::from_secs(10)),
                "the sleep it started is gone"
            );
        });
    }
}
