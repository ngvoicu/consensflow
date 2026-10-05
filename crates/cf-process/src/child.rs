//! A program started to run beside this one, as Node's `spawn` starts it:
//! its whole environment given, lines written to its input and read from
//! its output, asked or forced to end, and waited for. One left running
//! when it is let go is forced to end, with all it started, as Node's exit
//! hook forces it.

use std::path::Path;
use std::process::Stdio;

use cf_base::env::Env;
use cf_base::file::error_code;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};

use crate::{terminate, Ending, Run};

/// What a child's streams are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Streams {
    /// Its input and output are this side's, a line at a time; what it
    /// writes to its error stream is let go.
    Lines,
    /// None of them: it runs on its own.
    Silent,
}

/// A program running beside this one.
pub struct Child {
    process: tokio::process::Child,
    pid: Option<u32>,
    input: Option<ChildStdin>,
    output: Option<BufReader<ChildStdout>>,
    exited: bool,
}

/// Starts `run` in `cwd` with the environment `env`, all of it and nothing
/// inherited: the child, or Node's words for why it did not start
/// (`spawn <file> ENOENT`).
pub fn spawn(run: &Run, cwd: Option<&Path>, env: &Env, streams: Streams) -> Result<Child, String> {
    let mut command = tokio::process::Command::from(run.command());
    command.env_clear().envs(env.iter()).stderr(Stdio::null());
    match streams {
        Streams::Lines => command.stdin(Stdio::piped()).stdout(Stdio::piped()),
        Streams::Silent => command.stdin(Stdio::null()).stdout(Stdio::null()),
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
    Ok(Child {
        pid: process.id(),
        input: process.stdin.take(),
        output: process.stdout.take().map(BufReader::new),
        process,
        exited: false,
    })
}

impl Child {
    /// Writes `line` and a newline to its input.
    pub async fn write_line(&mut self, line: &str) -> Result<(), String> {
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| "the child takes no input".to_owned())?;
        input
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|failed| failed.to_string())?;
        input.flush().await.map_err(|failed| failed.to_string())
    }

    /// The next line of its output, its newline taken off: none once its
    /// output ended, a failure past `limit` bytes without a newline. Each
    /// line is read as UTF-8 whole, where Node decoded each chunk alone and
    /// broke a character two chunks shared.
    pub async fn read_line(&mut self, limit: usize) -> Result<Option<String>, String> {
        let output = self
            .output
            .as_mut()
            .ok_or_else(|| "the child gives no output".to_owned())?;
        let mut line = Vec::new();
        loop {
            let available = output
                .fill_buf()
                .await
                .map_err(|failed| failed.to_string())?;
            if available.is_empty() {
                return Ok((!line.is_empty()).then(|| text(&line)));
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
                return Ok(Some(text(&line)));
            }
        }
    }

    /// Whether it has exited.
    pub fn exited(&mut self) -> bool {
        if !self.exited {
            self.exited = matches!(self.process.try_wait(), Ok(Some(_)) | Err(_));
        }
        self.exited
    }

    /// Waits until it has exited.
    pub async fn closed(&mut self) {
        if !self.exited {
            let _ = self.process.wait().await;
            self.exited = true;
        }
    }

    /// Asks or forces it to end, not waiting: on Unix the signal `how`
    /// names, on Windows its whole tree at once whatever `how` asks
    /// (`terminate`, `src/harnesses.js`).
    pub fn terminate(&mut self, how: Ending) {
        if let (Some(pid), false) = (self.pid, self.exited()) {
            terminate(pid, how);
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        self.terminate(Ending::Forced);
    }
}

/// A line's bytes as text, what is no UTF-8 written U+FFFD.
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
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

    fn block_on<T>(work: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(work)
    }

    #[cfg(unix)]
    #[test]
    fn lines_go_both_ways_and_the_output_ends_with_none() {
        block_on(async {
            let mut child = spawn(
                &shell("read line; echo \"got $line\"; printf 'caf\\303\\251\\nlast'"),
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
            assert_eq!(child.read_line(64).await.unwrap().as_deref(), Some("last"));
            assert_eq!(child.read_line(64).await.unwrap(), None);
            child.closed().await;
            assert!(child.exited());
        });
    }

    #[test]
    fn a_line_past_its_limit_is_a_failure() {
        block_on(async {
            let mut child = spawn(
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
    fn a_child_asked_to_end_ends_and_is_waited_for() {
        block_on(async {
            let script = if cfg!(windows) {
                "ping -n 30 127.0.0.1 >NUL"
            } else {
                "exec sleep 30"
            };
            let mut child = spawn(&shell(script), None, &system_env(), Streams::Silent).unwrap();
            assert!(!child.exited());
            child.terminate(Ending::Asked);
            tokio::time::timeout(Duration::from_secs(10), child.closed())
                .await
                .unwrap();
            assert!(child.exited());
        });
    }

    #[test]
    fn a_program_that_is_not_there_says_spawn_and_enoent() {
        let run = Run {
            program: PathBuf::from("/nonexistent/cli"),
            args: Vec::new(),
            verbatim: false,
        };
        let failed = block_on(async { spawn(&run, None, &system_env(), Streams::Silent).err() });
        assert_eq!(failed.as_deref(), Some("spawn /nonexistent/cli ENOENT"));
    }
}
