//! The guard of a test build's real signals, on real processes and no signal: each test asks
//! what the guard would allow, or has a sender stop, and a sender that is stopped sends
//! nothing. A signal that is let through here would be signal 0, which delivers nothing.

use std::os::unix::process::CommandExt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::{Child, Command, Stdio};

use super::*;
use crate::process_tree::native;
use crate::pty::Sentinel;

/// Runs `step`, and says what it panicked with: none where it did not.
fn panic_of(step: impl FnOnce()) -> Option<String> {
    catch_unwind(AssertUnwindSafe(step)).err().map(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
            .unwrap_or_default()
    })
}

/// A process the test starts, which sleeps: its child.
#[allow(clippy::disallowed_methods)] // A test's own programs.
fn sleeper() -> Child {
    Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .spawn()
        .unwrap()
}

/// Ends a process the test started, and waits for it.
fn end(mut child: Child) {
    child.kill().unwrap();
    child.wait().unwrap();
}

/// Ends a process of the test's, by its number: what an orphan the test made needs, whose
/// parent is not the test.
fn end_by_number(pid: i32) {
    // SAFETY: kill takes two integers and touches no memory of ours; the process is one
    // this test started.
    unsafe { libc::kill(pid, libc::SIGKILL) };
}

/// How the guard is told that a shell is a pane's program.
#[derive(Clone, Copy)]
enum Told {
    /// It is not.
    No,
    /// By the guard's own note of it.
    ByNote,
    /// By a tree attaching to it, as a pane's does as it opens.
    ByAttach,
}

