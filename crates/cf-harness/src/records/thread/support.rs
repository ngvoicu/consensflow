//! What the thread's tests share: readers that do what a test says (count
//! their looks, tell what they were given, wait for the test to let them read,
//! or panic), and the ways a test drives the thread, on one thread that other
//! work shares.

use std::future::Future;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_proto::agents::Harness;
use tokio::task::LocalSet;

use super::{Thread, QUEUE};
use crate::contract::Work;
use crate::records::{Look, Open, Options, Reading};
use crate::seams::Time;
use crate::testing::ManualTime;

/// How long a test waits for the worker before it says the worker is stuck.
const PATIENCE: Duration = Duration::from_secs(10);

/// Runs `test` as the engine runs, on one thread that other work shares.
pub(super) fn on_engine<T>(test: impl Future<Output = T>) -> T {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    LocalSet::new().block_on(&runtime, test)
}

/// `work` polled once.
pub(super) fn polled<T>(work: &mut Work<'_, T>) -> Poll<T> {
    work.as_mut().poll(&mut Context::from_waker(Waker::noop()))
}

/// `work` asked: polled once in place, as the engine begins work, so a look
/// is on its way to the worker, and what is left is waited for later.
pub(super) fn asked<'a, T: 'a>(mut work: Work<'a, T>) -> Work<'a, T> {
    match polled(&mut work) {
        Poll::Ready(answer) => Box::pin(std::future::ready(answer)),
        Poll::Pending => work,
    }
}

/// What a scripted reading says; the scripted readers say nothing else.
pub(super) fn said(reading: &Reading) -> &str {
    match reading {
        Reading::Unknown(reason) => reason,
        Reading::Known(_) => panic!("a known reading, where a scripted one was read"),
    }
}

/// Waits until `condition` holds, or says the worker is stuck.
pub(super) fn eventually(what: &str, condition: impl Fn() -> bool) {
    let until = Instant::now() + PATIENCE;
    while !condition() {
        assert!(Instant::now() < until, "never came: {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// What the scripted readers share: how many were opened, what they did in
/// order, and the permits a look needs before it reads, where the test holds
/// looks back.
pub(super) struct Script {
    opened: AtomicUsize,
    log: Mutex<Vec<String>>,
    permits: Option<Mutex<std_mpsc::Receiver<()>>>,
}

/// Lets looks read, a permit for each.
pub(super) struct Gate(std_mpsc::Sender<()>);

impl Gate {
    pub(super) fn release(&self, looks: usize) {
        for _ in 0..looks {
            self.0.send(()).unwrap();
        }
    }
}

impl Script {
    /// Readers that read when they are asked to.
    pub(super) fn ungated() -> Arc<Self> {
        Arc::new(Self {
            opened: AtomicUsize::new(0),
            log: Mutex::new(Vec::new()),
            permits: None,
        })
    }

    /// Readers that wait for the gate before each look reads.
    pub(super) fn gated() -> (Arc<Self>, Gate) {
        let (gate, permits) = std_mpsc::channel();
        let script = Arc::new(Self {
            opened: AtomicUsize::new(0),
            log: Mutex::new(Vec::new()),
            permits: Some(Mutex::new(permits)),
        });
        (script, Gate(gate))
    }

    fn note(&self, line: String) {
        self.log.lock().unwrap().push(line);
    }

    /// What the readers did, in order.
    pub(super) fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    /// Waits until a reader has said a line that begins `start`.
    pub(super) fn wait_for(&self, start: &str) {
        eventually(start, || {
            self.log().iter().any(|line| line.starts_with(start))
        });
    }

    /// A look's wait for its permit: whether it got one.
    fn permit(&self) -> bool {
        self.permits
            .as_ref()
            .is_none_or(|permits| permits.lock().unwrap().recv_timeout(PATIENCE).is_ok())
    }
}

/// A reader that says which reader it is, which of its looks this is, and
/// what the look was given. A conversation named `boom` goes bad after one
/// good look: its reader panics in the second.
struct Scripted {
    script: Arc<Script>,
    number: usize,
    looks: usize,
    harness: Harness,
    session: String,
}

impl Look for Scripted {
    fn look(&mut self, options: &Options, now_ms: i64) -> Arc<Reading> {
        self.looks += 1;
        let who = format!(
            "{}.{} {} {}",
            self.number,
            self.looks,
            self.harness.as_str(),
            self.session
        );
        self.script.note(format!("look {who} at {now_ms}"));
        if self.session == "boom" && self.looks == 2 {
            panic!("a bug in reader {}", self.number);
        }
        if !self.script.permit() {
            return Arc::new(Reading::Unknown(format!("{who}: no permit came")));
        }
        let launch = options
            .pi_settlement
            .as_ref()
            .and_then(|settlement| settlement.launch_id.as_deref())
            .map(|id| format!(" for {id}"))
            .unwrap_or_default();
        Arc::new(Reading::Unknown(format!("{who} at {now_ms}{launch}")))
    }
}

impl Drop for Scripted {
    fn drop(&mut self) {
        self.script.note(format!("drop {}", self.number));
    }
}

/// Opens that open scripted readers, numbered in the order they are opened.
fn opens(script: &Arc<Script>) -> impl Fn() -> Open + Send + Sync + 'static {
    let script = Arc::clone(script);
    move || {
        let script = Arc::clone(&script);
        let open: Open = Box::new(move |harness, session, _env| {
            let number = script.opened.fetch_add(1, Ordering::Relaxed) + 1;
            script.note(format!("open {number} {} {session}", harness.as_str()));
            Ok(Box::new(Scripted {
                script: Arc::clone(&script),
                number,
                looks: 0,
                harness,
                session: session.to_owned(),
            }))
        });
        open
    }
}

/// A thread of scripted readers with a queue of `queue` asks, on a clock the
/// test moves.
pub(super) fn scripted_with(script: &Arc<Script>, time: &Rc<ManualTime>, queue: usize) -> Thread {
    let time: Rc<dyn Time> = Rc::clone(time) as Rc<dyn Time>;
    Thread::start(Env::default(), time, queue, opens(script)).unwrap()
}

pub(super) fn scripted(script: &Arc<Script>, time: &Rc<ManualTime>) -> Thread {
    scripted_with(script, time, QUEUE)
}
