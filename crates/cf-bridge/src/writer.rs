//! The writer: the bounded queue and the thread that writes it.

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use crate::shutdown::TransportShutdown;
use crate::transport::TransportWorker;
use crate::{Bridge, BridgeBuilder, BridgeError, BridgeInner, Lifecycle};
use crate::{FIRST_LOOKS, WRITER_QUEUE_CAPACITY};

pub(crate) struct WriteJob {
    bytes: Vec<u8>,
    canceled: Arc<AtomicBool>,
}

impl Bridge {
    /// How many responses wait for room in the writer queue now.
    #[cfg(test)]
    pub(super) fn waiting_responses(&self) -> usize {
        self.inner.responses_waiting.load(Ordering::Acquire)
    }

    /// Queues a response. A full writer queue never ends the transport, and a response
    /// is never dropped for waiting: it waits for room for as long as the transport is open,
    /// and the transport's end releases it. No bound of time or of number is right: only a
    /// request's own deadline says when its caller has stopped waiting, and that is the
    /// daemon's to know (a paste into a Windows window of a hundred thousand characters has
    /// 206 s).
    ///
    /// What the daemon does with an answer that comes late is settle nothing: a request
    /// settles once, by its id, and its caller was told `deadline` already. That undoes
    /// nothing the host did. A request that timed out has an unknown outcome (the operation
    /// may have happened, or may be running still), and an explicit retry is a new request,
    /// which can repeat the work.
    ///
    /// What a long wait costs is the handler's thread, which it had already, and the reply
    /// it holds. Nothing bounds either while a peer keeps asking and does not read (a probe
    /// of 256 requests with 96 KiB replies held 228 threads and 63 MiB until the peer's end);
    /// bounded admission and request-aware cancellation belong with the wedge watchdog (the
    /// issue "a daemon that is alive and reads nothing is not noticed"). The log says a peer
    /// is slow.
    pub(crate) fn write_response(&self, encoded: Vec<u8>) -> Result<(), BridgeError> {
        let inner = &self.inner;
        inner.responses_waiting.fetch_add(1, Ordering::AcqRel);
        let _counted = WaitingResponse(&inner.responses_waiting);
        self.enqueue_encoded(
            encoded,
            Arc::new(AtomicBool::new(false)),
            None,
            WRITER_QUEUE_CAPACITY,
        )?;
        Ok(())
    }

    /// Tells `on_error` that the peer is not taking frames, at most once in
    /// [`STALL_REPORT_MS`]: the log says a peer is slow, the transport does not end.
    fn tell_stall(&self, words: &str) {
        let inner = &self.inner;
        let now = u64::try_from(inner.built.elapsed().as_millis())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let told = inner.stalled_told.load(Ordering::Acquire);
        let every = u64::try_from(inner.stall_report.as_millis()).unwrap_or(u64::MAX);
        if told != 0 && now.saturating_sub(told) < every {
            return;
        }
        if inner
            .stalled_told
            .compare_exchange(told, now, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.report(BridgeError::Io(words.to_string()));
        }
    }

