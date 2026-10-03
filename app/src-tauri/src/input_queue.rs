//! Every open pane's input, in order: one worker and one bounded queue per
//! pane, the page's numbered input admitted before it is answered, and why an
//! input did not go in.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

use futures_channel::oneshot;
use serde_json::{json, Value};

use crate::arbiter::{ArbiterError, InputArbiter};
use crate::pty::{PaneKey, PaneTable};
use crate::validation::{validate_input, INVALID_BODY};

const INPUT_QUEUE_CAPACITY: usize = 1024;
const MAX_PENDING_INPUT_BYTES_PER_PANE: usize = 4 * 1024 * 1024;
const MAX_PENDING_INPUT_TICKETS: usize = 4096;
const INPUT_QUEUE_FULL: &str = "pane-input-queue-full";
/// The host is stopping, and takes no more input.
const INPUT_CLOSED: &str = "input-closed";
const LOCK_POISONED: &str = "lock-poisoned";
const INPUT_SEQUENCE_GAP: &str = "pane-input-sequence-gap";
const INPUT_SEQUENCE_REGRESSION: &str = "pane-input-sequence-regression";

/// What a pane's worker does, in order: the human's keys and the emulator's
/// replies, a delivery's paste, a native send's claim.
pub(crate) enum InputWork {
    Write(Vec<u8>),
    Paste(Vec<u8>),
    Claim,
}

impl InputWork {
    fn byte_count(&self) -> usize {
        match self {
            Self::Write(bytes) => bytes.len(),
            Self::Paste(body) => body.len().saturating_add(13),
            Self::Claim => 0,
        }
    }
}

/// Why a pane's input did not go in. Refused: nothing of it was written, so
/// it may be sent again. Uncertain: the write itself failed partway, and some
/// of it may have reached the window.
#[derive(Debug)]
pub(crate) enum InputError {
    Refused { code: &'static str, cause: String },
    Uncertain(String),
}

impl InputError {
    pub(crate) fn refused(code: &'static str, cause: impl Into<String>) -> Self {
        Self::Refused {
            code,
            cause: cause.into(),
        }
    }

    fn queue_full() -> Self {
        Self::refused(INPUT_QUEUE_FULL, "the pane's input queue is full")
    }

    /// A paste's answer: whether anything reached the window decides whether
    /// the daemon may paste again or must ask the harness what it got.
    pub(crate) fn paste_answer(&self) -> Value {
        match self {
            Self::Refused { code, cause } => json!({
                "ok":false,
                "admitted":false,
                "bytesWritten":0,
                "error":code,
                "cause":cause,
            }),
            Self::Uncertain(cause) => json!({
                "ok":false,
                "admitted":null,
                "error":"uncertain",
                "cause":cause,
            }),
        }
    }
}

/// The cause, which is all the page and the other operations answer.
impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { cause, .. } | Self::Uncertain(cause) => formatter.write_str(cause),
        }
    }
}

impl From<ArbiterError> for InputError {
    fn from(error: ArbiterError) -> Self {
        let code = match &error {
            ArbiterError::Pane(_) => return Self::Uncertain(error.to_string()),
            ArbiterError::Stale => "stale-pane",
            ArbiterError::Busy => "paste-in-flight",
            ArbiterError::InputFailed => "input-failed",
            ArbiterError::InvalidBody(_) => INVALID_BODY,
            ArbiterError::LockPoisoned => LOCK_POISONED,
        };
        Self::refused(code, error.to_string())
    }
}

pub(crate) type InputResponse = Result<(), InputError>;

struct InputJob {
    work: InputWork,
    response: oneshot::Sender<InputResponse>,
    reserved_bytes: usize,
    pending_bytes: Arc<AtomicUsize>,
}

pub(crate) struct PageInputCompletion {
    pub(crate) receiver: oneshot::Receiver<InputResponse>,
    pub(crate) human: bool,
}

