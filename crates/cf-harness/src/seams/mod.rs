//! What an adapter is given besides its launch: the time and its waits,
//! randomness, a free port, a peer on loopback to ask over HTTP, the
//! programs it runs, and the bundle ConsensFlow ships with. The
//! engine gives the system's (the types here); a test gives fakes it drives
//! by hand (`testing`, behind the `test-support` feature). Each adapter is
//! built with the ones it uses, so nothing in a window reads the system's
//! time or randomness on its own, and a test sees every wait.

use std::cell::{Cell, RefCell};
use std::future::{poll_fn, Future};
use std::net::TcpListener;
use std::path::PathBuf;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use cf_base::env::Env;
use cf_base::time::{Clock, SystemClock};
use jiff::tz::TimeZone;

use crate::contract::{Records, Work};

pub mod loopback;
pub mod processes;

pub use loopback::{Loopback, SystemLoopback};
pub use processes::{Probes, Processes, SystemProcesses};

/// What the engine gives every adapter it builds: the environment its
/// windows run with, the records it reads them through, and the seams.
#[derive(Clone)]
pub struct Services {
    pub env: Env,
    pub records: Rc<dyn Records>,
    pub time: Rc<dyn Time>,
    pub entropy: Rc<dyn Entropy>,
    pub ports: Rc<dyn Ports>,
    pub loopback: Rc<dyn Loopback>,
    pub processes: Rc<dyn Processes>,
    /// The probes asked so far, one map for every adapter, as Node's was
    /// one per process.
    pub probes: Rc<Probes>,
    pub bundle: Bundle,
    /// The machine's time zone: a time of day said with none is read in it,
    /// as Node read the process's own.
    pub zone: TimeZone,
}

/// Where an adapter reads the time and waits.
pub trait Time {
    /// The wall clock, in milliseconds since the epoch: what JavaScript's
    /// `Date.now()` read, for a deadline as for a time written down.
    fn wall_ms(&self) -> i64;
    /// A wait of `duration`: a poll's interval, a deadline's timer.
    fn sleep(&self, duration: Duration) -> Work<'_, ()>;
}

/// Where an adapter draws what must not repeat: a session's id, a window's
/// name, a token.
pub trait Entropy {
    /// Fills `bytes`, or says why the system would not.
    fn fill(&self, bytes: &mut [u8]) -> Result<(), String>;
}

/// Where an adapter finds a port on loopback no one listens on.
pub trait Ports {
    fn free_loopback(&self) -> Result<u16, String>;
}

/// What ConsensFlow ships beside the daemon (`src/core/pane-cf.js`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// Its `bin`, first on every window's PATH.
    pub bin: PathBuf,
    /// Its native `cf`, as a process is started: a Codex window's
    /// supervisor.
    pub cf: PathBuf,
    /// Its `cf` as a window names it (in a role's text, in a hook): with
    /// forward slashes on Windows, which Git Bash keeps where it drops
    /// backslashes, and PowerShell reads alike.
    pub pane_cf: String,
}

/// A timer armed on `time`, which bounds the waits put under it: what
/// JavaScript armed once and bounded several waits with (a fetch's
/// `AbortSignal.timeout` its head and its body, a lifetime's `setTimeout`
/// every request of a launch).
pub struct Armed<'a> {
    timer: RefCell<Work<'a, ()>>,
    fired: Cell<bool>,
}

/// A timer of `millis` armed on `time` now, as JavaScript arms one when it
/// asks for it, before the work it bounds begins.
pub fn arm(time: &dyn Time, millis: u64) -> Armed<'_> {
    Armed {
        timer: RefCell::new(time.sleep(Duration::from_millis(millis))),
        fired: Cell::new(false),
    }
}

impl Armed<'_> {
    /// Whether its time has come (`signal.aborted`).
    pub fn fired(&self) -> bool {
        let _ = self.poll(&mut Context::from_waker(Waker::noop()));
        self.fired.get()
    }

    /// `work`, or none once the timer fired first, the work then dropped:
    /// nothing is begun under a timer that has fired, and when both are
    /// done at once the work's answer is taken.
    pub async fn bound<T>(&self, work: impl Future<Output = T>) -> Option<T> {
        if self.fired() {
            return None;
        }
        let mut work = pin!(work);
        poll_fn(|context| {
            if let Poll::Ready(answer) = work.as_mut().poll(context) {
                return Poll::Ready(Some(answer));
            }
            self.poll(context).map(|()| None)
        })
        .await
    }

    fn poll(&self, context: &mut Context<'_>) -> Poll<()> {
        if self.fired.get() {
            return Poll::Ready(());
        }
        let ready = self.timer.borrow_mut().as_mut().poll(context);
        if ready.is_ready() {
            self.fired.set(true);
        }
        ready
    }
}

/// `work`, or none once `millis` passed on `time` first, the work then
/// dropped: a wait JavaScript bounded with a timer of its own, armed and
/// bound at once.
pub async fn within<T>(time: &dyn Time, millis: u64, work: impl Future<Output = T>) -> Option<T> {
    arm(time, millis).bound(work).await
}

