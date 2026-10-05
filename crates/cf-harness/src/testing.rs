//! Fakes of what the engine gives an adapter, which a test drives by hand:
//! a clock that moves only when told, randomness from a known stream, the
//! records and a pane host whose answers can be held, and a driver that
//! runs begun work until it waits on one of them. Nothing here starts a
//! runtime: a wait an adapter makes on anything but these never ends, and
//! the driver says so.
//!
//! Unit tests in `src/` name these `crate::testing::…`; the crate's own
//! `tests/` and other crates reach them through the `test-support` feature.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use jiff::tz::TimeZone;
use serde_json::Value;

use crate::contract::{HostError, PaneHost, Records, Work};
use crate::records::{self, Cache, Options, Reading, IDLE_MS};
use crate::seams::{Bundle, Entropy, Ports, Services, Time};

thread_local! {
    /// The work the driver polls now, which a wait made meanwhile belongs to.
    static POLLING: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The work a wait made now belongs to: the work the driver polls.
fn polling() -> Option<usize> {
    POLLING.with(Cell::get)
}

/// A wait that ends once its flag is set.
struct Flagged(Rc<Cell<bool>>);

impl Future for Flagged {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.0.get() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// A clock that moves only when the test moves it, from a time of day it
/// starts at, and the sleeps armed on it, in the order they were armed. A
/// sleep given up (dropped) is no longer armed.
pub struct ManualTime {
    now: Cell<i64>,
    sleepers: RefCell<Vec<Sleeper>>,
}

/// A sleep armed: when it is due, the work it belongs to, and its flag.
struct Sleeper {
    due: i64,
    work: Option<usize>,
    flag: Weak<Cell<bool>>,
}

impl ManualTime {
    pub fn new(wall_ms: i64) -> Self {
        Self {
            now: Cell::new(wall_ms),
            sleepers: RefCell::new(Vec::new()),
        }
    }

    /// Ends the sleep due first, if one is due by `until`, the one armed
    /// first of those due together, moving the clock to when it was due:
    /// whether there was one. A test runs its work between one firing and
    /// the next, as Node runs a timer's continuations before the next timer.
    pub fn fire_next(&self, until: i64) -> bool {
        let mut sleepers = self.sleepers.borrow_mut();
        sleepers.retain(|sleeper| sleeper.flag.strong_count() > 0);
        let Some(due) = sleepers.iter().map(|sleeper| sleeper.due).min() else {
            return false;
        };
        if due > until {
            return false;
        }
        self.now.set(due.max(self.now.get()));
        if let Some(first) = sleepers.iter().position(|sleeper| sleeper.due == due) {
            if let Some(flag) = sleepers.remove(first).flag.upgrade() {
                flag.set(true);
            }
        }
        true
    }

    /// How long until each sleep `work` waits on is due, in the order armed.
    pub fn waits(&self, work: usize) -> Vec<i64> {
        let now = self.now.get();
        self.sleepers
            .borrow()
            .iter()
            .filter(|sleeper| sleeper.work == Some(work) && sleeper.flag.strong_count() > 0)
            .map(|sleeper| sleeper.due - now)
            .collect()
    }

    /// Moves the clock to `until` with nothing more to fire.
    pub fn settle_at(&self, until: i64) {
        self.now.set(until.max(self.now.get()));
    }
}

impl Time for ManualTime {
    fn wall_ms(&self) -> i64 {
        self.now.get()
    }

    fn sleep(&self, duration: Duration) -> Work<'_, ()> {
        let flag = Rc::new(Cell::new(false));
        let millis = i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
        let due = self.now.get().saturating_add(millis);
        self.sleepers.borrow_mut().push(Sleeper {
            due,
            work: polling(),
            flag: Rc::downgrade(&flag),
        });
        Box::pin(Flagged(flag))
    }
}

/// Randomness from a known stream, the bytes the Node recorder hands out
/// too (`(index * 7 + 3) % 256`), and the size of every draw, in order.
#[derive(Default)]
pub struct ScriptedEntropy {
    next: Cell<usize>,
    draws: RefCell<Vec<usize>>,
}

impl ScriptedEntropy {
    /// The size of every draw since the last time they were taken.
    pub fn take_draws(&self) -> Vec<usize> {
        std::mem::take(&mut self.draws.borrow_mut())
    }
}

impl Entropy for ScriptedEntropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), String> {
        let start = self.next.get();
        for (offset, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::try_from(((start + offset) * 7 + 3) % 256).unwrap_or_default();
        }
        self.next.set(start + bytes.len());
        self.draws.borrow_mut().push(bytes.len());
        Ok(())
    }
}

/// Ports handed out from a list, in order.
pub struct FixedPorts(pub RefCell<VecDeque<u16>>);

