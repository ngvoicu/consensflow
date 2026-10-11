//! A run of ssh, tried on programs of the tests' own: the test program itself,
//! started to run one of its ignored tests as the program a case needs (it speaks
//! on both streams, it holds on to be ended, it exits as it is told), the way
//! `plants::stops` tries a stop. Which of them runs, and what it waits for, is
//! said by the case in variables and files, so that no case depends on how long
//! anything takes.

use std::fs;
use std::path::{Path, PathBuf};

use super::*;

const SAYS: &str = "windows::session::tests::helper_speaks";
const HOLDS: &str = "windows::session::tests::helper_holds";
const EXITS: &str = "windows::session::tests::helper_exits";

/// The file a program waits for, which the case makes when it has seen what it waits to see.
const FLAG: &str = "CF_WINDOWS_TEST_FLAG";
/// The status a program exits with.
const STATUS: &str = "CF_WINDOWS_TEST_STATUS";

/// The test program, started to run `helper`, which the cases give variables.
fn program(helper: &str) -> Invocation {
    let this = std::env::current_exe().unwrap();
    Invocation::new(this, ".").args(["--ignored", "--exact", helper, "--nocapture"])
}

fn flag_of(env: &Env) -> PathBuf {
    PathBuf::from(env.os(FLAG).unwrap())
}

/// Waits for the file the case makes: for a program that is to go on only when
/// the case has seen what it said.
fn wait_for_flag(env: &Env) {
    let flag = flag_of(env);
    for _ in 0..600 {
        if flag.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    // The case never made it: a run that ends with this is no run the case asked for.
    std::process::exit(99);
}

fn status_of(env: &Env) -> i32 {
    env.text(STATUS)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0)
}

/// A program that says a line on each stream, waits for the case to have seen
/// the first, says one more, and exits as it is told.
#[test]
#[ignore = "a program the tests of session start"]
fn helper_speaks() {
    let env = Env::from_process();
    println!("out one");
    eprintln!("err one");
    wait_for_flag(&env);
    println!("out two");
    std::process::exit(status_of(&env));
}

/// A program that says who it is and then goes on until it is ended: or for a
/// minute, which no case waits for (a run that has not ended it by then is the
/// case's failure, and it is not left running for ever).
#[test]
#[ignore = "a program the tests of session start"]
fn helper_holds() {
    println!("held {}", std::process::id());
    for _ in 0..60 {
        thread::sleep(Duration::from_secs(1));
    }
    std::process::exit(98);
}

/// A program that waits for the case to have made its file, and exits as it is told.
#[test]
#[ignore = "a program the tests of session start"]
fn helper_exits() {
    let env = Env::from_process();
    wait_for_flag(&env);
    std::process::exit(status_of(&env));
}

/// A stream that keeps what it is written, and when it has seen `seen` does what the case says.
struct Says<F: FnMut()> {
    text: Vec<u8>,
    seen: &'static str,
    then: F,
    done: bool,
}

impl<F: FnMut()> Says<F> {
    fn new(seen: &'static str, then: F) -> Self {
        Self {
            text: Vec::new(),
            seen,
            then,
            done: false,
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.text).into_owned()
    }
}

