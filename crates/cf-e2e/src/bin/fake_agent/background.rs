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
//! A command the agent runs is started by a shell, in a group of its own, as a
//! harness's tool runs a command for its agent (Claude Code's Bash tool, Pi's bash
//! tool): a `/bin/sh` that waits for the job and ends with its code (a PowerShell on
//! Windows), and the job stays where the shell put it, which the signals sent to the
//! pane's process group do not reach. With `--detach` the shell detaches it as the
//! agents of the harnesses do when they leave a job (`nohup … &`, `Start-Process`)
//! and ends a moment after, which leaves the job with no parent: no chain of processes
//! that run leads from it to a shell the harness started, so the host does not take it
//! for the turn's work and leaves it running (`cf-panes`' `background`). On Unix that
//! shell is in a session of its own, as Pi's Bash tool starts each command (the window's
//! does not hold it), and the job is started without the marker the pane host puts in
//! the environment of what a window starts, as a job that is a system program shows
//! none: nothing of the window's close finds it either. A server the harness starts is
//! no command: it is started as it is, and with `--in-group` ([`Group::Window`]) it
//! stays in the group of the window's program (an MCP server), or, as it is, makes a
//! session of its own (a helper). The host tells the commands from the servers by the
//! shell that started them (a command is what a shell the harness started started, and
//! what that started), and the harness's own by where it keeps them, by the programs it
//! is named for and by having seen them its own. On Windows nothing
//! asks a job to end (a window is ended at once, its job object ending the tree, and
//! what a turn left is ended by itself, process by process), so the job has no handler
//! there, no groups either, and `--deaf` changes nothing: it writes its process, runs
//! its seconds and is killed.
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

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[cfg(windows)]
use cf_e2e::process::own_var;
use cf_e2e::process::{Run, Spawned};

#[cfg(unix)]
use crate::signals;

/// What the job is started with, first: no window is launched with it.
pub const JOB: &str = "--background-job";

/// What the shell of a command that is detached is started with, first, on Unix:
/// `fake-agent --background-session <program> <words…>` makes a session of its own and
/// becomes the program, as a shell is started by a harness whose Bash tool detaches its
/// commands (Node's `detached` is a `setsid`: Pi's does).
#[cfg(unix)]
pub const SESSION: &str = "--background-session";

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

