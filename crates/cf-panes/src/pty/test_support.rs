//! What a test of the pane table, or of the app's, needs of it that no program does.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::PaneTable;

#[cfg(all(test, windows))]
thread_local! {
    /// Whether a test has the exit watcher of the windows this thread opens fail to start.
    pub(super) static WATCH_FAILS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
#[cfg(test)]
thread_local! {
    /// Whether a test has the terminal of the windows this thread opens fail to be taken.
    pub(super) static TERMINAL_FAILS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What a test holds of the opens that reach the table: each waits before it takes the
/// table's lock until the test lets it go, as an open that a loaded Mac is slow to begin
/// waits.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub(super) struct HoldState {
    released: Mutex<bool>,
    changed: Condvar,
    waiting: std::sync::atomic::AtomicUsize,
}

/// The hold a test put on the table's opens ([`PaneTable::hold_opens`]): opens that begin
/// while it is on wait for [`OpenHold::release`], or for it to be dropped.
#[cfg(any(test, feature = "test-support"))]
pub struct OpenHold(Arc<HoldState>);

#[cfg(any(test, feature = "test-support"))]
impl OpenHold {
    /// Waits until `count` opens wait on the hold: whether they did within `within`.
    pub fn wait_for_opens(&self, count: usize, within: Duration) -> bool {
        let until = Instant::now() + within;
        while self.0.waiting.load(Ordering::Acquire) < count {
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }

    /// Lets the opens that wait go, and every one that comes after.
    pub fn release(&self) {
        *self
            .0
            .released
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
        self.0.changed.notify_all();
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for OpenHold {
    fn drop(&mut self) {
        self.release();
    }
}
impl PaneTable {
    /// Holds every open that reaches the table from now until the hold is released or
    /// dropped: a test's way to have an open that is slow to begin.
    #[cfg(any(test, feature = "test-support"))]
    pub fn hold_opens(&self) -> OpenHold {
        let state = Arc::new(HoldState::default());
        *self
            .open_hold
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&state));
        OpenHold(state)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn wait_while_held(&self) {
        let held = self
            .open_hold
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let Some(held) = held else {
            return;
        };
        held.waiting.fetch_add(1, Ordering::AcqRel);
        let mut released = held.released.lock().unwrap_or_else(PoisonError::into_inner);
        while !*released {
            released = held
                .changed
                .wait(released)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The table as a thread that panicked holding its lock leaves it: every
    /// operation on it fails from then on, a kill's included.
    #[cfg(all(test, unix))]
    pub(crate) fn poison(&self) {
        std::thread::scope(|scope| {
            let poisoned = scope
                .spawn(|| {
                    let _held = self.panes.lock();
                    panic!("the pane table's lock is poisoned");
                })
                .join();
            assert!(poisoned.is_err());
        });
    }
}

/// The PTY tests' lock, one test at a time, held with a watchdog: a test that
/// hung holding it (three arbiter tests on a macOS runner, 2026-09-30) kept
/// the others waiting until the CI step's 15 minutes ran out, and nobody
/// learnt which. After 60 s, far past any PTY test's run, the watchdog names
/// the test on stderr (past the test's capture) and aborts, as the recorder's
/// ready read does.
#[cfg(any(test, feature = "test-support"))]
pub struct SerialPtyTest {
    _lock: MutexGuard<'static, ()>,
    done: Arc<AtomicBool>,
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for SerialPtyTest {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn serial_pty_test() -> SerialPtyTest {
    use std::io::Write as _;
    static LOCK: Mutex<()> = Mutex::new(());
    let lock = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let done = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&done);
    let test = std::thread::current()
        .name()
        .unwrap_or("a PTY test")
        .to_string();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            if finished.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = std::io::stderr().write_all(
            format!("{test} has held the PTY tests' lock for 60 s: it hung\n").as_bytes(),
        );
        std::process::abort();
    });
    SerialPtyTest { _lock: lock, done }
}

/// Whether a pid is still there — signal 0 delivers nothing and only asks.
#[cfg(all(any(test, feature = "test-support"), unix))]
pub fn process_exists(pid: i32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }

    // SAFETY: signal 0 does not deliver a signal; it only checks whether
    // the process exists and is signalable by this process.
    let result = unsafe { kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1)
}
