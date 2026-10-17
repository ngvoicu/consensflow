//! `execFile` with both streams kept, as Node's harness admin ran an update: a
//! program run to its end, its standard output and its standard error each read
//! as text within the size and the time `execute` holds a program to, and every
//! way it can fail said in Node's words (probed on Node v26.8.1). `execute`
//! answers the standard output alone; an update's output is the two together,
//! which Node's `execFile` hands over whole and `execute` has no use for, and
//! so no field for.

use std::path::Path;
use std::process::Stdio;

use cf_base::env::Env;
use cf_base::file::error_code;

use crate::execute::{command_line, hide_window, read_both, Read, FORCE_AFTER};
use crate::group::{lead, Group};
use crate::job::Job;
use crate::{Ender, Ending, Limits, Run};

/// How often a program asked to end is looked at for whether it has.
#[cfg(unix)]
const ENDED_EVERY: std::time::Duration = std::time::Duration::from_millis(10);

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
/// difference kept). Windows ends the whole tree at a stop and at its time
/// running out, as it does; a future dropped there ends the program alone.
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
/// - Nothing it started outlives it. On Unix, once it has ended, however it
///   ended, what is left of the group it leads is forced to end before it is
///   waited for (a process that leaves the group, by `setsid`, is out of its
///   reach). On Windows it runs in a job of its own, which nothing it starts
///   can leave (a program that asks to start one outside it is refused), and
///   which is ended once it has ended, at a stop, and with the future
///   dropped. A job that cannot be made is a program that did not start
///   (`spawn <file> …`).
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
    lead(&mut command);
    let mut child = command.spawn().map_err(|failed| {
        CaptureFailed::unstarted(format!(
            "spawn {} {}",
            run.program.to_string_lossy(),
            error_code(&failed)
        ))
    })?;
    // Held whole, its own job (Windows), which ends whatever is left of it as
    // the capture ends, however it ends; else the job of every child of this
    // process. A program not put in its own is ended as the child is dropped.
    let _job = match hold {
        Hold::Node => {
            crate::job::adopt(&child);
            None
        }
        Hold::Whole => Some(Job::of(&child).map_err(|why| {
            CaptureFailed::unstarted(format!("spawn {} {why}", run.program.to_string_lossy()))
        })?),
    };
    // Declared after the child, so it is dropped before it: a capture dropped
    // ends the group while its leader is not yet reaped.
    let group = Group::new(child.id());
    started(group.ender());
    let _input = child.stdin.take();
    let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(CaptureFailed::unstarted(
            "the program's output could not be read".to_owned(),
        ));
    };

    let (mut out, mut err) = (Read::default(), Read::default());
    // Its streams read to their ends, then its exit, all within its time:
    // none when it said too much. Held whole on Unix, it is not waited for
    // once it has exited (`Some(None)`): until it is, its pid holds the id of
    // its group, which what it left running is ended by.
    let finished = async {
        read_both(
            &mut stdout,
            &mut stderr,
            &mut out,
            &mut err,
            limits.max_buffer,
        )
        .await;
        if out.overflowed || err.overflowed {
            return None;
        }
        #[cfg(unix)]
        if hold == Hold::Whole {
            while !group.ended() {
                tokio::time::sleep(ENDED_EVERY).await;
            }
            return Some(None);
        }
        Some(Some(child.wait().await))
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
    let status = match finished {
        Some(Some(status)) => status,
        // Ended, and not yet waited for: what it left running goes first.
        Some(None) => {
            group.end(Ending::Forced);
            child.wait().await
        }
        None => {
            // As Node's `kill`: both streams closed first, so nothing it left
            // running holds the answer back, then the program asked to end,
            // and the group it leads with it.
            drop((stdout, stderr));
            group.end(Ending::Asked);
            // Once the program has ended at the ask, or its time to has run
            // out, whatever is left of the group it leads is forced to end: a
            // child that took no notice of the ask would go on after the
            // answer. The program is not waited for until then, so that its
            // id, and its group's, name no other.
            #[cfg(unix)]
            let status = {
                let until = tokio::time::Instant::now() + FORCE_AFTER;
                while !group.ended() && tokio::time::Instant::now() < until {
                    tokio::time::sleep(ENDED_EVERY).await;
                }
                group.end(Ending::Forced);
                child.wait().await
            };
            // Windows ended the whole tree at the ask.
            #[cfg(not(unix))]
            let status = tokio::select! {
                status = child.wait() => status,
                () = tokio::time::sleep(FORCE_AFTER) => {
                    group.end(Ending::Forced);
                    child.wait().await
                }
            };
            status
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

#[cfg(test)]
mod tests;
