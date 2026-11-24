//! The daemon's own log in the home, `daemon.log`: one line per thing worth
//! knowing afterwards (it started, it stopped and why, a pass that failed or
//! ran long, an error nobody caught), with the error's text indented under it.
//! One previous file is kept once the log passes its limit. Never a reason for
//! the daemon to fail: a home that cannot be written loses the line, not the
//! run.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, OnceLock};
use std::thread;
use std::time::Duration;

use cf_base::file::append_rotating;
use cf_base::time::{iso, Clock, SystemClock};

use crate::halt::Halt;

/// How big the log may get before it is moved aside: 5 MB.
pub const LIMIT: u64 = 5_000_000;

/// The log of a home. One may go to another thread: the records' thread
/// writes to the home's log with a log of its own.
pub struct Log {
    file: PathBuf,
    limit: u64,
    clock: RefCell<Box<dyn Clock + Send>>,
    /// Once the daemon is ending (a stop, a break) every line is written on a thread
    /// off the runtime that is waited for [`SETTLE`](super::SETTLE) at most.
    halt: OnceLock<Arc<Halt>>,
}

impl Log {
    /// The log of `home`, `daemon.log`, on the system's clock.
    pub fn new(home: &Path) -> Self {
        Self::with_clock(home.join("daemon.log"), LIMIT, Box::new(SystemClock))
    }

    /// A log in `file` that moves it aside past `limit` bytes and reads the
    /// time on `clock`.
    pub fn with_clock(file: PathBuf, limit: u64, clock: Box<dyn Clock + Send>) -> Self {
        Self {
            file,
            limit,
            clock: RefCell::new(clock),
            halt: OnceLock::new(),
        }
    }

    /// From the moment `halt` says the daemon is ending, a line is written only off the
    /// runtime and waited for a moment: a log that another process holds must keep nothing
    /// from stopping, whichever path writes it.
    pub fn ends_with(&self, halt: Arc<Halt>) {
        let _ = self.halt.set(halt);
    }

    /// Where the log is.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// A thing worth knowing.
    pub fn info(&self, message: &str) {
        self.write("info", message, None);
    }

    /// Something that went wrong and was gone past, with what is known of why.
    pub fn warn(&self, message: &str, cause: Option<&str>) {
        self.write("warn", message, cause);
    }

    /// Something that failed, with what is known of why, which goes
    /// underneath, four spaces in.
    pub fn error(&self, message: &str, cause: Option<&str>) {
        self.write("error", message, cause);
    }

    /// [`Log::info`] that gives up after `wait` if the log is held (another process
    /// holds its lock, a disk that does not answer): what the stop says, which must
    /// never keep the stop from ending.
    pub fn info_within(&self, message: &str, wait: Duration) {
        self.write_within("info", message, None, wait);
    }

    /// [`Log::warn`] that gives up after `wait`, as [`Log::info_within`].
    pub fn warn_within(&self, message: &str, wait: Duration) {
        self.write_within("warn", message, None, wait);
    }

    /// [`Log::error`] that gives up after `wait`, as [`Log::info_within`].
    pub fn error_within(&self, message: &str, cause: Option<&str>, wait: Duration) {
        self.write_within("error", message, cause, wait);
    }

    fn text(&self, level: &str, message: &str, cause: Option<&str>) -> String {
        let at = iso(self.clock.borrow_mut().now_ms());
        let mut lines = vec![format!("{at} {level} {message}")];
        if let Some(text) = cause {
            lines.extend(text.split('\n').map(|line| format!("    {line}")));
        }
        format!("{}\n", lines.join("\n"))
    }

    fn write(&self, level: &str, message: &str, cause: Option<&str>) {
        if self.halt.get().is_some_and(|halt| halt.stopping()) {
            return self.write_within(level, message, cause, super::SETTLE);
        }
        // The home may be read-only or gone mid-run.
        let _ = append_rotating(&self.file, &self.text(level, message, cause), self.limit);
    }

    /// The append is made on a thread of its own and waited for `wait` at most; one
    /// that is still held then is left to finish, or not, by itself.
    fn write_within(&self, level: &str, message: &str, cause: Option<&str>, wait: Duration) {
        let text = self.text(level, message, cause);
        let (file, limit) = (self.file.clone(), self.limit);
        let (done, heard) = mpsc::channel();
        let started = thread::Builder::new()
            .name("consensflow-log".to_owned())
            .spawn(move || {
                let _ = append_rotating(&file, &text, limit);
                let _ = done.send(());
            });
        if started.is_ok() {
            let _ = heard.recv_timeout(wait);
        }
    }
}

/// What the engine writes down of what failed apart from anything that waits
/// for it (a launch or a delivery that nobody awaits), and in one line of its
/// own of a window that did not come up.
impl cf_engine::seams::Log for Log {
    fn error(&self, message: &str, cause: &str) {
        Log::error(self, message, Some(cause));
    }