/// A shell that is a session leader, which starts a job that sleeps and then ends, leaving the
/// job to the system's init in the session of the shell: the shell's number and the job's.
/// The shell waits for its input to end, as the tree attaches to it, and the guard is `told`.
#[allow(clippy::disallowed_methods)] // A test's own programs.
fn a_job_its_shell_left(told: Told) -> (i32, i32) {
    use std::io::{BufRead, BufReader};

    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "/bin/sleep 30 & echo $!; read nothing"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and touches no memory of ours.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut shell = command.spawn().unwrap();
    let program = i32::try_from(shell.id()).unwrap();
    match told {
        Told::No => {}
        Told::ByNote => note_program(program),
        Told::ByAttach => crate::process_tree::ProcessTree::new().attach(program),
    }
    let mut line = String::new();
    BufReader::new(shell.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    // Its input ends, and the shell with it.
    drop(shell.stdin.take());
    shell.wait().unwrap();
    (program, line.trim().parse().unwrap())
}

#[test]
#[allow(clippy::disallowed_methods)] // A test's own programs.
fn a_child_of_the_test_and_its_child_are_allowed() {
    use std::io::{BufRead, BufReader};

    let child = sleeper();
    assert_eq!(refusal(i32::try_from(child.id()).unwrap()), None);

    let mut shell = Command::new("/bin/sh")
        .args(["-c", "/bin/sleep 30 & echo $!; wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(shell.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let grandchild: i32 = line.trim().parse().unwrap();

    assert_eq!(refusal(grandchild), None);
    assert_eq!(refusal(i32::try_from(shell.id()).unwrap()), None);

    end_by_number(grandchild);
    end(shell);
    end(child);
}

#[test]
fn the_process_that_started_the_test_is_refused_and_nothing_is_sent() {
    // SAFETY: getppid takes no arguments, reads no memory of ours and cannot fail.
    let parent = unsafe { libc::getppid() };

    let said = panic_of(|| drop(before_sending(parent, 0))).expect("the guard stopped it");

    assert!(said.contains("the test guard stopped signal 0"), "{said}");
    assert!(said.contains(&format!("the process {parent}")), "{said}");
    assert!(said.contains("nothing was sent"), "{said}");
    // The sender stops at the guard, before it sends: the signal here is 0, which delivers
    // nothing and says nothing either way, so what is held is that the sender panics.
    let process = native::process(parent).expect("the test's parent is one of this user's");
    assert!(panic_of(|| {
        let _ = native::signal(process.identity, 0);
    })
    .is_some());
}

#[test]
fn a_test_that_is_panicking_already_is_told_the_signal_failed_and_does_not_panic_again() {
    // A table dropped as a test unwinds closes its windows, which signals.
    struct Closes(std::sync::mpsc::Sender<std::io::Result<()>>, i32);
    impl Drop for Closes {
        fn drop(&mut self) {
            let _ = self.0.send(before_sending(self.1, 0));
        }
    }
    // SAFETY: getppid takes no arguments, reads no memory of ours and cannot fail.
    let parent = unsafe { libc::getppid() };
    let (sender, receiver) = std::sync::mpsc::channel();

    let unwound = catch_unwind(AssertUnwindSafe(move || {
        let _closes = Closes(sender, parent);
        panic!("the test failed");
    }));

    assert!(unwound.is_err());
    let told = receiver.recv().unwrap().unwrap_err();
    assert_eq!(told.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(told.to_string().contains("nothing was sent"), "{told}");
}

#[test]
fn the_test_process_itself_is_refused() {
    let own = std::process::id() as i32;

    assert!(refusal(own).is_some());
}

#[test]
fn a_sentinel_is_refused_as_a_stranger_is_by_the_senders_of_a_process_and_of_a_group() {
    let sentinel = Sentinel::start();
    let shell = sentinel.pid();
    // SAFETY: getpgid takes a number and reads no memory of ours.
    let group = unsafe { libc::getpgid(shell) };
    assert!(group > 1, "the sentinel has a group of its own");

    assert!(refusal(shell).is_some());
    assert!(panic_of(|| drop(before_sending(shell, 0))).is_some());
    assert!(panic_of(|| drop(before_sending_to_group(group, 0))).is_some());
    let process = native::process(shell).expect("the sentinel is one of this user's");
    assert!(panic_of(|| {
        let _ = native::signal(process.identity, 0);
    })
    .is_some());
    assert!(panic_of(|| {
        let _ = native::signal_group(group, 0);
    })
    .is_some());
}

#[test]
fn the_group_of_the_test_is_refused_whole_for_what_is_in_it_that_is_not_the_tests() {
    // SAFETY: getpgrp takes no arguments, reads no memory of ours and cannot fail.
    let group = unsafe { libc::getpgrp() };

    // The shell and the cargo that run the test are in it.
    assert!(panic_of(|| drop(before_sending_to_group(group, 0))).is_some());
}

#[test]
#[allow(clippy::disallowed_methods)] // A test's own programs.
fn a_group_of_processes_of_the_tests_is_allowed_whole() {
    let mut command = Command::new("/bin/sleep");
    command.arg("30").stdin(Stdio::null()).process_group(0);
    let child = command.spawn().unwrap();
    let group = i32::try_from(child.id()).unwrap();

    before_sending_to_group(group, 0).unwrap();

    end(child);
}

#[test]
fn a_process_that_has_gone_is_allowed_there_is_nothing_to_reach() {
    let mut child = sleeper();
    let pid = i32::try_from(child.id()).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!(refusal(pid), None);
}

#[test]
fn a_job_in_the_session_of_a_program_the_process_started_is_allowed_when_its_shell_has_gone() {
    let (program, job) = a_job_its_shell_left(Told::ByNote);

    assert_eq!(
        refusal(job),
        None,
        "the job is a child of the system's init, in the session of {program}"
    );

    end_by_number(job);
}

#[test]
fn a_tree_notes_the_program_it_attaches_to_for_the_guard_as_a_pane_does_as_it_opens() {
    let (program, job) = a_job_its_shell_left(Told::ByAttach);

    assert_eq!(
        refusal(job),
        None,
        "the tree attached to {program}, whose session the job is in"
    );

    end_by_number(job);
}

#[test]
fn a_job_in_a_session_of_no_program_the_process_started_is_refused() {
    let (_, job) = a_job_its_shell_left(Told::No);

    assert!(refusal(job).is_some());

    end_by_number(job);
}

#[test]
fn a_session_is_a_programs_while_no_process_that_began_later_holds_its_number() {
    let programs = [(500, (7, 1)), (600, (8, 2)), (600, (9, 3))];

    assert!(!is_ours(400, &programs, None), "no program had it");
    assert!(is_ours(500, &programs, Some((7, 1))), "the program runs");
    assert!(
        is_ours(500, &programs, None),
        "it has gone, its session held"
    );
    assert!(
        !is_ours(500, &programs, Some((20, 0))),
        "the number is another's"
    );
    assert!(
        is_ours(600, &programs, Some((9, 3))),
        "the later program with the number"
    );
    assert!(
        !is_ours(600, &programs, Some((10, 0))),
        "neither: another's"
    );
}

#[test]
fn a_registered_process_is_allowed_by_its_number_and_when_it_began_and_one_that_took_its_number_is_not(
) {
    let sentinel = Sentinel::start();
    let shell = sentinel.pid();
    assert!(
        refusal(shell).is_some(),
        "a stranger before it is registered"
    );

    register_began(shell, (1, 1));
    assert!(
        refusal(shell).is_some(),
        "registered as another process that began"
    );

    register(shell);
    assert_eq!(refusal(shell), None);
}
