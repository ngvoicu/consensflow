use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::pty::{write_paste_via, PaneError, PaneInputWriter, PaneKey, PaneTable};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaneEvent {
    Enter { pane: PaneKey, epoch: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClearOutcome {
    Cleared,
    PreservedNewerInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SanitizeError {
    ControlByte(u8),
    ControlCharacter(char),
    InvalidUtf8,
}

impl fmt::Display for SanitizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ControlByte(byte) => {
                write!(formatter, "control byte 0x{byte:02x} is not allowed")
            }
            Self::ControlCharacter(character) => write!(
                formatter,
                "control character U+{:04X} is not allowed",
                u32::from(*character)
            ),
            Self::InvalidUtf8 => write!(formatter, "paste body is not valid UTF-8"),
        }
    }
}

impl std::error::Error for SanitizeError {}

#[derive(Debug)]
pub enum ArbiterError {
    Stale,
    Draft,
    Busy,
    InputFailed,
    EpochOverflow,
    InvalidBody(SanitizeError),
    Pane(PaneError),
    EventChannelClosed,
    LockPoisoned,
}

impl fmt::Display for ArbiterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stale => write!(formatter, "stale pane generation or input epoch"),
            Self::Draft => write!(formatter, "human draft is latched"),
            Self::Busy => write!(formatter, "a paste is already in flight"),
            Self::InputFailed => write!(formatter, "pane input is failed until restart"),
            Self::EpochOverflow => write!(formatter, "input epoch overflow"),
            Self::InvalidBody(error) => write!(formatter, "invalid paste body: {error}"),
            Self::Pane(error) => error.fmt(formatter),
            Self::EventChannelClosed => write!(formatter, "pane event channel is closed"),
            Self::LockPoisoned => write!(formatter, "input arbiter lock is poisoned"),
        }
    }
}

impl std::error::Error for ArbiterError {}

impl From<PaneError> for ArbiterError {
    fn from(error: PaneError) -> Self {
        Self::Pane(error)
    }
}

impl From<SanitizeError> for ArbiterError {
    fn from(error: SanitizeError) -> Self {
        Self::InvalidBody(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArbiterSnapshot {
    pub generation: u64,
    pub input_epoch: u64,
    pub draft_latched: bool,
    pub paste_in_flight: bool,
    pub input_failed: bool,
    pub queued_human_bytes: usize,
    pub last_submission_id: Option<String>,
}

struct QueuedHumanInput {
    bytes: Vec<u8>,
    enter_epochs: Vec<u64>,
}

struct PaneInputState {
    generation: u64,
    input_epoch: u64,
    draft_epoch: Option<u64>,
    emitted_enter_epochs: BTreeSet<u64>,
    paste_in_flight: bool,
    input_failed: bool,
    queued_human: VecDeque<QueuedHumanInput>,
    last_submission_id: Option<String>,
}

type PaneState = Arc<Mutex<PaneInputState>>;
type PaneStateMap = HashMap<String, PaneState>;

impl PaneInputState {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            input_epoch: 0,
            draft_epoch: None,
            emitted_enter_epochs: BTreeSet::new(),
            paste_in_flight: false,
            input_failed: false,
            queued_human: VecDeque::new(),
            last_submission_id: None,
        }
    }

    fn snapshot(&self) -> ArbiterSnapshot {
        ArbiterSnapshot {
            generation: self.generation,
            input_epoch: self.input_epoch,
            draft_latched: self.draft_epoch.is_some(),
            paste_in_flight: self.paste_in_flight,
            input_failed: self.input_failed,
            queued_human_bytes: self
                .queued_human
                .iter()
                .map(|queued| queued.bytes.len())
                .sum(),
            last_submission_id: self.last_submission_id.clone(),
        }
    }
}

pub struct InputArbiter {
    panes: Mutex<PaneStateMap>,
    enter_delay_ms: u64,
    events: Sender<PaneEvent>,
}

impl InputArbiter {
    pub fn new(enter_delay_ms: u64, events: Sender<PaneEvent>) -> Self {
        Self {
            panes: Mutex::new(HashMap::new()),
            enter_delay_ms,
            events,
        }
    }

    pub fn register(&self, pane: &PaneKey) -> Result<(), ArbiterError> {
        self.register_inner(pane, || {}, || {})
    }

    #[cfg(test)]
    fn register_with_hook<F>(&self, pane: &PaneKey, before_replace: F) -> Result<(), ArbiterError>
    where
        F: FnMut(),
    {
        self.register_inner(pane, before_replace, || {})
    }

    #[cfg(test)]
    fn register_with_locked_hook<F>(
        &self,
        pane: &PaneKey,
        before_commit: F,
    ) -> Result<(), ArbiterError>
    where
        F: FnMut(),
    {
        self.register_inner(pane, || {}, before_commit)
    }

    fn register_inner<F, G>(
        &self,
        pane: &PaneKey,
        mut before_replace: F,
        mut before_commit: G,
    ) -> Result<(), ArbiterError>
    where
        F: FnMut(),
        G: FnMut(),
    {
        let (state, inserted) = {
            let mut panes = self.lock_panes()?;
            match panes.get(&pane.id) {
                Some(state) => (Arc::clone(state), false),
                None => {
                    let state = Arc::new(Mutex::new(PaneInputState::new(pane.generation)));
                    panes.insert(pane.id.clone(), Arc::clone(&state));
                    (state, true)
                }
            }
        };
        if inserted {
            return Ok(());
        }

        {
            let state = lock_state(&state)?;
            if state.generation > pane.generation {
                return Err(ArbiterError::Stale);
            }
            if state.generation == pane.generation {
                return Ok(());
            }
        }

        before_replace();
        let mut state = lock_state(&state)?;
        if state.generation > pane.generation {
            return Err(ArbiterError::Stale);
        }
        if state.generation == pane.generation {
            return Ok(());
        }
        if state.paste_in_flight {
            return Err(ArbiterError::Busy);
        }
        before_commit();
        *state = PaneInputState::new(pane.generation);
        Ok(())
    }