impl Ports for FixedPorts {
    fn free_loopback(&self) -> Result<u16, String> {
        self.0
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| "could not choose a loopback port".to_owned())
    }
}

/// The records as the engine serves them, read here on the test's own
/// thread through a cache of readers, at the time `time` says. A look can
/// be held: it then reads the record only when released, as it is then.
pub struct LocalRecords {
    env: Env,
    time: Rc<dyn Time>,
    cache: RefCell<Cache>,
    hold: Cell<bool>,
    held: RefCell<VecDeque<HeldLook>>,
}

/// A look held: the work it belongs to, and the flag that releases it.
struct HeldLook {
    work: Option<usize>,
    flag: Rc<Cell<bool>>,
}

impl LocalRecords {
    /// Records of `env`'s harnesses; a reset named by a time of day alone
    /// is read in UTC.
    pub fn new(env: Env, time: Rc<dyn Time>) -> Self {
        let cache = Cache::new(records::open(TimeZone::UTC), IDLE_MS, time.wall_ms());
        Self {
            env,
            time,
            cache: RefCell::new(cache),
            hold: Cell::new(false),
            held: RefCell::new(VecDeque::new()),
        }
    }

    /// Whether the looks to come wait to be released.
    pub fn hold(&self, hold: bool) {
        self.hold.set(hold);
    }

    /// Releases the look held longest: whether one was.
    pub fn release(&self) -> bool {
        self.held.borrow_mut().pop_front().is_some_and(|look| {
            look.flag.set(true);
            true
        })
    }

    /// How many held looks `work` waits on.
    pub fn waits(&self, work: usize) -> usize {
        self.held
            .borrow()
            .iter()
            .filter(|look| look.work == Some(work))
            .count()
    }
}

impl Records for LocalRecords {
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        let held = self.hold.get().then(|| {
            let flag = Rc::new(Cell::new(false));
            self.held.borrow_mut().push_back(HeldLook {
                work: polling(),
                flag: Rc::clone(&flag),
            });
            Flagged(flag)
        });
        Box::pin(async move {
            if let Some(released) = held {
                released.await;
            }
            let now = self.time.wall_ms();
            self.cache
                .borrow_mut()
                .look(harness, session, &self.env, options, now)
        })
    }

    fn has_transcript<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async move { records::has_transcript(harness, session, &self.env) })
    }
}

/// A host's answer to a request, or why it never came.
type Reply = Result<Value, HostError>;

/// Where a held request's answer is put when it is released.
type Hold = Rc<RefCell<Option<Reply>>>;

/// What a scripted host answers a request with.
#[derive(Debug, Clone)]
pub enum Answer {
    /// At once.
    Now(Reply),
    /// When the test releases it.
    Held,
}

/// A pane host that answers each request with the next answer scripted for
/// its operation, at once or when released, and keeps what it was asked.
/// A request with no answer left fails, as Node's scripted host threw.
#[derive(Default)]
pub struct ScriptedHost {
    answers: RefCell<HashMap<String, VecDeque<Answer>>>,
    asked: RefCell<Vec<(String, Value)>>,
    held: RefCell<VecDeque<HeldRequest>>,
}

/// A request held: its operation, the work it belongs to, and where its
/// answer is put.
struct HeldRequest {
    op: String,
    work: Option<usize>,
    hold: Hold,
}

impl ScriptedHost {
    /// Scripts the next answers to `op`, after those scripted already.
    pub fn answer(&self, op: &str, answers: impl IntoIterator<Item = Answer>) {
        self.answers
            .borrow_mut()
            .entry(op.to_owned())
            .or_default()
            .extend(answers);
    }

    /// The requests asked since the last time they were taken.
    pub fn take_asked(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.asked.borrow_mut())
    }

    /// The operations with answers scripted and not yet asked for.
    pub fn unused(&self) -> Vec<String> {
        let mut unused: Vec<String> = self
            .answers
            .borrow()
            .iter()
            .filter(|(_, left)| !left.is_empty())
            .map(|(op, _)| op.clone())
            .collect();
        unused.sort();
        unused
    }

    /// Answers the request to `op` held longest: whether one was.
    pub fn release(&self, op: &str, answer: Reply) -> bool {
        let mut held = self.held.borrow_mut();
        let Some(at) = held.iter().position(|request| request.op == op) else {
            return false;
        };
        if let Some(request) = held.remove(at) {
            *request.hold.borrow_mut() = Some(answer);
        }
        true
    }

    /// The operations of the held requests `work` waits on, in the order asked.
    pub fn waits(&self, work: usize) -> Vec<String> {
        self.held
            .borrow()
            .iter()
            .filter(|request| request.work == Some(work))
            .map(|request| request.op.clone())
            .collect()
    }
}

/// A held request's answer, once given.
struct Slot(Hold);

impl Future for Slot {
    type Output = Reply;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0.borrow_mut().take() {
            Some(answer) => Poll::Ready(answer),
            None => Poll::Pending,
        }
    }
}

