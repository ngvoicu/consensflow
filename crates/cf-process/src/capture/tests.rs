use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use super::*;
use crate::execute;

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

fn run_now(run: &Run, cwd: Option<&Path>, limits: Limits) -> Result<Captured, CaptureFailed> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(capture(run, cwd, &system_env(), limits))
}

const ROOMY: Limits = Limits {
    timeout: Duration::ZERO,
    max_buffer: 1024 * 1024,
};

/// How a line ends in what a shell writes here.
const EOL: &str = if cfg!(windows) { "\r\n" } else { "\n" };

/// A shell script that writes `out` to its standard output and `err` to its
/// standard error, then `more`.
fn writes(out: &str, err: &str, more: &str) -> Run {
    if cfg!(windows) {
        shell(&format!("echo {out}& (echo {err})1>&2{more}"))
    } else {
        shell(&format!("echo {out}; echo {err} 1>&2{more}"))
    }
}

#[test]
fn a_program_that_ends_with_0_answers_what_it_wrote_to_both_streams() {
    let answered = run_now(&writes("out", "err", ""), None, ROOMY).unwrap();
    assert_eq!(answered.stdout, format!("out{EOL}"));
    assert_eq!(answered.stderr, format!("err{EOL}"));
}

#[test]
fn another_exit_says_the_command_and_its_stderr_and_keeps_both_streams() {
    // Probed on Node v26.8.1: `execFile` of a program writing "out" and "err"
    // and exiting 3 failed with `Command failed: <cmd>\nerr`, its `stdout` and
    // `stderr` the two texts.
    let run = writes(
        "out",
        "err",
        if cfg!(windows) {
            "& exit 3"
        } else {
            "; exit 3"
        },
    );
    let failed = run_now(&run, None, ROOMY).unwrap_err();
    assert_eq!(failed.code, Some(3));
    assert!(!failed.killed);
    assert_eq!(
        failed.message,
        format!("Command failed: {}\nerr{EOL}", command_line(&run))
    );
    assert_eq!(failed.stdout, format!("out{EOL}"));
    assert_eq!(failed.stderr, format!("err{EOL}"));
}

#[test]
fn a_program_out_of_time_is_said_killed_with_what_it_wrote_to_each_stream() {
    // Probed on Node v26.8.1: a program that wrote "hi" and outlived a 300 ms
    // timeout failed with `killed: true`, no code, and "hi" in `stdout`.
    let tail = if cfg!(windows) {
        "& ping -n 30 127.0.0.1 >NUL"
    } else {
        "; exec sleep 30"
    };
    let limits = Limits {
        timeout: Duration::from_millis(300),
        max_buffer: 1024,
    };
    let run = writes("hi", "slow", tail);
    let failed = run_now(&run, None, limits).unwrap_err();
    assert!(failed.killed);
    assert_eq!(failed.code, None);
    assert_eq!(failed.stdout, format!("hi{EOL}"));
    assert_eq!(failed.stderr, format!("slow{EOL}"));
    assert!(
        failed
            .message
            .starts_with(&format!("Command failed: {}\nslow", command_line(&run))),
        "{}",
        failed.message
    );
}

#[test]
fn a_stream_that_says_too_much_is_named_in_the_message_and_kept_as_far_as_it_was_read() {
    // Probed on Node v26.8.1: `maxBuffer` 10 against 100 characters on either
    // stream, `stdout maxBuffer length exceeded` (or `stderr`), the stream's
    // first 10 characters kept, the other stream empty.
    let limits = Limits {
        timeout: Duration::ZERO,
        max_buffer: 10,
    };
    let failed = run_now(&shell("echo xxxxxxxxxxxxxxxxxxxx"), None, limits).unwrap_err();
    assert_eq!(failed.message, "stdout maxBuffer length exceeded");
    assert_eq!(failed.stdout, "xxxxxxxxxx");
    assert_eq!(failed.stderr, "");
    assert!(!failed.killed);
    assert_eq!(failed.code, None);
    let failed = run_now(&shell("echo yyyyyyyyyyyyyyyyyyyy 1>&2"), None, limits).unwrap_err();
    assert_eq!(failed.message, "stderr maxBuffer length exceeded");
    assert_eq!(failed.stdout, "");
    assert_eq!(failed.stderr, "yyyyyyyyyy");
}