    pub fn snapshot(&self, pane: &PaneKey) -> Result<ArbiterSnapshot, ArbiterError> {
        let state = self.pane_state(pane)?;
        let state = lock_state(&state)?;
        validate_generation(&state, pane)?;
        let snapshot = state.snapshot();
        Ok(snapshot)
    }

    pub fn write_human(
        &self,
        table: &PaneTable,
        pane: &PaneKey,
        bytes: &[u8],
    ) -> Result<u64, ArbiterError> {
        self.write_human_via(table, pane, bytes)
    }

    fn write_human_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        bytes: &[u8],
    ) -> Result<u64, ArbiterError> {
        let state = self.pane_state(pane)?;
        let mut state = lock_state(&state)?;
        validate_generation(&state, pane)?;
        if state.input_failed {
            return Err(ArbiterError::InputFailed);
        }
        if bytes.is_empty() {
            return Ok(state.input_epoch);
        }

        let byte_count = u64::try_from(bytes.len()).map_err(|_| ArbiterError::EpochOverflow)?;
        let first_epoch = state.input_epoch;
        let next_epoch = first_epoch
            .checked_add(byte_count)
            .ok_or(ArbiterError::EpochOverflow)?;
        let enter_epochs = bytes
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'\r')
            .map(|(index, _)| first_epoch + index as u64 + 1)
            .collect::<Vec<_>>();
        state.input_epoch = next_epoch;
        state.draft_epoch = Some(next_epoch);

        if state.paste_in_flight {
            state.queued_human.push_back(QueuedHumanInput {
                bytes: bytes.to_vec(),
                enter_epochs,
            });
            return Ok(next_epoch);
        }

        if let Err(error) = writer.write(pane, bytes) {
            state.input_failed = true;
            return Err(error.into());
        }
        self.emit_enters(pane, &mut state, enter_epochs)?;
        Ok(next_epoch)
    }

    pub fn write_reply(
        &self,
        table: &PaneTable,
        pane: &PaneKey,
        bytes: &[u8],
    ) -> Result<(), ArbiterError> {
        self.write_reply_via(table, pane, bytes)
    }

    fn write_reply_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        bytes: &[u8],
    ) -> Result<(), ArbiterError> {
        let state = self.pane_state(pane)?;
        let mut state = lock_state(&state)?;
        validate_generation(&state, pane)?;
        if state.input_failed {
            return Err(ArbiterError::InputFailed);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        if let Err(error) = writer.write(pane, bytes) {
            state.input_failed = true;
            return Err(error.into());
        }
        Ok(())
    }

    pub fn write_paste(
        &self,
        table: &PaneTable,
        pane: &PaneKey,
        epoch: u64,
        body: &[u8],
    ) -> Result<(), ArbiterError> {
        self.write_paste_via(table, pane, epoch, body)
    }

    /**
     * Admit a native-channel send under the same epoch and draft guard as a
     * PTY paste, without reserving the writer or sending any bytes.
     */
    pub fn claim_epoch(&self, pane: &PaneKey, epoch: u64) -> Result<(), ArbiterError> {
        let state = self.pane_state(pane)?;
        let state = lock_state(&state)?;
        validate_epoch_claim(&state, pane, epoch)
    }

    fn write_paste_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        epoch: u64,
        body: &[u8],
    ) -> Result<(), ArbiterError> {
        let body = sanitize(body)?;
        let state = self.pane_state(pane)?;
        {
            let mut state = lock_state(&state)?;
            validate_epoch_claim(&state, pane, epoch)?;
            state.paste_in_flight = true;
        }

        if let Err(error) = write_paste_via(writer, pane, &body, self.enter_delay_ms) {
            let mut admitted_state = lock_state(&state)?;
            validate_generation(&admitted_state, pane)?;
            admitted_state.paste_in_flight = false;
            admitted_state.input_failed = true;
            return Err(error.into());
        }
        self.finish_paste(writer, pane, &state)
    }

    /// A human assertion from the app, never inferred from PTY/transcript text.
    pub fn resume_replies(&self, pane: &PaneKey, epoch: u64) -> Result<(), ArbiterError> {
        let state = self.pane_state(pane)?;
        let mut state = lock_state(&state)?;
        validate_generation(&state, pane)?;
        if state.input_epoch != epoch {
            return Err(ArbiterError::Stale);
        }
        if state.input_failed {
            return Err(ArbiterError::InputFailed);
        }
        if state.paste_in_flight || !state.queued_human.is_empty() {
            return Err(ArbiterError::Busy);
        }
        state.draft_epoch = None;
        state.emitted_enter_epochs.clear();
        state.last_submission_id = None;
        Ok(())
    }

    pub fn clear_draft(
        &self,
        pane_id: &str,
        generation: u64,
        submitted_epoch: u64,
        submission_id: &str,
    ) -> Result<ClearOutcome, ArbiterError> {
        let state = self
            .lock_panes()?
            .get(pane_id)
            .cloned()
            .ok_or(ArbiterError::Stale)?;
        let mut state = lock_state(&state)?;
        if state.generation != generation {
            return Err(ArbiterError::Stale);
        }
        if !state.emitted_enter_epochs.contains(&submitted_epoch) {
            return Err(ArbiterError::Stale);
        }
        state
            .emitted_enter_epochs
            .retain(|epoch| *epoch > submitted_epoch);
        state.last_submission_id = Some(submission_id.to_string());
        if state
            .draft_epoch
            .is_some_and(|draft_epoch| draft_epoch > submitted_epoch)
        {
            return Ok(ClearOutcome::PreservedNewerInput);
        }
        state.draft_epoch = None;
        Ok(ClearOutcome::Cleared)
    }

    fn finish_paste<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        state: &PaneState,
    ) -> Result<(), ArbiterError> {
        let mut event_channel_closed = false;
        loop {
            let queued = {
                let mut state = lock_state(state)?;
                validate_generation(&state, pane)?;
                match state.queued_human.pop_front() {
                    Some(queued) => Some(queued),
                    None => {
                        state.paste_in_flight = false;
                        None
                    }
                }
            };
            let Some(queued) = queued else {
                return if event_channel_closed {
                    Err(ArbiterError::EventChannelClosed)
                } else {
                    Ok(())
                };
            };

            if let Err(error) = writer.write(pane, &queued.bytes) {
                if let Ok(mut state) = lock_state(state) {
                    state.queued_human.push_front(queued);
                    state.paste_in_flight = false;
                    state.input_failed = true;
                }
                return Err(error.into());
            }
            let mut state = lock_state(state)?;
            validate_generation(&state, pane)?;
            if self
                .emit_enters(pane, &mut state, queued.enter_epochs)
                .is_err()
            {
                event_channel_closed = true;
            }
        }
    }

    fn emit_enters(
        &self,
        pane: &PaneKey,
        state: &mut PaneInputState,
        enter_epochs: Vec<u64>,
    ) -> Result<(), ArbiterError> {
        for epoch in enter_epochs {
            self.events
                .send(PaneEvent::Enter {
                    pane: pane.clone(),
                    epoch,
                })
                .map_err(|_| ArbiterError::EventChannelClosed)?;
            state.emitted_enter_epochs.insert(epoch);
        }
        Ok(())
    }

    fn pane_state(&self, pane: &PaneKey) -> Result<PaneState, ArbiterError> {
        self.lock_panes()?
            .get(&pane.id)
            .cloned()
            .ok_or(ArbiterError::Stale)
    }

    fn lock_panes(&self) -> Result<MutexGuard<'_, PaneStateMap>, ArbiterError> {
        self.panes.lock().map_err(|_| ArbiterError::LockPoisoned)
    }
}

