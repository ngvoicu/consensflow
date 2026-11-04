//! The reader of the system's log of privacy dialogs, kept running while a window is open.
//!
//! It starts when a first window opens and ends when the last has been closed for
//! [`CLOSE_AFTER`], so that a Mac with no window pays nothing. It reads `log stream` through
//! [`Stream`], joins the lines of a request ([`super::lines::Joiner`]), and for each dialog raised
//! in the app's name tells [`Tells`] which window's tree holds the process that waits on it, if
//! one does. A reader that ends is started again after a pause that doubles up to
//! [`BACKOFF_MAX`]. A reader that cannot be read from (a user who is not in the admin group is
//! refused with "Must be admin to run 'stream' command"; or one that ends at once, again and
//! again) is told once as unreadable ([`Tells::unreadable`]) and is not started again: nothing
//! is told of any dialog after that, and the limit is written down.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::lines::{read_line, Joiner, Line, Prompt};
use crate::pty::PaneKey;

/// How long to wait before looking again for a first window: short, since `log stream` shows only
/// what happens after it is attached, and a command a window runs at once is not to be missed.
pub(crate) const IDLE_POLL: Duration = Duration::from_millis(250);
/// How long a read waits for a line before looking at the windows and the stop.
pub(crate) const READ_WAIT: Duration = Duration::from_secs(1);
/// How long no window is open before the reader is ended.
pub(crate) const CLOSE_AFTER: Duration = Duration::from_secs(10);
/// A reader that ends sooner than this after it started ended at once.
pub(crate) const QUICK_END: Duration = Duration::from_secs(5);
/// How many readers in a row may end at once before the log is called unreadable.
pub(crate) const QUICK_ENDS: u32 = 3;
/// The pause before a reader that ended is started again, first and at most.
pub(crate) const BACKOFF_FIRST: Duration = Duration::from_secs(1);
pub(crate) const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// A reader that ran this long before it ended starts the pauses over.
pub(crate) const STEADY: Duration = Duration::from_secs(60);
/// How many lines the reader printed that are no events are kept to say why it ended.
const SAID_KEPT: usize = 4;
/// What the reader prints first, once it is connected to the log.
const BANNER: &str = "Filtering the log data";

/// What a wait for the reader's next line found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Read {
    Line(String),
    /// Nothing came in the time.
    Quiet,
    /// The reader has ended.
    End,
}

/// A reader of the log, started.
pub(crate) trait Stream {
    /// The next line, or that none came within `wait`, or that the reader ended.
    fn recv(&mut self, wait: Duration) -> Read;
    /// Asks the reader to end: its end is read as [`Read::End`]. Only a reader this side
    /// started, and has not seen end, is asked.
    fn end(&mut self);
}

/// The windows that are open.
pub(crate) trait Windows {
    fn open(&self) -> usize;
    /// The window whose tree holds the process `pid`.
    fn holding(&self, pid: u32) -> Option<PaneKey>;
    /// Any window that is open.
    fn any(&self) -> Option<PaneKey>;
}

/// Who is told.
pub(crate) trait Tells {
    /// A dialog is raised in the app's name for `prompt`'s process, held by `window` where a
    /// window's tree holds it.
    fn prompt(&self, window: Option<PaneKey>, prompt: &Prompt);
    /// The log cannot be read, and the reader said `said`.
    fn unreadable(&self, window: Option<PaneKey>, said: &str);
}

/// The clock, and the waiting.
pub(crate) trait Time {
    fn now(&self) -> Instant;
    fn sleep(&self, how_long: Duration);
}

/// Starts a reader of the log for the dialogs raised in the name of the process `app`, or says
/// why it could not start.
pub(crate) type Start<'a> = &'a dyn Fn(u32) -> Result<Box<dyn Stream + 'a>, String>;

/// What one reader did before it ended.
struct Run {
    /// Whether it printed its banner, which it does once it is connected to the log.
    banner: bool,
    /// The first lines it printed that were no events: the words of why it ended.
    said: Vec<String>,
    /// Whether this side asked it to end (the windows have gone, or a stop).
    asked: bool,
}

/// Runs until `stop` is set: a reader while a window is open, the dialogs it reads told.
pub(crate) fn supervise(
    app: u32,
    start: Start<'_>,
    windows: &dyn Windows,
    tells: &dyn Tells,
    time: &dyn Time,
    stop: &AtomicBool,
) {
    let mut backoff = BACKOFF_FIRST;
    let mut quick = 0;
    while !stop.load(Ordering::SeqCst) {
        if windows.open() == 0 {
            time.sleep(IDLE_POLL);
            continue;
        }
        let began = time.now();
        let mut stream = match start(app) {
            Ok(stream) => stream,
            Err(why) => {
                tells.unreadable(windows.any(), &why);
                return;
            }
        };
        let run = read(&mut *stream, app, windows, tells, time, stop);
        let lasted = time.now().saturating_duration_since(began);
        // Ended by this side: the windows went, or the stop came. Neither is a failure.
        if run.asked {
            continue;
        }
        if !run.banner {
            tells.unreadable(
                windows.any(),
                &words(&run.said, "the reader ended before it read"),
            );
            return;
        }
        if lasted < QUICK_END {
            quick += 1;
            if quick >= QUICK_ENDS {
                tells.unreadable(
                    windows.any(),
                    &words(&run.said, "the reader ended at once, again and again"),
                );
                return;
            }
        } else {
            quick = 0;
        }
        if lasted >= STEADY {
            backoff = BACKOFF_FIRST;
        }
        time.sleep(backoff);
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// What the reader said before it ended, or `otherwise` where it said nothing.
fn words(said: &[String], otherwise: &str) -> String {
    if said.is_empty() {
        otherwise.to_owned()
    } else {
        said.join(" / ")
    }
}

/// Reads one reader to its end: its dialogs are told as they come, and it is asked to end
/// when the stop is set or no window has been open for [`CLOSE_AFTER`].
fn read(
    stream: &mut dyn Stream,
    app: u32,
    windows: &dyn Windows,
    tells: &dyn Tells,
    time: &dyn Time,
    stop: &AtomicBool,
) -> Run {
    let mut joiner = Joiner::new(app);
    let mut run = Run {
        banner: false,
        said: Vec::new(),
        asked: false,
    };
    let mut empty_since: Option<Instant> = None;
    loop {
        if stop.load(Ordering::SeqCst) && !run.asked {
            stream.end();
            run.asked = true;
        }
        match stream.recv(READ_WAIT) {
            Read::End => return run,
            Read::Quiet => {}
            Read::Line(line) => match read_line(&line) {
                Line::Event { tccd, message } => {
                    if let Some(prompt) = joiner.feed(tccd, &message) {
                        tells.prompt(windows.holding(prompt.pid), &prompt);
                    }
                }
                Line::Text(text) if text.starts_with(BANNER) => run.banner = true,
                Line::Text(text) => {
                    if !text.is_empty() && run.said.len() < SAID_KEPT {
                        run.said.push(text);
                    }
                }
            },
        }
        if run.asked {
            continue;
        }
        if windows.open() == 0 {
            let since = *empty_since.get_or_insert_with(|| time.now());
            if time.now().saturating_duration_since(since) >= CLOSE_AFTER {
                stream.end();
                run.asked = true;
            }
        } else {
            empty_since = None;
        }
    }
}

#[cfg(test)]
mod tests;
