//! The work a window leaves running past its turn. An agent can start a job in
//! the background and end its turn, and a harness starts servers of its own
//! (an MCP server): the stand-in does both, so that the rig holds what
//! ConsensFlow does with work a turn left running (the report of 2026-10-09: a
//! recording run left in the background died, killed outright, with the window
//! its worker's turn had ended) and with what the harness itself keeps running
//! (found on 3.0.1: its servers, begun after the first message was marked, held
//! a worker's task for ever).
//!
//! The job is this program started again with [`JOB`] before anything else:
//! `fake-agent --background-job <seconds> <file> [--deaf] [--in-group] [--browser
//! <file>]`. It runs that many seconds and ends by writing `<file>.finished`. A
//! request to end it (SIGINT, SIGTERM or SIGHUP) makes it write `<file>.released`
//! and go, as a tool that releases a lock does; one that kills it leaves nothing.
//! `<file>.pid` is its process, for whoever asks whether it is still there, and is
//! written once the job can be asked to end. With `--deaf` it ignores the requests
//! and is only ever killed. `cf_e2e::rig::Rig::jobs` reads what it wrote.
//!
//! On Unix a job runs in a session of its own by default ([`Group::Own`]), as a
//! command a harness's tool runs for its agent does (Claude Code's Bash tool, Pi's
//! bash tool), where the signals sent to the pane's process group do not reach it.
//! With `--in-group` ([`Group::Window`]) it stays in the group of the window's
//! program, as a server the harness starts does (an MCP server). The windows tell
//! which of them are the harness's by that group, and by what started them. On
//! Windows nothing asks a job to end (a window is ended at once, its job object
//! ending the tree), so the job has no handler there, no groups either, and
//! `--deaf` changes nothing: it writes its process, runs its seconds and is killed.
//!
//! With `--browser <file>` the job starts a job of its own once it is ready: a
//! child of it, as the browser an MCP server launches (`@playwright/mcp`,
//! `chrome-devtools-mcp`) is, and in a session of its own, as that browser is
//! launched, so that the group of the window's program does not hold it.
//!
//! The server a harness starts ([`Server`], told by `CF_TEST_MCP_SERVER`) is such
//! a job in the window's group, as the window opens or, as Claude Code starts its
//! servers, after the first message of the window has shown; or in a group of its
//! own as the window opens, as Codex starts its helpers; with
//! `CF_TEST_MCP_BROWSER` set it launches its browser as it starts.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cf_e2e::process::{Run, Spawned};

#[cfg(unix)]
use crate::signals;

/// What the job is started with, first: no window is launched with it.
pub const JOB: &str = "--background-job";

/// What makes a job stay in the group of the window's program, after its file.
const IN_GROUP: &str = "--in-group";

/// What makes a job start a job of its own, the file of which is the word after it.
const BROWSER: &str = "--browser";

/// How long the agent thinks before it starts a job: well past the daemon's
/// pass (a second), which is how late it may say a launch's turn has begun.
pub const THINKING: Duration = Duration::from_secs(4);

/// What the server a window's harness starts is called in the jobs' files
/// (`server-<window>-<n>`), and how long it runs: past any case. The browser it
/// launches is `browser-<window>-<n>`, of the same number.
const SERVER: &str = "server";
const SERVER_SECONDS: u64 = 600;
const BROWSER_LABEL: &str = "browser";

/// How often a job that is waited for is looked at.
const LOOK: Duration = Duration::from_millis(100);

/// Where a job is among the processes of its window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// In a session of its own on Unix: what a command the agent's tool runs is.
    Own,
    /// In the group of the window's program: what a server the harness starts is.
    Window,
}

/// When the harness of a window starts its MCP server, as `CF_TEST_MCP_SERVER`
/// says: any word but `late` and `apart` is with the window, before any turn (the
/// server a window has before the turn is marked); `late` is as the window takes
/// its first message, which is when Claude Code starts its servers, and so after
/// the daemon has marked where that turn began; `apart` is with the window too,
/// in a group of its own, as Codex starts its helpers: only what the window had
/// before its turn tells it from the turn's work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Server {
    /// The harness starts none.
    Never,
    /// It starts one as the window opens, before any turn.
    WithTheWindow,
    /// It starts one as the window opens, in a group of its own.
    ApartWithTheWindow,
    /// It starts one as the window takes its first message.
    AfterTheFirstMessage,
}

impl Server {
    /// The server `CF_TEST_MCP_SERVER`, when it is set to `variable`, asks for.
    pub fn of(variable: Option<&str>) -> Self {
        match variable {
            None | Some("") => Self::Never,
            Some("late") => Self::AfterTheFirstMessage,
            Some("apart") => Self::ApartWithTheWindow,
            Some(_) => Self::WithTheWindow,
        }
    }

