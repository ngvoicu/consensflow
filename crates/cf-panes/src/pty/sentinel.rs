//! A process that is none of any window's, for the tests of what a close of a window may
//! signal: as the user's own programs are beside a window, it is none of the
//! test's either. A shell the test starts detached (a child of the system's init, in a
//! session of its own: no process of the test started it), with a job of its own, both
//! asleep. The pane host signals only the window's own, whatever its selection says, so a
//! close that is right never reaches them, and a test says so once it is over
//! ([`Sentinel::runs`]), and once more when the sentinel is dropped, which a test that
//! failed before its own look at it reaches too. Every test that closes a window of real
//! processes has one: the bound of the tree is what keeps a wrong selection from them,
//! and this is what shows it did. The guard of a test build's real signals
//! (`process_tree::guard`) refuses it as a stranger, which a child of the test would not be.

use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::process_exists;

unsafe extern "C" {
    fn setsid() -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
}

const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;

/// What a test that finds its sentinel ended says.
const ENDED: &str = "a process outside the window was ended";

/// What the first shell runs: the second, in the background, which it leaves to the system's
/// init as it ends. The second starts its job, sets its trap once the job has started (a
/// job still to exec has its shell's traps, which take the request to end in the shell's
/// place, and it outlives it), says its number and its job's, and waits: for its job, and
/// for as long as the test that started it is there, which is not its parent (T-693).
const FIRST: &str = "/bin/sh -c \"$0\" &";

/// What the second shell runs, for as long as the process `owner` is there.
fn second(owner: u32) -> String {
    format!(
        "sleep 120 & job=$!; trap 'kill $job 2>/dev/null; exit 0' TERM; echo $$ $job; \
         while kill -0 $job 2>/dev/null && {}; do sleep 0.2; done; kill $job 2>/dev/null",
        cf_base::fixture::owner_there(owner)
    )
}

/// A process by its number and by when it began, as `ps` says it: one that took the number
/// of another is not it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Process {
    pid: i32,
    began: String,
}

impl Process {
    /// The process `pid` as it is now.
    #[allow(clippy::disallowed_methods)] // A test's own programs.
    fn of(pid: i32) -> Option<Self> {
        let ran = Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let began = String::from_utf8_lossy(&ran.stdout).trim().to_owned();
        (!began.is_empty()).then_some(Self { pid, began })
    }

    /// Whether it is there, and is the process it was.
    fn runs(&self) -> bool {
        process_exists(self.pid) && Self::of(self.pid).is_some_and(|now| now == *self)
    }

    /// Ends it with `signal`, if it is the process it was.
    fn end(&self, signal: i32) {
        if self.runs() {
            // SAFETY: kill takes two integers and touches no memory of ours; the process is
            // the one this was made of, which `runs` has just seen.
            unsafe { kill(self.pid, signal) };
        }
    }

