//! The one place cf-publish starts a program: `curl` for what it reads, `gh`
//! for GitHub and `tar` for the list of an archive. `clippy.toml` lets only
//! cf-process start programs, and cf-process is async, on a runtime this crate
//! is built without: what runs under the write token is `serde_json` and
//! `sha2` and the standard library, and nothing else.
//!
//! A program inherits this one's environment (`gh` needs its token and
//! repository from it) and has no input.
// The one place a process starts; see above.
#![allow(clippy::disallowed_methods)]

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// What a program that ran to its end left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The code it exited with: 1 where a signal ended it and it left none.
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn command<I, S>(program: &str, args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null());
    command
}

/// Runs `program` with `args` to its end, in `cwd` if one is given, and keeps
/// what it wrote. A nonzero code is an answer for the caller to read: only a
/// program that could not be started is an error.
pub fn capture<I, S>(program: &str, args: I, cwd: Option<&Path>) -> io::Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = command(program, args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    Ok(Output {
        code: output.status.code().unwrap_or(1),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

/// Starts `program` with `args`, its output piped for a caller that reads it as
/// it arrives (a file that is hashed, not kept).
pub fn start<I, S>(program: &str, args: I) -> io::Result<Child>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    command(program, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}