impl PaneHost for ScriptedHost {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        self.asked.borrow_mut().push((op.to_owned(), body));
        let next = self
            .answers
            .borrow_mut()
            .get_mut(op)
            .and_then(VecDeque::pop_front);
        match next {
            Some(Answer::Now(answer)) => Box::pin(async move { answer }),
            Some(Answer::Held) => {
                let slot = Rc::new(RefCell::new(None));
                self.held.borrow_mut().push_back(HeldRequest {
                    op: op.to_owned(),
                    work: polling(),
                    hold: Rc::clone(&slot),
                });
                Box::pin(Slot(slot))
            }
            None => {
                let message = format!("no answer for {op}");
                Box::pin(async move {
                    Err(HostError {
                        error: None,
                        message,
                    })
                })
            }
        }
    }
}

/// A pane host that answers each request as `answer` says, at once, and
/// keeps what it was asked.
pub struct AnsweringHost<F> {
    answer: F,
    pub asked: RefCell<Vec<(String, Value)>>,
}

impl<F: Fn(&str) -> Result<Value, HostError>> AnsweringHost<F> {
    pub fn new(answer: F) -> Self {
        Self {
            answer,
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl<F: Fn(&str) -> Result<Value, HostError>> PaneHost for AnsweringHost<F> {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        self.asked.borrow_mut().push((op.to_owned(), body));
        let answer = (self.answer)(op);
        Box::pin(async move { answer })
    }
}

/// `work`, done: it must not wait on anything, and a test that gave it
/// something to wait on uses a [`Driver`].
pub fn finished<T>(mut work: Work<'_, T>) -> T {
    match work.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("work that waited on nothing it was given"),
    }
}

/// Work begun, as the driver holds it.
type Begun<T> = Pin<Box<dyn Future<Output = T>>>;

/// Work begun and not yet settled, run by hand: each is polled until it
/// waits on something a test releases, or settles.
pub struct Driver<T> {
    begun: Vec<(usize, Begun<T>)>,
}

impl<T> Default for Driver<T> {
    fn default() -> Self {
        Self { begun: Vec::new() }
    }
}

impl<T> Driver<T> {
    /// Begins `work`, named `id`.
    pub fn begin(&mut self, id: usize, work: impl Future<Output = T> + 'static) {
        self.begun.push((id, Box::pin(work)));
    }

    /// Runs every begun work until none moves: what settled, in the order
    /// it was begun.
    pub fn run(&mut self) -> Vec<(usize, T)> {
        let mut context = Context::from_waker(Waker::noop());
        let mut settled = Vec::new();
        loop {
            let mut moved = false;
            let mut index = 0;
            while index < self.begun.len() {
                POLLING.with(|polling| polling.set(Some(self.begun[index].0)));
                let polled = self.begun[index].1.as_mut().poll(&mut context);
                POLLING.with(|polling| polling.set(None));
                if let Poll::Ready(value) = polled {
                    let (id, _) = self.begun.remove(index);
                    settled.push((id, value));
                    moved = true;
                } else {
                    index += 1;
                }
            }
            if !moved {
                return settled;
            }
        }
    }

    /// The work begun and still waiting, by name.
    pub fn pending(&self) -> Vec<usize> {
        self.begun.iter().map(|(id, _)| *id).collect()
    }
}

/// When a scenario's clock starts: 2026-09-19T12:00:00Z, as the Node
/// recorder's does.
pub const EPOCH_MS: i64 = 1_789_819_200_000;

/// The fakes a test drives, and the services an adapter is built with of
/// them.
pub struct Fakes {
    pub time: Rc<ManualTime>,
    pub entropy: Rc<ScriptedEntropy>,
    pub records: Rc<LocalRecords>,
    pub ports: Rc<FixedPorts>,
}

impl Fakes {
    /// Fakes for windows that run with `env`, the clock at [`EPOCH_MS`],
    /// the ports from 41_000 up.
    pub fn new(env: &Env) -> Self {
        let time = Rc::new(ManualTime::new(EPOCH_MS));
        let records = Rc::new(LocalRecords::new(
            env.clone(),
            Rc::clone(&time) as Rc<dyn Time>,
        ));
        Self {
            time,
            entropy: Rc::new(ScriptedEntropy::default()),
            records,
            ports: Rc::new(FixedPorts(RefCell::new((41_000..41_100).collect()))),
        }
    }