    fn info(&self, message: &str) {
        Log::info(self, message);
    }

    fn warn(&self, message: &str) {
        Log::warn(self, message, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_base::file::aside;

    /// A clock that reads a moment of 2026-10-05 and moves a second a reading.
    struct Ticks(i64);

    impl Clock for Ticks {
        fn now_ms(&mut self) -> i64 {
            self.0 += 1000;
            self.0
        }
    }

    fn log_in(dir: &Path, limit: u64) -> Log {
        Log::with_clock(
            dir.join("daemon.log"),
            limit,
            Box::new(Ticks(1_791_194_400_000)),
        )
    }

    /// From the moment the daemon is ending, a line that another process holds the lock of
    /// is waited for a moment and let go, whichever path writes it (a pass's error, the
    /// bridge's fatal callback, a caught panic): none of them is written on the runtime.
    #[test]
    fn a_line_written_while_the_daemon_ends_does_not_wait_for_a_held_log() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(dir.path(), LIMIT);
        let halt = crate::halt::Halt::new();
        log.ends_with(std::sync::Arc::clone(&halt));
        let held = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(cf_base::file::lock_beside(log.file()))
            .unwrap();
        held.lock().unwrap();
        let letting_go = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            drop(held);
        });
        halt.break_ledger();
        let began = std::time::Instant::now();
        log.error("a pass failed", Some("after the hook"));
        log.warn("the bridge failed", None);
        log.info("caught");
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
        letting_go.join().unwrap();
    }

    #[test]
    fn a_line_is_its_time_its_level_and_its_message() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(dir.path(), LIMIT);
        log.info("start pid 7 home /h");
        log.warn("a pass took 6000 ms", None);
        assert_eq!(
            std::fs::read_to_string(log.file()).unwrap(),
            "2026-10-05T10:00:01.000Z info start pid 7 home /h\n\
             2026-10-05T10:00:02.000Z warn a pass took 6000 ms\n"
        );
    }

    #[test]
    fn what_is_known_of_a_failure_goes_under_it_four_spaces_in_a_line_each() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(dir.path(), LIMIT);
        log.error("a pass failed", Some("it said no\nat the second step\n"));
        assert_eq!(
            std::fs::read_to_string(log.file()).unwrap(),
            "2026-10-05T10:00:01.000Z error a pass failed\n    it said no\n    at the second step\n    \n"
        );
    }

    #[test]
    fn the_log_is_moved_aside_once_past_its_limit_and_one_previous_file_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(dir.path(), 60);
        // Each line is 37 bytes: the file is past 60 with its second.
        for number in 0..6 {
            log.info(&format!("line {number}"));
        }
        let kept = std::fs::read_to_string(aside(log.file())).unwrap();
        let current = std::fs::read_to_string(log.file()).unwrap();
        assert!(kept.contains("line 2") && kept.contains("line 3"), "{kept}");
        assert!(
            current.contains("line 4") && current.contains("line 5"),
            "{current}"
        );
        assert!(
            !kept.contains("line 0"),
            "the first went with the one before"
        );
    }

    #[test]
    fn the_log_of_a_home_is_moved_aside_past_five_million_bytes_and_not_at_them() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("daemon.log"), vec![b'x'; 5_000_000]).unwrap();
        let log = Log::new(dir.path());
        log.info("at the limit");
        assert!(!aside(log.file()).exists(), "five million are not past it");
        log.info("past the limit");
        assert!(aside(log.file()).exists(), "past it, it is moved aside");
    }

    #[test]
    fn a_home_that_cannot_be_written_loses_the_line_and_not_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::with_clock(
            dir.path().join("gone").join("daemon.log"),
            LIMIT,
            Box::new(Ticks(0)),
        );
        log.info("nobody hears this");
        log.error("nor this", Some("cause"));
        assert!(!log.file().exists());
    }

    #[test]
    fn the_engine_s_warning_is_one_line_and_nothing_underneath() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(dir.path(), LIMIT);
        cf_engine::seams::Log::warn(
            &log,
            "the launch of p1-zeus failed: the window closed (exit code 3)",
        );
        assert_eq!(
            std::fs::read_to_string(log.file()).unwrap(),
            "2026-10-05T10:00:01.000Z warn the launch of p1-zeus failed: the window closed (exit code 3)\n"
        );
    }

    #[test]
    fn the_engine_s_error_is_the_log_s_with_its_cause_underneath() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(dir.path(), LIMIT);
        cf_engine::seams::Log::error(&log, "the launch of @zeus failed", "no model");
        assert_eq!(
            std::fs::read_to_string(log.file()).unwrap(),
            "2026-10-05T10:00:01.000Z error the launch of @zeus failed\n    no model\n"
        );
    }
}
