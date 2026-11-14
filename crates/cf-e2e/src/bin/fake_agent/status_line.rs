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
//! four tenths of a second long, as a status line's script takes a moment, and a new one
//! begins before the last has ended, so that one is at work at any moment (a run of a
//! second, ten `sleep`s, outlived the host's second look on GitHub's loaded macOS runner,
//! CI on 9eb0eab2); `group` is a helper of the harness that stays in the group of the
//! window's program, as its servers do, and goes on for longer than the host waits
//! before it looks again.
//!
//! The host names a run that is still there at its second look, 2 s after the first
//! (`background::SETTLE`), as it names any process, so a run of four tenths of a second is not
//! named unless the machine is so loaded that it lasts past the look, which a loaded Mac does
//! (T-1328: a suite at a load of 83, three of its runs in a close's note; T-1343). Each run
//! therefore says how long it lived, in `<file>.lived` beside the job's `.pid`, in milliseconds,
//! once it has ended, so that a case can tell a run the host was right to name (it lived past
//! the look) from one it was not.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::process::Run;

use crate::background::Jobs;

/// What the user's command is called: `~/.claude/statusline-command.sh` is where Claude
/// Code's is, and the stand-in's is in its own folder.
const SCRIPT: &str = "statusline-command.sh";

/// The `git` the script runs, in the folder beside it: the branch at once, as the machine's `git`
/// answers where it is a program of its own and not a shim that starts one.
const GIT: &str = "git";

