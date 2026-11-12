//! Shutdown: closing the transport, waiting for it and telling the close.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::{Bridge, BridgeError, BridgeInner};

pub(crate) struct TransportShutdown {
    requested: AtomicBool,
    #[cfg_attr(not(unix), allow(dead_code))]
    wait_lock: Mutex<()>,
    ready: Condvar,
}

impl Bridge {
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// Ends the peer's input, the way a parent tells a child it is done:
    /// the writer goes, and with it the pipe the peer reads. What the peer
    /// still writes is read to its own EOF, which alone closes the
    /// transport, so nothing it says while stopping is lost. A frame queued
    /// after this is refused as EOF.
    pub fn close_input(&self) {
        match self.inner.writer.lock() {
            Ok(mut writer) => drop(writer.take()),
            Err(poisoned) => drop(poisoned.into_inner().take()),
        }
    }

    /// Ends the output of a bridge that keeps it past the end of its input
    /// ([`BridgeBuilder::keep_output_after_eof`]): what was said is written, and the
    /// peer's input ends after it. The writer is given `within` to write what is queued;
    /// a peer that reads nothing has it cut off then, and the answer is `false`. Of no
    /// use before the input has ended, which the writer's end is waited for until.
    pub fn end_output(&self, within: Duration) -> bool {
        self.close_input();
        let deadline = Instant::now() + within;
        let flushed = self.wait_for_the_writer(deadline);
        self.cut_output();
        flushed
    }

    /// Whether the transport's workers have ended by `deadline`.
    fn wait_for_the_writer(&self, deadline: Instant) -> bool {
        let mut guard = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while guard.transport_workers != 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            guard = self
                .inner
                .closed_ready
                .wait_timeout(guard, left)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        true
    }

    pub fn wait_closed(&self) -> Result<(), BridgeError> {
        let mut guard = self
            .inner
            .lifecycle
            .lock()
            .map_err(|_| BridgeError::LockPoisoned)?;
        while !guard.closed || guard.in_flight_handlers != 0 || guard.transport_workers != 0 {
            guard = self
                .inner
                .closed_ready
                .wait(guard)
                .map_err(|_| BridgeError::LockPoisoned)?;
        }
        Ok(())
    }

    pub fn wait_launches_closed(&self) -> Result<(), BridgeError> {
        let mut guard = self
            .inner
            .lifecycle
            .lock()
            .map_err(|_| BridgeError::LockPoisoned)?;
        while !guard.closed || guard.in_flight_launches != 0 {
            guard = self
                .inner
                .closed_ready
                .wait(guard)
                .map_err(|_| BridgeError::LockPoisoned)?;
        }
        Ok(())
    }

    pub(crate) fn close_with_eof(&self) {
        self.close_transport(BridgeError::Eof, false, self.inner.keeps_output);
    }

    pub(crate) fn fail_transport(&self, error: BridgeError) {
        self.close_transport(error, true, false);
    }

    /// Closes the transport: nothing more is admitted from the peer, and what waits
    /// on it is refused with `error`. The output ends with it, unless it is `keeps_output`
    /// (the end of an input that the bridge keeps its output past): a failure of the
    /// transport after that ends the output all the same.
    fn close_transport(&self, error: BridgeError, report: bool, keeps_output: bool) {
        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if lifecycle.closed {
            drop(lifecycle);
            if !keeps_output {
                self.cut_output();
            }
            return;
        }
        lifecycle.closed = true;
        self.inner.closed.store(true, Ordering::Release);
        if !keeps_output {
            self.cut_output();
        }
        let pending = match self.inner.pending.lock() {
            Ok(mut pending) => pending
                .drain()
                .map(|(_, request)| request)
                .collect::<Vec<_>>(),
            Err(poisoned) => poisoned
                .into_inner()
                .drain()
                .map(|(_, request)| request)
                .collect(),
        };
        for request in pending {
            let _ = request.sender.send(Err(error.clone()));
        }
        self.inner.closed_ready.notify_all();
        drop(lifecycle);
        if report {
            self.report(error);
        }
        if let Some(handler) = &self.inner.on_close {
            handler();
        }
    }

    /// Ends the output at once: what is queued and not written is dropped, and a write
    /// that waits for a peer that reads nothing is interrupted (on Unix).
    fn cut_output(&self) {
        self.inner.output_ended.store(true, Ordering::Release);
        self.inner.transport_shutdown.request();
        match self.inner.writer.lock() {
            Ok(mut writer) => drop(writer.take()),
            Err(poisoned) => drop(poisoned.into_inner().take()),
        }
    }

    pub(crate) fn report(&self, error: BridgeError) {
        if let Some(handler) = &self.inner.on_error {
            handler(error);
        }
    }
}

impl Drop for BridgeInner {
    fn drop(&mut self) {
        self.transport_shutdown.request();
    }
}

impl TransportShutdown {
    pub(crate) fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            wait_lock: Mutex::new(()),
            ready: Condvar::new(),
        }
    }

    fn request(&self) {
        self.requested.store(true, Ordering::Release);
        self.ready.notify_all();
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn wait_for_retry(&self) {
        let guard = self
            .wait_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !self.is_requested() {
            let _ = self.ready.wait_timeout(guard, Duration::from_millis(10));
        }
    }
}
