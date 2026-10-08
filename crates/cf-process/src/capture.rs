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
use crate::{Ender, Ending, Limits, Run};

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
    crate::job::adopt(&child);
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
    // none when it said too much.
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
            None
        } else {
            Some(child.wait().await)
        }
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
        Some(status) => status,
        None => {
            // As Node's `kill`: both streams closed first, so nothing it left
            // running holds the answer back, then the program asked to end,
            // and the group it leads with it.
            drop((stdout, stderr));
            group.end(Ending::Asked);
            tokio::select! {
                status = child.wait() => status,
                () = tokio::time::sleep(FORCE_AFTER) => {
                    group.end(Ending::Forced);
                    child.wait().await
                }
            }
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
    if status.success() {
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
