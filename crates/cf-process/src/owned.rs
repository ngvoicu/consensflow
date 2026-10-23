//! A program started with a command of the caller's own: the streams it chose, the console it
//! was given, and nothing else of how it starts. Its owner ends it, and everything it started,
//! with [`Owned::end`].
//!
//! The Codex window's supervisor starts its two programs so. Its TUI is given the terminal the
//! window runs in, which `spawn`'s hidden, piped program cannot have: on Windows `CREATE_NO_WINDOW`
//! takes the console from a program ("the console handle for the application is not set").
//!
//! On Windows the program runs in a job of its own, made before it runs (`job`'s module has why,
//! and what a breakaway is), and ending it ends the job: whatever the program starts as it is
//! ended too. The id of a program alone, ended with `taskkill /T`, does not (zeewin, T-322:
//! 21 to 50 of a loop's programs outlived every one of 26 ends, and a program whose parent had
//! gone was never read at all). A program that ended of itself has finished its work: its job
//! is released and what it left running is left, as everywhere in this crate. Elsewhere there is
//! no job, and the program is signalled alone, as `terminate` has it.

use std::io;
use std::process::ExitStatus;

use tokio::process::{Child, ChildStderr, Command};

use crate::job::{self, Job};
use crate::terminate::{end, Reach};
use crate::Ending;

/// A program running beside this one, and the job it runs in.
pub struct Owned {
    child: Child,
    job: Job,
}

/// Starts `command` as the caller built it. On Windows it starts suspended, its console and its
/// window left to it and its creation flags that one alone, and is put in a job of its own
/// before it runs. The caller's `kill_on_drop` is kept: the job's end-on-close ends the rest.
/// Why it did not start, if it did not (a program that could not be let run did not start).
pub fn start_owned(command: &mut Command) -> io::Result<Owned> {
    job::start_suspended(command);
    let mut child = command.spawn()?;
    match Job::of(&child, false) {
        Ok(job) => Ok(Owned { child, job }),
        Err(why) => {
            let _ = child.start_kill();
            Err(io::Error::other(why))
        }
    }
}