impl<F: FnMut()> Write for Says<F> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.text.extend_from_slice(bytes);
        if !self.done && self.text().contains(self.seen) {
            self.done = true;
            (self.then)();
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A stream that cannot be written.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "the pipe is closed",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The environment of the test, with what a case gives the program it starts.
fn with(vars: &[(&str, &Path)], status: &str) -> Env {
    let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = Env::from_process()
        .iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    env.extend(
        vars.iter()
            .map(|(name, value)| ((*name).into(), value.as_os_str().to_owned())),
    );
    env.push((STATUS.into(), status.into()));
    Env::from_vars(env)
}

/// The pid a program that holds says it has.
fn pid_in(text: &str) -> u32 {
    text.lines()
        .find_map(|line| line.strip_prefix("held "))
        .and_then(|pid| pid.trim().parse().ok())
        .unwrap_or_else(|| panic!("no pid in {text:?}"))
}

#[test]
fn what_a_run_writes_is_written_as_it_comes_to_the_stream_it_came_on_and_its_status_is_the_runs() {
    let dir = tempfile::tempdir().unwrap();
    let flag = dir.path().join("flag");
    let env = with(&[(FLAG, &flag)], "75");
    // The program goes on only when its first line has been written here: that it
    // is, while the program is still running, is what writing it as it comes is.
    let mut out = Says::new("out one", || fs::write(&flag, "").unwrap());
    let mut err = Vec::new();
    let ended = stream(
        &program(SAYS),
        &env,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &Interrupt::default(),
    )
    .unwrap();
    assert!(matches!(ended, Ended::Exited(75)), "{ended:?}");
    let (out, err) = (out.text(), String::from_utf8(err).unwrap());
    assert!(
        out.find("out one").unwrap() < out.find("out two").unwrap(),
        "{out}"
    );
    assert!(err.contains("err one"), "{err}");
    assert!(
        !err.contains("out ") && !out.contains("err one"),
        "{out}{err}"
    );
}

#[test]
fn the_status_a_run_ends_with_is_the_status_of_the_program_whatever_it_is() {
    for status in ["0", "1", "3", "75", "254"] {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join("flag");
        fs::write(&flag, "").unwrap();
        let env = with(&[(FLAG, &flag)], status);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ended = stream(
            &program(EXITS),
            &env,
            &mut Console {
                out: &mut out,
                err: &mut err,
            },
            &Interrupt::default(),
        )
        .unwrap();
        let expected: i32 = status.parse().unwrap();
        assert!(
            matches!(ended, Ended::Exited(code) if code == expected),
            "{status}: {ended:?}"
        );
    }
}

#[test]
fn a_stop_ends_the_program_whole_and_is_said_and_what_it_had_written_is_kept() {
    let interrupt = Interrupt::default();
    let raise = interrupt.clone();
    let mut out = Says::new("held ", move || raise.raise());
    let mut err = Vec::new();
    let ended = stream(
        &program(HOLDS),
        &Env::from_process(),
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &interrupt,
    )
    .unwrap();
    assert!(matches!(ended, Ended::Stopped), "{ended:?}");
    // Ended, and waited for: it is not there.
    assert!(!cf_process::alive(pid_in(&out.text())), "{}", out.text());
}

#[test]
fn a_run_whose_output_cannot_be_written_is_ended_and_says_why() {
    let mut err = Vec::new();
    let ended = stream(
        &program(HOLDS),
        &Env::from_process(),
        &mut Console {
            out: &mut Closed,
            err: &mut err,
        },
        &Interrupt::default(),
    )
    .unwrap();
    let Ended::Unwritten(cause) = ended else {
        panic!("{ended:?}");
    };
    assert_eq!(cause.kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn a_status_a_stop_may_have_made_waits_a_moment_for_the_stop_and_another_does_not() {
    // What ssh ends with when the Ctrl-C ended it (a signal, or its own 255), and
    // the stop is heard a moment after: within the grace, which here is long, for
    // the wait ends when the stop is heard.
    let interrupt = Interrupt::graced(Duration::from_secs(60));
    let late = interrupt.clone();
    let telling = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        late.raise();
    });
    assert!(matches!(exited(Some(255), &interrupt), Ended::Stopped));
    telling.join().unwrap();
    // Not heard within the grace, it is the status it is (a signal is told as 1).
    assert!(matches!(
        exited(Some(255), &Interrupt::default()),
        Ended::Exited(255)
    ));
    assert!(matches!(
        exited(None, &Interrupt::default()),
        Ended::Exited(1)
    ));
    // Heard, the same two are the stop's; any other status is the run's, heard or not.
    assert!(matches!(exited(Some(255), &interrupt), Ended::Stopped));
    assert!(matches!(exited(None, &interrupt), Ended::Stopped));
    assert!(matches!(exited(Some(3), &interrupt), Ended::Exited(3)));
    assert!(matches!(exited(Some(0), &interrupt), Ended::Exited(0)));
}

#[test]
fn a_program_that_is_not_there_is_said_and_starts_nothing() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let missing = Invocation::new("no-such-program-for-the-windows-tests", ".");
    let said = stream(
        &missing,
        &Env::default(),
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &Interrupt::default(),
    )
    .unwrap_err();
    assert!(
        matches!(said, Error::Process(process::Failure::NotFound { .. })),
        "{said}"
    );
}
