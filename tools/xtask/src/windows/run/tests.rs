//! A whole run, on a repository made for the test: the `git` and the `tar` of the
//! machine, and an `scp` and an `ssh` that are shell scripts, which write down
//! what they were asked and do as the case wants. Where the system has no
//! `/bin/sh` the stand-ins cannot be made, and the cases are not: what they stand
//! for (a program that writes on two streams, that ends with a status, that is
//! ended by a stop) is tried on every system by the tests of `session`.
#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use cf_base::env::Env;

use super::*;
use crate::windows::options::DEFAULT_DIR;
use crate::windows::script::SYNCED;
use crate::windows::testing::script_of;

/// What an ssh is asked to run, as a person would read it: the script, decoded.
fn decoded(line: &str) -> String {
    let encoded = line
        .strip_prefix("powershell -NoProfile -NonInteractive -EncodedCommand ")
        .unwrap();
    script_of(encoded)
}

/// A machine for the tests: a checkout with files in it, and the folder of the
/// stand-ins' programs and of what they write down.
struct Rig {
    checkout: tempfile::TempDir,
    bin: tempfile::TempDir,
    context: Context,
}

impl Rig {
    fn new() -> Self {
        let checkout = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let path = std::env::join_paths(std::iter::once(bin.path().to_path_buf()).chain(
            std::env::split_paths(Env::from_process().os("PATH").unwrap()),
        ))
        .unwrap();
        let own = Env::from_process();
        let vars: Vec<(OsString, OsString)> = own
            .iter()
            .filter(|(name, _)| !name.to_string_lossy().starts_with("GIT_") && *name != "PATH")
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([
                ("PATH".into(), path),
                ("CF_LOG".into(), bin.path().join("log").into_os_string()),
                (
                    "CF_TARBALL".into(),
                    bin.path().join("tree.tgz").into_os_string(),
                ),
                ("CF_FIRST".into(), bin.path().join("first").into_os_string()),
            ])
            .collect();
        let context = Context {
            root: checkout.path().to_path_buf(),
            env: Env::from_vars(vars),
        };
        let rig = Self {
            checkout,
            bin,
            context,
        };
        rig.git(&["init", "-q"]);
        rig.file("keep.txt", "kept");
        rig.file("gone.txt", "to be deleted");
        rig.git(&["add", "-A"]);
        fs::remove_file(rig.checkout.path().join("gone.txt")).unwrap();
        rig
    }

    fn git(&self, words: &[&str]) {
        let asked = Invocation::new("git", &self.context.root).args(words);
        let answered = process::capture(&asked, &self.context.env).unwrap();
        assert_eq!(answered.code, 0, "{}", answered.stderr);
    }

    fn file(&self, name: &str, text: &str) {
        fs::write(self.checkout.path().join(name), text).unwrap();
    }