#[test]
fn a_program_that_is_not_there_says_spawn_and_enoent_with_nothing_written() {
    let run = Run {
        program: PathBuf::from("/nonexistent/cli"),
        args: vec![OsString::from("update")],
        verbatim: false,
    };
    let failed = run_now(&run, None, ROOMY).unwrap_err();
    assert_eq!(failed.message, "spawn /nonexistent/cli ENOENT");
    assert_eq!(failed.code, None);
    assert!(!failed.killed);
    assert_eq!((failed.stdout.as_str(), failed.stderr.as_str()), ("", ""));
}

#[cfg(unix)]
#[test]
fn both_streams_are_read_at_once_so_a_program_filling_one_never_waits_on_the_other() {
    // Each stream is filled past a pipe's buffer, the standard error first:
    // read one after the other, the program would block on the stream not
    // yet read and the run would end only at its limit.
    let run =
        shell("head -c 200000 /dev/zero | tr '\\0' x 1>&2; head -c 200000 /dev/zero | tr '\\0' y");
    let limits = Limits {
        timeout: Duration::from_secs(20),
        max_buffer: 1024 * 1024,
    };
    let answered = run_now(&run, None, limits).unwrap();
    assert_eq!(answered.stdout, "y".repeat(200_000));
    assert_eq!(answered.stderr, "x".repeat(200_000));
}

#[cfg(unix)]
#[test]
fn a_program_a_signal_ended_has_no_code_and_both_streams_as_written() {
    let run = writes("out", "err", "; kill -9 $$");
    let failed = run_now(&run, None, ROOMY).unwrap_err();
    assert_eq!(failed.code, None);
    assert!(!failed.killed);
    assert_eq!(failed.stdout, "out\n");
    assert_eq!(failed.stderr, "err\n");
    assert_eq!(
        failed.message,
        format!("Command failed: {}\nerr\n", command_line(&run))
    );
}

#[cfg(unix)]
#[test]
fn the_program_runs_in_the_folder_given() {
    let folder = tempfile::tempdir().unwrap();
    let answered = run_now(&shell("pwd"), Some(folder.path()), ROOMY).unwrap();
    let real = std::fs::canonicalize(folder.path()).unwrap();
    assert_eq!(answered.stdout, format!("{}\n", real.display()));
}

#[test]
fn capture_and_execute_say_the_same_of_every_way_a_program_ends() {
    // `capture` runs a program as `execute` does and keeps the other stream
    // besides: one text, the other's failure, so a change to either that the
    // other does not follow is caught here.
    let mut cases = vec![
        (writes("out", "err", ""), ROOMY),
        (
            writes(
                "out",
                "err",
                if cfg!(windows) {
                    "& exit 3"
                } else {
                    "; exit 3"
                },
            ),
            ROOMY,
        ),
        (
            shell("echo xxxxxxxxxxxxxxxxxxxx"),
            Limits {
                timeout: Duration::ZERO,
                max_buffer: 10,
            },
        ),
        (
            Run {
                program: PathBuf::from("/nonexistent/cli"),
                args: Vec::new(),
                verbatim: false,
            },
            ROOMY,
        ),
    ];
    if cfg!(windows) {
        cases.push((
            writes("hi", "slow", "& ping -n 30 127.0.0.1 >NUL"),
            Limits {
                timeout: Duration::from_millis(300),
                max_buffer: 1024,
            },
        ));
    } else {
        cases.push((
            writes("hi", "slow", "; exec sleep 30"),
            Limits {
                timeout: Duration::from_millis(300),
                max_buffer: 1024,
            },
        ));
        cases.push((writes("out", "err", "; kill -9 $$"), ROOMY));
    }
    for (run, limits) in cases {
        let ran = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(execute(&run, None, &system_env(), limits));
        let captured = run_now(&run, None, limits);
        match (ran, captured) {
            (Ok(stdout), Ok(captured)) => assert_eq!(stdout, captured.stdout),
            (Err(ran), Err(captured)) => {
                assert_eq!(ran.message, captured.message, "{run:?}");
                assert_eq!(ran.code, captured.code, "{run:?}");
                assert_eq!(ran.killed, captured.killed, "{run:?}");
                assert_eq!(ran.stdout, captured.stdout, "{run:?}");
            }
            (ran, captured) => panic!("{run:?}: execute {ran:?}, capture {captured:?}"),
        }
    }
}
