//! `execFile`, as Node's `child_process` runs a program to its end: its
//! output read as text within a size and a time, and every way it can fail
//! said in Node's words (probed on Node v26.8.1).

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::file::error_code;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::{terminate, Ending, Run};

/// How long a program asked to end at its timeout has before it is forced:
/// Node waits for it as long as it runs, and a probe every launch shares
/// would wait with it (a difference kept).
const FORCE_AFTER: Duration = Duration::from_secs(2);

/// How long a program may run and how much it may write to each stream:
/// `execFile`'s `timeout` (none when zero) and `maxBuffer`, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub timeout: Duration,
    pub max_buffer: usize,
}

/// A program that did not answer, as `execFile`'s error has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    /// Its message: `Command failed: <cmd>\n<stderr>` when it ended
    /// otherwise than with 0, `spawn <file> ENOENT` (or another code) when
    /// it did not start, `stdout maxBuffer length exceeded` when it wrote
    /// too much.
    pub message: String,
    /// The code it exited with, when it exited with one: none when a signal
    /// ended it, and on Windows none when this side ended it, as libuv
    /// says a signal there.
    pub code: Option<i32>,
    /// Whether it was asked to end because its time ran out.
    pub killed: bool,
    /// What it wrote to its standard output, as far as it was read.
    pub stdout: String,
}

/// Runs `run` in `cwd` with the environment `env`, all of it and nothing
/// inherited, until it ends: its standard output as text, or how it failed.
/// Its input stays open and unwritten, as Node's does. It has ended when it
/// has exited and its streams have closed, as Node's `close`. At its
/// timeout, or once a stream says more than its limit, both streams are
/// closed and it is asked to end, and how it ends decides: one that ends
/// with 0 has answered, as with Node.
pub async fn execute(
    run: &Run,
    cwd: Option<&Path>,
    env: &Env,
    limits: Limits,
) -> Result<String, Failed> {
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
    let mut child = command.spawn().map_err(|failed| Failed {
        message: format!(
            "spawn {} {}",
            run.program.to_string_lossy(),
            error_code(&failed)
        ),
        code: None,
        killed: false,
        stdout: String::new(),
    })?;
    crate::job::adopt(&child);
    let pid = child.id();
    let _input = child.stdin.take();
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(Failed {
            message: "the program's output could not be read".to_owned(),
            code: None,
            killed: false,
            stdout: String::new(),
        });
    };

    let (mut stdout, mut stderr) = (stdout, stderr);
    let (mut out, mut err) = (Read::default(), Read::default());
    let read = read_both(
        &mut stdout,
        &mut stderr,
        &mut out,
        &mut err,
        limits.max_buffer,
    );
    let killed = if limits.timeout.is_zero() {
        read.await;
        false
    } else {
        tokio::select! {
            () = read => false,
            () = tokio::time::sleep(limits.timeout) => true,
        }
    };
    let status = if killed || out.overflowed || err.overflowed {
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
    } else {
        child.wait().await
    };

    let stdout = String::from_utf8_lossy(&out.bytes).into_owned();
    let stderr = String::from_utf8_lossy(&err.bytes).into_owned();
    let status = status.map_err(|failed| Failed {
        message: failed.to_string(),
        code: None,
        killed,
        stdout: stdout.clone(),
    })?;
    for (stream, read) in [("stdout", &out), ("stderr", &err)] {
        if read.overflowed {
            return Err(Failed {
                message: format!("{stream} maxBuffer length exceeded"),
                code: None,
                killed: false,
                stdout,
            });
        }
    }
    if status.success() {
        return Ok(stdout);
    }
    // Ended by a signal, Node has no code for it; on Windows this side's
    // end is one too, where the system says 1.
    let code = if cfg!(windows) && killed {
        None
    } else {
        status.code()
    };
    Err(Failed {
        message: format!("Command failed: {}\n{stderr}", command_line(run)),
        code,
        killed,
        stdout,
    })
}

/// What a stream gave, up to the limit, and whether it gave more.
#[derive(Default)]
struct Read {
    bytes: Vec<u8>,
    overflowed: bool,
}

