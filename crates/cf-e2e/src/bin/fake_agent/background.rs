//! The work a window leaves running past its turn. An agent can start a job in
//! the background and end its turn, and a harness starts servers of its own as
//! it opens (an MCP server): the stand-in does both, so that the rig holds what
//! ConsensFlow does with work a turn left running (the report of 2026-10-09: a
//! recording run left in the background died, killed outright, with the window
//! its worker's turn had ended).
//!
//! The job is this program started again with [`JOB`] before anything else:
//! `fake-agent --background-job <seconds> <file> [--deaf]`. It runs that many
//! seconds and ends by writing `<file>.finished`. A request to end it (SIGINT,
//! SIGTERM or SIGHUP; on Windows Ctrl+C, Ctrl+Break or the console closing)
//! makes it write `<file>.released` and go, as a tool that releases a lock
//! does; one that kills it leaves nothing. `<file>.pid` is its process, for
//! whoever asks whether it is still there, and is written once the job can be
//! asked to end. With `--deaf` it ignores the requests and is only ever killed.
//! `cf_e2e::rig::Rig::jobs` reads what it wrote.
//!
//! On Unix the job runs in a session of its own, as a harness's background
//! shell does, where the signals sent to the pane's process group do not reach
//! it. On Windows it stays on the window's console, where a process detached
//! from it would never be sent a Break.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use cf_e2e::process::Run;

#[cfg(unix)]
use crate::signals;

/// What the job is started with, first: no window is launched with it.
pub const JOB: &str = "--background-job";

/// How long the agent thinks before it starts a job: well past the daemon's
/// pass (a second), which is how late it may say a launch's turn has begun.
pub const THINKING: Duration = Duration::from_secs(4);

/// How often a job that is waited for is looked at.
const LOOK: Duration = Duration::from_millis(100);

/// The jobs one window starts.
pub struct Jobs {
    folder: PathBuf,
    window: u32,
    started: AtomicU32,
}

impl Jobs {
    /// The jobs of the window with process id `window`, whose daemon's home is
    /// `home`: their files are in `jobs/` beside it, where the rig looks.
    pub fn new(home: &Path, window: u32) -> Self {
        Self {
            folder: home.parent().unwrap_or(Path::new(".")).join("jobs"),
            window,
            started: AtomicU32::new(0),
        }
    }

    /// Starts the job `label`, which runs `seconds` (and ignores the requests
    /// to end if it is `deaf`). Its files are `<label>-<window>-<n>.*`. `then`
    /// runs once it has ended, however it ended.
    pub fn start(
        &self,
        label: &str,
        seconds: u64,
        deaf: bool,
        then: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        fs::create_dir_all(&self.folder)
            .map_err(|failed| format!("{}: {failed}", self.folder.display()))?;
        let number = self.started.fetch_add(1, Ordering::Relaxed) + 1;
        let file = self
            .folder
            .join(format!("{label}-{}-{number}", self.window));
        let program = std::env::current_exe().map_err(|failed| failed.to_string())?;
        let mut run = Run::new(program)
            .arg(JOB)
            .arg(seconds.to_string())
            .arg(&file);
        if deaf {
            run = run.arg("--deaf");
        }
        // The window's environment, which every program of a window has.
        let mut job = run
            .inheriting_env()
            .closed_input()
            .spawn()
            .map_err(|failed| failed.to_string())?;
        // The handle ends the program it is dropped with, so it is held on a
        // thread of its own until the job is over; this program's own end
        // leaves the job running, as a window's end leaves what it started.
        thread::spawn(move || {
            while !job.has_exited() {
                thread::sleep(LOOK);
            }
            then();
        });
        Ok(())
    }
}

/// The job: runs as `args` ask, and the exit code of the program.
pub fn run(args: &[String]) -> ExitCode {
    let Some(job) = Job::parse(args) else {
        let _ = writeln!(io::stderr(), "fake-agent: {JOB} <seconds> <file> [--deaf]");
        return ExitCode::FAILURE;
    };
    match job.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(failed) => {
            let _ = writeln!(io::stderr(), "fake-agent: {failed}");
            ExitCode::FAILURE
        }
    }
}

/// What a job is asked to be.
#[derive(Debug, PartialEq, Eq)]
struct Job {
    seconds: u64,
    file: PathBuf,
    deaf: bool,
}

impl Job {
    /// Reads `<seconds> <file> [--deaf]`.
    fn parse(args: &[String]) -> Option<Self> {
        let seconds = args.first()?.parse().ok()?;
        let file = PathBuf::from(args.get(1)?);
        let deaf = match args.get(2).map(String::as_str) {
            None => false,
            Some("--deaf") => true,
            Some(_) => return None,
        };
        if args.len() > 3 {
            return None;
        }
        Some(Self {
            seconds,
            file,
            deaf,
        })
    }

