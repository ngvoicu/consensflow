//! The daemon's own log in the home, `daemon.log`: one line per thing worth
//! knowing afterwards (it started, it stopped and why, a pass that failed or
//! ran long, an error nobody caught), with the error's text indented under it.
//! One previous file is kept once the log passes its limit. Never a reason for
//! the daemon to fail: a home that cannot be written loses the line, not the
//! run.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cf_base::file::append_rotating;
use cf_base::time::{iso, Clock, SystemClock};

/// How big the log may get before it is moved aside: 5 MB.
pub const LIMIT: u64 = 5_000_000;

/// The log of a home. One may go to another thread: the records' thread
/// writes to the home's log with a log of its own.
pub struct Log {
    file: PathBuf,
    limit: u64,
    clock: RefCell<Box<dyn Clock + Send>>,
    /// The thread that appends the lines, where whoever says them must not wait for the file:
    /// another process's lock on it or a slow disk holds that thread alone. None: the line is
    /// appended where it is said, which is also where the thread cannot be made.
    writer: Option<Writer>,
}

/// How many lines wait for the writer, beyond those it is appending (at most as many again,
/// taken at once): past that a line is dropped and counted, and the next one that is written
/// says how many were.
const QUEUE: usize = 1024;

/// The queue to the writer's thread, and how many lines it dropped.
#[derive(Clone)]
struct Writer {
    lines: SyncSender<Job>,
    dropped: Arc<AtomicU64>,
    /// Lines that are never dropped ([`Log::keep`]) and found the queue full: written once
    /// what the queue held is, after the count of the dropped.
    kept: Arc<Mutex<String>>,
}

/// A piece of the writer's work.
enum Job {
    Line(String),
    /// Who waits to hear that everything said so far is written, and when it asked: the
    /// count of the dropped lines and the lines kept are written first.
    Written {
        at: String,
        heard: std::sync::mpsc::Sender<()>,
    },
}