impl Read {
    /// Keeps `bytes` within `limit`: whether they all fit.
    fn keep(&mut self, bytes: &[u8], limit: usize) -> bool {
        let room = limit.saturating_sub(self.bytes.len());
        if bytes.len() > room {
            self.bytes.extend_from_slice(&bytes[..room]);
            self.overflowed = true;
            return false;
        }
        self.bytes.extend_from_slice(bytes);
        true
    }
}

/// Reads both streams to their ends, keeping each one's first `limit`
/// bytes, and stops at once when either says more, as Node's `maxBuffer`
/// does.
async fn read_both(
    stdout: &mut (impl AsyncRead + Unpin),
    stderr: &mut (impl AsyncRead + Unpin),
    out: &mut Read,
    err: &mut Read,
    limit: usize,
) {
    let (mut out_open, mut err_open) = (true, true);
    let (mut out_chunk, mut err_chunk) = ([0; 16 * 1024], [0; 16 * 1024]);
    while out_open || err_open {
        tokio::select! {
            read = stdout.read(&mut out_chunk), if out_open => match read {
                Ok(0) | Err(_) => out_open = false,
                Ok(count) => if !out.keep(&out_chunk[..count], limit) { return },
            },
            read = stderr.read(&mut err_chunk), if err_open => match read {
                Ok(0) | Err(_) => err_open = false,
                Ok(count) => if !err.keep(&err_chunk[..count], limit) { return },
            },
        }
    }
}

/// Asks the program to end, as `child.kill()` does: SIGTERM on Unix. On
/// Windows its whole tree goes, where Node ends the program alone (and a
/// `.cmd`'s own program, left running, keeps its streams open).
fn end(pid: Option<u32>) {
    if let Some(pid) = pid {
        terminate(pid, Ending::Asked);
    }
}

