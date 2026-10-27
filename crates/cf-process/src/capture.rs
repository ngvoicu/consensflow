//! `execFile` with both streams kept, as Node's harness admin ran an update: a
//! program run to its end, its standard output and its standard error each read
//! as text within the size and the time `execute` holds a program to, and every
//! way it can fail said in Node's words (probed on Node v26.8.1). `execute`
//! answers the standard output alone; an update's output is the two together,
//! which Node's `execFile` hands over whole and `execute` has no use for, and
//! so no field for.

use std::path::Path;
use std::process::{ExitStatus, Stdio};

use cf_base::env::Env;
use cf_base::file::error_code;
use tokio::process::{ChildStderr, ChildStdout};

use crate::execute::{command_line, hide_window, read_both, Read, FORCE_AFTER};
#[cfg(unix)]
use crate::group::ENDED_EVERY;
use crate::group::{lead, lead_whole, Group};
use crate::job::{start_in_job, Job};
use crate::{Ender, Ending, Limits, Run};

/// How the message of a capture held whole (`capture_whole`) begins where what its
/// program started did not end once it was ended (Windows: a job's processes that
/// outlived its end): what follows names them. Nothing of such a run is over.
pub const LEFT_RUNNING: &str = "what the program started did not end";

/// What a program that ended with 0 wrote: `execFile`'s `{ stdout, stderr }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub stdout: String,
    pub stderr: String,
}