    /// The window is open and has had no turn: the server it starts with the
    /// window begins.
    pub fn as_the_window_opens(self, jobs: &Jobs) -> Result<(), String> {
        match self {
            Self::WithTheWindow => jobs.start_server(Group::Window),
            Self::ApartWithTheWindow => jobs.start_server(Group::Own),
            Self::Never | Self::AfterTheFirstMessage => Ok(()),
        }
    }

    /// The window's first message has shown in its record: the server it starts
    /// as it takes it begins an agent's thinking from now, on a thread of its own,
    /// beside the turn, as Claude Code starts its servers, and so well past the
    /// daemon's pass that marks where the turn began.
    pub fn as_the_first_message_shows(self, jobs: &Arc<Jobs>) {
        if self != Self::AfterTheFirstMessage {
            return;
        }
        let jobs = Arc::clone(jobs);
        thread::spawn(move || {
            thread::sleep(THINKING);
            if let Err(failed) = jobs.start_server(Group::Window) {
                let _ = writeln!(io::stderr(), "fake-agent: the server: {failed}");
            }
        });
    }
}

/// The jobs one window starts.
pub struct Jobs {
    folder: PathBuf,
    window: u32,
    started: AtomicU32,
    /// Whether the server the window's harness starts launches a browser.
    browser: bool,
}

impl Jobs {
    /// The jobs of the window with process id `window`, whose daemon's home is
    /// `home`: their files are in `jobs/` beside it, where the rig looks.
    pub fn new(home: &Path, window: u32) -> Self {
        Self {
            folder: home.parent().unwrap_or(Path::new(".")).join("jobs"),
            window,
            started: AtomicU32::new(0),
            browser: false,
        }
    }

    /// The jobs of a window whose harness's server launches a browser, as `browser`
    /// says (`CF_TEST_MCP_BROWSER`: any word but none).
    #[must_use]
    pub fn launching_a_browser(mut self, browser: Option<&str>) -> Self {
        self.browser = browser.is_some_and(|word| !word.is_empty());
        self
    }

    /// Starts the server of the window's harness, which runs past any case, in
    /// `group`, and the browser it launches if it does.
    fn start_server(&self, group: Group) -> Result<(), String> {
        self.begin(
            SERVER,
            SERVER_SECONDS,
            false,
            group,
            self.browser,
            || {},
        )
    }

    /// Starts the job `label`, which runs `seconds` (and ignores the requests
    /// to end if it is `deaf`), in `group`. Its files are `<label>-<window>-<n>.*`.
    /// `then` runs once it has ended, however it ended.
    pub fn start(
        &self,
        label: &str,
        seconds: u64,
        deaf: bool,
        group: Group,
        then: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        self.begin(label, seconds, deaf, group, false, then)
    }

