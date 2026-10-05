//! The daemon's own log in the home, `daemon.log` (`src/core/log.js`): one
//! line per thing worth knowing afterwards (it started, it stopped and why, a
//! pass that failed or ran long, an error nobody caught), with the error's
//! text indented under it. One previous file is kept once the log passes its
//! limit. Never a reason for the daemon to fail: a home that cannot be written
//! loses the line, not the run.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use cf_base::file::append_rotating;
use cf_base::time::{iso, Clock, SystemClock};

/// How big the log may get before it is moved aside: 5 MB.
pub const LIMIT: u64 = 5_000_000;

/// The log of a home.
pub struct Log {
    file: PathBuf,
    limit: u64,
    clock: RefCell<Box<dyn Clock>>,
}

impl Log {
    /// The log of `home`, `daemon.log`, on the system's clock.
    pub fn new(home: &Path) -> Self {
        Self::with_clock(home.join("daemon.log"), LIMIT, Box::new(SystemClock))
    }

    /// A log in `file` that moves it aside past `limit` bytes and reads the
    /// time on `clock`.
    pub fn with_clock(file: PathBuf, limit: u64, clock: Box<dyn Clock>) -> Self {
        Self {
            file,
            limit,
            clock: RefCell::new(clock),
        }
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

    fn write(&self, level: &str, message: &str, cause: Option<&str>) {
        let at = iso(self.clock.borrow_mut().now_ms());
        let mut lines = vec![format!("{at} {level} {message}")];
        if let Some(text) = cause {
            lines.extend(text.split('\n').map(|line| format!("    {line}")));
        }
        // The home may be read-only or gone mid-run.
        let _ = append_rotating(&self.file, &format!("{}\n", lines.join("\n")), self.limit);
    }
}

/// What the engine writes down of what failed apart from anything that waits
/// for it: a launch or a delivery that nobody awaits.
impl cf_engine::seams::Log for Log {
    fn error(&self, message: &str, cause: &str) {
        Log::error(self, message, Some(cause));
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