/// A program that did not answer, as `execFile`'s error has it, with what it
/// wrote to each stream as far as it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureFailed {
    /// Its message, in `execute`'s words: `Command failed: <cmd>\n<stderr>`
    /// when it ended otherwise than with 0, `spawn <file> ENOENT` (or
    /// another code) when it did not start, `stdout maxBuffer length
    /// exceeded` when it wrote too much.
    pub message: String,
    /// The code it exited with, when it exited with one: none when a signal
    /// ended it, and on Windows none when this side ended it.
    pub code: Option<i32>,
    /// Whether it was asked to end because its time ran out.
    pub killed: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CaptureFailed {
    /// A program that never ran, `message` the reason: it exited with nothing
    /// and wrote nothing.
    #[must_use]
    pub fn unstarted(message: String) -> Self {
        Self {
            message,
            code: None,
            killed: false,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

/// Runs `run` as `execute` does (in `cwd`, with the environment `env` and
/// nothing inherited, its input open and unwritten, to the end of its
/// streams and its exit, ended at its timeout or when a stream says more
/// than its limit) and answers what it wrote to both streams, or how it
/// failed with what it wrote.
///
/// The program leads a process group of its own on Unix, and what ends it (a
/// stop, its time running out, this future dropped) ends the group: the
/// installer's children with it, which Node's `kill` left running (a
/// difference kept). On Windows it runs in a job of its own, which a stop, its
/// time running out, and this future dropped end, whole and at once: the
/// installer's children, and what they start as it ends. One that ended of itself
/// is let go of, with what it left running.
///
/// `started` is given the program's [`Ender`] once it has started, for
/// whoever ends this process's children on its way out: the program, and the
/// group it leads, are its to end until the program has been waited for or
/// this future is dropped (which ends them too), and no longer after.
pub async fn capture(
    run: &Run,
    cwd: Option<&Path>,
    env: &Env,
    limits: Limits,
    started: impl FnOnce(Ender),
) -> Result<Captured, CaptureFailed> {
    captured(run, cwd, env, limits, started, Hold::Node).await
}

/// As [`capture`], the program held to its time and its tree whole, as the
/// plants' runner holds a run (a run's child that went on writing the sources
/// once they were put back, 2026-10-17):
///
/// - A program asked to end because its time ran out has failed (`killed`),
///   whatever it exits with once asked, where Node answers one that exits
///   with 0.
/// - Nothing it started outlives it. On Unix it leads a session of its own, and
///   once it has ended, however it ended, every process of that session (the
///   groups nextest gives its tests too), and every process still descended from
///   it, is forced to end before it is waited for; where its time ran out or the
///   future is dropped, it is asked first, and given a moment (the group module
///   says how). A process that starts a session of its own, and whose parent
///   ended before then, is out of its reach. On Windows it is started suspended,
///   put in a job of its own, and then let run: nothing it starts can leave that
///   job (a program that asks to start one outside it is refused), which is ended
///   once it has ended, at a stop, and with the future dropped; and the capture
///   answers once each process of the job has ended, as its handle says. A job that
///   cannot be made, or a program that cannot be let run, is a program that did not
///   start (`spawn <file> …`).
/// - It has ended when it exits, not when its streams close: a process it left
///   may hold them (Windows gives a child every handle it may inherit). What it
///   left is ended first, then its streams are read to their ends, for a moment at
///   most.
/// - Where what it left did not end (a process of its job had not ended a moment
///   after the job was ended, a process that could not be held to be forced had not
///   a moment after its group was), or its streams are still held open a moment
///   after, it has failed, whatever it exited with: the message begins with
///   [`LEFT_RUNNING`].
pub async fn capture_whole(
    run: &Run,
    cwd: Option<&Path>,
    env: &Env,
    limits: Limits,
    started: impl FnOnce(Ender),
) -> Result<Captured, CaptureFailed> {
    captured(run, cwd, env, limits, started, Hold::Whole).await
}

/// What a capture holds its program to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hold {
    /// As Node's `execFile`: what the program left running once it was waited
    /// for is let go of, and one that ends with 0 at the ask has answered.
    Node,
    /// [`capture_whole`]'s.
    Whole,
}

async fn captured(
    run: &Run,
    cwd: Option<&Path>,
    env: &Env,
    limits: Limits,
    started: impl FnOnce(Ender),
    hold: Hold,
) -> Result<Captured, CaptureFailed> {
    let mut command = tokio::process::Command::from(run.command());
    // What it starts may run on after it has answered: an installer's own children.
    crate::descriptors::hold_only_the_streams(command.as_std_mut());
    command
        .env_clear()
        .envs(env.iter())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    hide_window(&mut command);
    match hold {
        Hold::Node => lead(&mut command),
        Hold::Whole => lead_whole(&mut command),
    }
    start_in_job(&mut command);
    let mut child = crate::spawning::one_at_a_time(|| command.spawn()).map_err(|failed| {
        CaptureFailed::unstarted(format!(
            "spawn {} {}",
            run.program.to_string_lossy(),
            error_code(&failed)
        ))
    })?;
    // In a job of its own (Windows), which it is put in before it runs and which ends
    // whatever is left of it as the capture ends, however it ends, and with this process;
    // one that ended of itself leaves what it left running (`release`), unless it is held
    // whole. A program not put in its job, or not let run, is ended as the child is dropped.
    let job = Job::of(&child, hold == Hold::Whole).map_err(|why| {
        CaptureFailed::unstarted(format!("spawn {} {why}", run.program.to_string_lossy()))
    })?;
    // Declared after the child, so it is dropped before it: a capture dropped
    // ends the group (and with it the job) while its leader is not yet reaped.
    let group = Group::new(child.id(), hold == Hold::Whole, job);
    started(group.ender());
    let _input = child.stdin.take();
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(CaptureFailed::unstarted(
            "the program's output could not be read".to_owned(),
        ));
    };

    let (mut out, mut err) = (Read::default(), Read::default());
    let (status, killed, left) = match hold {
        Hold::Node => {
            let (status, killed) = node_end(
                &mut child,
                &group,
                (stdout, stderr),
                (&mut out, &mut err),
                limits,
            )
            .await;
            (status, killed, Ok(()))
        }
        Hold::Whole => {
            held_end(
                &mut child,
                &group,
                (stdout, stderr),
                (&mut out, &mut err),
                limits,
            )
            .await
        }
    };

    // Waited for: its pid is no longer its to end, nor its group's id.
    group.release();

    let stdout = String::from_utf8_lossy(&out.bytes).into_owned();
    let stderr = String::from_utf8_lossy(&err.bytes).into_owned();
    let failed = |message: String, code: Option<i32>, killed: bool| CaptureFailed {
        message,
        code,
        killed,
        stdout: stdout.clone(),
        stderr: stderr.clone(),
    };
    let status = status.map_err(|cause| failed(cause.to_string(), None, killed))?;
    // Held whole, what it started did not end once ended: nothing of the run is over.
    if let Err(why) = left {
        return Err(failed(
            format!("{LEFT_RUNNING}: {why}"),
            status.code(),
            killed,
        ));
    }
    for (stream, read) in [("stdout", &out), ("stderr", &err)] {
        if read.overflowed {
            return Err(failed(
                format!("{stream} maxBuffer length exceeded"),
                None,
                false,
            ));
        }
    }
    // Asked to end because its time ran out, a program held whole has failed
    // whatever it then exited with.
    if status.success() && !(killed && hold == Hold::Whole) {
        return Ok(Captured { stdout, stderr });
    }
    // Ended by a signal, Node has no code for it; on Windows this side's
    // end is one too, where the system says 1.
    let code = if cfg!(windows) && killed {
        None
    } else {
        status.code()
    };
    Err(failed(
        format!("Command failed: {}\n{stderr}", command_line(run)),
        code,
        killed,
    ))
}

/// The streams of a program, and what was read of each.
type Streams = (ChildStdout, ChildStderr);
type Reads<'a> = (&'a mut Read, &'a mut Read);

/// How a program run as Node's `execFile` runs it ends: its streams read to their ends,
/// then its exit, all within its time, or not; then, where it said too much or its
/// time ran out, asked to end, and forced a moment later. What it came to, and whether
/// its time ran out.
async fn node_end(
    child: &mut tokio::process::Child,
    group: &Group,
    (mut stdout, mut stderr): Streams,
    (out, err): Reads<'_>,
    limits: Limits,
) -> (std::io::Result<ExitStatus>, bool) {
    // Its streams read to their ends, then its exit, all within its time: none when it
    // said too much.
    let finished = async {
        if read_both(&mut stdout, &mut stderr, out, err, limits.max_buffer).await {
            return None;
        }
        Some(child.wait().await)
    };
    let mut killed = false;
    let finished = if limits.timeout.is_zero() {
        finished.await
    } else if let Ok(finished) = tokio::time::timeout(limits.timeout, finished).await {
        finished
    } else {
        killed = true;
        None
    };
    if let Some(status) = finished {
        return (status, killed);
    }
    // As Node's `kill`: both streams closed first, so nothing it left running holds the
    // answer back, then the program asked to end, and the group it leads with it.
    drop((stdout, stderr));
    group.end(Ending::Asked);
    (forced(child, group).await, killed)
}

/// Once a program asked to end has ended at the ask, or its time to has run out,
/// whatever is left of the group it leads is forced to end: a child that took no notice
/// of the ask would go on after the answer. On Unix the program is not waited for until
/// then, so that its id, and its group's, name no other; Windows ended the whole tree at
/// the ask. What it came to.
async fn forced(child: &mut tokio::process::Child, group: &Group) -> std::io::Result<ExitStatus> {
    #[cfg(unix)]
    {
        let until = tokio::time::Instant::now() + FORCE_AFTER;
        while !group.ended() && tokio::time::Instant::now() < until {
            tokio::time::sleep(ENDED_EVERY).await;
        }
        group.end(Ending::Forced);
        child.wait().await
    }
    #[cfg(not(unix))]
    tokio::select! {
        status = child.wait() => status,
        () = tokio::time::sleep(FORCE_AFTER) => {
            group.end(Ending::Forced);
            child.wait().await
        }
    }
}

/// How a program held whole ended, before what it left was.
enum Held {
    /// It exited of itself: what it came to, where it was waited for (Windows; on
    /// Unix it is not, its pid holding the id of the session and the group it leads).
    Exited(Option<std::io::Result<ExitStatus>>),
    /// A stream of it said more than a run may.
    Overflowed,
    /// Its time ran out.
    OutOfTime,
}

/// How a program held whole ends (`capture_whole`): when it exits, whatever still
/// holds its streams (a grandchild given them as Windows gives a child every handle it
/// may inherit, which held a run as long as it lived, zeewin 2026-10-17), its streams
/// read meanwhile. Then what it left is ended (`end_the_rest`), and only then are its
/// streams read to their ends, for a moment at most. Where it said too much or its
/// time ran out, it is asked to end and forced as Node's is, and what it left with it.
/// What it came to, whether its time ran out, and whether what it left ended.
async fn held_end(
    child: &mut tokio::process::Child,
    group: &Group,
    (mut stdout, mut stderr): Streams,
    (out, err): Reads<'_>,
    limits: Limits,
) -> (std::io::Result<ExitStatus>, bool, Result<(), String>) {
    let mut reading = Box::pin(read_both(
        &mut stdout,
        &mut stderr,
        out,
        err,
        limits.max_buffer,
    ));
    let mut read = false;
    let held = {
        let mut exit = Box::pin(exited(child, group));
        let ending = async {
            loop {
                tokio::select! {
                    overflowed = &mut reading, if !read => {
                        if overflowed {
                            return Held::Overflowed;
                        }
                        read = true;
                    }
                    status = &mut exit => return Held::Exited(status),
                }
            }
        };
        if limits.timeout.is_zero() {
            ending.await
        } else {
            tokio::time::timeout(limits.timeout, ending)
                .await
                .unwrap_or(Held::OutOfTime)
        }
    };
    match held {
        Held::Exited(status) => {
            let mut left = end_the_rest(group);
            // Streams still held once all it left was ended: something out of reach
            // holds them, and what was read of them is not all.
            if !read
                && tokio::time::timeout(FORCE_AFTER, &mut reading)
                    .await
                    .is_err()
            {
                left = left.and(Err(format!(
                    "its output was still held open {} s after it ended",
                    FORCE_AFTER.as_secs()
                )));
            }
            drop(reading);
            let status = match status {
                Some(status) => status,
                None => child.wait().await,
            };
            (status, false, left)
        }
        Held::Overflowed | Held::OutOfTime => {
            drop(reading);
            drop((stdout, stderr));
            // On Windows its job is ended first, each process of it held before it is and
            // waited for after (`Job::end`): ended by the group's ask, the job lists none of
            // those that are still ending, and what is waited for after is nothing (T-363).
            // Elsewhere there is no job, and the group is asked.
            let job_ended = group.job().end();
            group.end(Ending::Asked);
            // Forced before it is reaped on Unix, its session and descendants with it.
            let status = forced(child, group).await;
            let left = job_ended.and(group.unpinned());
            (status, matches!(held, Held::OutOfTime), left)
        }
    }
}

/// When a program held whole has exited, asked without reaping it on Unix (its pid
/// holds the id of its session and group until it is), and waited for on Windows (its
/// job holds what it left): what it came to, where it was waited for.
async fn exited(
    child: &mut tokio::process::Child,
    group: &Group,
) -> Option<std::io::Result<ExitStatus>> {
    #[cfg(unix)]
    {
        let _ = child;
        while !group.ended() {
            tokio::time::sleep(ENDED_EVERY).await;
        }
        None
    }
    #[cfg(not(unix))]
    {
        let _ = group;
        Some(child.wait().await)
    }
}

/// Ends what a program held whole left once it exited of itself: on Unix every process
/// of its session and every one descended from it, then its group (it is not reaped
/// yet); on Windows, where it was waited for and its pid may be another's, every process
/// of its job, waited for until none is left. Why not all of it ended, where not all did
/// (a process of the job, or one that could not be held to be forced, that outlived it).
fn end_the_rest(group: &Group) -> Result<(), String> {
    if cfg!(unix) {
        group.end(Ending::Forced);
    }
    group.job().end().and(group.unpinned())
}

#[cfg(test)]
mod tests;