/// What the stand-in's `git` is: a program that shows the branch and ends.
const GIT_SCRIPT: &str = "#!/bin/sh\necho main\n";

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

    /// How many tenths of a second the script goes on after it has shown the branch,
    /// unless the window's program is gone before.
    fn tenths(self) -> u64 {
        match self {
            Self::InAGroupOfItsOwn => 4,
            Self::InTheWindowsGroup => 80,
        }
    }

    /// How long before the next run begins.
    fn every(self) -> Duration {
        match self {
            Self::InAGroupOfItsOwn => Duration::from_millis(300),
            Self::InTheWindowsGroup => Duration::from_secs(2),
        }
    }

    /// The user's script: it shows the branch of the folder, as the owner's does, and is
    /// busy for the time the rest of what it shows takes, in tenths of a second, which
    /// it gives up when the program that ran it has gone: a helper does not outlive the
    /// harness, and a window that closes does not wait for one.
    ///
    /// It runs only what a run must: the `git` beside it, a `sleep` for each tenth, and builtins of
    /// the shell for the rest (no `dirname`, no `seq`: a process started under a load of 80 takes
    /// a moment of its own).
    ///
    /// It runs the `git` beside it ([`GIT`]), not the machine's: GitHub's macOS runner has Xcode's
    /// shim for `/usr/bin/git`, which on a cold cache runs `xcodebuild -sdk … -find git` and takes
    /// seconds, so that a run of four tenths of a second outlasted the host's second look, 2 s
    /// on, and was told as work its window left running (CI run 39136355690).
    fn script(self) -> String {
        format!(
            "#!/bin/bash\n\
             \"${{0%/*}}/{GIT}\" -C \"$PWD\" --no-optional-locks branch --show-current 2>/dev/null\n\
             for ((tenth = 0; tenth < {}; tenth++)); do\n  kill -0 \"$PPID\" 2>/dev/null || exit 0\n  /bin/sleep 0.1\ndone\n",
            self.tenths()
        )
    }

    /// The user's script and the `git` it runs written in `config`: the script's path.
    fn install(self, config: &Path) -> Result<std::path::PathBuf, String> {
        let (script, git) = (config.join(SCRIPT), config.join(GIT));
        cf_e2e::door::write(&script, self.script())
            .map_err(|failed| format!("{}: {failed}", script.display()))?;
        cf_e2e::door::write(&git, GIT_SCRIPT)
            .map_err(|failed| format!("{}: {failed}", git.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            cf_e2e::door::set_permissions(&git, fs::Permissions::from_mode(0o755))
                .map_err(|failed| format!("{}: {failed}", git.display()))?;
        }
        Ok(script)
    }

    /// The window runs its status line from now on, the script written in `config`, on a
    /// thread of its own; a run is noted among the jobs of the window.
    pub fn go(self, config: &Path, jobs: &Arc<Jobs>) -> Result<(), String> {
        let script = self.install(config)?;
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
        // The life of the run is counted from before it is started, and to the look that sees it
        // ended, so that it is never counted shorter than it was.
        let began = Instant::now();
        let mut shell = run.spawn().map_err(|failed| failed.to_string())?;
        let file = jobs.note(LABEL, shell.id())?;
        // The handle ends the program it is dropped with, so it is held until the run is
        // over.
        thread::spawn(move || {
            while !shell.has_exited() {
                thread::sleep(LOOK);
            }
            let lived = began.elapsed().as_millis();
            let _ = cf_e2e::door::write_whole(file.with_extension("lived"), &format!("{lived}\n"));
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
    fn the_script_shows_the_branch_as_the_owners_does_and_goes_on_for_the_time_of_its_kind() {
        let own = StatusLine::InAGroupOfItsOwn.script();
        assert!(
            own.contains("/git\" -C \"$PWD\" --no-optional-locks branch --show-current"),
            "{own}"
        );
        assert!(
            own.contains("for ((tenth = 0; tenth < 4; tenth++)); do"),
            "{own}"
        );
        assert!(
            !own.contains("dirname") && !own.contains("seq"),
            "no process for what the shell does: {own}"
        );
        assert!(
            own.contains("kill -0 \"$PPID\""),
            "gone with its program: {own}"
        );
        assert!(StatusLine::InTheWindowsGroup
            .script()
            .contains("tenth < 80; tenth++"));
        // A run of the first is over well before the host looks again, 2 s on, and one
        // of the second is not.
        assert!(StatusLine::InAGroupOfItsOwn.tenths() * 4 <= 20);
        assert!(StatusLine::InTheWindowsGroup.tenths() > 20);
    }

    /// A run says how long it lived, beside its `.pid`, once it has ended: a case that finds
    /// the host naming a run judges it by that, and not by what a loaded machine made of the
    /// four tenths of a second the script goes on for (T-1343). It is never shorter than the
    /// script's own sleeps.
    #[cfg(unix)]
    #[test]
    fn a_run_says_how_long_it_lived_beside_its_pid_once_it_has_ended() {
        let home = cf_e2e::door::made("cf-test-").unwrap();
        let jobs = Jobs::new(&home.path().join("consensflow"), 7);
        let config = cf_e2e::door::made("cf-test-").unwrap();
        let script = StatusLine::InAGroupOfItsOwn.install(config.path()).unwrap();
        StatusLine::InAGroupOfItsOwn.run(&script, &jobs).unwrap();
        let folder = home.path().join("jobs");
        let lived = folder.join("statusline-7-1.lived");
        // Only a run that never ends runs out of this.
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while !lived.exists() {
            assert!(std::time::Instant::now() < deadline, "the run never ended");
            thread::sleep(Duration::from_millis(20));
        }
        let ms: u64 = fs::read_to_string(&lived).unwrap().trim().parse().unwrap();
        assert!(
            ms >= 400,
            "four tenths of a second of sleeps at least: {ms}"
        );
        assert!(folder.join("statusline-7-1.pid").is_file());
    }

    /// A run of the script takes the time of its kind whatever the machine's `git` takes to
    /// start. GitHub's macOS runner has Xcode's shim for it, which on a cold cache takes seconds
    /// (the first `git` of a runner ran `xcodebuild -sdk … -find git`, and the status line was
    /// told as work its window left running, CI run 39136355690): here the machine's `git` is
    /// first on the PATH and takes three.
    #[cfg(unix)]
    #[test]
    fn a_run_of_the_script_takes_the_time_of_its_kind_however_long_the_machines_git_takes_to_start()
    {
        use std::os::unix::fs::PermissionsExt;

        let machine = cf_e2e::door::made("cf-test-").unwrap();
        let slow = machine.path().join("git");
        fs::write(&slow, "#!/bin/sh\n/bin/sleep 3\necho machine\n").unwrap();
        fs::set_permissions(&slow, fs::Permissions::from_mode(0o755)).unwrap();
        let config = cf_e2e::door::made("cf-test-").unwrap();
        let script = StatusLine::InAGroupOfItsOwn.install(config.path()).unwrap();
        let ran = Run::new("/bin/bash")
            .arg(&script)
            .cwd(config.path())
            .var(
                "PATH",
                format!("{}:/usr/bin:/bin", machine.path().display()),
            )
            .closed_input()
            .run()
            .unwrap();
        assert_eq!(ran.code, Some(0), "{}", ran.stderr);
        // It shows the branch of the folder, as the owner's does, from its own `git`: the
        // machine's, three seconds slow and first on the PATH, would have said "machine" (and
        // no time of the machine's is in the case: how long a run takes is what the run says of
        // itself, `.lived`).
        assert_eq!(ran.stdout.trim(), "main");
    }
}