/// What a job is started as, besides its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct How {
    /// It launches a browser of its own once it is ready.
    browser: bool,
    /// A command of the agent's: a shell starts it (a PowerShell on Windows), as the
    /// harnesses start the commands their agents run, and the shell is in a group of
    /// its own.
    command: bool,
    /// The agent detaches it from that shell (`nohup … &`, `Start-Process`), which
    /// ends at once and leaves it with no parent.
    detached: bool,
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

    /// Notes the process `pid` as one of the window's the rig looks for, named `label`: a
    /// program the window started that is no job of this program (a command of the
    /// harness's own that a script runs), in the file a job's would be
    /// (`<label>-<window>-<n>.pid`), for a case to ask whether it ran.
    #[cfg(unix)]
    pub fn note(&self, label: &str, pid: u32) -> Result<(), String> {
        fs::create_dir_all(&self.folder)
            .map_err(|failed| format!("{}: {failed}", self.folder.display()))?;
        let number = self.started.fetch_add(1, Ordering::Relaxed) + 1;
        let file = self
            .folder
            .join(format!("{label}-{}-{number}.pid", self.window));
        fs::write(&file, format!("{pid}\n"))
            .map_err(|failed| format!("{}: {failed}", file.display()))
    }

    /// The file of the window's next job named `label` (`<label>-<window>-<n>`), for a
    /// program that is started otherwise than [`Jobs::start`] starts one to write its
    /// files by: a build, and the daemon it leaves (`build`).
    pub fn file(&self, label: &str) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.folder)
            .map_err(|failed| format!("{}: {failed}", self.folder.display()))?;
        let number = self.started.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(self
            .folder
            .join(format!("{label}-{}-{number}", self.window)))
    }

    /// Starts the server of the window's harness, which runs past any case, in
    /// `group`, and the browser it launches if it does.
    fn start_server(&self, group: Group) -> Result<(), String> {
        let how = How {
            browser: self.browser,
            command: false,
            detached: false,
        };
        self.begin(SERVER, SERVER_SECONDS, false, group, how, || {})
            .map(|_| ())
    }

    /// Starts the job `label`, a command the agent's tool runs, which runs
    /// `seconds` (and ignores the requests to end if it is `deaf`), in `group`. Its
    /// files are `<label>-<window>-<n>.*`. `then` runs once it has ended, however
    /// it ended; where the agent `detached` it, once the shell it was started by has,
    /// which is at once. What it returns is the job's file, which [`until_up`] waits on.
    pub fn start(
        &self,
        label: &str,
        seconds: u64,
        deaf: bool,
        detached: bool,
        then: impl FnOnce() + Send + 'static,
    ) -> Result<PathBuf, String> {
        let how = How {
            browser: false,
            command: true,
            detached,
        };
        self.begin(label, seconds, deaf, Group::Own, how, then)
    }

    /// [`Jobs::start`], as `how` says: the browser the job launches, if it launches
    /// one, has files `browser-<window>-<n>.*`, of the job's number.
    fn begin(
        &self,
        label: &str,
        seconds: u64,
        deaf: bool,
        group: Group,
        how: How,
        then: impl FnOnce() + Send + 'static,
    ) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.folder)
            .map_err(|failed| format!("{}: {failed}", self.folder.display()))?;
        let number = self.started.fetch_add(1, Ordering::Relaxed) + 1;
        let file = self
            .folder
            .join(format!("{label}-{}-{number}", self.window));
        let program = std::env::current_exe().map_err(|failed| failed.to_string())?;
        let mut words: Vec<OsString> = vec![
            JOB.into(),
            seconds.to_string().into(),
            file.clone().into_os_string(),
        ];
        if deaf {
            words.push("--deaf".into());
        }
        // A command stays where its shell put it: in the shell's group, in the window's
        // session, as the command a harness's tool runs does.
        if group == Group::Window || (how.command && cfg!(unix)) {
            words.push(IN_GROUP.into());
        }
        if how.browser {
            words.push(BROWSER.into());
            words.push(
                self.folder
                    .join(format!("{BROWSER_LABEL}-{}-{number}", self.window))
                    .into_os_string(),
            );
        }
        // The window's environment, which every program of a window has.
        let mut job = Self::running(&program, words, how)?
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
        Ok(file)
    }

    /// The program that runs the job `words` say: this one, as it is, or where `how`
    /// says it is a command of the agent's, a shell that runs it ([`in_a_shell`]).
    fn running(program: &Path, words: Vec<OsString>, how: How) -> Result<Run, String> {
        if how.command {
            in_a_shell(program, words, how.detached)
        } else {
            Ok(Run::new(program).args(words))
        }
    }
}

/// Waits until the job whose file is `file` runs: it has written its process
/// (`<file>.pid`), as a tool returns once the command it started runs. An agent that
/// answered before that left a turn whose end could come before the job was there to
/// be seen (GitHub's Windows runner, 2026-10-15).
pub fn until_up(file: &Path) -> Result<(), String> {
    let mut pid = file.as_os_str().to_owned();
    pid.push(".pid");
    let pid = PathBuf::from(pid);
    let deadline = std::time::Instant::now() + UP;
    while !pid.exists() {
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "{} never ran: no {}",
                file.display(),
                pid.display()
            ));
        }
        thread::sleep(LOOK);
    }
    Ok(())
}

/// How long a job has to write its process once it is started.
const UP: Duration = Duration::from_secs(30);