    /// The file of the kind `kind` it writes: `<file>.<kind>`.
    fn beside(&self, kind: &str) -> PathBuf {
        let mut name = self.file.clone().into_os_string();
        name.push(".");
        name.push(kind);
        PathBuf::from(name)
    }

    /// Runs the job to its end by itself. A request to end it is answered where
    /// it is heard, on a thread of its own.
    fn run(&self) -> Result<(), String> {
        // Hearing the requests comes first: once the process id is written, a
        // request is never lost.
        self.hear()?;
        fs::write(self.beside("pid"), std::process::id().to_string())
            .map_err(|failed| failed.to_string())?;
        thread::sleep(Duration::from_secs(self.seconds));
        fs::write(self.beside("finished"), "FINISHED\n").map_err(|failed| failed.to_string())
    }

    /// Hears the requests to end: a session of its own, and the signals held
    /// until one thread waits for them (or for ever, if the job is deaf).
    #[cfg(unix)]
    fn hear(&self) -> Result<(), String> {
        use nix::sys::signal::{SigSet, Signal};

        // This program does not lead a process group: the session can be made.
        let _ = nix::unistd::setsid();
        let mut requests = SigSet::empty();
        for request in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGHUP] {
            requests.add(request);
        }
        requests
            .thread_block()
            .map_err(|failed| failed.to_string())?;
        if !self.deaf {
            let released = self.beside("released");
            thread::spawn(move || {
                if signals::wait(&requests) {
                    release(&released);
                }
            });
        }
        Ok(())
    }

    /// Hears the requests to end: a handler of the console's events, which
    /// answers Ctrl+C, Ctrl+Break and the console closing.
    #[cfg(windows)]
    fn hear(&self) -> Result<(), String> {
        use std::sync::OnceLock;

        use windows_sys::core::BOOL;
        use windows_sys::Win32::System::Console::{
            SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT,
        };

        /// Where it says it was asked to end; nowhere for a deaf job.
        static RELEASED: OnceLock<PathBuf> = OnceLock::new();

        unsafe extern "system" fn on_event(event: u32) -> BOOL {
            if ![CTRL_C_EVENT, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT].contains(&event) {
                return BOOL::from(false);
            }
            match RELEASED.get() {
                Some(released) => release(released),
                // Handled, and ignored.
                None => BOOL::from(true),
            }
        }

        if !self.deaf {
            let _ = RELEASED.set(self.beside("released"));
        }
        // SAFETY: the handler is a function of this file that touches only
        // `RELEASED` and the end of the process.
        if unsafe { SetConsoleCtrlHandler(Some(on_event), 1) } == 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        Ok(())
    }
}

/// Says it was asked to end, and goes.
fn release(file: &Path) -> ! {
    let _ = fs::write(file, "RELEASED\n");
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split(' ')
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_job_is_a_number_of_seconds_and_a_file_and_may_be_deaf() {
        assert_eq!(
            Job::parse(&words("12 /jobs/job-1-1")),
            Some(Job {
                seconds: 12,
                file: PathBuf::from("/jobs/job-1-1"),
                deaf: false
            })
        );
        assert_eq!(
            Job::parse(&words("120 /jobs/job-1-2 --deaf")),
            Some(Job {
                seconds: 120,
                file: PathBuf::from("/jobs/job-1-2"),
                deaf: true
            })
        );
    }

    #[test]
    fn anything_else_is_no_job() {
        for line in [
            "",
            "12",
            "soon /jobs/a",
            "12 /jobs/a --loud",
            "12 /jobs/a --deaf more",
        ] {
            assert_eq!(Job::parse(&words(line)), None, "{line:?}");
        }
    }

    #[test]
    fn what_a_job_writes_is_its_file_and_a_kind_after_a_dot() {
        let job = Job::parse(&words("1 /jobs/job-1-1")).unwrap();
        assert_eq!(job.beside("pid"), PathBuf::from("/jobs/job-1-1.pid"));
        // A name with a dot of its own keeps it.
        let job = Job::parse(&words("1 /jobs/a.b")).unwrap();
        assert_eq!(job.beside("released"), PathBuf::from("/jobs/a.b.released"));
    }

    #[test]
    fn the_jobs_of_a_window_are_in_a_folder_beside_its_home() {
        let jobs = Jobs::new(Path::new("/root/consensflow"), 4242);
        assert_eq!(jobs.folder, PathBuf::from("/root/jobs"));
        assert_eq!(jobs.window, 4242);
    }
}
