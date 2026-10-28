//! What the app does with a panic, and on macOS with an exception of the system's, in the code
//! the operating system's event loop calls (T-908).
//!
//! On 2026-10-28 the live app (3.0.6-alpha.3) aborted inside tao's `sendEvent:` override, an
//! `extern "C"` function, after 2 h 26 min. The error log held one line of it, "panic in a
//! function that cannot unwind", and no panic before it: what unwound into tao's function was no
//! Rust panic but, as far as can be told, an Objective-C exception AppKit raised while it
//! dispatched an event. AppKit's own handler of it, in `-[NSApplication run]`, is above tao's
//! frame and never saw it, so nothing named it.
//!
//! So every panic is written to the error log as it happens, with its place and its backtrace,
//! before anything can abort ([`record_in`]); an abort that no panic began is said to be the
//! foreign unwind it is. The app's own callbacks stop a panic at their edge ([`contained`]), and
//! on macOS AppKit's dispatch of an event is run so that what unwinds out of it ends there,
//! below tao's frame ([`appkit`]). What is stopped is written down, the person is told once
//! ([`tell_with`]), and the app runs on, as AppKit itself would have it.

use std::backtrace::Backtrace;
use std::cell::Cell;
use std::io::Write;
use std::panic::{self, AssertUnwindSafe, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
pub(crate) mod appkit;

/// What Rust says as it aborts an unwind that reached a function that cannot unwind.
const CANNOT_UNWIND: &str = "panic in a function that cannot unwind";

/// How soon after a panic of its thread such an abort is that panic's.
const UNWOUND_WITHIN: Duration = Duration::from_secs(1);

thread_local! {
    /// When this thread's last panic began.
    static LAST_PANIC: Cell<Option<Instant>> = const { Cell::new(None) };
}

type LogPath = Box<dyn Fn() -> Option<PathBuf> + Send + Sync>;

/// Where the panics are written, as `record_in` was given it.
static LOG: OnceLock<LogPath> = OnceLock::new();

/// How the person is told that something was stopped, as `tell_with` was given it.
static TELLER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Whether the person was told.
static TOLD: AtomicBool = AtomicBool::new(false);

/// Writes every panic of the process from now on to the file `log` names at the time, in place
/// of std's hook, which writes to stderr without a backtrace (`RUST_BACKTRACE` is not set where
/// the app runs). Written with one append and synced before the hook returns, so a panic that
/// ends in an abort is on disk before it. Called once, by `run`.
pub(crate) fn record_in(log: impl Fn() -> Option<PathBuf> + Send + Sync + 'static) {
    let _ = LOG.set(Box::new(log));
    panic::set_hook(Box::new(|info| write(&describe(info))));
}

/// How the person is told, once, that the app stopped something and ran on.
pub(crate) fn tell_with(tell: impl Fn() + Send + Sync + 'static) {
    let _ = TELLER.set(Box::new(tell));
}

/// What the person is told, with the log it is written in.
pub(crate) fn told(log: Option<&Path>) -> String {
    let log = log.map_or_else(
        || "the app's error log".to_string(),
        |log| log.display().to_string(),
    );
    format!(
        "ConsensFlow stopped an error inside the app and kept running. What happened is written in \
         {log}. If something looks wrong, quit ConsensFlow and open it again."
    )
}

/// `work`, a callback of the app's own that the event loop calls, with a panic in it stopped
/// at its edge: written down, told, and None for it. Unstopped, a panic there unwinds into the
/// event loop's frames, and those of the system's that cannot unwind abort the app.
pub(crate) fn contained<T>(edge: &str, work: impl FnOnce() -> T) -> Option<T> {
    match panic::catch_unwind(AssertUnwindSafe(work)) {
        Ok(done) => Some(done),
        Err(_) => {
            stopped(&format!("{edge} panicked"));
            None
        }
    }
}

/// Writes down what was stopped, and tells the person the first time.
pub(crate) fn stopped(what: &str) {
    write(&format!(
        "{} consensflow: {what}; the app stopped it there and runs on\n",
        now()
    ));
    if let Some(tell) = TELLER.get() {
        if !TOLD.swap(true, Ordering::AcqRel) {
            tell();
        }
    }
}

/// A panic as it is written: when, which thread, where, what, and the backtrace.
fn describe(info: &PanicHookInfo<'_>) -> String {
    let message = info
        .payload_as_str()
        .unwrap_or("(a panic that says nothing)");
    let place = info
        .location()
        .map_or_else(|| "an unknown place".to_string(), ToString::to_string);
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("unnamed");
    let earlier = LAST_PANIC.with(|last| last.replace(Some(Instant::now())));
    let foreign =
        message == CANNOT_UNWIND && !earlier.is_some_and(|began| began.elapsed() < UNWOUND_WITHIN);
    let note = if foreign {
        "No panic began on this thread before it: what unwound was no Rust panic but a foreign \
         exception, most likely an Objective-C one the system's code raised.\n"
    } else {
        ""
    };
    format!(
        "{} consensflow: thread '{name}' panicked at {place}:\n{message}\n{note}backtrace:\n{}\n",
        now(),
        Backtrace::force_capture()
    )
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Appends `text` to the log, or to stderr where there is none.
fn write(text: &str) {
    let opened = LOG.get().and_then(|log| log()).and_then(|log| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .ok()
    });
    match opened {
        Some(mut file) => {
            let _ = file.write_all(text.as_bytes());
            let _ = file.sync_data();
        }
        None => {
            let _ = std::io::stderr().write_all(text.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests;
