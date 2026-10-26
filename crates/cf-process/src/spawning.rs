//! Starting a process, one at a time in this one.
//!
//! macOS makes a pipe in two steps, `pipe` and then `fcntl(FD_CLOEXEC)`, and so does std for the
//! pipes of a program it starts. A program that another thread forks between the two steps
//! inherits the pipe's end, and holds it for as long as it lives. A capture whose program has
//! ended then waits for the stream of a program it never started, a program that is asked to end
//! after its reader was dropped meets a pipe that is not closed (it does not die of SIGPIPE), and
//! the crate's tests failed under load for it (T-581: probed with a thread that makes children
//! with piped output beside three that fork a sleep, 8 reads of 4,500 waited out the stranger's
//! 150 ms, and none of 4,500 once a lock was held for each start).
//!
//! So every program this crate starts is started under one lock, the making of its pipes and the
//! fork together: no other start of the crate forks between the two steps. A program another
//! crate, or a test, starts with its own `Command` is not under it, and the tests of this crate
//! start theirs through [`StartedUnderTheLock`].
//!
//! Windows makes no pipe in two steps (std makes the handles of a pipe not inheritable as it
//! makes them, and the child is given the ones it is to have), so the lock guards nothing there
//! and costs the starts their order only. `taskkill`, which `terminate` starts there to end a
//! program that has no job, is started outside it: it has no stream of ours to inherit.

use std::sync::{Mutex, PoisonError};

static STARTING: Mutex<()> = Mutex::new(());

/// Runs `start`, which starts a program, with no other start of this crate under way. It is held
/// for the start alone: never across an `.await`, nor while a program is waited for.
pub(crate) fn one_at_a_time<T>(start: impl FnOnce() -> T) -> T {
    let _starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
    start()
}

/// The lock, held by a test that looks at what waits for it.
#[cfg(all(test, unix))]
pub(crate) fn hold() -> std::sync::MutexGuard<'static, ()> {
    STARTING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How a test starts a program of its own beside the crate's: `spawn` and `status` of a `Command`
/// the test built (`Command::new` is the method clippy refuses, and the test allows it where it
/// builds one), under the lock the crate's starts take, since a test that forks outside it, as another thread makes the pipes of a capture,
/// hands the program the pipes' ends (T-581: a `sleep` a test started held the read of a
/// capture's output for its 30 s).
#[cfg(test)]
pub(crate) trait StartedUnderTheLock {
    fn spawn_locked(&mut self) -> std::io::Result<std::process::Child>;
    #[cfg(unix)]
    fn status_locked(&mut self) -> std::io::Result<std::process::ExitStatus>;
}

#[cfg(test)]
impl StartedUnderTheLock for std::process::Command {
    fn spawn_locked(&mut self) -> std::io::Result<std::process::Child> {
        one_at_a_time(|| self.spawn())
    }

