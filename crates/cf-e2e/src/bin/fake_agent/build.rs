//! A build the agent's shell tool runs in the foreground, which leaves a daemon of its
//! own running when it ends, as a Gradle build leaves Gradle's (the report of another
//! Mac, 2026-10-15: tasks stayed working after their workers had finished a Gradle
//! build, on 3.0.3). A message with a line `BUILD <seconds> <words>` is a turn whose
//! tool runs the build by a shell, as a harness runs its agent's command (`tool`), and
//! waits for the shell to end; the build starts its daemon, which runs `<seconds>`, and
//! ends once the daemon is up, and the shell with it; then the turn answers the words.
//! The daemon was started by no command the agent left running: the call that started
//! it was waited for, and is over.
//!
//! The build is this program started with [`BUILD`] first: `fake-agent --build <seconds>
//! <build file> <daemon file>`. It writes `<build file>.pid`, starts the daemon (a job of
//! `<seconds>` whose file is `<daemon file>`: `background`), waits until the daemon has
//! written its process, lets go of it and ends, having written `<build file>.finished`.
//! The daemon is a child of the build's and outlives it, as Gradle's daemon is its
//! client's, with the environment it was given (the window's marker in it, as Gradle's
//! daemon keeps its client's); on Unix it makes a session of its own, as Gradle's daemon
//! detaches itself from its client's terminal. On Windows the build hands it none of its
//! own handles (the shell's output among them), as a daemon that writes its own log holds
//! none of its client's: a PowerShell waits for the output of the command it runs to end.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use cf_e2e::pattern;
use cf_e2e::process::Run;
use regex::Regex;

use crate::background::{in_a_shell, until_up, JOB, THINKING};
use crate::reply::{Reply, Window};

/// What the build is started with, first: no window is launched with it.
pub const BUILD: &str = "--build";

/// How often the build's shell is looked at for its end while the turn waits for it.
const LOOK: Duration = Duration::from_millis(200);

/// What a turn is asked to build: how long its daemon runs, and what the turn answers.
#[derive(Debug, PartialEq, Eq)]
struct Asked<'a> {
    seconds: u64,
    words: &'a str,
}

impl<'a> Asked<'a> {
    /// Reads the line `BUILD <seconds> <words>` of `text`: none for a message with no
    /// such line.
    fn parse(text: &'a str) -> Option<Self> {
        static LINE: OnceLock<Regex> = OnceLock::new();
        let line = pattern::once(&LINE, r"(?m)^BUILD (\d+) (.+)$").captures(text)?;
        let words = line.get(2)?.as_str();
        Some(Self {
            seconds: line[1].parse().ok()?,
            words,
        })
    }
}

/// The reply to `text` if it asks for a build: the turn that runs it. None for another
/// message.
pub fn reply(text: &str, window: &Window) -> Option<Result<Reply, String>> {
    Asked::parse(text).map(|asked| run_in_the_foreground(&asked, window))
}

/// The turn: the build is started once the agent has thought a moment (what the tree has
/// before the daemon marks the turn is not the turn's), by a shell in a group of its own,
/// and waited for to its end, the shell's; then the turn answers. An Escape stops the
/// turn, and its shell with it.
fn run_in_the_foreground(asked: &Asked, window: &Window) -> Result<Reply, String> {
    thread::sleep(THINKING);
    let build = window.jobs.file("build")?;
    let daemon = window.jobs.file("daemon")?;
    let program = std::env::current_exe().map_err(|failed| failed.to_string())?;
    let words: Vec<OsString> = vec![
        BUILD.into(),
        asked.seconds.to_string().into(),
        build.into_os_string(),
        daemon.into_os_string(),
    ];
    let mut shell = in_a_shell(&program, words, false)?
        .inheriting_env()
        .closed_input()
        .spawn()
        .map_err(|failed| failed.to_string())?;
    while !shell.has_exited() {
        if window.stop.work(LOOK) {
            return Ok(Reply::Stopped);
        }
    }
    Ok(Reply::Text(asked.words.to_owned()))
}

/// The build: runs as `args` ask (`<seconds> <build file> <daemon file>`), and the exit
/// code of the program.
pub fn run(args: &[String]) -> ExitCode {
    let [seconds, build, daemon] = args else {
        let _ = writeln!(
            io::stderr(),
            "fake-agent: {BUILD} <seconds> <build file> <daemon file>"
        );
        return ExitCode::FAILURE;
    };
    match leave_a_daemon(seconds, Path::new(build), Path::new(daemon)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failed) => {
            let _ = writeln!(io::stderr(), "fake-agent: the build: {failed}");
            ExitCode::FAILURE
        }
    }
}

/// Starts the daemon of `seconds` whose file is `daemon`, waits until it runs, and lets go
/// of it; `build` is the build's own file.
fn leave_a_daemon(seconds: &str, build: &Path, daemon: &Path) -> Result<(), String> {
    let beside = |kind: &str| {
        let mut name = build.as_os_str().to_owned();
        name.push(".");
        name.push(kind);
        PathBuf::from(name)
    };
    let written = |file: PathBuf, text: String| {
        fs::write(&file, text).map_err(|failed| format!("{}: {failed}", file.display()))
    };
    cf_e2e::files::say_its_process(build)
        .map_err(|failed| format!("{}: {failed}", build.display()))?;
    #[cfg(windows)]
    keep_own_handles();
    let program = std::env::current_exe().map_err(|failed| failed.to_string())?;
    let started = Run::new(program)
        .arg(JOB)
        .arg(seconds)
        .arg(daemon)
        .inheriting_env()
        .closed_input()
        .spawn()
        .map_err(|failed| failed.to_string())?;
    until_up(daemon)?;
    // The handle ends the program it is dropped with: the build lets go of its daemon,
    // which runs on after it, as Gradle's client leaves its daemon.
    std::mem::forget(started);
    written(beside("finished"), "FINISHED\n".to_owned())
}

/// Makes the build's own standard handles its own: the daemon it starts inherits none of
/// them, and with them none of the shell's output, which a PowerShell reads to its end.
#[cfg(windows)]
fn keep_own_handles() {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };

    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: the standard handles are the process's own and stay open while it
        // runs; `SetHandleInformation` changes one flag of a handle, and fails with a
        // zero for a handle that is none (no such stream), which leaves it as it was.
        unsafe {
            let handle = GetStdHandle(which);
            let _ = SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_is_how_long_its_daemon_runs_and_what_the_turn_answers() {
        assert_eq!(
            Asked::parse("[ConsensFlow m-1 · T-1 · task from @chief]\nBUILD 600 built it"),
            Some(Asked {
                seconds: 600,
                words: "built it"
            })
        );
        for text in [
            "",
            "BUILD",
            "BUILD 600",
            "BUILD soon built",
            "say BUILD 5 x",
        ] {
            assert_eq!(Asked::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_build_told_less_than_its_three_words_starts_nothing() {
        for args in [&[][..], &["600".to_owned()][..]] {
            assert_eq!(
                format!("{:?}", run(args)),
                format!("{:?}", ExitCode::FAILURE)
            );
        }
    }
}