struct PageInputState {
    last_sequences: HashMap<PaneKey, u64>,
    completions: HashMap<String, PageInputCompletion>,
    next_ticket: u64,
}

struct InputRoute {
    sender: mpsc::SyncSender<InputJob>,
    pending_bytes: Arc<AtomicUsize>,
}

/// Every open pane's input, in order: one worker and one bounded queue per
/// pane, from its `pane.open` until it leaves the table.
pub(crate) struct InputQueue {
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    senders: Mutex<HashMap<PaneKey, InputRoute>>,
    workers: Mutex<HashMap<PaneKey, JoinHandle<()>>>,
    page: Mutex<PageInputState>,
    accepting: AtomicBool,
}

impl InputQueue {
    pub(crate) fn new(panes: Arc<PaneTable>, arbiter: Arc<InputArbiter>) -> Self {
        Self {
            panes,
            arbiter,
            senders: Mutex::new(HashMap::new()),
            workers: Mutex::new(HashMap::new()),
            page: Mutex::new(PageInputState {
                last_sequences: HashMap::new(),
                completions: HashMap::new(),
                next_ticket: 1,
            }),
            accepting: AtomicBool::new(true),
        }
    }

    pub(crate) fn enqueue_page(
        &self,
        key: PaneKey,
        sequence: u64,
        work: InputWork,
        human: bool,
    ) -> Result<String, String> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        let expected = page
            .last_sequences
            .get(&key)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| "pane-input-sequence-overflow".to_string())?;
        if sequence < expected {
            return Err(INPUT_SEQUENCE_REGRESSION.to_string());
        }
        if sequence > expected {
            return Err(INPUT_SEQUENCE_GAP.to_string());
        }

        page.last_sequences.insert(key.clone(), sequence);
        match &work {
            InputWork::Write(bytes) | InputWork::Paste(bytes) => validate_input(bytes)?,
            InputWork::Claim => {}
        }
        if page.completions.len() >= MAX_PENDING_INPUT_TICKETS {
            return Err("pane-input-ticket-capacity".to_string());
        }
        let next_ticket = page
            .next_ticket
            .checked_add(1)
            .ok_or_else(|| "pane-input-ticket-overflow".to_string())?;
        let receiver = self.submit(key, work).map_err(|error| error.to_string())?;
        let ticket = format!("pane-input-{}", page.next_ticket);
        page.next_ticket = next_ticket;
        page.completions
            .insert(ticket.clone(), PageInputCompletion { receiver, human });
        Ok(ticket)
    }

    /// A new page counts every pane's input from 1: the page that sent the
    /// old numbers is gone (a reload replaced it), and so are the tickets it
    /// never came back for. What it had admitted still goes in, in order.
    pub(crate) fn begin_page(&self) {
        let mut page = self.page.lock().unwrap_or_else(|error| error.into_inner());
        page.last_sequences.clear();
        page.completions.clear();
    }

    pub(crate) fn take_page_completion(&self, ticket: &str) -> Result<PageInputCompletion, String> {
        self.page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?
            .completions
            .remove(ticket)
            .ok_or_else(|| "pane-input-ticket-not-found".to_string())
    }

    pub(crate) fn write(
        &self,
        key: PaneKey,
        bytes: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit(key, InputWork::Write(bytes))
    }

    pub(crate) fn paste(
        &self,
        key: PaneKey,
        body: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit(key, InputWork::Paste(body))
    }

    /// Admit a native-channel send: the pane is current and its input works,
    /// and no paste is going in, since the pane's worker runs one job at a
    /// time.
    pub(crate) fn claim(
        &self,
        key: PaneKey,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit(key, InputWork::Claim)
    }

    /// A pane just opened gets its worker and queue.
    pub(crate) fn open(&self, key: &PaneKey) -> Result<(), String> {
        let mut senders = self
            .senders
            .lock()
            .map_err(|_| "pane input queue lock is poisoned".to_string())?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err("pane input admission is closed".to_string());
        }
        let (sender, jobs) = mpsc::sync_channel(INPUT_QUEUE_CAPACITY);
        let panes = Arc::clone(&self.panes);
        let arbiter = Arc::clone(&self.arbiter);
        let worker_key = key.clone();
        let worker = thread::Builder::new()
            .name(format!("consensflow-input-{}", worker_key.id))
            .spawn(move || input_worker(jobs, panes, arbiter, worker_key))
            .map_err(|error| format!("could not start pane input queue: {error}"))?;
        self.workers
            .lock()
            .map_err(|_| "pane input worker lock is poisoned".to_string())?
            .insert(key.clone(), worker);
        senders.insert(
            key.clone(),
            InputRoute {
                sender,
                pending_bytes: Arc::new(AtomicUsize::new(0)),
            },
        );
        Ok(())
    }

    /// A pane gone from the table, killed or retired when its program ended,
    /// takes its input with it: its queue closes (the worker ends once what
    /// was queued has gone in or failed), and its page count and arbiter
    /// state go. They used to stay until the app quit, a parked thread per
    /// window ever opened.
    pub(crate) fn retire(&self, key: &PaneKey) {
        self.senders
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key);
        self.workers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key);
        self.page
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .last_sequences
            .remove(key);
        self.arbiter.retire(key);
    }

    fn submit(
        &self,
        key: PaneKey,
        work: InputWork,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        let closed = || InputError::refused(INPUT_CLOSED, "pane input admission is closed");
        if !self.accepting.load(Ordering::Acquire) {
            return Err(closed());
        }
        let senders = self
            .senders
            .lock()
            .map_err(|_| InputError::refused(LOCK_POISONED, "pane input queue lock is poisoned"))?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err(closed());
        }
        // A pane the table does not hold has no queue: it was never opened,
        // or it is gone.
        let route = senders.get(&key).ok_or(ArbiterError::Stale)?;
        let reserved_bytes = work.byte_count();
        route
            .pending_bytes
            .try_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                pending
                    .checked_add(reserved_bytes)
                    .filter(|next| *next <= MAX_PENDING_INPUT_BYTES_PER_PANE)
            })
            .map_err(|_| InputError::queue_full())?;
        let (response, receiver) = oneshot::channel();
        let job = InputJob {
            work,
            response,
            reserved_bytes,
            pending_bytes: Arc::clone(&route.pending_bytes),
        };
        match route.sender.try_send(job) {
            Ok(()) => Ok(receiver),
            Err(mpsc::TrySendError::Full(job)) => {
                job.pending_bytes
                    .fetch_sub(job.reserved_bytes, Ordering::AcqRel);
                Err(InputError::queue_full())
            }
            Err(mpsc::TrySendError::Disconnected(job)) => {
                job.pending_bytes
                    .fetch_sub(job.reserved_bytes, Ordering::AcqRel);
                Err(InputError::refused(
                    INPUT_CLOSED,
                    format!("pane input queue for {} is closed", key.id),
                ))
            }
        }
    }

    pub(crate) fn close_and_drain(&self) {
        if !self.accepting.swap(false, Ordering::AcqRel) {
            return;
        }
        self.senders
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        let workers = std::mem::take(
            &mut *self
                .workers
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        for worker in workers.into_values() {
            let _ = worker.join();
        }
    }
}