fn lock_state(
    state: &Mutex<PaneInputState>,
) -> Result<MutexGuard<'_, PaneInputState>, ArbiterError> {
    state.lock().map_err(|_| ArbiterError::LockPoisoned)
}

fn validate_generation(state: &PaneInputState, pane: &PaneKey) -> Result<(), ArbiterError> {
    if state.generation == pane.generation {
        Ok(())
    } else {
        Err(ArbiterError::Stale)
    }
}

fn validate_epoch_claim(
    state: &PaneInputState,
    pane: &PaneKey,
    epoch: u64,
) -> Result<(), ArbiterError> {
    validate_generation(state, pane)?;
    if state.input_failed {
        return Err(ArbiterError::InputFailed);
    }
    // Once human input is latched, Draft is the useful refusal even when
    // those bytes also advanced the epoch after the caller's snapshot.
    if state.draft_epoch.is_some() {
        return Err(ArbiterError::Draft);
    }
    if state.input_epoch != epoch {
        return Err(ArbiterError::Stale);
    }
    if state.paste_in_flight {
        return Err(ArbiterError::Busy);
    }
    Ok(())
}

pub fn sanitize(body: &[u8]) -> Result<Vec<u8>, SanitizeError> {
    let mut sanitized = Vec::with_capacity(body.len());
    let mut index = 0;
    while index < body.len() {
        let byte = body[index];
        if byte == b'\r' && body.get(index + 1) == Some(&b'\n') {
            sanitized.push(b'\n');
            index += 2;
            continue;
        }
        sanitized.push(byte);
        index += 1;
    }

    let text = std::str::from_utf8(&sanitized).map_err(|_| SanitizeError::InvalidUtf8)?;
    for character in text.chars() {
        if character.is_control() && !matches!(character, '\t' | '\n') {
            return if character.is_ascii() {
                Err(SanitizeError::ControlByte(character as u8))
            } else {
                Err(SanitizeError::ControlCharacter(character))
            };
        }
    }
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{Read, Result as IoResult};
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Barrier, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use portable_pty::PtySize;

    use super::{sanitize, ArbiterError, ClearOutcome, InputArbiter, PaneEvent, SanitizeError};
    use crate::pty::{serial_pty_test, OpenedPane, PaneError, PaneInputWriter, PaneKey, PaneTable};

    fn terminal_size() -> PtySize {
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn raw_recorder(table: &PaneTable, byte_count: usize) -> (PaneKey, Box<dyn Read + Send>) {
        let script = format!("stty raw -echo; printf ready; od -An -tx1 -N {byte_count}");
        let OpenedPane { key, mut reader } = table
            .open(
                Path::new("/tmp"),
                &["/bin/sh".to_string(), "-c".to_string(), script],
                &HashMap::new(),
                terminal_size(),
            )
            .expect("open raw recorder");
        await_ready(&mut reader);
        (key, reader)
    }

    fn raw_recorder_at(table: &PaneTable, key: PaneKey, byte_count: usize) -> Box<dyn Read + Send> {
        let script = format!("stty raw -echo; printf ready; od -An -tx1 -N {byte_count}");
        let mut reader = table
            .open_at(
                key,
                Path::new("/tmp"),
                &["/bin/sh".to_string(), "-c".to_string(), script],
                &HashMap::new(),
                terminal_size(),
            )
            .expect("open raw recorder at key");
        await_ready(&mut reader);
        reader
    }

    fn await_ready(reader: &mut Box<dyn Read + Send>) {
        let mut ready = [0_u8; 5];
        reader.read_exact(&mut ready).expect("raw recorder ready");
        assert_eq!(&ready, b"ready");
    }

    fn read_hex(mut reader: Box<dyn Read + Send>) -> String {
        let (sender, receiver) = mpsc::channel::<IoResult<Vec<u8>>>();
        thread::spawn(move || {
            let mut output = Vec::new();
            let result = reader.read_to_end(&mut output).map(|_| output);
            let _ = sender.send(result);
        });
        let output = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("raw recorder did not reach EOF")
            .expect("read raw recorder");
        String::from_utf8(output)
            .expect("UTF-8 od output")
            .split_whitespace()
            .collect()
    }

    #[derive(Clone, Debug)]
    struct RecordedWrite {
        at: Instant,
        bytes: Vec<u8>,
    }

    struct RecordingWriter {
        attempts: AtomicUsize,
        fail_on_attempt: Option<usize>,
        records: Mutex<Vec<RecordedWrite>>,
        observed: mpsc::Sender<RecordedWrite>,
    }

    struct GatedRecordingWriter {
        attempts: AtomicUsize,
        records: Mutex<Vec<Vec<u8>>>,
        first_write: Arc<Barrier>,
        release_first_write: Arc<Barrier>,
    }

    struct PartialRecordingWriter {
        attempts: AtomicUsize,
        records: Mutex<Vec<Vec<u8>>>,
        observed: mpsc::Sender<Vec<u8>>,
    }

    impl PaneInputWriter for GatedRecordingWriter {
        fn write(&self, _pane: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
            self.records
                .lock()
                .expect("gated record lock")
                .push(bytes.to_vec());
            if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                self.first_write.wait();
                self.release_first_write.wait();
            }
            Ok(())
        }
    }

    impl PaneInputWriter for PartialRecordingWriter {
        fn write(&self, _pane: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            let recorded = if attempt == 3 {
                bytes[..bytes.len().min(2)].to_vec()
            } else {
                bytes.to_vec()
            };
            self.records
                .lock()
                .expect("partial record lock")
                .push(recorded.clone());
            self.observed
                .send(recorded)
                .expect("observe partial writer attempt");
            if attempt == 3 {
                Err(PaneError::Io(std::io::Error::other(
                    "injected partial write failure",
                )))
            } else {
                Ok(())
            }
        }
    }

    impl RecordingWriter {
        fn new(fail_on_attempt: Option<usize>) -> (Arc<Self>, mpsc::Receiver<RecordedWrite>) {
            let (observed, receiver) = mpsc::channel();
            (
                Arc::new(Self {
                    attempts: AtomicUsize::new(0),
                    fail_on_attempt,
                    records: Mutex::new(Vec::new()),
                    observed,
                }),
                receiver,
            )
        }

        fn records(&self) -> Vec<RecordedWrite> {
            self.records.lock().expect("record lock").clone()
        }
    }

    impl PaneInputWriter for RecordingWriter {
        fn write(&self, _pane: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            let record = RecordedWrite {
                at: Instant::now(),
                bytes: bytes.to_vec(),
            };
            self.records
                .lock()
                .expect("record lock")
                .push(record.clone());
            self.observed.send(record).expect("observe write");
            if self.fail_on_attempt == Some(attempt) {
                return Err(PaneError::Io(std::io::Error::other(
                    "injected write failure",
                )));
            }
            Ok(())
        }
    }

    #[test]
    fn human_resume_requires_exact_epoch_and_generation_without_guessing_editor_text() {
        let key = PaneKey::new("human-resume", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let (events, _receiver) = mpsc::channel();
        let arbiter = InputArbiter::new(0, events);
        arbiter.register(&key).unwrap();
        let old = arbiter
            .write_human_via(writer.as_ref(), &key, b"\x1b[Aedited\r")
            .unwrap();
        let current = arbiter
            .write_human_via(writer.as_ref(), &key, b"new draft")
            .unwrap();
        assert!(matches!(
            arbiter.resume_replies(&key, old),
            Err(ArbiterError::Stale)
        ));
        assert!(matches!(
            arbiter.resume_replies(&PaneKey::new(&key.id, 2), current),
            Err(ArbiterError::Stale)
        ));
        assert!(arbiter.snapshot(&key).unwrap().draft_latched);
        arbiter.resume_replies(&key, current).unwrap();
        assert!(!arbiter.snapshot(&key).unwrap().draft_latched);
        let next = arbiter
            .write_human_via(writer.as_ref(), &key, b"later")
            .unwrap();
        assert!(next > current);
        assert!(arbiter.snapshot(&key).unwrap().draft_latched);
    }

    #[test]
    fn paste_is_bracketed_then_enter_is_a_delayed_separate_write() {
        let key = PaneKey::new("recorded", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let (events, _receiver) = mpsc::channel();
        let arbiter = InputArbiter::new(25, events);
        arbiter.register(&key).expect("register pane");

        arbiter
            .write_paste_via(writer.as_ref(), &key, 0, b"body")
            .expect("write paste");

        let records = writer.records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].bytes, b"\x1b[200~body\x1b[201~");
        assert_eq!(records[1].bytes, b"\r");
        assert!(records[1].at.duration_since(records[0].at) >= Duration::from_millis(25));
    }

    #[test]
    fn human_bytes_latch_the_draft_bump_epoch_and_emit_enter() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let (key, reader) = raw_recorder(&table, 2);
        let (events, receiver) = mpsc::channel();
        let arbiter = InputArbiter::new(5, events);
        arbiter.register(&key).expect("register pane");

        assert_eq!(
            arbiter
                .write_human(&table, &key, b"x\r")
                .expect("write human bytes"),
            2
        );

        let state = arbiter.snapshot(&key).expect("read arbiter state");
        assert!(state.draft_latched);
        assert_eq!(state.input_epoch, 2);
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("Enter event"),
            PaneEvent::Enter {
                pane: key,
                epoch: 2,
            }
        );
        assert_eq!(read_hex(reader), "780d");
    }

    #[test]
    fn emulator_reply_writes_without_latching_a_draft_or_advancing_the_epoch() {
        let key = PaneKey::new("reply", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let (events, receiver) = mpsc::channel();
        let arbiter = InputArbiter::new(5, events);
        arbiter.register(&key).expect("register pane");

        arbiter
            .write_reply_via(writer.as_ref(), &key, b"\x1b[0n")
            .expect("write terminal reply");

        assert_eq!(
            writer
                .records()
                .iter()
                .map(|record| &record.bytes)
                .collect::<Vec<_>>(),
            vec![b"\x1b[0n"]
        );
        let snapshot = arbiter.snapshot(&key).expect("reply state");
        assert_eq!(snapshot.generation, 1);
        assert_eq!(snapshot.input_epoch, 0);
        assert!(!snapshot.draft_latched);
        assert!(!snapshot.paste_in_flight);
        assert!(!snapshot.input_failed);
        assert_eq!(snapshot.queued_human_bytes, 0);
        assert_eq!(snapshot.last_submission_id, None);
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn blocked_human_write_does_not_stall_an_unrelated_pane() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let OpenedPane {
            key: blocked_key,
            reader: mut blocked_reader,
        } = table
            .open(
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "stty raw -echo; printf ready; sleep 1000".to_string(),
                ],
                &HashMap::new(),
                terminal_size(),
            )
            .expect("open non-consuming pane");
        let mut ready = [0_u8; 5];
        blocked_reader
            .read_exact(&mut ready)
            .expect("blocked pane ready");
        assert_eq!(&ready, b"ready");
        let (responsive_key, reader) = raw_recorder(&table, 1);
        let (events, _receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(5, events));
        arbiter
            .register(&blocked_key)
            .expect("register blocked pane");
        arbiter
            .register(&responsive_key)
            .expect("register responsive pane");

        let (started_sender, started_receiver) = mpsc::channel();
        let (blocked_sender, blocked_receiver) = mpsc::channel();
        let blocked_arbiter = Arc::clone(&arbiter);
        let blocked_table = Arc::clone(&table);
        let writer_key = blocked_key.clone();
        let blocked_writer = thread::spawn(move || {
            started_sender
                .send(())
                .expect("announce blocked human write");
            let result = blocked_arbiter.write_human(
                &blocked_table,
                &writer_key,
                &vec![b'x'; 8 * 1024 * 1024],
            );
            let _ = blocked_sender.send(result);
        });
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("blocked human writer started");
        thread::sleep(Duration::from_millis(50));
        assert!(matches!(
            blocked_receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        let (responsive_sender, responsive_receiver) = mpsc::channel();
        let responsive_arbiter = Arc::clone(&arbiter);
        let responsive_table = Arc::clone(&table);
        let writer_key = responsive_key.clone();
        let responsive_writer = thread::spawn(move || {
            let _ = responsive_sender.send(responsive_arbiter.write_human(
                &responsive_table,
                &writer_key,
                b"R",
            ));
        });
        let responsive_on_time = responsive_receiver.recv_timeout(Duration::from_millis(500));

        if responsive_on_time.is_err() {
            let cleanup_table = Arc::clone(&table);
            let cleanup_key = blocked_key.clone();
            let (cleanup_sender, cleanup_receiver) = mpsc::channel();
            thread::spawn(move || {
                let _ = cleanup_sender.send(cleanup_table.kill(&cleanup_key));
            });
            let _ = cleanup_receiver.recv_timeout(Duration::from_millis(500));
            drop(blocked_reader);
            panic!("an unrelated pane waited on the blocked pane's arbiter lock");
        }

        table.kill(&blocked_key).expect("kill blocked pane");
        let responsive_result = responsive_on_time
            .or_else(|_| responsive_receiver.recv_timeout(Duration::from_secs(2)))
            .expect("responsive human write eventually finishes");
        let _ = blocked_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("blocked human write unblocks after kill");
        blocked_writer.join().expect("blocked human writer thread");
        responsive_writer
            .join()
            .expect("responsive human writer thread");

        assert_eq!(responsive_result.expect("responsive write epoch"), 1);
        assert_eq!(read_hex(reader), "52");
        let _ = table.kill(&responsive_key);
    }

    #[test]
    fn epoch_claims_and_pastes_share_stale_and_draft_guards() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let (key, _reader) = raw_recorder(&table, 1);
        let (events, _receiver) = mpsc::channel();
        let arbiter = InputArbiter::new(5, events);
        arbiter.register(&key).expect("register pane");
        arbiter.claim_epoch(&key, 0).expect("current epoch claims");
        assert!(matches!(
            arbiter.claim_epoch(&key, 1),
            Err(ArbiterError::Stale)
        ));
        arbiter
            .write_human(&table, &key, b"x")
            .expect("write human byte");

        assert!(matches!(
            arbiter.write_paste(&table, &key, 0, b"automated"),
            Err(ArbiterError::Draft)
        ));
        assert!(matches!(
            arbiter.write_paste(&table, &key, 1, b"automated"),
            Err(ArbiterError::Draft)
        ));
        assert!(matches!(
            arbiter.claim_epoch(&key, 0),
            Err(ArbiterError::Draft)
        ));
        let stale_generation = PaneKey::new(&key.id, key.generation + 1);
        assert!(matches!(
            arbiter.write_paste(&table, &stale_generation, 1, b"automated"),
            Err(ArbiterError::Stale)
        ));
        assert!(matches!(
            arbiter.claim_epoch(&stale_generation, 1),
            Err(ArbiterError::Stale)
        ));
    }

    #[test]
    fn delayed_clear_for_submission_a_preserves_newer_draft_b() {
        let _pty_guard = serial_pty_test();
        const ENTER_DELAY_MS: u64 = 2;

        let table = PaneTable::new();
        let key = PaneKey::new("worker", 2);
        let reader = raw_recorder_at(&table, key.clone(), 10);
        let (events, receiver) = mpsc::channel();
        let arbiter = InputArbiter::new(ENTER_DELAY_MS, events);
        arbiter.register(&key).expect("register pane");

        arbiter
            .write_human(&table, &key, b"draftA\r")
            .expect("submit A");
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("A Enter event"),
            PaneEvent::Enter {
                pane: key.clone(),
                epoch: 7,
            }
        );
        arbiter
            .write_human(&table, &key, b"B!")
            .expect("type draft B");
        thread::sleep(Duration::from_millis(ENTER_DELAY_MS * 10));
        assert!(
            arbiter
                .snapshot(&key)
                .expect("state after timer")
                .draft_latched
        );

        assert_eq!(
            arbiter
                .clear_draft(&key.id, key.generation, 7, "submission-A")
                .expect("delayed clear for A"),
            ClearOutcome::PreservedNewerInput
        );
        let after_a = arbiter.snapshot(&key).expect("state after A clear");
        assert!(after_a.draft_latched);
        assert_eq!(after_a.input_epoch, 9);

        arbiter
            .write_human(&table, &key, b"\r")
            .expect("submit draft B");
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("B Enter event"),
            PaneEvent::Enter {
                pane: key.clone(),
                epoch: 10,
            }
        );
        assert!(matches!(
            arbiter.clear_draft(&key.id, key.generation - 1, 10, "stale-generation"),
            Err(ArbiterError::Stale)
        ));

        assert_eq!(
            arbiter
                .clear_draft(&key.id, key.generation, 10, "submission-B")
                .expect("clear B"),
            ClearOutcome::Cleared
        );
        assert!(!arbiter.snapshot(&key).expect("cleared state").draft_latched);
        assert_eq!(read_hex(reader), "6472616674410d42210d");
    }

    #[test]
    fn clear_rejects_unissued_and_future_submission_epochs() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let (key, reader) = raw_recorder(&table, 2);
        let (events, receiver) = mpsc::channel();
        let arbiter = InputArbiter::new(5, events);
        arbiter.register(&key).expect("register pane");

        assert!(matches!(
            arbiter.clear_draft(&key.id, key.generation, 0, "never-submitted"),
            Err(ArbiterError::Stale)
        ));
        arbiter
            .write_human(&table, &key, b"x")
            .expect("write non-Enter input");
        assert!(matches!(
            arbiter.clear_draft(&key.id, key.generation, 1, "not-an-Enter"),
            Err(ArbiterError::Stale)
        ));
        arbiter
            .write_human(&table, &key, b"\r")
            .expect("submit input");
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("Enter event"),
            PaneEvent::Enter {
                pane: key.clone(),
                epoch: 2,
            }
        );
        assert!(matches!(
            arbiter.clear_draft(&key.id, key.generation, 3, "future"),
            Err(ArbiterError::Stale)
        ));
        assert_eq!(
            arbiter
                .clear_draft(&key.id, key.generation, 2, "submitted")
                .expect("clear emitted Enter"),
            ClearOutcome::Cleared
        );
        assert_eq!(read_hex(reader), "780d");
    }

    #[test]
    fn generation_replacement_cannot_detach_an_admitted_paste_and_its_queue() {
        let old_key = PaneKey::new("worker", 1);
        let new_key = PaneKey::new("worker", 2);
        let (events, _receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(0, events));
        arbiter.register(&old_key).expect("register old generation");

        let registration_checked = Arc::new(Barrier::new(2));
        let resume_registration = Arc::new(Barrier::new(2));
        let replacing_arbiter = Arc::clone(&arbiter);
        let replacing_key = new_key.clone();
        let register_checked = Arc::clone(&registration_checked);
        let register_resume = Arc::clone(&resume_registration);
        let replacement = thread::spawn(move || {
            replacing_arbiter.register_with_hook(&replacing_key, || {
                register_checked.wait();
                register_resume.wait();
            })
        });
        registration_checked.wait();

        let first_write = Arc::new(Barrier::new(2));
        let release_first_write = Arc::new(Barrier::new(2));
        let writer = Arc::new(GatedRecordingWriter {
            attempts: AtomicUsize::new(0),
            records: Mutex::new(Vec::new()),
            first_write: Arc::clone(&first_write),
            release_first_write: Arc::clone(&release_first_write),
        });
        let paste_arbiter = Arc::clone(&arbiter);
        let paste_writer = Arc::clone(&writer);
        let paste_key = old_key.clone();
        let paste = thread::spawn(move || {
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, 0, b"body")
        });
        first_write.wait();
        assert_eq!(
            arbiter
                .write_human_via(writer.as_ref(), &old_key, b"H")
                .expect("queue human input behind paste"),
            1
        );

        resume_registration.wait();
        let replacement_result = replacement.join().expect("replacement thread");
        release_first_write.wait();
        let paste_result = paste.join().expect("paste thread");

        assert!(matches!(replacement_result, Err(ArbiterError::Busy)));
        paste_result.expect("admitted paste finishes against its stable state");
        assert_eq!(
            *writer.records.lock().expect("gated records"),
            vec![
                b"\x1b[200~body\x1b[201~".to_vec(),
                b"\r".to_vec(),
                b"H".to_vec(),
            ]
        );
        let snapshot = arbiter
            .snapshot(&old_key)
            .expect("old generation remains active");
        assert!(!snapshot.paste_in_flight);
        assert_eq!(snapshot.queued_human_bytes, 0);
        assert!(snapshot.draft_latched);
    }

    #[test]
    fn generation_replacement_that_holds_the_cell_rejects_old_input_before_write() {
        let old_key = PaneKey::new("worker", 1);
        let new_key = PaneKey::new("worker", 2);
        let (events, _receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(0, events));
        arbiter.register(&old_key).expect("register old generation");
        let cell = arbiter
            .lock_panes()
            .expect("pane map lock")
            .get(&old_key.id)
            .cloned()
            .expect("stable state cell");

        let replacement_locked = Arc::new(Barrier::new(2));
        let release_replacement = Arc::new(Barrier::new(2));
        let replacing_arbiter = Arc::clone(&arbiter);
        let replacing_key = new_key.clone();
        let locked = Arc::clone(&replacement_locked);
        let release = Arc::clone(&release_replacement);
        let replacement = thread::spawn(move || {
            replacing_arbiter.register_with_locked_hook(&replacing_key, || {
                locked.wait();
                release.wait();
            })
        });
        replacement_locked.wait();

        let (writer, _observed) = RecordingWriter::new(None);
        let paste_arbiter = Arc::clone(&arbiter);
        let paste_writer = Arc::clone(&writer);
        let paste_key = old_key.clone();
        let paste = thread::spawn(move || {
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, 0, b"old")
        });
        let clone_deadline = Instant::now() + Duration::from_secs(1);
        while Arc::strong_count(&cell) < 4 && Instant::now() < clone_deadline {
            thread::yield_now();
        }
        assert!(
            Arc::strong_count(&cell) >= 4,
            "old paste did not reach the locked state cell"
        );

        release_replacement.wait();
        replacement
            .join()
            .expect("replacement thread")
            .expect("replacement wins cell lock");
        assert!(matches!(
            paste.join().expect("old paste thread"),
            Err(ArbiterError::Stale)
        ));
        assert!(writer.records().is_empty());
        assert_eq!(
            arbiter.snapshot(&new_key).expect("new generation state"),
            super::ArbiterSnapshot {
                generation: 2,
                input_epoch: 0,
                draft_latched: false,
                paste_in_flight: false,
                input_failed: false,
                queued_human_bytes: 0,
                last_submission_id: None,
            }
        );
    }

    #[test]
    fn human_bytes_during_paste_are_queued_after_automated_enter() {
        let key = PaneKey::new("recorded", 1);
        let (writer, observed) = RecordingWriter::new(None);
        let (events, receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(100, events));
        arbiter.register(&key).expect("register pane");

        let paste_writer = Arc::clone(&writer);
        let paste_arbiter = Arc::clone(&arbiter);
        let paste_key = key.clone();
        let paste = thread::spawn(move || {
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, 0, b"body")
        });
        assert_eq!(
            observed
                .recv_timeout(Duration::from_secs(1))
                .expect("observe bracketed paste")
                .bytes,
            b"\x1b[200~body\x1b[201~"
        );

        arbiter
            .write_human_via(writer.as_ref(), &key, b"H\r")
            .expect("queue human bytes");
        let queued = arbiter.snapshot(&key).expect("queued state");
        assert_eq!(queued.input_epoch, 2);
        assert_eq!(queued.queued_human_bytes, 2);
        paste.join().expect("paste thread").expect("write paste");

        let records = writer.records();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].bytes, b"\x1b[200~body\x1b[201~");
        assert_eq!(records[1].bytes, b"\r");
        assert_eq!(records[2].bytes, b"H\r");
        assert!(records[2].at >= records[1].at);
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("queued Enter event"),
            PaneEvent::Enter {
                pane: key.clone(),
                epoch: 2,
            }
        );
        let final_state = arbiter.snapshot(&key).expect("final state");
        assert!(!final_state.paste_in_flight);
        assert_eq!(final_state.queued_human_bytes, 0);
    }

    #[test]
    fn disconnected_enter_events_flush_the_queue_and_release_the_paste_lock() {
        let key = PaneKey::new("recorded", 1);
        let (writer, observed) = RecordingWriter::new(None);
        let (events, receiver) = mpsc::channel();
        drop(receiver);
        let arbiter = Arc::new(InputArbiter::new(25, events));
        arbiter.register(&key).expect("register pane");

        let paste_writer = Arc::clone(&writer);
        let paste_arbiter = Arc::clone(&arbiter);
        let paste_key = key.clone();
        let paste = thread::spawn(move || {
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, 0, b"body")
        });
        observed
            .recv_timeout(Duration::from_secs(1))
            .expect("observe bracketed paste");
        arbiter
            .write_human_via(writer.as_ref(), &key, b"H\r")
            .expect("queue human Enter");

        assert!(matches!(
            paste.join().expect("paste thread"),
            Err(ArbiterError::EventChannelClosed)
        ));
        assert_eq!(
            writer
                .records()
                .into_iter()
                .map(|record| record.bytes)
                .collect::<Vec<_>>(),
            vec![
                b"\x1b[200~body\x1b[201~".to_vec(),
                b"\r".to_vec(),
                b"H\r".to_vec(),
            ]
        );
        let state = arbiter.snapshot(&key).expect("state after event failure");
        assert!(!state.paste_in_flight);
        assert_eq!(state.queued_human_bytes, 0);
        assert!(matches!(
            arbiter.clear_draft(&key.id, key.generation, 2, "unobserved"),
            Err(ArbiterError::Stale)
        ));
    }

    #[test]
    fn write_failures_poison_input_and_preserve_unwritten_human_input() {
        let key = PaneKey::new("recorded", 1);
        let (first_write_fails, _observed) = RecordingWriter::new(Some(1));
        let (events, _receiver) = mpsc::channel();
        let arbiter = InputArbiter::new(0, events);
        arbiter.register(&key).expect("register pane");

        assert!(matches!(
            arbiter.write_paste_via(first_write_fails.as_ref(), &key, 0, b"body"),
            Err(ArbiterError::Pane(PaneError::Io(_)))
        ));
        assert!(
            !arbiter
                .snapshot(&key)
                .expect("state after paste write failure")
                .paste_in_flight
        );
        assert!(
            arbiter
                .snapshot(&key)
                .expect("state after paste write failure")
                .input_failed
        );
        let writes_after_failure = first_write_fails.records().len();
        assert!(matches!(
            arbiter.write_human_via(first_write_fails.as_ref(), &key, b"later"),
            Err(ArbiterError::InputFailed)
        ));
        assert_eq!(first_write_fails.records().len(), writes_after_failure);
        let restarted_key = PaneKey::new(&key.id, key.generation + 1);
        arbiter
            .register(&restarted_key)
            .expect("restart input with a new generation");
        assert_eq!(
            arbiter
                .write_human_via(first_write_fails.as_ref(), &restarted_key, b"R")
                .expect("new generation accepts input"),
            1
        );
        assert!(
            !arbiter
                .snapshot(&restarted_key)
                .expect("restarted state")
                .input_failed
        );

        let key = PaneKey::new("queued", 1);
        let (queued_write_fails, observed) = RecordingWriter::new(Some(3));
        let (events, _receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(25, events));
        arbiter.register(&key).expect("register queued pane");
        let paste_writer = Arc::clone(&queued_write_fails);
        let paste_arbiter = Arc::clone(&arbiter);
        let paste_key = key.clone();
        let paste = thread::spawn(move || {
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, 0, b"body")
        });
        observed
            .recv_timeout(Duration::from_secs(1))
            .expect("observe bracketed paste");
        arbiter
            .write_human_via(queued_write_fails.as_ref(), &key, b"Q")
            .expect("queue human input");

        assert!(matches!(
            paste.join().expect("paste thread"),
            Err(ArbiterError::Pane(PaneError::Io(_)))
        ));
        let state = arbiter.snapshot(&key).expect("state after queued failure");
        assert!(!state.paste_in_flight);
        assert_eq!(state.queued_human_bytes, 1);
        assert!(state.draft_latched);
        assert!(state.input_failed);
        let writes_after_failure = queued_write_fails.records().len();
        assert!(matches!(
            arbiter.write_human_via(queued_write_fails.as_ref(), &key, b"later"),
            Err(ArbiterError::InputFailed)
        ));
        assert_eq!(queued_write_fails.records().len(), writes_after_failure);
    }

    #[test]
    fn partial_queued_write_poisoning_prevents_an_unsafe_retry_or_bypass() {
        let key = PaneKey::new("partial", 1);
        let (observed_sender, observed) = mpsc::channel();
        let writer = Arc::new(PartialRecordingWriter {
            attempts: AtomicUsize::new(0),
            records: Mutex::new(Vec::new()),
            observed: observed_sender,
        });
        let (events, _receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(25, events));
        arbiter.register(&key).expect("register pane");
        let paste_arbiter = Arc::clone(&arbiter);
        let paste_writer = Arc::clone(&writer);
        let paste_key = key.clone();
        let paste = thread::spawn(move || {
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, 0, b"body")
        });
        observed
            .recv_timeout(Duration::from_secs(1))
            .expect("observe bracketed paste");
        arbiter
            .write_human_via(writer.as_ref(), &key, b"QUEUE")
            .expect("queue human bytes");

        assert!(matches!(
            paste.join().expect("paste thread"),
            Err(ArbiterError::Pane(PaneError::Io(_)))
        ));
        assert_eq!(
            *writer.records.lock().expect("partial records"),
            vec![
                b"\x1b[200~body\x1b[201~".to_vec(),
                b"\r".to_vec(),
                b"QU".to_vec(),
            ]
        );
        let failed = arbiter.snapshot(&key).expect("failed pane state");
        assert!(failed.draft_latched);
        assert_eq!(failed.input_epoch, 5);
        assert_eq!(failed.queued_human_bytes, 5);
        assert!(failed.input_failed);

        assert!(matches!(
            arbiter.write_human_via(writer.as_ref(), &key, b"NEXT"),
            Err(ArbiterError::InputFailed)
        ));
        assert_eq!(writer.records.lock().expect("partial records").len(), 3);
    }

    #[test]
    fn sanitize_normalizes_crlf_and_rejects_other_controls() {
        assert_eq!(
            sanitize(b"first\r\nsecond\tthird\n").expect("sanitize text"),
            b"first\nsecond\tthird\n"
        );
        assert_eq!(
            sanitize("caf\u{e9}".as_bytes()).expect("allow UTF-8"),
            "caf\u{e9}".as_bytes()
        );

        for byte in (0_u8..=0x1f).filter(|byte| !matches!(byte, b'\t' | b'\n')) {
            assert_eq!(sanitize(&[byte]), Err(SanitizeError::ControlByte(byte)));
        }
        assert_eq!(sanitize(&[0x7f]), Err(SanitizeError::ControlByte(0x7f)));
        assert!(sanitize("\u{009b}".as_bytes()).is_err());
        assert!(sanitize(&[0x9b]).is_err());
        assert!(sanitize(&[0xff]).is_err());
    }

    #[cfg(windows)]
    mod windows {
        #[test]
        #[ignore = "macOS-first PTY contract"]
        fn raw_paste_and_enter_contract() {
            panic!("implement with the Windows PTY backend");
        }

        #[test]
        #[ignore = "macOS-first PTY contract"]
        fn queued_human_input_contract() {
            panic!("implement with the Windows PTY backend");
        }
    }
}