/// The command as Node's message writes it: the file, then each argument,
/// one space between, nothing quoted.
fn command_line(run: &Run) -> String {
    std::iter::once(run.program.to_string_lossy())
        .chain(run.args.iter().map(|arg| arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// No console window opens for the program on Windows.
#[cfg(windows)]
pub(crate) fn hide_window(command: &mut tokio::process::Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn hide_window(_command: &mut tokio::process::Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// A shell running `script`: `sh -c` on Unix, `cmd /d /c` on Windows.
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

    fn run_now(run: &Run, env: &Env, limits: Limits) -> Result<String, Failed> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(execute(run, None, env, limits))
    }

    const ROOMY: Limits = Limits {
        timeout: Duration::ZERO,
        max_buffer: 1024 * 1024,
    };

    #[test]
    fn a_program_that_ends_with_0_answers_its_output() {
        assert_eq!(
            run_now(&shell("echo fine"), &system_env(), ROOMY)
                .unwrap()
                .trim_end(),
            "fine"
        );
    }

    #[test]
    fn another_exit_says_the_command_and_what_it_wrote_to_stderr() {
        // Probed on Node v26.8.1: `execFile` of a program exiting 3.
        let (script, stderr) = if cfg!(windows) {
            ("(echo bad)1>&2&& exit 3", "bad\r\n")
        } else {
            ("echo bad 1>&2 && exit 3", "bad\n")
        };
        let run = shell(script);
        let failed = run_now(&run, &system_env(), ROOMY).unwrap_err();
        assert_eq!(failed.code, Some(3));
        assert!(!failed.killed);
        assert_eq!(
            failed.message,
            format!("Command failed: {}\n{stderr}", command_line(&run))
        );
    }

    #[test]
    fn a_program_out_of_time_is_ended_and_said_killed_with_no_code() {
        let run = if cfg!(windows) {
            shell("echo slow 1>&2 && ping -n 30 127.0.0.1 >NUL")
        } else {
            shell("echo slow 1>&2; exec sleep 30")
        };
        let limits = Limits {
            timeout: Duration::from_millis(300),
            max_buffer: 1024,
        };
        let failed = run_now(&run, &system_env(), limits).unwrap_err();
        assert!(failed.killed);
        assert_eq!(failed.code, None);
        assert!(
            failed
                .message
                .starts_with(&format!("Command failed: {}\nslow", command_line(&run))),
            "{}",
            failed.message
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_program_out_of_time_that_writes_once_asked_dies_of_its_closed_stream() {
        // Probed on Node v26.8.1: the streams are closed before the program is
        // asked to end, so the trap's `echo` meets a closed pipe (SIGPIPE).
        let run = shell("trap 'echo bye; exit 0' TERM; echo hi; while true; do sleep 0.05; done");
        let limits = Limits {
            timeout: Duration::from_millis(300),
            max_buffer: 1024,
        };
        let failed = run_now(&run, &system_env(), limits).unwrap_err();
        assert!(failed.killed);
        assert_eq!(failed.code, None);
        assert_eq!(failed.stdout, "hi\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_left_another_holding_its_streams_answers_at_its_timeout() {
        // Probed on Node v26.8.1: `/bin/sh -c 'sleep 1 & printf ready'`, 100 ms,
        // answered in 104 ms, the shell's own exit with 0 its answer.
        let limits = Limits {
            timeout: Duration::from_millis(100),
            max_buffer: 1024,
        };
        let started = std::time::Instant::now();
        let answered = run_now(&shell("sleep 1 & printf ready"), &system_env(), limits);
        assert_eq!(answered.unwrap(), "ready");
        assert!(
            started.elapsed() < Duration::from_millis(900),
            "{:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_will_not_end_when_asked_is_forced_a_moment_later() {
        let run = shell("trap '' TERM; while true; do sleep 0.05; done");
        let limits = Limits {
            timeout: Duration::from_millis(200),
            max_buffer: 1024,
        };
        let started = std::time::Instant::now();
        let failed = run_now(&run, &system_env(), limits).unwrap_err();
        assert!(failed.killed);
        assert_eq!(failed.code, None);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_program_that_writes_too_much_is_ended_and_said_so_by_its_stream() {
        let limits = Limits {
            timeout: Duration::ZERO,
            max_buffer: 10,
        };
        let failed =
            run_now(&shell("echo xxxxxxxxxxxxxxxxxxxx"), &system_env(), limits).unwrap_err();
        assert_eq!(failed.message, "stdout maxBuffer length exceeded");
        assert_eq!(failed.stdout, "xxxxxxxxxx");
        let failed = run_now(
            &shell("echo yyyyyyyyyyyyyyyyyyyy 1>&2"),
            &system_env(),
            limits,
        )
        .unwrap_err();
        assert_eq!(failed.message, "stderr maxBuffer length exceeded");
    }

    #[test]
    fn a_program_that_is_not_there_says_spawn_and_enoent() {
        let run = Run {
            program: PathBuf::from("/nonexistent/cli"),
            args: vec![OsString::from("--version")],
            verbatim: false,
        };
        let failed = run_now(&run, &system_env(), ROOMY).unwrap_err();
        assert_eq!(failed.message, "spawn /nonexistent/cli ENOENT");
        assert_eq!(failed.code, None);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_may_not_run_says_spawn_and_eacces() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("noexec");
        std::fs::write(&file, "#!/bin/sh\necho hi\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let run = Run {
            program: file.clone(),
            args: Vec::new(),
            verbatim: false,
        };
        let failed = run_now(&run, &system_env(), ROOMY).unwrap_err();
        assert_eq!(failed.message, format!("spawn {} EACCES", file.display()));
    }

    #[cfg(unix)]
    #[test]
    fn a_program_a_signal_ended_has_no_code_and_was_not_killed_by_time() {
        let run = shell("kill -9 $$");
        let failed = run_now(&run, &system_env(), ROOMY).unwrap_err();
        assert_eq!(failed.code, None);
        assert!(!failed.killed);
        assert_eq!(
            failed.message,
            format!("Command failed: {}\n", command_line(&run))
        );
    }

    #[test]
    fn the_program_has_the_environment_given_and_nothing_else() {
        let env = system_env();
        let script = if cfg!(windows) {
            "if defined CF_ONLY_HERE (echo set) else (echo unset)"
        } else {
            "echo ${CF_ONLY_HERE:-unset}"
        };
        assert_eq!(
            run_now(&shell(script), &env, ROOMY).unwrap().trim_end(),
            "unset"
        );
        let mut vars: Vec<(OsString, OsString)> = env
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        vars.push(("CF_ONLY_HERE".into(), "here".into()));
        let given = Env::from_vars(vars);
        let expected = if cfg!(windows) { "set" } else { "here" };
        assert_eq!(
            run_now(&shell(script), &given, ROOMY).unwrap().trim_end(),
            expected
        );
    }
}
