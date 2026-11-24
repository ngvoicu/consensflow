//! A window's output: what its program prints, read and sent on within what the page has
//! acknowledged.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Instant;

use super::{output_end, Pane, PaneError, PaneKey, PaneOutput, PaneTable};
use crate::journal::{Journal, Piece};
use crate::output_hub::{OutputHub, OutputSink};

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

    /// Windows opened from now on keep a journal of their output and resizes
    /// ([`crate::journal`]). A table that does not call this (the headless helper's) keeps
    /// main's flow, and its messages carry no offset.
    pub fn keep_journals(&self) {
        self.journals
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// A page subscribes. Its sink becomes the hub's destination, and, in the one step that
    /// swaps it, the boundary of every window that keeps a journal is fixed: what had been
    /// handed to a page before it. The new page's history is the journal before the
    /// boundary, which it replays muted; every byte from it on, including what was parked for
    /// this page, comes on the sink. The room the old page left unacknowledged is settled, as
    /// on main. Call this under [`crate::input_queue::InputQueue::with_current_page`].
    pub fn attach_page(&self, hub: &OutputHub, sink: OutputSink) -> HashMap<PaneKey, u64> {
        let boundaries = hub.attach_page(sink);
        let journals = self.journals_held();
        for (key, journal) in &journals {
            journal.begin_page(boundaries.get(key).copied().unwrap_or(0));
        }
        hub.retain_handed(|key| journals.iter().any(|(held, _)| held == key));
        self.ack_all();
        boundaries
    }

    /// One piece of a window's history for the page now subscribed, from `from` (`None`: the
    /// page has nothing), of a window that runs or one that has ended and is not forgotten.
    /// Call this under [`crate::input_queue::InputQueue::with_current_page`].
    pub fn history(&self, key: &PaneKey, from: Option<u64>) -> Result<Piece, PaneError> {
        let journal = self
            .journal_of(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?;
        Ok(journal.piece(from))
    }

    fn journals_held(&self) -> Vec<(PaneKey, Arc<Journal>)> {
        let mut held: Vec<_> = self
            .panes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .filter_map(|(key, pane)| Some((key.clone(), pane.journal.clone()?)))
            .collect();
        held.extend(
            self.ended_journals
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .iter()
                .cloned(),
        );
        held
    }

    fn journal_of(&self, key: &PaneKey) -> Option<Arc<Journal>> {
        if let Some(journal) = self
            .panes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(key)
            .and_then(|pane| pane.journal.clone())
        {
            return Some(journal);
        }
        self.ended_journals
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .find(|(kept, _)| kept == key)
            .map(|(_, journal)| Arc::clone(journal))
    }

    /// A window that has ended keeps its journal for its page to pull, the latest sixteen,
    /// until [`PaneTable::forget_ended_journal`].
    pub(crate) fn keep_ended_journal(&self, key: PaneKey, journal: Arc<Journal>) {
        let mut kept = self
            .ended_journals
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        kept.push_back((key, journal));
        while kept.len() > super::ENDED_JOURNALS_KEPT {
            kept.pop_front();
        }
    }

    /// The page has what it needs of an ended window, or has retired it: its journal is
    /// forgotten. Until then, whichever page asks gets its pieces again (a page that reloaded
    /// before it took the last one), and the sixteenth after it makes room.
    pub fn forget_ended_journal(&self, key: &PaneKey) {
        self.ended_journals
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|(held, _)| held != key);
    }

    /// How many journals of ended windows the table holds.
    #[cfg(test)]
    pub(crate) fn ended_journals_held(&self) -> usize {
        self.ended_journals
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
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
    let mut buffer = [0; MAX_CHUNK_BYTES];

    while let Some(allowance) = flow.read_allowance() {
        let output_end::Got::Bytes(byte_count) =
            output.read(&mut buffer[..allowance.min(MAX_CHUNK_BYTES)])
        else {
            break;
        };
        let Some(seq) = flow.record(byte_count) else {
            break;
        };
        if sender
            .send(PaneOutput {
                key: key.clone(),
                seq,
                bytes: buffer[..byte_count].to_vec(),
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

#[cfg(test)]
mod tiny_read_tests {
    use super::*;

    #[test]
    fn a_one_byte_read_does_not_keep_an_eight_kibibyte_allocation() {
        #[cfg(unix)]
        let (reader, terminal) = {
            use std::io::Write;
            use std::os::unix::net::UnixStream;

            let (reader, mut writer) = UnixStream::pair().expect("socket pair");
            writer.write_all(b"x").expect("write one byte");
            drop(writer);
            let terminal = reader.try_clone().expect("duplicate descriptor").into();
            (Box::new(reader) as Box<dyn Read + Send>, terminal)
        };
        #[cfg(not(unix))]
        let (reader, terminal) = (
            Box::new(std::io::Cursor::new(vec![b'x'])) as Box<dyn Read + Send>,
            output_end::Terminal,
        );

        let output = output_end::Output::new(reader, terminal, Box::new(|_| true));
        let (sender, received) = mpsc::channel();
        stream_output(
            PaneKey::new("one-byte", 1),
            output,
            Arc::new(OutputFlow::new(1024 * 1024)),
            sender,
        );
        let message = received.recv().expect("one byte was read");
        assert_eq!(message.bytes, b"x");
        assert_eq!(
            message.bytes.capacity(),
            1,
            "a queued byte holds only itself"
        );
        assert!(received.try_recv().is_err());
    }
}
