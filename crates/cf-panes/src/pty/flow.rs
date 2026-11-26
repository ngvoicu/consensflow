//! A window's output: what its program prints, read and sent on within what the page has
//! acknowledged.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Instant;

use crate::arbiter::OutputCount;

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
    /// The bytes of the reads acknowledged since the window opened: read for read, offsets of
    /// the window's journal (each read is recorded, then appended).
    acked_bytes: u64,
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
                acked_bytes: 0,
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

    /// Settle the reads up to `seq`: the bytes acknowledged since the window opened.
    fn acknowledge(&self, seq: u64) -> Result<u64, u64> {
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
            state.acked_bytes += byte_count as u64;
        }
        self.ready.notify_all();
        Ok(state.acked_bytes)
    }

    pub(super) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        self.ready.notify_all();
    }
}
impl PaneTable {
    pub fn ack(&self, key: &PaneKey, seq: u64) -> Result<(), PaneError> {
        let (flow, journal) = {
            let panes = self.lock_panes()?;
            let pane = panes
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            let flow = pane
                .output_flow
                .clone()
                .ok_or_else(|| PaneError::NoOutputStream(key.clone()))?;
            (flow, pane.journal.clone())
        };
        let acked = flow
            .acknowledge(seq)
            .map_err(|highest_issued| PaneError::FutureAck {
                key: key.clone(),
                seq,
                highest_issued,
            })?;
        // With no lock held but the journal's own. An acknowledgement that races a cut leaves it
        // lower for a moment, which only holds the cut back.
        if let Some(journal) = journal {
            journal.acknowledged(acked);
        }
        Ok(())
    }