impl Owned {
    /// Its pid; none once it has been waited for, when the pid may be another's.
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// Its error stream, once.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    /// Whether it has exited. One that has exited of itself has its job released.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.job.release();
        }
        Ok(status)
    }

    /// Waits until it has exited. One that has exited of itself has its job released.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait().await?;
        self.job.release();
        Ok(status)
    }

    /// Ends it, not waiting: on Unix the process alone, with the signal `how` names; on Windows its
    /// whole job at once, everything in it, whatever `how` asks. Nothing is sent to one that has
    /// been waited for.
    pub fn end(&self, how: Ending) {
        if let Some(pid) = self.child.id() {
            end(pid, how, Reach::Process, true, &self.job);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::testing::{leaves_a_writer, pids_in, Survivors};
    use crate::Run;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    /// `run` as a caller of `start_owned` builds it: its streams its own, ended with it should it
    /// be dropped.
    fn command_of(run: &Run) -> Command {
        let mut command = Command::from(run.command());
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command
    }

    #[cfg(unix)]
    #[test]
    fn an_owned_program_is_ended_with_the_signal_asked_for_and_never_once_waited_for() {
        use std::os::unix::process::ExitStatusExt;
        runtime().block_on(async {
            for (how, signal) in [
                (Ending::Asked, libc::SIGTERM),
                (Ending::Forced, libc::SIGKILL),
            ] {
                let run = Run {
                    program: "/bin/sleep".into(),
                    args: vec!["30".into()],
                    verbatim: false,
                };
                let mut owned = start_owned(&mut command_of(&run)).unwrap();
                assert!(owned.id().is_some());
                owned.end(how);
                let status = owned.wait().await.unwrap();
                assert_eq!(status.signal(), Some(signal), "{how:?}");
                assert_eq!(owned.id(), None, "its pid may be another's now");
                // Nothing is sent to it.
                owned.end(how);
            }
        });
    }

    #[test]
    fn a_program_that_is_not_there_is_not_started() {
        let run = Run {
            program: "/nonexistent/cli".into(),
            args: Vec::new(),
            verbatim: false,
        };
        let failed = start_owned(&mut command_of(&run)).err().map(|e| e.kind());
        assert_eq!(failed, Some(io::ErrorKind::NotFound));
    }

    #[test]
    fn a_program_that_exits_of_itself_leaves_what_it_started_running() {
        // It is seen to have exited by being waited for, and by being looked at.
        for waited in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let (file, marked) = (dir.path().join("pids"), dir.path().join("late"));
            runtime().block_on(async {
                let mut owned =
                    start_owned(&mut command_of(&leaves_a_writer(&file, &marked, 2))).unwrap();
                if waited {
                    owned.wait().await.unwrap();
                } else {
                    while owned.try_wait().unwrap().is_none() {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                }
                // Dropped with it: a job that was not released would end what the program left.
                drop(owned);
            });
            let pids = pids_in(&file, 2).expect("written before it ended");
            let _left = Survivors(pids);
            let until = Instant::now() + Duration::from_secs(15);
            while !marked.exists() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                marked.exists(),
                "waited for: {waited}: what it left running was let go of, and wrote"
            );
        }
    }

    // Windows ends the job whatever it is asked; Unix signals the process alone.
    #[cfg(windows)]
    #[test]
    fn an_owned_program_that_is_starting_programs_is_ended_with_everything_it_started() {
        use crate::testing::{
            keeps_starting_programs, left_running, pids_so_far, Recorded, STARTS_WITHIN,
        };
        for how in [Ending::Asked, Ending::Forced] {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("pids");
            let _left = Recorded(&file);
            let runtime = runtime();
            let (left, started) = runtime.block_on(async {
                let mut owned =
                    start_owned(&mut command_of(&keeps_starting_programs(dir.path()))).unwrap();
                let until = Instant::now() + STARTS_WITHIN;
                while pids_so_far(&file).len() < 4 {
                    assert!(Instant::now() < until, "it started none");
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                owned.end(how);
                let left = left_running(&file, Duration::from_secs(5)).await;
                let _ = owned.wait().await;
                (left, pids_so_far(&file).len())
            });
            runtime.shutdown_timeout(Duration::from_secs(10));
            assert!(
                left.is_empty(),
                "{how:?}: {} of the {started} processes it started are still running: {left:?}",
                left.len()
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn an_owned_program_that_asks_to_break_away_may_and_the_one_that_does_not_ask_is_ended_with_its_job(
    ) {
        use crate::testing::{
            gone_within, pids_so_far, starts_with_and_without_breakaway, Recorded, STARTS_WITHIN,
        };
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pids");
        let _left = Recorded(&file);
        let runtime = runtime();
        let said = runtime.block_on(async {
            let mut owned = start_owned(&mut command_of(&starts_with_and_without_breakaway(
                dir.path(),
            )))
            .unwrap();
            let until = Instant::now() + STARTS_WITHIN;
            let said = loop {
                let said = std::fs::read_to_string(dir.path().join("outcome")).unwrap_or_default();
                let lines: Vec<String> = said.lines().map(str::to_owned).collect();
                if lines.len() >= 2 {
                    break lines;
                }
                assert!(Instant::now() < until, "the program said {lines:?}");
                tokio::time::sleep(Duration::from_millis(20)).await;
            };
            owned.end(Ending::Forced);
            let _ = owned.wait().await;
            said
        });
        runtime.shutdown_timeout(Duration::from_secs(10));
        assert_eq!(
            said,
            ["stays", "breaks"],
            "the program that asked to leave started"
        );
        let pids = pids_so_far(&file);
        assert_eq!(pids.len(), 3, "{pids:?}");
        assert!(
            gone_within(pids[1], Duration::from_secs(5)),
            "the program that asked for nothing is in the job, and ended with it"
        );
        assert!(
            crate::alive(pids[2]),
            "the program that asked to be outside the job is outside it: the end left it"
        );
    }
}