    /// The services of these fakes, for windows that run with `env`, the
    /// bundle under `root`.
    pub fn services(&self, env: &Env, root: &Path) -> Services {
        Services {
            env: env.clone(),
            records: Rc::clone(&self.records) as Rc<dyn Records>,
            time: Rc::clone(&self.time) as Rc<dyn Time>,
            entropy: Rc::clone(&self.entropy) as Rc<dyn Entropy>,
            ports: Rc::clone(&self.ports) as Rc<dyn Ports>,
            bundle: bundle(root),
        }
    }
}

/// A bundle under `root` (`$ROOT/bundle`), as the Node recorder names its own.
pub fn bundle(root: &Path) -> Bundle {
    let bin = root.join("bundle").join("bin");
    let cf = bin.join(if cfg!(windows) { "cf.exe" } else { "cf" });
    let pane_cf = cf.to_string_lossy().replace('\\', "/");
    Bundle { bin, cf, pane_cf }
}

/// A stand-in CLI at `file` (`fakeExecutable`, tests/helpers.mjs): a shell
/// script on POSIX, a `.cmd` on Windows. The path it is found at.
pub fn fake_executable(file: &Path) -> PathBuf {
    if cfg!(windows) {
        let mut shim = file.as_os_str().to_owned();
        shim.push(".cmd");
        fs::write(&shim, "@echo off\r\nexit /b 0\r\n").expect("a stand-in written");
        return PathBuf::from(shim);
    }
    fs::write(file, "#!/bin/sh\nexit 0\n").expect("a stand-in written");
    #[cfg(unix)]
    fs::set_permissions(file, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("a stand-in made executable");
    file.to_path_buf()
}

/// A live process of the test's own besides the test, ended with it.
pub struct OtherProcess(std::process::Child);

impl OtherProcess {
    #[allow(clippy::disallowed_methods)] // A test starts what it ends.
    pub fn start() -> Self {
        let child = if cfg!(windows) {
            std::process::Command::new("ping")
                .args(["-n", "600", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .spawn()
        } else {
            std::process::Command::new("sleep").arg("600").spawn()
        };
        Self(child.expect("a second process started"))
    }

    pub fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for OtherProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sleep_ends_when_the_clock_reaches_it_and_one_given_up_is_forgotten() {
        let time = Rc::new(ManualTime::new(1_000));
        let mut driver = Driver::default();
        let clock = Rc::clone(&time);
        driver.begin(0, async move {
            clock.sleep(Duration::from_millis(20)).await;
            clock.wall_ms()
        });
        let given_up = time.sleep(Duration::from_millis(10));
        drop(given_up);
        assert!(driver.run().is_empty());
        assert_eq!(time.waits(0), [20], "the 10 ms sleep given up is no wait");
        assert!(
            time.fire_next(1_050),
            "the 20 ms sleep, the 10 ms one forgotten"
        );
        assert_eq!(driver.run(), [(0, 1_020)]);
        assert!(!time.fire_next(1_050));
    }

    #[test]
    fn sleeps_due_together_end_one_at_a_time_in_the_order_armed() {
        let time = Rc::new(ManualTime::new(0));
        let order = Rc::new(RefCell::new(Vec::new()));
        let mut driver = Driver::default();
        for (id, name) in [(0, "first"), (1, "second")] {
            let (clock, order) = (Rc::clone(&time), Rc::clone(&order));
            driver.begin(id, async move {
                clock.sleep(Duration::from_millis(10)).await;
                order.borrow_mut().push(name);
            });
        }
        assert!(driver.run().is_empty());
        assert!(time.fire_next(10));
        assert_eq!(driver.run(), [(0, ())]);
        assert_eq!(*order.borrow(), ["first"], "the second still armed");
        assert!(time.fire_next(10));
        assert_eq!(driver.run(), [(1, ())]);
    }

    #[test]
    fn a_scenario_s_clock_starts_at_the_instant_node_s_does() {
        assert_eq!(cf_base::time::parse("2026-09-19T12:00:00Z"), Some(EPOCH_MS));
    }

    #[test]
    fn the_stream_is_known_and_every_draw_counted() {
        let entropy = ScriptedEntropy::default();
        let mut bytes = [0; 3];
        entropy.fill(&mut bytes).unwrap();
        assert_eq!(bytes, [3, 10, 17]);
        entropy.fill(&mut [0; 16]).unwrap();
        assert_eq!(entropy.take_draws(), [3, 16]);
        assert!(entropy.take_draws().is_empty());
    }

    #[test]
    fn a_held_request_waits_for_its_answer() {
        let host = Rc::new(ScriptedHost::default());
        host.answer("pane.claim", [Answer::Held]);
        let mut driver = Driver::default();
        let asking = Rc::clone(&host);
        driver.begin(4, async move {
            asking
                .request("pane.claim", serde_json::json!({}))
                .await
                .unwrap()
        });
        assert!(driver.run().is_empty());
        assert_eq!(driver.pending(), [4]);
        assert!(host.release("pane.claim", Ok(serde_json::json!({ "ok": true }))));
        assert_eq!(driver.run(), [(4, serde_json::json!({ "ok": true }))]);
    }
}