    #[cfg(unix)]
    fn status_locked(&mut self) -> std::io::Result<std::process::ExitStatus> {
        one_at_a_time(|| self.status())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use cf_base::env::Env;

    use super::*;
    use crate::{capture, hand_over, spawn, start_owned, Limits, Run, Streams};

    /// A start of a program, given the run and the file the program writes.
    type Start = Box<dyn Fn(&Run, &Path) + Send>;

    fn writes(marker: &Path) -> Run {
        Run {
            program: PathBuf::from("/bin/sh"),
            args: [
                "-c".to_owned(),
                format!("echo started > '{}'", marker.display()),
            ]
            .map(OsString::from)
            .to_vec(),
            verbatim: false,
        }
    }

    #[allow(clippy::unwrap_used)]
    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    /// Whether `marker` is written within `within`.
    fn written_within(marker: &Path, within: Duration) -> bool {
        let until = Instant::now() + within;
        while !marker.exists() {
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }

    /// How many of `rounds` reads of the output of a program that has ended waited out a
    /// stranger: a program that exits at once is started with a piped output while three
    /// threads start sleeps of half a second, each start under the lock if `locked`. The read
    /// ends as the pipe's write ends close, and a sleep that was forked between the two steps
    /// of the pipe's making holds one for its half second. A read that took over 400 ms is one.
    #[allow(clippy::disallowed_methods, clippy::unwrap_used)] // The test starts what it probes.
    fn reads_that_waited_out_a_stranger(rounds: usize, locked: bool) -> usize {
        use std::io::Read;
        use std::process::{Command, Stdio};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let start = move |command: &mut Command| {
            if locked {
                one_at_a_time(|| command.spawn())
            } else {
                command.spawn()
            }
        };
        let done = Arc::new(AtomicBool::new(false));
        // At most 25 sleeps each at a time (half a second, one every 20 ms): tens.
        let strangers: Vec<_> = (0..3)
            .map(|_| {
                let done = Arc::clone(&done);
                std::thread::spawn(move || {
                    while !done.load(Ordering::Relaxed) {
                        let mut sleep = Command::new("/bin/sleep");
                        sleep.arg("0.5");
                        if let Ok(mut child) = start(&mut sleep) {
                            std::thread::spawn(move || {
                                let _ = child.wait();
                            });
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                })
            })
            .collect();
        let mut slow = 0;
        for _ in 0..rounds {
            let mut exits = Command::new("/bin/sh");
            exits.args(["-c", "exit 0"]).stdout(Stdio::piped());
            let mut child = start(&mut exits).unwrap();
            let mut output = child.stdout.take().unwrap();
            let began = Instant::now();
            output.read_to_end(&mut Vec::new()).unwrap();
            if began.elapsed() > Duration::from_millis(400) {
                slow += 1;
            }
            child.wait().unwrap();
        }
        done.store(true, Ordering::Relaxed);
        for stranger in strangers {
            stranger.join().unwrap();
        }
        slow
    }

    /// The probe behind the lock (T-581; the lock came of 8 reads of 4,500 that waited out a
    /// stranger without it, none with it). Opt in, 3,000 starts of a shell and some 1,000 of
    /// sleeps for each of the two arms, some tens of programs at a time, a minute in all:
    ///
    /// `cargo test -p cf-process --lib -- --ignored a_pipe_is_not_inherited --nocapture`
    ///
    /// Only the locked arm is asserted: that the other loses reads is a race, and says how often
    /// on this machine.
    #[test]
    #[ignore = "a stress of some 4,500 starts: cargo test -p cf-process --lib -- --ignored a_pipe_is_not_inherited --nocapture"]
    fn a_pipe_is_not_inherited_by_a_program_forked_beside_its_making() {
        const ROUNDS: usize = 3_000;
        let without = reads_that_waited_out_a_stranger(ROUNDS, false);
        let with = reads_that_waited_out_a_stranger(ROUNDS, true);
        eprintln!("reads of {ROUNDS} that waited out a stranger: {without} without the lock, {with} with it");
        assert_eq!(with, 0, "a read waited out a stranger with the lock held");
    }

    #[test]
    fn every_way_a_program_is_started_waits_for_a_start_that_is_under_way() {
        // The start of another is held, as it is while it makes its pipes and forks; a program
        // started now would be forked in the middle of that.
        let folder = tempfile::tempdir().unwrap();
        let starts: [(&str, Start); 4] = [
            (
                "capture",
                Box::new(|run, _| {
                    let limits = Limits {
                        timeout: Duration::ZERO,
                        max_buffer: 1024,
                    };
                    let _ = runtime().block_on(capture(
                        run,
                        None,
                        &Env::from_process(),
                        limits,
                        |_| {},
                    ));
                }),
            ),
            (
                "spawn",
                Box::new(|run, marker| {
                    let runtime = runtime();
                    let _guard = runtime.enter();
                    // Kept until it has written: a program dropped is ended.
                    let _child = spawn(run, None, &Env::from_process(), Streams::Quiet);
                    written_within(marker, Duration::from_secs(30));
                }),
            ),
            (
                "hand over",
                Box::new(|run, _| {
                    let _ = runtime().block_on(hand_over(
                        run,
                        &Env::from_process(),
                        Duration::from_secs(10),
                    ));
                }),
            ),
            (
                "start owned",
                Box::new(|run, marker| {
                    let runtime = runtime();
                    let _guard = runtime.enter();
                    let mut command = tokio::process::Command::from(run.command());
                    let _owned = start_owned(&mut command);
                    written_within(marker, Duration::from_secs(30));
                }),
            ),
        ];
        for (name, start) in starts {
            let marker = folder
                .path()
                .join(format!("{}.marker", name.replace(' ', "-")));
            let held = hold();
            let (run, waited_for) = (writes(&marker), marker.clone());
            let starting = std::thread::spawn(move || start(&run, &waited_for));
            assert!(
                !written_within(&marker, Duration::from_millis(500)),
                "{name} started a program while another was being started"
            );
            drop(held);
            assert!(
                written_within(&marker, Duration::from_secs(30)),
                "{name} did not start its program once no other was being started"
            );
            starting.join().unwrap();
        }
    }
}
