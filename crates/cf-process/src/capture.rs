//! `execFile` with both streams kept, as `harness-admin.js` runs an update:
//! a program run to its end, its standard output and its standard error each
//! read as text within the size and the time `execute` holds a program to,
//! and every way it can fail said in Node's words (probed on Node v26.8.1).
//! `execute` answers the standard output alone; an update's output is the
//! two together, which Node's `execFile` hands over whole and `execute` has
//! no use for, and so no field for.

use std::path::Path;
use std::process::Stdio;

use cf_base::env::Env;
use cf_base::file::error_code;

use crate::execute::{command_line, end, hide_window, read_both, Read, FORCE_AFTER};
use crate::{terminate, Ending, Limits, Run};

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

/// Runs `run` as `execute` does (in `cwd`, with the environment `env` and
/// nothing inherited, its input open and unwritten, to the end of its
/// streams and its exit, ended at its timeout or when a stream says more
/// than its limit) and answers what it wrote to both streams, or how it
/// failed with what it wrote.
pub async fn capture(
    run: &Run,
    cwd: Option<&Path>,
    env: &Env,
    limits: Limits,
) -> Result<Captured, CaptureFailed> {
    let unstarted = |message: String| CaptureFailed {
        message,
        code: None,
        killed: false,
        stdout: String::new(),
        stderr: String::new(),
    };
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
    let mut child = command.spawn().map_err(|failed| {
        unstarted(format!(
            "spawn {} {}",
            run.program.to_string_lossy(),
            error_code(&failed)
        ))
    })?;
    crate::job::adopt(&child);
    let pid = child.id();
    let _input = child.stdin.take();
    let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(unstarted(
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
            // running holds the answer back, then the program asked to end.
            drop((stdout, stderr));
            end(pid);
            tokio::select! {
                status = child.wait() => status,
                () = tokio::time::sleep(FORCE_AFTER) => {
                    if let Some(pid) = pid {
                        terminate(pid, Ending::Forced);
                    }
                    child.wait().await
                }
            }
        }
    };

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
