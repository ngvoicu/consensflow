//! A process that is none of any window's, for the tests of what an ending or a close of
//! a window may signal: a shell the test starts, in a session of its own (`setsid`), with
//! a job of its own, both asleep. The pane host signals only the window's own, whatever its
//! selection says, so an ending or a close that is right never reaches them, and a test says
//! so once it is over ([`Sentinel::runs`]), and once more when the sentinel is dropped, which
//! a test that failed before its own look at it reaches too. Every test that ends or closes a
//! window of real processes has one: the bound of the tree is what keeps a wrong selection
//! from them, and this is what shows it did.

use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

use super::process_exists;

unsafe extern "C" {
    fn setsid() -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
}

const SIGTERM: i32 = 15;

/// What a test that finds its sentinel ended says.
const ENDED: &str = "a process outside the window was ended";

/// A shell and its job, in a session of their own, outside every window.
pub struct Sentinel {
    shell: Child,
    job: i32,
}

impl Sentinel {
    /// Starts the shell and its job, and waits for the job's number.
    #[allow(clippy::disallowed_methods)] // A test's own programs.
    #[allow(clippy::expect_used)] // A test helper: a sentinel that did not start fails the test.
    pub fn start() -> Self {
        let mut command = Command::new("/bin/sh");
        // The trap is set once the job is started: a job still to exec has its shell's
        // traps, which take the request to end in the shell's place, and it outlives it.
        command
            .args([
                "-c",
                "sleep 120 & job=$!; trap 'kill $job 2>/dev/null; exit 0' TERM; echo $job; wait $job",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // SAFETY: setsid is async-signal-safe and touches no memory of ours.
        unsafe {
            command.pre_exec(|| {
                setsid();
                Ok(())
            });
        }
        let mut shell = command.spawn().expect("start the sentinel");
        let mut line = String::new();
        BufReader::new(shell.stdout.take().expect("its output"))
            .read_line(&mut line)
            .expect("its job's number");
        let job = line.trim().parse().expect("a process number");
        Self { shell, job }
    }

    /// Whether the shell and its job run still.
    #[allow(clippy::expect_used)] // A test helper.
    pub fn runs(&mut self) -> bool {
        self.shell
            .try_wait()
            .expect("look at the sentinel")
            .is_none()
            && process_exists(self.job)
    }
}

impl Drop for Sentinel {
    /// What the sentinel is for is checked here whatever became of the test, so that a test
    /// that failed before its own look at it still says if it was ended: it fails with it,
    /// or where it failed already, says so in its output. Then the shell is asked to end,
    /// which ends its job with it: it is the test's own child, which nothing has reaped, so
    /// its number is still its own.
    #[allow(clippy::print_stderr)] // Said in the output of the test that failed.
    fn drop(&mut self) {
        let outlived = self.runs();
        if let Ok(shell) = i32::try_from(self.shell.id()) {
            // SAFETY: kill takes two integers and touches no memory of ours.
            unsafe { kill(shell, SIGTERM) };
        }
        let _ = self.shell.wait();
        if outlived {
            return;
        }
        if std::thread::panicking() {
            eprintln!("{ENDED}");
        } else {
            panic!("{ENDED}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn a_sentinel_runs_until_it_is_dropped_and_its_job_goes_with_it() {
        let mut sentinel = Sentinel::start();
        let job = sentinel.job;
        assert!(sentinel.runs());

        drop(sentinel);

        let deadline = Instant::now() + Duration::from_secs(5);
        while process_exists(job) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!process_exists(job), "the shell asked to end took its job");
    }

    #[test]
    fn a_sentinel_dropped_as_soon_as_it_starts_takes_its_job_with_it() {
        // The job is started with the shell's traps until it executes: a request to end
        // that reaches it then is taken in its place, and it would outlive the shell.
        for _ in 0..20 {
            let sentinel = Sentinel::start();
            let job = sentinel.job;
            drop(sentinel);
            let deadline = Instant::now() + Duration::from_secs(5);
            while process_exists(job) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(!process_exists(job), "the job {job} outlived its shell");
        }
    }

    #[test]
    fn a_sentinel_that_was_ended_fails_the_test_that_drops_it() {
        let mut sentinel = Sentinel::start();
        let job = sentinel.job;
        sentinel.shell.kill().unwrap();
        // SAFETY: kill takes two integers and touches no memory of ours; the job is the
        // test's own.
        unsafe { kill(job, 9) };
        let _ = sentinel.shell.wait();
        assert!(!sentinel.runs());

        let dropped = catch_unwind(AssertUnwindSafe(move || drop(sentinel)));

        assert!(dropped.is_err(), "it said so");
    }
}