    /// Windows opened from now on keep a journal of their output and resizes
    /// ([`crate::journal`]). A table that does not call this (the headless helper's) keeps
    /// main's flow, and its messages carry no offset.
    pub fn keep_journals(&self) {
        self.journals
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// The page `page_id` subscribes. Its sink becomes the hub's destination, and, in the one
    /// step that swaps it, the boundary of every window that keeps a journal is fixed: what had
    /// been handed to a page before it. The new page's history is the journal before the
    /// boundary, which it replays muted; every byte from it on, including what was parked for
    /// this page, comes on the sink.
    ///
    /// The room is settled only up to the last message a page before was handed, read in the
    /// same step: what was parked stays charged until this page acknowledges it, and a
    /// subscription whose sink refuses settles nothing more (its parked messages are parked
    /// again, still charged). So the room is the bound whatever the number of subscriptions.
    ///
    /// Call this under [`crate::input_queue::InputQueue::with_current_page`].
    pub fn attach_page(
        &self,
        page_id: &str,
        hub: &OutputHub,
        sink: OutputSink,
    ) -> HashMap<PaneKey, u64> {
        *self.lock_subscribed() = Some(page_id.to_owned());
        let handed = hub.attach_page(sink);
        let journals = self.journals_held();
        let boundaries: HashMap<PaneKey, u64> = handed
            .iter()
            .filter_map(|(key, taken)| Some((key.clone(), taken.end?)))
            .collect();
        for (key, journal) in &journals {
            journal.begin_page(boundaries.get(key).copied().unwrap_or(0));
        }
        // Looked up as each key is judged, under the hub's lock: a window that opened since the
        // snapshot above has its entry kept.
        hub.retain_handed(|key| self.holds(key));
        for (key, taken) in &handed {
            // A window that has gone takes no acknowledgement, and none is owed it.
            let _ = self.ack(key, taken.seq);
        }
        boundaries
    }

    /// One piece of a window's history for the page that subscribed last, from `from` (`None`:
    /// the page has nothing), of a window that runs or one that has ended and is not forgotten.
    /// A page that has not subscribed is refused ([`PaneError::NotSubscribed`]): its history
    /// would be answered against the boundary of the page before. Call this under
    /// [`crate::input_queue::InputQueue::with_current_page`].
    pub fn history(
        &self,
        page_id: &str,
        key: &PaneKey,
        from: Option<u64>,
    ) -> Result<Piece, PaneError> {
        if self.lock_subscribed().as_deref() != Some(page_id) {
            return Err(PaneError::NotSubscribed);
        }
        let journal = self
            .journal_of(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?;
        Ok(journal.piece(from))
    }

    /// The journal of an ended window, forgotten where `consumed` is the journal's end: the
    /// page's emulator has taken all of it. Whether it was forgotten; one asked short of its end
    /// keeps its journal, and so does a window that still runs.
    pub fn forget_consumed_journal(&self, key: &PaneKey, consumed: u64) -> bool {
        let mut kept = self
            .ended_journals
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let before = kept.len();
        kept.retain(|(held, journal)| held != key || journal.end() != consumed);
        kept.len() != before
    }

    fn lock_subscribed(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.subscribed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Whether the table holds the window: open, or ended with its journal kept.
    fn holds(&self, key: &PaneKey) -> bool {
        self.panes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(key)
            || self.journal_of(key).is_some()
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
    /// until [`PaneTable::forget_consumed_journal`].
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

    /// How many journals of ended windows the table holds.
    #[cfg(test)]
    pub(crate) fn ended_journals_held(&self) -> usize {
        self.ended_journals
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }
}

pub(crate) struct ActivityReader {
    pub(crate) inner: Box<dyn Read + Send>,
    pub(crate) last_activity: Arc<Mutex<Instant>>,
    /// The window's byte counts: a read is counted here, the moment it returns and before the
    /// lock of the activity time, so that the two move together and a thread that waits for the
    /// lock has already told the paste's Enter and the input box's readers of its bytes.
    pub(crate) counts: Arc<OutputCount>,
}

impl Read for ActivityReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        // Inside the gate, which is held across a read that cannot wait (the thread waited for
        // the terminal before, outside it): while the gate is free, no byte is in hand and not
        // counted.
        let byte_count = {
            let _intake = self.counts.intake();
            let byte_count = self.inner.read(buffer)?;
            if byte_count > 0 {
                self.counts.took(byte_count);
            }
            byte_count
        };
        if byte_count > 0 {
            *crate::sync::lock(&self.last_activity)
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
    counts: Arc<OutputCount>,
) {
    const MAX_CHUNK_BYTES: usize = 8 * 1024;
    let mut buffer = [0; MAX_CHUNK_BYTES];

    // Held at the page's credit the thread reads nothing: what the window prints stays at its
    // terminal, where the question finds it ([`OutputCount::sample`]).
    while let Some(allowance) = flow.read_allowance() {
        let output_end::Got::Bytes(byte_count) =
            output.read(&mut buffer[..allowance.min(MAX_CHUNK_BYTES)])
        else {
            break;
        };
        // The bytes were counted as read as the pty returned them ([`ActivityReader`]): before
        // the activity lock, the flow, the queue and the lock of the screen.
        let Some(seq) = flow.record(byte_count) else {
            counts.give_up();
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
            // Nobody takes the output in: what was read will not be drawn.
            counts.give_up();
            break;
        }
    }
    let mut discarded = [0; MAX_CHUNK_BYTES];
    while let output_end::Got::Bytes(_) = output.read(&mut discarded) {}
    flow.close();
    // The question holds the terminal open, so it is let go with the output.
    counts.unwatch();
}

/// A descriptor of the pane's terminal of the output's own, which it waits on and asks
/// whether there is anything to read; a second one is the pane's question
/// ([`question_of`]).
#[cfg(unix)]
pub(super) fn terminal_of(pane: &Pane) -> Result<output_end::Terminal, PaneError> {
    use std::os::fd::BorrowedFd;

    let fd = pane
        .master
        .fd
        .ok_or_else(|| PaneError::Pty("the terminal has no descriptor".to_owned()))?;
    // SAFETY: the master's descriptor, open for as long as the pane holds it, which it
    // does while the table's lock is held; a copy of it is taken, which the output owns.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    Ok(borrowed.try_clone_to_owned()?)
}

#[cfg(windows)]
pub(super) fn terminal_of(pane: &Pane) -> Result<output_end::Terminal, PaneError> {
    let watch = pane
        .master
        .watch
        .as_ref()
        .ok_or_else(|| PaneError::Pty("the terminal has no output to ask".to_owned()))?;
    Ok(Some(watch.try_clone()?))
}

/// The question to the pane's terminal, whether it holds bytes not yet read
/// (`OutputCount::sample`), with a handle of its own, which it holds while the output is read.
/// A terminal that cannot be asked answers that it holds some: such a window is behind.
#[cfg(unix)]
pub(super) fn question_of(
    terminal: &output_end::Terminal,
) -> Result<crate::arbiter::Waiting, PaneError> {
    let terminal = terminal.try_clone()?;
    Ok(Box::new(move || output_end::holds_bytes(&terminal)))
}

#[cfg(windows)]
pub(super) fn question_of(
    terminal: &output_end::Terminal,
) -> Result<crate::arbiter::Waiting, PaneError> {
    let Some(terminal) = terminal else {
        return Ok(Box::new(|| false));
    };
    let terminal = terminal.try_clone()?;
    Ok(Box::new(move || terminal.holds_bytes()))
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
        #[cfg(windows)]
        let (reader, terminal) = (
            Box::new(std::io::Cursor::new(vec![b'x'])) as Box<dyn Read + Send>,
            None,
        );

        let output = output_end::Output::new(reader, terminal, Box::new(|_| true));
        let (sender, received) = mpsc::channel();
        stream_output(
            PaneKey::new("one-byte", 1),
            output,
            Arc::new(OutputFlow::new(1024 * 1024)),
            sender,
            Arc::new(OutputCount::default()),
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