    /// [`Jobs::start`], and the browser the job launches if it `launches` one: its
    /// files are `browser-<window>-<n>.*`, of the job's number.
    fn begin(
        &self,
        label: &str,
        seconds: u64,
        deaf: bool,
        group: Group,
        launches: bool,
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
        if group == Group::Window {
            run = run.arg(IN_GROUP);
        }
        if launches {
            run = run.arg(BROWSER).arg(
                self.folder
                    .join(format!("{BROWSER_LABEL}-{}-{number}", self.window)),
            );
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
        let _ = writeln!(
            io::stderr(),
            "fake-agent: {JOB} <seconds> <file> [--deaf] [{IN_GROUP}] [{BROWSER} <file>]"
        );
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
    group: Group,
    /// The file of the job it starts as it is ready, if it starts one.
    browser: Option<PathBuf>,
}

impl Job {
    /// Reads `<seconds> <file> [--deaf] [--in-group] [--browser <file>]`: the
    /// words after the file, each once and in any order.
    fn parse(args: &[String]) -> Option<Self> {
        let seconds = args.first()?.parse().ok()?;
        let file = PathBuf::from(args.get(1)?);
        let (mut deaf, mut stays, mut browser) = (false, false, None);
        let mut words = args.get(2..)?.iter();
        while let Some(word) = words.next() {
            match word.as_str() {
                BROWSER => {
                    if browser.replace(PathBuf::from(words.next()?)).is_some() {
                        return None;
                    }
                }
                "--deaf" | IN_GROUP => {
                    let said = if word == IN_GROUP {
                        &mut stays
                    } else {
                        &mut deaf
                    };
                    if std::mem::replace(said, true) {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        Some(Self {
            seconds,
            file,
            deaf,
            group: if stays { Group::Window } else { Group::Own },
            browser,
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
        #[cfg(unix)]
        self.hear()?;
        fs::write(self.beside("pid"), std::process::id().to_string())
            .map_err(|failed| failed.to_string())?;
        // Held to the end, as the server holds the browser it launched.
        let _browser = self.launch_browser()?;
        thread::sleep(Duration::from_secs(self.seconds));
        fs::write(self.beside("finished"), "FINISHED\n").map_err(|failed| failed.to_string())
    }

    /// Starts the job it was asked to, which is a child of it and runs as long as it
    /// does: in a session of its own, as a job is where it is not told to stay in
    /// its window's group.
    fn launch_browser(&self) -> Result<Option<Spawned>, String> {
        let Some(file) = &self.browser else {
            return Ok(None);
        };
        let program = std::env::current_exe().map_err(|failed| failed.to_string())?;
        Run::new(program)
            .arg(JOB)
            .arg(self.seconds.to_string())
            .arg(file)
            .inheriting_env()
            .closed_input()
            .spawn()
            .map(Some)
            .map_err(|failed| failed.to_string())
    }

    /// Hears the requests to end: a session of its own unless the job stays in
    /// its window's group, and the signals held until one thread waits for them
    /// (or for ever, if the job is deaf).
    #[cfg(unix)]
    fn hear(&self) -> Result<(), String> {
        use nix::sys::signal::{SigSet, Signal};

        if self.group == Group::Own {
            // This program does not lead a process group: the session can be made.
            let _ = nix::unistd::setsid();
        }
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
}

/// Says it was asked to end, and goes.
#[cfg(unix)]
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
                deaf: false,
                group: Group::Own,
                browser: None,
            })
        );
        assert_eq!(
            Job::parse(&words("120 /jobs/job-1-2 --deaf")),
            Some(Job {
                seconds: 120,
                file: PathBuf::from("/jobs/job-1-2"),
                deaf: true,
                group: Group::Own,
                browser: None,
            })
        );
    }

    #[test]
    fn a_job_that_launches_a_browser_is_told_its_file_in_any_order_with_the_other_words() {
        for line in [
            "600 /jobs/server-1-1 --in-group --browser /jobs/browser-1-1",
            "600 /jobs/server-1-1 --browser /jobs/browser-1-1 --in-group",
        ] {
            let job = Job::parse(&words(line)).unwrap();
            assert_eq!(
                job.browser,
                Some(PathBuf::from("/jobs/browser-1-1")),
                "{line}"
            );
            assert_eq!(job.group, Group::Window, "{line}");
        }
        assert_eq!(
            Job::parse(&words("600 /jobs/job-1-1")).unwrap().browser,
            None
        );
    }

    #[test]
    fn a_job_is_in_a_group_of_its_own_unless_it_is_told_to_stay_in_its_windows() {
        for line in [
            "600 /jobs/server-1-1 --in-group",
            "600 /jobs/server-1-1 --in-group --deaf",
            "600 /jobs/server-1-1 --deaf --in-group",
        ] {
            let job = Job::parse(&words(line)).unwrap();
            assert_eq!(job.group, Group::Window, "{line}");
            assert_eq!(job.deaf, line.contains("--deaf"), "{line}");
        }
        assert_eq!(
            Job::parse(&words("600 /jobs/job-1-1")).unwrap().group,
            Group::Own
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
            "12 /jobs/a --deaf --deaf",
            "12 /jobs/a --in-group --in-group",
            "12 /jobs/a --in-group --deaf more",
            "12 /jobs/a --browser",
            "12 /jobs/a --browser /jobs/b --browser /jobs/c",
        ] {
            assert_eq!(Job::parse(&words(line)), None, "{line:?}");
        }
    }

    #[test]
    fn a_harness_starts_a_server_with_its_window_after_its_first_message_or_not_at_all() {
        assert_eq!(Server::of(None), Server::Never);
        assert_eq!(Server::of(Some("")), Server::Never);
        // Any other word is the server the window has before any turn.
        assert_eq!(Server::of(Some("1")), Server::WithTheWindow);
        assert_eq!(Server::of(Some("yes")), Server::WithTheWindow);
        assert_eq!(Server::of(Some("late")), Server::AfterTheFirstMessage);
        assert_eq!(Server::of(Some("apart")), Server::ApartWithTheWindow);
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

    #[test]
    fn a_server_launches_a_browser_where_the_variable_says_any_word() {
        let launches = |word| {
            Jobs::new(Path::new("/root/consensflow"), 1)
                .launching_a_browser(word)
                .browser
        };
        assert!(!launches(None));
        assert!(!launches(Some("")));
        assert!(launches(Some("1")));
    }
}