fn input_worker(
    jobs: mpsc::Receiver<InputJob>,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    key: PaneKey,
) {
    for job in jobs {
        let result = match job.work {
            InputWork::Write(bytes) => arbiter.write(&panes, &key, &bytes),
            InputWork::Paste(body) => arbiter.write_paste(&panes, &key, &body),
            InputWork::Claim => arbiter.claim(&key),
        }
        .map_err(InputError::from);
        job.pending_bytes
            .fetch_sub(job.reserved_bytes, Ordering::AcqRel);
        let _ = job.response.send(result);
    }
}

pub(crate) async fn wait_for_input(receiver: oneshot::Receiver<InputResponse>) -> InputResponse {
    // The worker answers every job it takes: one left unanswered went down
    // with the worker, perhaps in the middle of its write.
    receiver
        .await
        .map_err(|_| InputError::Uncertain("pane input queue ended before answering".to_string()))?
}

pub(crate) fn wait_for_input_blocking(receiver: oneshot::Receiver<InputResponse>) -> InputResponse {
    tauri::async_runtime::block_on(wait_for_input(receiver))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbiter::EnterTiming;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    use crate::arbiter::SanitizeError;
    #[cfg(unix)]
    use crate::bridge::BridgeBuilder;
    #[cfg(unix)]
    use crate::output_hub::OutputHub;
    #[cfg(unix)]
    use crate::pane_handlers::register_pane_handlers;
    use crate::pty::PaneError;

    /// A pane's input lives as long as the pane: killed, or ended on its own,
    /// it takes its input worker and queue, its page sequence and its arbiter
    /// state with it. Each used to stay until the app quit, a parked thread
    /// per window.
    #[cfg(unix)]
    #[test]
    fn a_pane_gone_from_the_table_takes_its_input_with_it() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let mut builder = BridgeBuilder::new(1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            Arc::clone(&arbiter),
            Arc::new(OutputHub::new()),
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut number = 0;
        let mut ask = |op: &str, body: Value| -> Value {
            number += 1;
            let id = format!("n-retired-{number}");
            let mut frame =
                serde_json::to_vec(&json!({"v":1,"id":id,"kind":"req","op":op,"body":body}))
                    .expect("serialize request");
            frame.push(b'\n');
            node_stream.write_all(&frame).expect("write request");
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read response");
                let frame: Value = serde_json::from_str(line.trim()).expect("response JSON");
                if frame["kind"] == "res" && frame["id"] == id.as_str() {
                    return frame["body"].clone();
                }
            }
        };
        let gone = |key: &PaneKey| {
            !inputs.senders.lock().unwrap().contains_key(key)
                && !inputs.workers.lock().unwrap().contains_key(key)
                && !inputs.page.lock().unwrap().last_sequences.contains_key(key)
                && arbiter.snapshot(key).is_err()
        };

        let killed = PaneKey::new("killed-pane", 1);
        let ended = PaneKey::new("ended-pane", 1);
        for (key, script) in [(&killed, "sleep 30"), (&ended, "read line")] {
            let opened = ask(
                "pane.open",
                json!({"id":key.id,"generation":1,"cwd":"/tmp","argv":["/bin/sh","-c",script],
                       "env":{},"size":{"rows":24,"cols":80},"backlogBytes":1024}),
            );
            assert_eq!(opened["ok"], true, "{opened}");
            inputs
                .enqueue_page(key.clone(), 1, InputWork::Write(b"x".to_vec()), true)
                .expect("the page types into the pane");
            assert!(!gone(key), "the pane has its input");
        }

        assert_eq!(
            ask("pane.kill", json!({"id":killed.id,"generation":1})),
            json!({"ok":true})
        );
        assert!(gone(&killed), "a killed pane kept its input");

        assert_eq!(
            ask(
                "pane.input",
                json!({"id":ended.id,"generation":1,"bytes":[13]})
            ),
            json!({"ok":true})
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gone(&ended) {
            assert!(
                Instant::now() < deadline,
                "a pane that ended kept its input"
            );
            thread::sleep(Duration::from_millis(20));
        }

        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    /// Input for a pane the table never held is refused, and starts no
    /// worker: every key used to get a thread and a queue of its own.
    #[test]
    fn input_for_a_pane_never_opened_is_refused() {
        let panes = Arc::new(PaneTable::new());
        let inputs = InputQueue::new(
            Arc::clone(&panes),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        let refused = inputs.write(PaneKey::new("never-opened", 1), b"x".to_vec());
        assert!(matches!(
            refused,
            Err(InputError::Refused { code: "stale-pane", ref cause }) if cause == "stale pane generation"
        ));
        assert!(inputs.senders.lock().unwrap().is_empty());
        assert!(inputs.workers.lock().unwrap().is_empty());
    }

    /// A host that is stopping takes no more input: what is sent is refused
    /// as closed, nothing of it written, and no pane gets a queue.
    #[test]
    fn a_stopping_host_refuses_input_as_closed() {
        let panes = Arc::new(PaneTable::new());
        let inputs = InputQueue::new(
            Arc::clone(&panes),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        let key = PaneKey::new("open-pane", 1);
        inputs.open(&key).expect("open the pane's input");
        inputs.close_and_drain();

        let refused = inputs.write(key, b"x".to_vec());
        assert!(matches!(
            refused,
            Err(InputError::Refused { code: "input-closed", ref cause }) if cause == "pane input admission is closed"
        ));
        assert_eq!(
            inputs.open(&PaneKey::new("late-pane", 1)),
            Err("pane input admission is closed".to_string())
        );
        assert!(inputs.workers.lock().unwrap().is_empty());
    }

    /// A paste refused by the arbiter answers that nothing of it went in,
    /// with the code and the cause, so the daemon may send it again; a write
    /// that failed once begun answers that it may have.
    #[test]
    fn a_refused_paste_says_nothing_went_in_and_a_failed_one_is_uncertain() {
        for (error, code) in [
            (ArbiterError::Stale, "stale-pane"),
            (ArbiterError::Busy, "paste-in-flight"),
            (ArbiterError::InputFailed, "input-failed"),
            (
                ArbiterError::InvalidBody(SanitizeError::ControlByte(7)),
                "invalid-body",
            ),
            (ArbiterError::LockPoisoned, "lock-poisoned"),
        ] {
            let cause = error.to_string();
            assert_eq!(
                InputError::from(error).paste_answer(),
                json!({"ok":false,"admitted":false,"bytesWritten":0,"error":code,"cause":cause})
            );
        }
        assert_eq!(
            InputError::from(ArbiterError::Pane(PaneError::LockPoisoned)).paste_answer(),
            json!({"ok":false,"admitted":null,"error":"uncertain","cause":"pane table lock is poisoned"})
        );
    }

    /// The page's input whose answers it never collects is held to a bound,
    /// and a new page, which owes none of them, starts with none.
    #[test]
    fn unanswered_page_input_is_bounded_until_a_new_page() {
        let panes = Arc::new(PaneTable::new());
        let inputs = InputQueue::new(
            Arc::clone(&panes),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        let key = PaneKey::new("unanswered-pane", 1);
        inputs.open(&key).expect("open the pane's input");
        let typed = || InputWork::Write(b"x".to_vec());
        // The pane's queue holds fewer jobs than the bound holds tickets, and
        // its worker may lag on a busy machine: input the full queue refuses
        // is sent again under the next number (a refused number is spent).
        let mut sequence = 0;
        let mut admitted = 0;
        while admitted < MAX_PENDING_INPUT_TICKETS {
            sequence += 1;
            match inputs.enqueue_page(key.clone(), sequence, typed(), true) {
                Ok(_) => admitted += 1,
                Err(error) if error == "the pane's input queue is full" => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(error) => panic!("refused: {error}"),
            }
        }
        // The bound is checked before the queue: refused whatever the queue holds.
        assert_eq!(
            inputs.enqueue_page(key.clone(), sequence + 1, typed(), true),
            Err("pane-input-ticket-capacity".to_string())
        );

        inputs.begin_page();
        assert!(inputs.enqueue_page(key, 1, typed(), true).is_ok());
        inputs.close_and_drain();
    }
}