    /// Waits until it is gone, up to five seconds.
    fn until_gone(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while process_exists(self.pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// A shell and its job, detached, in a session of their own, outside every window and the
/// test that keeps them.
pub struct Sentinel {
    shell: Process,
    job: Process,
}

impl Sentinel {
    /// Starts the shell and its job, and waits for them to be left to the system's init. They
    /// end when the test that started them does.
    pub fn start() -> Self {
        Self::start_owned_by(std::process::id())
    }

    /// As [`Sentinel::start`], the shell and its job going on for as long as the process
    /// `owner` is there.
    #[allow(clippy::disallowed_methods)] // A test's own programs.
    #[allow(clippy::expect_used)] // A test helper: a sentinel that did not start fails the test.
    fn start_owned_by(owner: u32) -> Self {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", FIRST, &second(owner)])
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
        // It outlives the test, beside the others: what it was left of theirs it would hold.
        cf_process::hold_only_the_streams(&mut command);
        let mut first = command.spawn().expect("start the sentinel");
        let mut line = String::new();
        BufReader::new(first.stdout.take().expect("its output"))
            .read_line(&mut line)
            .expect("its numbers");
        // The first shell has ended once it is waited for: the second is no child of ours.
        first.wait().expect("the first shell ends");
        let numbers: Vec<i32> = line
            .split_whitespace()
            .map(|number| number.parse().expect("a process number"))
            .collect();
        let [shell, job] = numbers[..] else {
            panic!("the sentinel said {line:?}, not its number and its job's");
        };
        Self {
            shell: Process::of(shell).expect("the sentinel's shell runs"),
            job: Process::of(job).expect("the sentinel's job runs"),
        }
    }

    /// The number of the shell.
    pub fn pid(&self) -> i32 {
        self.shell.pid
    }

    /// Whether the shell and its job run still.
    pub fn runs(&mut self) -> bool {
        process_exists(self.shell.pid) && process_exists(self.job.pid)
    }
}

impl Drop for Sentinel {
    /// What the sentinel is for is checked here whatever became of the test, so that a test
    /// that failed before its own look at it still says if it was ended: it fails with it,
    /// or where it failed already, says so in its output. Then the shell is asked to end,
    /// which ends its job with it, and what is left of either is ended, each if it is the
    /// process it was.
    #[allow(clippy::print_stderr)] // Said in the output of the test that failed.
    fn drop(&mut self) {
        let outlived = self.runs();
        self.shell.end(SIGTERM);
        self.shell.until_gone();
        self.job.end(SIGKILL);
        self.shell.end(SIGKILL);
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

    use super::*;

    /// The number of the process that started `pid`, as `ps` says it.
    #[allow(clippy::disallowed_methods)] // A test's own programs.
    fn parent_of(pid: i32) -> u32 {
        let ran = Command::new("ps")
            .args(["-o", "ppid=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&ran.stdout).trim().parse().unwrap()
    }

    #[test]
    fn a_sentinel_is_no_child_of_the_test_that_starts_it() {
        let sentinel = Sentinel::start();

        assert_ne!(parent_of(sentinel.shell.pid), std::process::id());
        assert_ne!(parent_of(sentinel.job.pid), std::process::id());
        assert_eq!(parent_of(sentinel.shell.pid), 1, "the system's init has it");
    }

    #[test]
    fn a_sentinel_runs_until_it_is_dropped_and_its_job_goes_with_it() {
        let mut sentinel = Sentinel::start();
        let job = sentinel.job.clone();
        assert!(sentinel.runs());

        drop(sentinel);

        job.until_gone();
        assert!(
            !process_exists(job.pid),
            "the shell asked to end took its job"
        );
    }

    #[test]
    fn a_sentinel_dropped_as_soon_as_it_starts_takes_its_job_with_it() {
        // The job is started with the shell's traps until it executes: a request to end
        // that reaches it then is taken in its place, and it would outlive the shell.
        for _ in 0..20 {
            let sentinel = Sentinel::start();
            let job = sentinel.job.clone();
            drop(sentinel);
            job.until_gone();
            assert!(
                !process_exists(job.pid),
                "the job {} outlived its shell",
                job.pid
            );
        }
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // A test's own programs.
    fn a_sentinel_ends_with_the_test_that_started_it_and_its_job_with_it() {
        // A stand-in for the test: a process the case ends, as a run that is killed is.
        let mut owner = Command::new("/bin/sleep").arg("60").spawn().unwrap();
        let sentinel = Sentinel::start_owned_by(owner.id());
        let (shell, job) = (sentinel.shell.clone(), sentinel.job.clone());
        assert!(shell.runs() && job.runs());

        owner.kill().unwrap();
        owner.wait().unwrap();
        shell.until_gone();
        job.until_gone();

        // It ended of itself, which a sentinel dropped would say, and fail for.
        std::mem::forget(sentinel);
        assert!(!process_exists(shell.pid), "the shell outlived its owner");
        assert!(!process_exists(job.pid), "the job outlived its owner");
    }

    #[test]
    fn a_sentinel_that_was_ended_fails_the_test_that_drops_it() {
        let mut sentinel = Sentinel::start();
        sentinel.shell.end(SIGKILL);
        sentinel.job.end(SIGKILL);
        sentinel.shell.until_gone();
        sentinel.job.until_gone();
        assert!(!sentinel.runs());

        let dropped = catch_unwind(AssertUnwindSafe(move || drop(sentinel)));

        assert!(dropped.is_err(), "it said so");
    }

    /// The sentinel and its job outlive the test, beside the other tests, which open sockets
    /// and pipes: a descriptor left open and not close-on-exec as it starts is held by neither,
    /// as `lsof` says.
    #[cfg(target_os = "macos")]
    #[test]
    #[allow(clippy::disallowed_methods)] // `lsof`, asked what each holds.
    fn a_sentinel_holds_nothing_of_the_tests_but_its_streams() {
        use std::os::fd::AsRawFd;

        let stray = std::fs::File::open("/dev/null").expect("a file");
        let fd = stray.as_raw_fd();
        // SAFETY: `fcntl` clears the flags of a descriptor this test holds.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
        let sentinel = Sentinel::start();
        for process in [&sentinel.shell, &sentinel.job] {
            let held = Command::new("/usr/sbin/lsof")
                .args(["-a", "-p", &process.pid.to_string(), "-d", &fd.to_string()])
                .output()
                .expect("lsof");
            assert!(
                !String::from_utf8_lossy(&held.stdout).contains("/dev/null"),
                "{} holds it",
                process.pid
            );
        }
    }
}