/// A uuid drawn from `entropy`, 16 bytes with the version 4 bits set, as
/// `randomUUID` writes one.
pub fn uuid(entropy: &dyn Entropy) -> Result<String, String> {
    let mut bytes = [0; 16];
    entropy.fill(&mut bytes)?;
    Ok(uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .to_string())
}

/// The system's time: the time of day as `SystemClock` reads it, waits on
/// the runtime's timer, which the engine's runtime drives.
pub struct SystemTime;

impl Time for SystemTime {
    fn wall_ms(&self) -> i64 {
        SystemClock.now_ms()
    }

    fn sleep(&self, duration: Duration) -> Work<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// The system's randomness.
pub struct SystemEntropy;

impl Entropy for SystemEntropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), String> {
        getrandom::fill(bytes).map_err(|failed| failed.to_string())
    }
}

/// A port the system hands out on loopback, let go at once
/// (`freeLoopbackPort`, `src/channels.js`).
pub struct LoopbackPorts;

impl Ports for LoopbackPorts {
    fn free_loopback(&self) -> Result<u16, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|failed| failed.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|failed| failed.to_string())?
            .port();
        if port == 0 {
            return Err("could not choose a loopback port".to_owned());
        }
        Ok(port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Driver, ManualTime};
    use std::cell::Cell;

    /// Marks when it is dropped.
    struct Dropped(Rc<Cell<bool>>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    #[test]
    fn work_done_in_time_is_answered_and_its_timer_forgotten() {
        let time = Rc::new(ManualTime::new(0));
        let mut driver = Driver::default();
        let clock = Rc::clone(&time);
        driver.begin(0, async move { within(&*clock, 10, async { 7 }).await });
        assert_eq!(driver.run(), [(0, Some(7))]);
        assert!(time.waits(0).is_empty());
    }

    #[test]
    fn work_out_of_time_is_dropped_and_none_answered() {
        let time = Rc::new(ManualTime::new(0));
        let dropped = Rc::new(Cell::new(false));
        let mut driver = Driver::default();
        let (clock, mark) = (Rc::clone(&time), Rc::clone(&dropped));
        driver.begin(0, async move {
            let held = async move {
                let _guard = Dropped(mark);
                std::future::pending::<()>().await;
            };
            within(&*clock, 10, held).await
        });
        assert!(driver.run().is_empty());
        assert!(!dropped.get());
        assert!(time.fire_next(10));
        assert_eq!(driver.run(), [(0, None)]);
        assert!(dropped.get(), "the work is let go");
    }

    #[test]
    fn one_timer_bounds_a_head_and_then_its_body_and_says_when_it_fired() {
        let time = Rc::new(ManualTime::new(0));
        let mut driver = Driver::default();
        let clock = Rc::clone(&time);
        driver.begin(0, async move {
            let attempt = arm(&*clock, 10);
            let inner = Rc::clone(&clock);
            let head = attempt.bound(async { 1 }).await;
            let body = attempt
                .bound(async move { inner.sleep(Duration::from_millis(50)).await })
                .await;
            let after = attempt.bound(async { 3 }).await;
            (head, body, after, attempt.fired())
        });
        assert!(driver.run().is_empty());
        assert_eq!(time.waits(0), [10, 50], "one timer for both, armed first");
        assert!(time.fire_next(10));
        assert_eq!(
            driver.run(),
            [(0, (Some(1), None, None, true))],
            "nothing begun under a timer that fired"
        );
    }

    #[test]
    fn the_timer_is_armed_before_the_work_begins() {
        let time = Rc::new(ManualTime::new(0));
        let mut driver = Driver::default();
        let clock = Rc::clone(&time);
        driver.begin(0, async move {
            let inner = Rc::clone(&clock);
            within(&*clock, 10, async move {
                inner.sleep(Duration::from_millis(5)).await
            })
            .await
        });
        assert!(driver.run().is_empty());
        assert_eq!(time.waits(0), [10, 5]);
    }

    /// A stream of the same byte.
    struct Same(u8);

    impl Entropy for Same {
        fn fill(&self, bytes: &mut [u8]) -> Result<(), String> {
            bytes.fill(self.0);
            Ok(())
        }
    }

    #[test]
    fn a_uuid_is_sixteen_bytes_drawn_with_the_version_4_bits_set() {
        assert_eq!(
            uuid(&Same(0)).unwrap(),
            "00000000-0000-4000-8000-000000000000"
        );
        assert_eq!(
            uuid(&Same(0xff)).unwrap(),
            "ffffffff-ffff-4fff-bfff-ffffffffffff"
        );
        let drawn = uuid(&SystemEntropy).unwrap();
        assert_eq!(drawn.len(), 36);
        assert_eq!(&drawn[14..15], "4");
    }

    #[test]
    fn a_free_port_on_loopback_is_one_a_listener_may_take() {
        let port = LoopbackPorts.free_loopback().unwrap();
        assert_ne!(port, 0);
        TcpListener::bind(("127.0.0.1", port)).unwrap();
    }
}
