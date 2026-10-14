//! The status line of a window's harness. Claude Code runs the user's command for it
//! (`statusLine` in its settings: a script of the user's that shows the branch of the
//! folder, among other things) whenever the window changes and every few seconds, and
//! that script is a `bash` that runs `git … branch --show-current`, goes on for a moment
//! and ends. Claude Code runs it as it runs the commands its agent asks for, in a group
//! of its own, so a look at the window's tree that comes while one runs finds a shell the
//! harness started after the turn did, with a `git` under it (T-42, 3.0.3, 2026-10-14:
//! `bash ~/.claude/statusline-command.sh` twice and `git -C … --no-optional-locks branch
//! --show-current`, told as work its window left running when it closed).
//!
//! With `CF_TEST_STATUS_LINE` set the window runs its status line for as long as it
//! lives, from the moment it opens, and the rig finds what ran as `statusline` among the
//! window's jobs: any word but `group` is the script of the report, in a group of its own,
//! a second long, and a new one begins before the last has ended, so that one is at work
//! at any moment; `group` is a helper of the harness that stays in the group of the
//! window's program, as its servers do, and goes on for longer than the host waits
//! before it looks again.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cf_e2e::process::Run;

use crate::background::Jobs;

/// What the user's command is called: `~/.claude/statusline-command.sh` is where Claude
/// Code's is, and the stand-in's is in its own folder.
const SCRIPT: &str = "statusline-command.sh";

/// What the rig calls the runs of the command, among the jobs of a window.
const LABEL: &str = "statusline";

/// How often a run is looked at, to see that it has ended.
const LOOK: Duration = Duration::from_millis(100);

/// Where the harness runs its status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLine {
    /// In a group of its own, as Claude Code runs the commands its agent asks for: a
    /// second long, and a new run begins before the last has ended.
    InAGroupOfItsOwn,
    /// A helper that stays in the group of the window's program, as the harness's
    /// servers do: it goes on for longer than the host waits before it looks again.
    InTheWindowsGroup,
}

impl StatusLine {
    /// The status line `CF_TEST_STATUS_LINE`, set to `variable`, asks for: none where it
    /// is not set.
    pub fn of(variable: Option<&str>) -> Option<Self> {
        match variable {
            None | Some("") => None,
            Some("group") => Some(Self::InTheWindowsGroup),
            Some(_) => Some(Self::InAGroupOfItsOwn),
        }
    }

    /// How many seconds the script goes on after it has shown the branch, unless the
    /// window's program is gone before.
    fn seconds(self) -> u64 {
        match self {
            Self::InAGroupOfItsOwn => 1,
            Self::InTheWindowsGroup => 8,
        }
    }

    /// How long before the next run begins.
    fn every(self) -> Duration {
        match self {
            Self::InAGroupOfItsOwn => Duration::from_millis(700),
            Self::InTheWindowsGroup => Duration::from_secs(2),
        }
    }

    /// The user's script: it shows the branch of the folder, as the owner's does, and is
    /// busy for the seconds the rest of what it shows takes, in tenths of a second, which
    /// it gives up when the program that ran it has gone: a helper does not outlive the
    /// harness, and a window that closes does not wait for one.
    fn script(self) -> String {
        format!(
            "#!/bin/bash\n\
             git -C \"$PWD\" --no-optional-locks branch --show-current 2>/dev/null\n\
             for _ in $(seq {}); do\n  kill -0 \"$PPID\" 2>/dev/null || exit 0\n  sleep 0.1\ndone\n",
            self.seconds() * 10
        )
    }

    /// The window runs its status line from now on, the script written in `config`, on a
    /// thread of its own; a run is noted among the jobs of the window.
    pub fn go(self, config: &Path, jobs: &Arc<Jobs>) -> Result<(), String> {
        let script = config.join(SCRIPT);
        fs::write(&script, self.script())
            .map_err(|failed| format!("{}: {failed}", script.display()))?;
        let jobs = Arc::clone(jobs);
        thread::spawn(move || loop {
            if let Err(failed) = self.run(&script, &jobs) {
                let _ = writeln!(io::stderr(), "fake-agent: the status line: {failed}");
            }
            thread::sleep(self.every());
        });
        Ok(())
    }

    /// One run of the command: `bash` and the script, as the owner's shows them.
    fn run(self, script: &Path, jobs: &Jobs) -> Result<(), String> {
        let mut run = Run::new("/bin/bash")
            .arg(script)
            .inheriting_env()
            .closed_input();
        if self == Self::InAGroupOfItsOwn {
            run = run.own_group();
        }
        let mut shell = run.spawn().map_err(|failed| failed.to_string())?;
        jobs.note(LABEL, shell.id())?;
        // The handle ends the program it is dropped with, so it is held until the run is
        // over.
        thread::spawn(move || {
            while !shell.has_exited() {
                thread::sleep(LOOK);
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_line_is_asked_for_by_a_word_and_the_word_group_keeps_it_in_the_windows_group() {
        assert_eq!(StatusLine::of(None), None);
        assert_eq!(StatusLine::of(Some("")), None);
        assert_eq!(
            StatusLine::of(Some("own")),
            Some(StatusLine::InAGroupOfItsOwn)
        );
        assert_eq!(
            StatusLine::of(Some("group")),
            Some(StatusLine::InTheWindowsGroup)
        );
    }

    #[test]
    fn the_script_shows_the_branch_as_the_owners_does_and_goes_on_for_the_seconds_of_its_kind() {
        let own = StatusLine::InAGroupOfItsOwn.script();
        assert!(
            own.contains("git -C \"$PWD\" --no-optional-locks branch --show-current"),
            "{own}"
        );
        assert!(own.contains("for _ in $(seq 10); do"), "{own}");
        assert!(
            own.contains("kill -0 \"$PPID\""),
            "gone with its program: {own}"
        );
        assert!(StatusLine::InTheWindowsGroup
            .script()
            .contains("for _ in $(seq 80); do"));
        // A run of the first is over before the host looks again, and one of the second
        // is not.
        assert!(StatusLine::InAGroupOfItsOwn.seconds() < 2);
        assert!(StatusLine::InTheWindowsGroup.seconds() > 2);
    }
}
