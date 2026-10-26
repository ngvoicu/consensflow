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
//! crate, or a test, starts with its own `Command` is not under it.

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

/// How a test starts a program of its own beside the crate's: under the lock the crate's starts
/// take, since a test that forks outside it, as another thread makes the pipes of a capture,
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
    #[allow(clippy::disallowed_methods)] // The one place a test starts a program.
    fn spawn_locked(&mut self) -> std::io::Result<std::process::Child> {
        one_at_a_time(|| self.spawn())
    }

    #[cfg(unix)]
    #[allow(clippy::disallowed_methods)] // The one place a test starts a program.
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