    /// Queues a frame once fewer than `depth` frames are waiting, retrying
    /// until `deadline`; with no deadline it waits for as long as the
    /// transport is open.
    pub(crate) fn enqueue_encoded(
        &self,
        mut encoded: Vec<u8>,
        canceled: Arc<AtomicBool>,
        deadline: Option<Instant>,
        depth: usize,
    ) -> Result<bool, BridgeError> {
        encoded.push(b'\n');
        let mut job = WriteJob {
            bytes: encoded,
            canceled,
        };
        let waiting_since = Instant::now();
        let mut looks = 0_u32;
        loop {
            if self.inner.output_closed() {
                return Err(BridgeError::Eof);
            }
            let send_result = {
                let writer = self
                    .inner
                    .writer
                    .lock()
                    .map_err(|_| BridgeError::LockPoisoned)?;
                let Some(writer) = writer.as_ref() else {
                    return Err(BridgeError::Eof);
                };
                // Counted under the writer lock, so no other sender slips in
                // between the look and the send. The count runs one ahead of
                // the channel while the writer holds a job it has taken but not
                // yet counted, so only a share stops at it: a frame allowed the
                // whole queue asks the channel itself.
                if depth < WRITER_QUEUE_CAPACITY
                    && self.inner.queued.load(Ordering::Acquire) >= depth
                {
                    Err(mpsc::TrySendError::Full(job))
                } else {
                    self.inner.queued.fetch_add(1, Ordering::AcqRel);
                    writer.try_send(job).inspect_err(|_| {
                        self.inner.queued.fetch_sub(1, Ordering::AcqRel);
                    })
                }
            };
            match send_result {
                Ok(()) => return Ok(true),
                Err(mpsc::TrySendError::Full(returned)) => {
                    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        return Ok(false);
                    }
                    if waiting_since.elapsed() >= self.inner.stall_report {
                        self.tell_stall(&format!(
                            "the peer has taken no frame for {} s",
                            waiting_since.elapsed().as_secs()
                        ));
                    }
                    job = returned;
                    looks = looks.saturating_add(1);
                    thread::sleep(Duration::from_millis(if looks <= FIRST_LOOKS {
                        1
                    } else {
                        10
                    }));
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    let error = BridgeError::Io("bridge writer queue is closed".to_string());
                    self.fail_transport(error.clone());
                    return Err(error);
                }
            }
        }
    }
}

impl BridgeBuilder {
    pub(crate) fn build<W>(self, output: W, shutdown: Arc<TransportShutdown>) -> Bridge
    where
        W: Write + Send + 'static,
    {
        let (writer, jobs) = mpsc::sync_channel(WRITER_QUEUE_CAPACITY);
        let bridge = Bridge {
            inner: Arc::new(BridgeInner {
                role: self.role,
                writer: Mutex::new(Some(writer)),
                queued: AtomicUsize::new(0),
                pending: Mutex::new(HashMap::new()),
                handlers: self.handlers,
                event_handlers: self.event_handlers,
                on_error: self.on_error,
                on_close: self.on_close,
                next_id: AtomicU64::new(0),
                closed: AtomicBool::new(false),
                keeps_output: self.keeps_output,
                output_ended: AtomicBool::new(false),
                lifecycle: Mutex::new(Lifecycle {
                    closed: false,
                    in_flight_handlers: 0,
                    in_flight_launches: 0,
                    transport_workers: 1,
                }),
                closed_ready: Condvar::new(),
                transport_shutdown: shutdown,
                max_frame_bytes: self.max_frame_bytes,
                default_request_deadline: self.default_request_deadline,
                responses_waiting: AtomicUsize::new(0),
                stall_report: self.stall_report,
                stalled_told: AtomicU64::new(0),
                built: Instant::now(),
            }),
        };
        let inner = Arc::downgrade(&bridge.inner);
        let worker = TransportWorker::new(Weak::clone(&inner));
        thread::spawn(move || {
            let _worker = worker;
            writer_loop(Box::new(output), jobs, inner);
        });
        bridge
    }
}

/// A response waiting for room, counted for as long as it waits.
struct WaitingResponse<'a>(&'a AtomicUsize);

impl Drop for WaitingResponse<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn writer_loop(
    mut writer: Box<dyn Write + Send>,
    jobs: mpsc::Receiver<WriteJob>,
    weak_inner: Weak<BridgeInner>,
) {
    while let Ok(job) = jobs.recv() {
        let Some(inner) = weak_inner.upgrade() else {
            return;
        };
        inner.queued.fetch_sub(1, Ordering::AcqRel);
        if job.canceled.load(Ordering::Acquire) {
            continue;
        }
        if inner.output_closed() {
            return;
        }
        drop(inner);
        if let Err(error) = writer.write_all(&job.bytes).and_then(|()| writer.flush()) {
            if let Some(inner) = weak_inner.upgrade() {
                Bridge { inner }.fail_transport(error.into());
            }
            return;
        }
    }
}
