//! A window's output: what its program prints, read and sent on within what the page has
//! acknowledged.

use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Instant;

use super::{output_end, Pane, PaneError, PaneKey, PaneOutput, PaneTable};

pub(super) struct OutputFlow {
    backlog_bytes: usize,
    state: Mutex<OutputFlowState>,
    ready: Condvar,
}

struct OutputFlowState {
    next_seq: u64,
    unacked: VecDeque<(u64, usize)>,
    unacked_bytes: usize,
    closed: bool,
}

impl OutputFlow {
    pub(super) fn new(backlog_bytes: usize) -> Self {
        Self {
            backlog_bytes,
            state: Mutex::new(OutputFlowState {
                next_seq: 1,
                unacked: VecDeque::new(),
                unacked_bytes: 0,
                closed: false,
            }),
            ready: Condvar::new(),
        }
    }

    fn read_allowance(&self) -> Option<usize> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        while !state.closed && state.unacked_bytes >= self.backlog_bytes {
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
        (!state.closed).then(|| self.backlog_bytes - state.unacked_bytes)
    }

    fn record(&self, byte_count: usize) -> Option<u64> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return None;
        }
        let seq = state.next_seq;
        state.next_seq += 1;
        state.unacked.push_back((seq, byte_count));
        state.unacked_bytes += byte_count;
        Some(seq)
    }

    fn acknowledge(&self, seq: u64) -> Result<(), u64> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let highest_issued = state.next_seq.saturating_sub(1);
        if seq > highest_issued {
            return Err(highest_issued);
        }
        while let Some(&(pending_seq, byte_count)) = state.unacked.front() {
            if pending_seq > seq {
                break;
            }
            state.unacked.pop_front();
            state.unacked_bytes -= byte_count;
        }
        self.ready.notify_all();
        Ok(())
    }

    /// Everything issued so far counts as read.
    fn acknowledge_all(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.unacked.clear();
        state.unacked_bytes = 0;
        self.ready.notify_all();
    }

    pub(super) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        self.ready.notify_all();
    }
}
impl PaneTable {
    pub fn ack(&self, key: &PaneKey, seq: u64) -> Result<(), PaneError> {
        let flow = self
            .lock_panes()?
            .get(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?
            .output_flow
            .clone()
            .ok_or_else(|| PaneError::NoOutputStream(key.clone()))?;
        flow.acknowledge(seq)
            .map_err(|highest_issued| PaneError::FutureAck {
                key: key.clone(),
                seq,
                highest_issued,
            })
    }

    /// Every pane's output so far, acknowledged: a page that has gone never
    /// acknowledges what it was sent, and only acknowledgements free a pane's
    /// window of unread output, so a pane whose window it filled printed
    /// nothing more.
    pub fn ack_all(&self) {
        let flows: Vec<_> = self
            .panes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .filter_map(|pane| pane.output_flow.clone())
            .collect();
        for flow in flows {
            flow.acknowledge_all();
        }
    }
}

pub(super) struct ActivityReader {
    pub(super) inner: Box<dyn Read + Send>,
    pub(super) last_activity: Arc<Mutex<Instant>>,
}

impl Read for ActivityReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let byte_count = self.inner.read(buffer)?;
        if byte_count > 0 {
            *self
                .last_activity
                .lock()
                .map_err(|_| io::Error::other("pane activity lock is poisoned"))? = Instant::now();
        }
        Ok(byte_count)
    }
}

/// A pane's output, read until it has ended for good ([`output_end::Output`]: on Unix,
/// not while the program runs, for a terminal it let go of may be opened again), and sent
/// on as it comes, within what the flow allows.
pub(super) fn stream_output(
    key: PaneKey,
    mut output: output_end::Output,
    flow: Arc<OutputFlow>,
    sender: mpsc::Sender<PaneOutput>,
) {
    const MAX_CHUNK_BYTES: usize = 8 * 1024;

    while let Some(allowance) = flow.read_allowance() {
        let mut bytes = vec![0; allowance.min(MAX_CHUNK_BYTES)];
        let output_end::Got::Bytes(byte_count) = output.read(&mut bytes) else {
            break;
        };
        bytes.truncate(byte_count);
        let Some(seq) = flow.record(byte_count) else {
            break;
        };
        if sender
            .send(PaneOutput {
                key: key.clone(),
                seq,
                bytes,
            })
            .is_err()
        {
            break;
        }
    }
    let mut discarded = [0; MAX_CHUNK_BYTES];
    while let output_end::Got::Bytes(_) = output.read(&mut discarded) {}
    flow.close();
}

/// A descriptor of the pane's terminal of the output's own, which it asks whether there
/// is anything to read (Unix); none is needed elsewhere.
#[cfg(unix)]
pub(super) fn terminal_of(pane: &Pane) -> Result<output_end::Terminal, PaneError> {
    use std::os::fd::BorrowedFd;

    let fd = pane
        .master
        .as_raw_fd()
        .ok_or_else(|| PaneError::Pty("the terminal has no descriptor".to_owned()))?;
    // SAFETY: the master's descriptor, open for as long as the pane holds it, which it
    // does while the table's lock is held; a copy of it is taken, which the output owns.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    Ok(borrowed.try_clone_to_owned()?)
}

#[cfg(not(unix))]
pub(super) fn terminal_of(_pane: &Pane) -> Result<output_end::Terminal, PaneError> {
    Ok(output_end::Terminal)
}

pub(super) fn close_output(pane: &Pane) {
    if let Some(flow) = &pane.output_flow {
        flow.close();
    }
}
