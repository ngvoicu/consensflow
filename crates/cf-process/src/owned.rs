//! A program started with a command of the caller's own: the streams it chose, the console it
//! was given, and nothing else of how it starts. Its owner ends it, and everything it started,
//! with [`Owned::end`].
//!
//! The Codex window's supervisor starts its two programs so. Its TUI is given the terminal the
//! window runs in, which `spawn`'s hidden, piped program cannot have: on Windows `CREATE_NO_WINDOW`
//! takes the console from a program ("the console handle for the application is not set").
//!
//! On Windows the program runs in a job of its own, made before it runs (`job`'s module has why,
//! and what a breakaway is; this job refuses one, as the window's job does), and ending it ends
//! the job: whatever the program starts as it is ended too. The id of a program alone, ended with `taskkill /T`, does not (zeewin, T-322:
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
/// before it runs, from which nothing breaks away. The caller's `kill_on_drop` is kept: the
/// job's end-on-close ends the rest. Why it did not start, if it did not (a program that could
/// not be let run did not start). A program the system would not put in a job runs in none:
/// see [`Owned::refusal`].
pub fn start_owned(command: &mut Command) -> io::Result<Owned> {
    job::start_suspended(command);
    let mut child = crate::spawning::one_at_a_time(|| command.spawn())?;
    match Job::sealed(&child) {
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

    /// Why the system gave it no job, if it gave none: it then runs outside any of ours, and
    /// [`Owned::end`] reaches it with `taskkill /T`, which loses what it starts as it is ended
    /// (T-322). Never on Unix, where there is no job to give.
    pub fn refusal(&self) -> Option<&str> {
        self.job.refusal()
    }

    /// Whether it has exited. Its job is released once it has (see [`Owned::wait`]).
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.job.release();
        }
        Ok(status)
    }

    /// Waits until it has exited, and releases its job then, whichever way it exited. One that
    /// exited of itself has what it left running let go of. One that was ended (`end`) has
    /// nothing left to hold: its job's processes were all ended with it.
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

    /// What an owned program that starts two programs, one of which asks to start a process outside
    /// its job (`CREATE_BREAKAWAY_FROM_JOB`), said of each, once it has said both lines.
    #[cfg(windows)]
    fn said_of_the_two(dir: &std::path::Path, run: &Run) -> (Vec<String>, Vec<u32>) {
        use crate::testing::{pids_so_far, Recorded, STARTS_WITHIN};
        let file = dir.join("pids");
        let _left = Recorded(&file);
        let runtime = runtime();
        let said = runtime.block_on(async {
            let mut owned = start_owned(&mut command_of(run)).unwrap();
            let until = Instant::now() + STARTS_WITHIN;
            let said = loop {
                let said = std::fs::read_to_string(dir.join("outcome")).unwrap_or_default();
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
        (said, pids_so_far(&file))
    }

    // The jobs of the Codex window's programs refuse the breakaway (Codex's `ensure_detached_launch`
    // asks for one, and is to be refused in a window, as the pane's job refuses it: a daemon let
    // through lands in the pane's job and dies at the window's close, T-424's P3-1).
    #[cfg(windows)]
    #[test]
    fn an_owned_program_that_asks_to_break_away_is_refused_and_all_it_started_is_ended_with_its_job(
    ) {
        use crate::testing::{gone_within, starts_with_and_without_breakaway};
        let dir = tempfile::tempdir().unwrap();
        let (said, pids) =
            said_of_the_two(dir.path(), &starts_with_and_without_breakaway(dir.path()));
        assert_eq!(said[0], "stays");
        assert!(
            said[1].starts_with("refused"),
            "nothing leaves the job of an owned program: {said:?}"
        );
        assert_eq!(pids.len(), 2, "{pids:?}");
        assert!(
            gone_within(pids[1], Duration::from_secs(5)),
            "the program that asked for nothing is in the job, and ended with it"
        );
    }

    /// The ids of the processes attached to this process's console, once it has one: given one
    /// where it had none (a test run with no console has the console of nothing to ask about).
    #[cfg(windows)]
    fn on_this_console() -> Vec<u32> {
        use windows_sys::Win32::System::Console::{AllocConsole, GetConsoleProcessList};
        let mut list = [0_u32; 1024];
        #[allow(unsafe_code)]
        // SAFETY: AllocConsole takes nothing and fails where this process has a console already,
        // which is then the one asked about; GetConsoleProcessList writes at most the 1024 ids
        // it is given room for, into `list`.
        let count = unsafe {
            AllocConsole();
            GetConsoleProcessList(list.as_mut_ptr(), 1024)
        };
        list[..usize::try_from(count).unwrap_or(0).min(1024)].to_vec()
    }

    /// A program that runs for half a minute at most, to be started and looked at.
    #[cfg(windows)]
    fn waits() -> Run {
        Run {
            program: r"C:\Windows\System32\PING.EXE".into(),
            args: ["-n", "31", "127.0.55.9"].map(Into::into).to_vec(),
            verbatim: false,
        }
    }

    // A TUI is given the terminal its window runs in: the program an owned start makes is attached
    // to the console of its caller (`CREATE_NO_WINDOW` would give it one of its own).
    #[cfg(windows)]
    #[test]
    fn an_owned_program_is_attached_to_the_console_of_its_caller() {
        // This process has a console before the program starts: one that starts where it has
        // none is given a console of its own, and a console given later is not its.
        on_this_console();
        let runtime = runtime();
        runtime.block_on(async {
            let mut owned = start_owned(&mut command_of(&waits())).unwrap();
            let pid = owned.id().unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while !on_this_console().contains(&pid) {
                assert!(
                    Instant::now() < until,
                    "it is not on this console, which holds {:?}",
                    on_this_console()
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            owned.end(Ending::Forced);
            let _ = owned.wait().await;
        });
        runtime.shutdown_timeout(Duration::from_secs(10));
    }

    // And a program started hidden, as every other of this crate's is, is not.
    #[cfg(windows)]
    #[test]
    fn a_program_started_hidden_has_a_console_of_its_own() {
        on_this_console();
        let runtime = runtime();
        runtime.block_on(async {
            let mut command = command_of(&waits());
            job::start_in_job(&mut command);
            let child = command.spawn().unwrap();
            let job = Job::of(&child, false).unwrap();
            let pid = child.id().unwrap();
            // The program is running; a console it was to share is shared by now.
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let on_this = on_this_console();
            job.terminate();
            assert!(
                !on_this.contains(&pid),
                "it is on this console, which holds {on_this:?}"
            );
        });
        runtime.shutdown_timeout(Duration::from_secs(10));
    }

    // A program the system will not put in a job runs in none and says why, and is ended all the
    // same, by `taskkill` (T-424's P3-2: the window's supervisor says so when it has ended). The
    // system refuses none that a test can ask it to (see `Job::of_through`), so this one is refused.
    #[cfg(windows)]
    #[test]
    fn an_owned_program_the_system_will_not_put_in_a_job_says_so_and_is_ended_without_one() {
        const WHY: &str = "could not be put in a job of its own: Access is denied. (os error 5)";
        let runtime = runtime();
        runtime.block_on(async {
            let mut command = command_of(&waits());
            job::start_suspended(&mut command);
            let child = command.spawn().unwrap();
            let job = Job::of_through(&child, false, false, |_, _, _| Err(WHY.to_owned()))
                .expect("a program that is not held whole is let run all the same");
            let mut owned = Owned { child, job };
            assert_eq!(owned.refusal(), Some(WHY));
            let pid = owned.id().unwrap();
            assert!(crate::alive(pid), "it was let run");
            owned.end(Ending::Forced);
            let _ = owned.wait().await;
            assert!(
                crate::testing::gone_within(pid, Duration::from_secs(5)),
                "ended by taskkill, which is all that reaches a program in no job"
            );
        });
        runtime.shutdown_timeout(Duration::from_secs(10));
    }
}