    /// A program of the machine this runs on, which is this script.
    fn program(&self, name: &str, body: &str) {
        let path = self.bin.path().join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// What the stand-ins wrote down, a line for each thing they were asked.
    fn log(&self) -> Vec<String> {
        fs::read_to_string(self.bin.path().join("log"))
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn options(&self) -> Options {
        Options {
            host: "zeewin".into(),
            build: false,
            dir: DEFAULT_DIR.into(),
            inside: ".".into(),
            env: vec![("CONSENSFLOW_TEST_DAEMON".into(), "native".into())],
            command: vec![
                "cargo".into(),
                "xtask".into(),
                "test".into(),
                "integration".into(),
            ],
        }
    }

    fn tarball(&self) -> PathBuf {
        self.bin.path().join("tree.tgz")
    }
}

/// An scp that keeps what it is sent, and says so.
const KEEPS: &str = r#"printf 'scp\t%s\t%s\n' "$1" "$3" >> "$CF_LOG"; cp "$2" "$CF_TARBALL""#;

/// An ssh as zeewin's is: it says a word on each stream, then how the command
/// ended (`== exit N`, which is `said`), and then ends with 1 if that was a
/// failure and 0 if not, for its shell makes every failure 1.
fn machine(said: i32) -> String {
    let flattened = i32::from(said != 0);
    format!(
        r#"printf 'ssh\t%s\t%s\n' "$1" "$2" >> "$CF_LOG"; echo "out"; echo "err" >&2; echo "== exit {said}"; exit {flattened}"#
    )
}

/// An ssh that says a word and ends with `status`, as a machine does that does not
/// say how the command ended (a connection that was lost is one).
fn silent(status: i32) -> String {
    format!(r#"printf 'ssh\t%s\t%s\n' "$1" "$2" >> "$CF_LOG"; echo "out"; exit {status}"#)
}

/// An ssh that goes on until it is ended (the first time), and that answers
/// the second time it is asked (the run's end there) with `status`. It writes
/// down its own process's id as well, which is what is to be gone.
fn holds(status: u8) -> String {
    format!(
        r#"printf 'ssh\t%s\t%s\t%s\n' "$1" "$2" "$$" >> "$CF_LOG"
if [ -e "$CF_FIRST" ]; then exit {status}; fi
: > "$CF_FIRST"
echo started
exec sleep 120"#
    )
}

fn run_it(rig: &Rig, interrupt: &Interrupt) -> (Result<Outcome, Error>, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let ended = run(
        &rig.context,
        &rig.options(),
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        interrupt,
    );
    (
        ended,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_run_sends_the_tree_then_runs_the_script_there_and_answers_what_the_command_answered() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    // The machine's lock: 75 said, and ssh ends with 1.
    rig.program("ssh", &machine(75));
    let (ended, out, err) = run_it(&rig, &Interrupt::default());
    assert_eq!(ended.unwrap(), Outcome::Exited(75));
    // What the machine said is the run's output, its last line too.
    assert_eq!((out.as_str(), err.as_str()), ("out\n== exit 75\n", "err\n"));

    // The tree went first, to the profile's `consensflow-build.tgz`, then ssh was asked.
    let log = rig.log();
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(log[0], "scp\t-q\tzeewin:consensflow-build.tgz");
    let ssh: Vec<&str> = log[1].split('\t').collect();
    assert_eq!(&ssh[..2], ["ssh", "zeewin"]);
    let command = command_line(&["cargo", "xtask", "test", "integration"]);
    let env = rig.options().env;
    assert_eq!(
        decoded(ssh[2]),
        remote_script(&Script {
            dir: DEFAULT_DIR,
            inside: ".",
            steps: &[],
            command: &command,
            env: &env,
        })
    );

    // What went over is the tree as it is on disk, with the list of it.
    let there = tempfile::tempdir().unwrap();
    let untar = Invocation::new("tar", there.path())
        .arg("xzf")
        .arg(rig.tarball())
        .arg("-C")
        .arg(there.path());
    assert_eq!(process::capture(&untar, &rig.context.env).unwrap().code, 0);
    assert_eq!(
        fs::read_to_string(there.path().join("keep.txt")).unwrap(),
        "kept"
    );
    assert!(!there.path().join("gone.txt").exists());
    assert_eq!(
        fs::read_to_string(there.path().join(SYNCED)).unwrap(),
        "keep.txt\n"
    );
    // And what it was made in is gone, with the list it was made with.
    assert!(!rig.checkout.path().join(SYNCED).exists());
}

#[test]
fn a_run_answers_what_the_machine_said_of_the_command_whatever_ssh_answered() {
    // The machine's shell turns a failure into 1 (the lock's 75, the command's 7),
    // and a success into 0: the run answers what was said, and says nothing more.
    for (said, ssh) in [(75, 1), (7, 1), (255, 1), (0, 0), (130, 1)] {
        let rig = Rig::new();
        rig.program("scp", KEEPS);
        rig.program("ssh", &machine(said));
        let (ended, _, err) = run_it(&rig, &Interrupt::default());
        assert_eq!(ended.unwrap(), Outcome::Exited(said), "ssh {ssh}");
        assert_eq!(err, "err\n", "{said}");
    }
}

#[test]
fn a_run_whose_machine_said_nothing_of_the_command_answers_what_ssh_did_and_says_so() {
    // A connection lost, a machine gone: ssh's status is what there is, and 0 is no success.
    for (ssh, answer, why) in [
        (255, 255, "so ssh's own status is the answer: 255"),
        (1, 1, "so ssh's own status is the answer: 1"),
        (
            0,
            1,
            "and ssh's status 0 is not taken for a command's without it: answering 1",
        ),
    ] {
        let rig = Rig::new();
        rig.program("scp", KEEPS);
        rig.program("ssh", &silent(ssh));
        let (ended, out, err) = run_it(&rig, &Interrupt::default());
        assert_eq!(ended.unwrap(), Outcome::Exited(answer), "ssh {ssh}");
        assert_eq!(out, "out\n");
        assert_eq!(
            err,
            format!("windows: no `== exit N` line ended the output, {why}\n"),
            "ssh {ssh}"
        );
    }
}

#[test]
fn a_run_asks_nothing_of_the_machine_when_the_tree_could_not_be_sent() {
    let rig = Rig::new();
    rig.program("scp", r#"echo "scp: connection refused" >&2; exit 3"#);
    rig.program("ssh", &machine(0));
    let (ended, out, err) = run_it(&rig, &Interrupt::default());
    let said = ended.unwrap_err().to_string();
    assert!(said.starts_with("`scp -q "), "{said}");
    assert!(
        said.ends_with(":consensflow-build.tgz` ended with status 3"),
        "{said}"
    );
    // What scp said is the run's own words, said as it said them.
    assert_eq!(
        (out.as_str(), err.as_str()),
        ("", "scp: connection refused\n")
    );
    assert!(rig.log().is_empty(), "{:?}", rig.log());
}

#[test]
fn a_step_that_failed_as_a_stop_ended_it_is_the_stop_when_the_stop_is_heard_a_moment_after() {
    let rig = Rig::new();
    // The scp that is ended by the Ctrl-C: it says it ran, and ends badly.
    rig.program("scp", r#": > "$CF_FIRST"; exit 3"#);
    rig.program("ssh", &machine(0));
    // The grace is long: the wait for the stop ends when the stop is heard.
    let interrupt = Interrupt::graced(std::time::Duration::from_secs(60));
    let late = interrupt.clone();
    let ran = rig.bin.path().join("first");
    // The program is told of the stop after the scp has failed, not before.
    let hearing = std::thread::spawn(move || {
        while !ran.exists() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        late.raise();
    });
    let (ended, out, err) = run_it(&rig, &interrupt);
    hearing.join().unwrap();
    assert_eq!(ended.unwrap(), Outcome::Stopped);
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    assert!(rig.log().is_empty(), "no ssh was asked: {:?}", rig.log());
}

#[test]
fn a_stop_ends_the_run_there_by_a_second_ssh_that_runs_the_script_that_ends_it() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &holds(0));
    let interrupt = Interrupt::default();
    let raise = interrupt.clone();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    // The stop is heard once the run has said it started.
    struct Then<'a>(&'a mut Vec<u8>, Interrupt);
    impl Write for Then<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(bytes);
            self.1.raise();
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let ended = run(
        &rig.context,
        &rig.options(),
        &mut Console {
            out: &mut Then(&mut out, raise),
            err: &mut err,
        },
        &interrupt,
    );
    assert_eq!(ended.unwrap(), Outcome::Stopped);
    assert_eq!(String::from_utf8(out).unwrap(), "started\n");
    assert!(err.is_empty());
    let log = rig.log();
    assert_eq!(log.len(), 3, "{log:?}");
    let ends: Vec<&str> = log[2].split('\t').collect();
    assert_eq!(&ends[..2], ["ssh", "zeewin"]);
    assert_eq!(decoded(ends[2]), stop_script(DEFAULT_DIR));
    // The ssh that held the run is gone: ended, before the machine was asked to end it.
    let held: Vec<&str> = log[1].split('\t').collect();
    assert!(!cf_process::alive(held[3].parse().unwrap()), "{log:?}");
}

#[test]
fn a_stop_the_machine_could_not_be_asked_to_carry_out_is_said() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &holds(5));
    let interrupt = Interrupt::default();
    let raise = interrupt.clone();
    struct Then(Interrupt);
    impl Write for Then {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.raise();
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut err = Vec::new();
    let ended = run(
        &rig.context,
        &rig.options(),
        &mut Console {
            out: &mut Then(raise),
            err: &mut err,
        },
        &interrupt,
    );
    assert_eq!(ended.unwrap(), Outcome::Stopped);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "windows: the run may still be going on the machine: ssh ended with status 5 when asked to end it\n"
    );
}

#[test]
fn a_stop_heard_before_ssh_has_started_ends_nothing_there_for_nothing_has_started() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &machine(0));
    let interrupt = Interrupt::default();
    interrupt.raise();
    let (ended, out, err) = run_it(&rig, &interrupt);
    assert_eq!(ended.unwrap(), Outcome::Stopped);
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    assert!(rig.log().is_empty(), "{:?}", rig.log());
}

#[test]
fn a_run_whose_output_cannot_be_written_is_ended_there_too_and_says_why() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &holds(0));
    let mut err = Vec::new();
    let ended = run(
        &rig.context,
        &rig.options(),
        &mut Console {
            out: &mut Closed,
            err: &mut err,
        },
        &Interrupt::default(),
    );
    let said = ended.unwrap_err().to_string();
    assert_eq!(said, "could not write what the run says: closed");
    // The machine was asked to end it: a third thing written down.
    assert_eq!(rig.log().len(), 3, "{:?}", rig.log());
}

#[test]
fn what_cannot_be_sent_is_refused_before_anything_is_done() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &machine(0));
    let mut options = rig.options();
    options.env = (0..12)
        .map(|at| (format!("CONSENSFLOW_TEST_VARIABLE_{at}"), "x".repeat(40)))
        .collect();
    options.build = true;
    let refused = check(&options).unwrap_err().to_string();
    assert!(
        refused.starts_with("the script for the machine is "),
        "{refused}"
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let ended = run(
        &rig.context,
        &options,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &Interrupt::default(),
    );
    assert_eq!(ended.unwrap_err().to_string(), refused);
    assert!(rig.log().is_empty());
}