/// `program` with `words`, run as a harness's tool runs a command for its agent: by a
/// shell (a PowerShell on Windows) that waits for it and ends with its code, in a group
/// of its own on Unix. Where `detached` the shell detaches it as the agents do
/// (`nohup … &`, `Start-Process`) and ends a moment after, which leaves the command with
/// no parent; on Unix that shell is in a session of its own ([`SESSION`]).
pub fn in_a_shell(program: &Path, words: Vec<OsString>, detached: bool) -> Result<Run, String> {
    #[cfg(windows)]
    {
        let root = own_var("SystemRoot").unwrap_or_else(|| r"C:\Windows".to_owned());
        Ok(Run::new(format!(
            r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe"
        ))
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(powershell_script(program, &words, detached)))
    }
    #[cfg(not(windows))]
    {
        if detached {
            let itself = std::env::current_exe().map_err(|failed| failed.to_string())?;
            Ok(Run::new(itself)
                .args([SESSION, "/bin/sh", "-c", shell_script(true)])
                .arg(program)
                .args(words))
        } else {
            Ok(Run::new("/bin/sh")
                .args(["-c", shell_script(false)])
                .arg(program)
                .args(words)
                .own_group())
        }
    }
}

/// What the shell of a command is told to run, with the program as its `$0` and the
/// job's words after it: the program, which the shell waits for and ends with the
/// code of (it is no `exec`, so the shell stays a parent), or detached from the
/// shell as `nohup … &` does, which ends the shell, a moment after, and leaves the job
/// with no parent. A job that is a system program shows no environment, so the
/// window's marker (`CF_PANE_PROCESS_OWNER`, which the pane host puts in the
/// environment of everything a window starts) is not there to find it by: the
/// detached job is started without it.
#[cfg(any(unix, test))]
fn shell_script(detached: bool) -> &'static str {
    if detached {
        "unset CF_PANE_PROCESS_OWNER; nohup \"$0\" \"$@\" >/dev/null 2>&1 & sleep 0.3"
    } else {
        "\"$0\" \"$@\"; exit $?"
    }
}

/// Makes a session of its own and becomes the program `args` name, with its words:
/// the shell of a detached command, as a harness starts it ([`SESSION`]).
#[cfg(unix)]
pub fn run_in_session(args: &[String]) -> ExitCode {
    use std::os::unix::process::CommandExt;

    let Some((program, words)) = args.split_first() else {
        let _ = writeln!(io::stderr(), "fake-agent: {SESSION} <program> [words…]");
        return ExitCode::FAILURE;
    };
    // Only a process group's leader cannot: this one was started to be none.
    let _ = nix::unistd::setsid();
    // The one place a program is run in place of this one, which no `Run` does.
    #[allow(clippy::disallowed_methods)]
    let failed = std::process::Command::new(program).args(words).exec();
    let _ = writeln!(io::stderr(), "fake-agent: {program}: {failed}");
    ExitCode::FAILURE
}

/// What PowerShell is told to run to start `program` with `words`: `& 'program'
/// 'word' …`, each in single quotes, the quotes in it doubled; or, to detach it,
/// `Start-Process`, which ends the shell and leaves the job with no parent.
#[cfg(any(windows, test))]
fn powershell_script(program: &Path, words: &[OsString], detached: bool) -> String {
    let quoted =
        |word: &std::ffi::OsStr| format!("'{}'", word.to_string_lossy().replace('\'', "''"));
    if detached {
        // Each word is quoted again for the command line `Start-Process` makes of them.
        let words: Vec<String> = words
            .iter()
            .map(|word| {
                let word = word.to_string_lossy().replace('\'', "''");
                format!("'\"{word}\"'")
            })
            .collect();
        return format!(
            "Start-Process -WindowStyle Hidden -FilePath {} -ArgumentList {}",
            quoted(program.as_os_str()),
            words.join(",")
        );
    }
    let words: Vec<String> = words.iter().map(|word| quoted(word)).collect();
    format!("& {} {}", quoted(program.as_os_str()), words.join(" "))
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
#[path = "background/tests.rs"]
mod tests;