/// The line that says how many lines found no room.
fn dropped_line(at: &str, dropped: u64) -> String {
    format!("{at} warn {dropped} lines of the log were dropped: its file was held\n")
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
            writer: None,
        }
    }

    /// The log of `home` as the daemon says it: each line is handed to a thread of the log's
    /// own, and whoever says it never waits for the file (T-2232). A line the thread is too far
    /// behind to take is dropped and counted. [`Log::written`] waits for what was said.
    pub fn off_thread(home: &Path) -> Self {
        let mut log = Self::new(home);
        let (lines, queued) = sync_channel::<Job>(QUEUE);
        let (file, limit) = (log.file.clone(), log.limit);
        let dropped = Arc::new(AtomicU64::new(0));
        let kept = Arc::new(Mutex::new(String::new()));
        let (counted, held) = (Arc::clone(&dropped), Arc::clone(&kept));
        // A thread that cannot be made leaves the lines to be appended where they are said.
        let spawned = std::thread::Builder::new()
            .name("daemon-log".to_owned())
            .spawn(move || {
                while let Ok(first) = queued.recv() {
                    // What waits is appended at once: a file that was held gets all of it as
                    // soon as it is free, within the little time a stop has.
                    let mut text = String::new();
                    let mut waiting = None;
                    let mut next = Some(first);
                    while let Some(job) = next {
                        match job {
                            Job::Line(line) => text.push_str(&line),
                            Job::Written { at, heard } => {
                                let dropped = counted.swap(0, Ordering::SeqCst);
                                if dropped > 0 {
                                    text.push_str(&dropped_line(&at, dropped));
                                }
                                let mut kept = held.lock().unwrap_or_else(PoisonError::into_inner);
                                text.push_str(&std::mem::take(&mut *kept));
                                waiting = Some(heard);
                                break;
                            }
                        }
                        next = queued.try_recv().ok();
                    }
                    if !text.is_empty() {
                        // The home may be read-only or gone mid-run.
                        let _ = append_rotating(&file, &text, limit);
                    }
                    if let Some(heard) = waiting {
                        let _ = heard.send(());
                    }
                }
            });
        if spawned.is_ok() {
            log.writer = Some(Writer {
                lines,
                dropped,
                kept,
            });
        }
        log
    }

    /// Waits for every line said so far to be written, the count of those dropped after them
    /// and the lines kept after that, `within` at most: whether they were.
    pub fn written(&self, within: Duration) -> bool {
        let Some(writer) = &self.writer else {
            return true;
        };
        let (heard, hear) = std::sync::mpsc::channel();
        let began = Instant::now();
        let mut job = Job::Written {
            at: iso(self.clock.borrow_mut().now_ms()),
            heard,
        };
        loop {
            match writer.lines.try_send(job) {
                Ok(()) => break,
                Err(TrySendError::Full(again)) if began.elapsed() < within => {
                    job = again;
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(_) => return false,
            }
        }
        hear.recv_timeout(within.saturating_sub(began.elapsed()))
            .is_ok()
    }

    /// Where the log is.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// A thing worth knowing.
    pub fn info(&self, message: &str) {
        self.write("info", message, None);
    }

    /// A thing worth knowing that is never dropped (a stop's own lines): one the writer has no
    /// room for is written once it has written what it holds ([`Log::written`]), after the
    /// count of the lines dropped, and every such line after it follows it there.
    pub fn keep(&self, message: &str) {
        let Some(writer) = &self.writer else {
            self.info(message);
            return;
        };
        let at = iso(self.clock.borrow_mut().now_ms());
        let line = format!("{at} info {message}\n");
        let mut kept = writer.kept.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.is_empty() {
            let dropped = writer.dropped.swap(0, Ordering::SeqCst);
            let text = if dropped == 0 {
                line.clone()
            } else {
                format!("{}{line}", dropped_line(&at, dropped))
            };
            if writer.lines.try_send(Job::Line(text)).is_ok() {
                return;
            }
            writer.dropped.fetch_add(dropped, Ordering::SeqCst);
        }
        kept.push_str(&line);
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
        let text = format!("{}\n", lines.join("\n"));
        let Some(writer) = &self.writer else {
            // The home may be read-only or gone mid-run.
            let _ = append_rotating(&self.file, &text, self.limit);
            return;
        };
        let dropped = writer.dropped.swap(0, Ordering::SeqCst);
        let text = if dropped == 0 {
            text
        } else {
            format!("{}{text}", dropped_line(&at, dropped))
        };
        if writer.lines.try_send(Job::Line(text)).is_err() {
            writer.dropped.fetch_add(dropped + 1, Ordering::SeqCst);
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
    /// A log whose file another process holds takes none of the daemon's time: the line goes to
    /// a writer of its own (T-2232, finding 4).
    #[test]
    fn a_line_said_while_the_log_is_held_does_not_wait_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::off_thread(dir.path());
        let beside = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(cf_base::file::lock_beside(log.file()))
            .unwrap();
        beside.lock().unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(700));
            beside.unlock().unwrap();
        });
        let began = std::time::Instant::now();
        cf_engine::seams::Log::warn(&log, "the launch of @chief went on without it");
        let took = began.elapsed();
        release.join().unwrap();
        assert!(
            took < std::time::Duration::from_millis(200),
            "the line waited for the log's lock: {took:?}"
        );
        assert!(log.written(std::time::Duration::from_secs(5)));
        assert!(std::fs::read_to_string(log.file())
            .unwrap()
            .ends_with("warn the launch of @chief went on without it\n"));
    }

    #[test]
    fn lines_the_writer_is_too_far_behind_to_take_are_dropped_and_counted() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::off_thread(dir.path());
        let beside = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(cf_base::file::lock_beside(log.file()))
            .unwrap();
        beside.lock().unwrap();
        // The writer holds at most two queues' worth: more is dropped.
        let said = 4 * QUEUE;
        for line in 0..said {
            log.info(&format!("line {line}"));
        }
        beside.unlock().unwrap();
        assert!(log.written(std::time::Duration::from_secs(20)));
        log.info("after");
        assert!(log.written(std::time::Duration::from_secs(20)));
        let text = std::fs::read_to_string(log.file()).unwrap();
        let written = text
            .lines()
            .filter(|line| line.contains(" info line "))
            .count();
        let counted: usize = text
            .lines()
            .filter(|line| line.contains("lines of the log were dropped"))
            .map(|line| {
                line.split(" warn ")
                    .nth(1)
                    .and_then(|rest| rest.split(' ').next())
                    .and_then(|count| count.parse::<usize>().ok())
                    .unwrap()
            })
            .sum();
        assert!(counted > 0, "some were dropped: {written} written");
        assert_eq!(written + counted, said, "every line is written or counted");
        assert!(text.ends_with("info after\n"));
    }
}
