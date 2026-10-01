use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use crate::pty::{write_paste_via, PaneError, PaneInputWriter, PaneKey, PaneTable};

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
    Busy,
    InputFailed,
    InvalidBody(SanitizeError),
    Pane(PaneError),
    LockPoisoned,
}

impl fmt::Display for ArbiterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stale => write!(formatter, "stale pane generation"),
            Self::Busy => write!(formatter, "a paste is already in flight"),
            Self::InputFailed => write!(formatter, "pane input is failed until restart"),
            Self::InvalidBody(error) => write!(formatter, "invalid paste body: {error}"),
            Self::Pane(error) => error.fmt(formatter),
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
    pub paste_in_flight: bool,
    pub input_failed: bool,
    /// How long the pane has printed nothing, once it has printed anything:
    /// a harness still drawing its screen is not ready for input.
    pub output_quiet_ms: Option<u64>,
}

/// When one generation of a pane last printed. Its output thread notes every
/// chunk here and takes no lock: output never waits on the pane's input.
pub struct OutputClock {
    started: Instant,
    /// Milliseconds after `started` of the last output, plus one; 0 before
    /// the pane has printed anything.
    printed: AtomicU64,
}

impl OutputClock {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            printed: AtomicU64::new(0),
        }
    }

    /// The pane printed something now.
    pub fn note(&self) {
        let now = self.elapsed_ms();
        self.printed.store(now.saturating_add(1), Ordering::Release);
    }

    fn quiet_ms(&self) -> Option<u64> {
        let printed = self.printed.load(Ordering::Acquire);
        (printed != 0).then(|| self.elapsed_ms().saturating_sub(printed - 1))
    }

    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// One pane's input. A pane's worker runs its keys, replies, pastes and
/// claims one at a time, in order, so nothing waits in here: none of the
/// human's keys can land inside a paste or before its Enter, and they hold
/// nothing back either (a delivery is pasted into a window whatever its input
/// box holds, the human's unsent text included: the owner's choice,
/// 2026-10-01). The lock is never held across a write.
struct PaneInputState {
    generation: u64,
    paste_in_flight: bool,
    input_failed: bool,
    output: Arc<OutputClock>,
}

type PaneState = Arc<Mutex<PaneInputState>>;
type PaneStateMap = HashMap<String, PaneState>;

impl PaneInputState {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            paste_in_flight: false,
            input_failed: false,
            output: Arc::new(OutputClock::new()),
        }
    }

    fn snapshot(&self) -> ArbiterSnapshot {
        ArbiterSnapshot {
            generation: self.generation,
            paste_in_flight: self.paste_in_flight,
            input_failed: self.input_failed,
            output_quiet_ms: self.output.quiet_ms(),
        }
    }
}

pub struct InputArbiter {
    panes: Mutex<PaneStateMap>,
    enter_delay_ms: u64,
}

impl InputArbiter {
    pub fn new(enter_delay_ms: u64) -> Self {
        Self {
            panes: Mutex::new(HashMap::new()),
            enter_delay_ms,
        }
    }

    /// Takes a pane in, or a newer generation of it. The answer is the
    /// generation's output clock, for the pane's output thread to note.
    pub fn register(&self, pane: &PaneKey) -> Result<Arc<OutputClock>, ArbiterError> {
        self.register_inner(pane, || {}, || {})
    }

    #[cfg(test)]
    fn register_with_hook<F>(
        &self,
        pane: &PaneKey,
        before_replace: F,
    ) -> Result<Arc<OutputClock>, ArbiterError>
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
    ) -> Result<Arc<OutputClock>, ArbiterError>
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
    ) -> Result<Arc<OutputClock>, ArbiterError>
    where
        F: FnMut(),
        G: FnMut(),
    {
        let state = {
            let mut panes = self.lock_panes()?;
            match panes.get(&pane.id) {
                Some(state) => Arc::clone(state),
                None => {
                    let fresh = PaneInputState::new(pane.generation);
                    let output = Arc::clone(&fresh.output);
                    panes.insert(pane.id.clone(), Arc::new(Mutex::new(fresh)));
                    return Ok(output);
                }
            }
        };

        {
            let state = lock_state(&state)?;
            if state.generation > pane.generation {
                return Err(ArbiterError::Stale);
            }
            if state.generation == pane.generation {
                return Ok(Arc::clone(&state.output));
            }
        }

        before_replace();
        let mut state = lock_state(&state)?;
        if state.generation > pane.generation {
            return Err(ArbiterError::Stale);
        }
        if state.generation == pane.generation {
            return Ok(Arc::clone(&state.output));
        }
        if state.paste_in_flight {
            return Err(ArbiterError::Busy);
        }
        before_commit();
        *state = PaneInputState::new(pane.generation);
        Ok(Arc::clone(&state.output))
    }

    /// A pane gone from the table leaves the arbiter. A newer generation of
    /// the same id that took its place keeps its own state.
    pub fn retire(&self, pane: &PaneKey) {
        let Ok(mut panes) = self.lock_panes() else {
            return;
        };
        let current = panes
            .get(&pane.id)
            .and_then(|state| lock_state(state).ok().map(|state| state.generation));
        if current == Some(pane.generation) {
            panes.remove(&pane.id);
        }
    }

    pub fn snapshot(&self, pane: &PaneKey) -> Result<ArbiterSnapshot, ArbiterError> {
        let state = self.pane_state(pane)?;
        let state = lock_state(&state)?;
        validate_generation(&state, pane)?;
        let snapshot = state.snapshot();
        Ok(snapshot)
    }

    /// The human's keys and the emulator's replies, written at once.
    pub fn write(&self, table: &PaneTable, pane: &PaneKey, bytes: &[u8]) -> Result<(), ArbiterError> {
        self.write_via(table, pane, bytes)
    }

    fn write_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        bytes: &[u8],
    ) -> Result<(), ArbiterError> {
        let state = self.pane_state(pane)?;
        validate_admission(&*lock_state(&state)?, pane)?;
        if bytes.is_empty() {
            return Ok(());
        }
        writer
            .write(pane, bytes)
            .map_err(|error| fail_input(&state, pane, error))
    }

    pub fn write_paste(
        &self,
        table: &PaneTable,
        pane: &PaneKey,
        body: &[u8],
    ) -> Result<(), ArbiterError> {
        self.write_paste_via(table, pane, body)
    }

    /// Admit a native-channel send (a harness's own server, inbox or socket)
    /// as a paste is admitted: the pane is current and its input works. It
    /// reserves nothing and sends nothing; its place in the pane's worker is
    /// what keeps it out of a paste going in.
    pub fn claim(&self, pane: &PaneKey) -> Result<(), ArbiterError> {
        let state = self.pane_state(pane)?;
        let state = lock_state(&state)?;
        validate_admission(&state, pane)
    }

    /// A paste and its Enter. While it goes in, the pane's snapshot says so,
    /// and a newer generation of the pane cannot take its place.
    fn write_paste_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        body: &[u8],
    ) -> Result<(), ArbiterError> {
        let body = sanitize(body)?;
        let state = self.pane_state(pane)?;
        {
            let mut state = lock_state(&state)?;
            validate_admission(&state, pane)?;
            state.paste_in_flight = true;
        }
        let written = write_paste_via(writer, pane, &body, self.enter_delay_ms);
        lock_state(&state)?.paste_in_flight = false;
        written.map_err(|error| fail_input(&state, pane, error))
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

fn validate_admission(state: &PaneInputState, pane: &PaneKey) -> Result<(), ArbiterError> {
    validate_generation(state, pane)?;
    if state.input_failed {
        return Err(ArbiterError::InputFailed);
    }
    Ok(())
}

/// A write that failed may have reached the pane in part, so the pane takes
/// no more input until a new generation replaces it. A newer generation that
/// took its place meanwhile keeps working input.
fn fail_input(state: &PaneState, pane: &PaneKey, error: PaneError) -> ArbiterError {
    if let Ok(mut state) = lock_state(state) {
        if state.generation == pane.generation {
            state.input_failed = true;
        }
    }
    error.into()
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
    #[cfg(unix)]
    use std::collections::HashMap;
    #[cfg(unix)]
    use std::io::{Read, Result as IoResult};
    #[cfg(unix)]
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Barrier, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    use portable_pty::PtySize;

    use super::{sanitize, ArbiterError, InputArbiter, SanitizeError};
    #[cfg(unix)]
    use crate::pty::{serial_pty_test, OpenedPane, PaneTable};
    use crate::pty::{PaneError, PaneInputWriter, PaneKey};

    #[test]
    fn a_snapshot_says_how_long_the_pane_has_printed_nothing() {
        let arbiter = InputArbiter::new(0);
        let pane = PaneKey::new("quiet-pane", 1);
        let printed = arbiter.register(&pane).expect("register");
        assert_eq!(
            arbiter.snapshot(&pane).expect("snapshot").output_quiet_ms,
            None
        );
        printed.note();
        thread::sleep(Duration::from_millis(20));
        let quiet = arbiter.snapshot(&pane).expect("snapshot").output_quiet_ms;
        assert!(
            quiet.is_some_and(|ms| (20..5_000).contains(&ms)),
            "{quiet:?}"
        );

        let newer = PaneKey::new("quiet-pane", 2);
        arbiter.register(&newer).expect("register a newer generation");
        printed.note();
        assert_eq!(
            arbiter.snapshot(&newer).expect("snapshot").output_quiet_ms,
            None,
            "an older generation's output is not this pane's"
        );
    }

    #[cfg(unix)]
    fn terminal_size() -> PtySize {
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    #[cfg(unix)]
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

    /// The recorder's first word, read with a watchdog: on a CI runner this
    /// read once waited for good, holding the PTY tests' lock. After 20 s the
    /// watchdog writes what arrived to stderr (past the test's capture) and
    /// aborts, so the run fails at once and says why.
    #[cfg(unix)]
    fn await_ready(reader: &mut Box<dyn Read + Send>) {
        use std::io::Write as _;
        use std::sync::atomic::AtomicBool;
        let seen = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let (watched, finished) = (Arc::clone(&seen), Arc::clone(&done));
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline {
                if finished.load(Ordering::SeqCst) {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
            let bytes = watched.lock().map(|b| b.clone()).unwrap_or_default();
            let _ = std::io::stderr().write_all(
                format!(
                    "raw recorder never said ready in 20 s; it printed {:?}\n",
                    String::from_utf8_lossy(&bytes)
                )
                .as_bytes(),
            );
            std::process::abort();
        });
        let mut byte = [0_u8; 1];
        while seen.lock().expect("ready bytes").len() < 5 {
            reader.read_exact(&mut byte).expect("raw recorder ready");
            seen.lock().expect("ready bytes").push(byte[0]);
        }
        done.store(true, Ordering::SeqCst);
        assert_eq!(&seen.lock().expect("ready bytes")[..], b"ready");
    }

    #[cfg(unix)]
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

    /// Unsent typing holds nothing: a paste admitted after it goes in behind
    /// the human's text, and a native send is admitted too (the owner's choice
    /// on 2026-10-01: an idle window gets its message at once).
    #[test]
    fn typing_never_holds_a_paste_or_a_claim() {
        let key = PaneKey::new("typed", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(0);
        arbiter.register(&key).expect("register pane");

        arbiter
            .write_via(writer.as_ref(), &key, b"half a thought")
            .expect("type without sending");
        arbiter.claim(&key).expect("a native send is admitted");
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"result")
            .expect("a paste is admitted");

        assert_eq!(
            writer
                .records()
                .into_iter()
                .map(|record| record.bytes)
                .collect::<Vec<_>>(),
            vec![
                b"half a thought".to_vec(),
                b"\x1b[200~result\x1b[201~".to_vec(),
                b"\r".to_vec(),
            ]
        );
    }

    /// A pane that leaves the table takes its state with it, but an older
    /// generation retiring late leaves the newer one's state alone.
    #[test]
    fn a_retired_pane_leaves_and_a_newer_generation_stays() {
        let arbiter = InputArbiter::new(0);
        let old = PaneKey::new("worker", 1);
        let new = PaneKey::new("worker", 2);
        arbiter.register(&old).expect("register the old generation");
        arbiter.register(&new).expect("register the new generation");

        arbiter.retire(&old);
        assert_eq!(arbiter.snapshot(&new).expect("still there").generation, 2);
        arbiter.retire(&new);
        assert!(matches!(arbiter.snapshot(&new), Err(ArbiterError::Stale)));
    }

    /// A write waiting on a busy harness holds no lock: the pane's output is
    /// noted and its state read meanwhile. The lock used to be held across
    /// the write, and the output thread took it for every chunk, so a long
    /// paste into a briefly busy harness held its output back from the page.
    #[test]
    fn a_blocked_write_holds_up_no_one_else() {
        let key = PaneKey::new("busy", 1);
        let first_write = Arc::new(Barrier::new(2));
        let release_first_write = Arc::new(Barrier::new(2));
        let writer = Arc::new(GatedRecordingWriter {
            attempts: AtomicUsize::new(0),
            records: Mutex::new(Vec::new()),
            first_write: Arc::clone(&first_write),
            release_first_write: Arc::clone(&release_first_write),
        });
        let arbiter = Arc::new(InputArbiter::new(0));
        let printed = arbiter.register(&key).expect("register pane");

        let writing_arbiter = Arc::clone(&arbiter);
        let writing_key = key.clone();
        let writing =
            thread::spawn(move || writing_arbiter.write_via(writer.as_ref(), &writing_key, b"typed"));
        first_write.wait();
        printed.note();
        let (read, snapshot) = mpsc::channel();
        let reading_arbiter = Arc::clone(&arbiter);
        let reading_key = key.clone();
        thread::spawn(move || {
            let quiet = reading_arbiter
                .snapshot(&reading_key)
                .map(|snapshot| snapshot.output_quiet_ms);
            let _ = read.send(quiet.is_ok_and(|quiet| quiet.is_some()));
        });
        let read_meanwhile = snapshot.recv_timeout(Duration::from_secs(1));
        release_first_write.wait();
        writing.join().expect("writing thread").expect("the write");
        assert_eq!(read_meanwhile, Ok(true), "the write held the pane's lock");
    }

    #[test]
    fn paste_is_bracketed_then_enter_is_a_delayed_separate_write() {
        let key = PaneKey::new("recorded", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(25);
        arbiter.register(&key).expect("register pane");

        arbiter
            .write_paste_via(writer.as_ref(), &key, b"body")
            .expect("write paste");

        let records = writer.records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].bytes, b"\x1b[200~body\x1b[201~");
        assert_eq!(records[1].bytes, b"\r");
        assert!(records[1].at.duration_since(records[0].at) >= Duration::from_millis(25));
    }

    #[cfg(unix)]
    #[test]
    fn a_blocked_write_does_not_stall_an_unrelated_pane() {
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
        let arbiter = Arc::new(InputArbiter::new(5));
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
            let result =
                blocked_arbiter.write(&blocked_table, &writer_key, &vec![b'x'; 8 * 1024 * 1024]);
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
            let _ = responsive_sender.send(responsive_arbiter.write(
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

        responsive_result.expect("responsive write");
        assert_eq!(read_hex(reader), "52");
        let _ = table.kill(&responsive_key);
    }

    /// What still refuses a delivery: a pane replaced since, or failed input
    /// (see below). Another paste cannot be going in: the pane's worker runs
    /// one at a time.
    #[test]
    fn claims_and_pastes_refuse_a_stale_pane() {
        let key = PaneKey::new("guarded", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(0);
        arbiter.register(&key).expect("register pane");
        let stale = PaneKey::new(&key.id, key.generation + 1);
        assert!(matches!(arbiter.claim(&stale), Err(ArbiterError::Stale)));
        assert!(matches!(
            arbiter.write_paste_via(writer.as_ref(), &stale, b"late"),
            Err(ArbiterError::Stale)
        ));
        assert!(writer.records().is_empty());
    }

    /// A newer generation cannot take a pane's place while its paste goes in:
    /// the paste finishes against the state it was admitted to.
    #[test]
    fn a_new_generation_cannot_cut_a_paste_short() {
        let old_key = PaneKey::new("worker", 1);
        let new_key = PaneKey::new("worker", 2);
        let arbiter = Arc::new(InputArbiter::new(0));
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
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, b"body")
        });
        first_write.wait();
        assert!(
            arbiter
                .snapshot(&old_key)
                .expect("a paste is going in")
                .paste_in_flight
        );

        resume_registration.wait();
        let replacement_result = replacement.join().expect("replacement thread");
        release_first_write.wait();
        let paste_result = paste.join().expect("paste thread");

        assert!(matches!(replacement_result, Err(ArbiterError::Busy)));
        paste_result.expect("admitted paste finishes against its stable state");
        assert_eq!(
            *writer.records.lock().expect("gated records"),
            vec![b"\x1b[200~body\x1b[201~".to_vec(), b"\r".to_vec()]
        );
        let snapshot = arbiter
            .snapshot(&old_key)
            .expect("old generation remains active");
        assert!(!snapshot.paste_in_flight);
    }

    #[test]
    fn generation_replacement_that_holds_the_cell_rejects_old_input_before_write() {
        let old_key = PaneKey::new("worker", 1);
        let new_key = PaneKey::new("worker", 2);
        let arbiter = Arc::new(InputArbiter::new(0));
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
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, b"old")
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
                paste_in_flight: false,
                input_failed: false,
                output_quiet_ms: None,
            }
        );
    }

    /// A write that failed may have reached the pane in part: the pane takes
    /// no more input until a new generation replaces it.
    #[test]
    fn a_failed_write_stops_the_pane_input_until_a_new_generation() {
        let key = PaneKey::new("recorded", 1);
        let (first_write_fails, _observed) = RecordingWriter::new(Some(1));
        let arbiter = InputArbiter::new(0);
        arbiter.register(&key).expect("register pane");

        assert!(matches!(
            arbiter.write_paste_via(first_write_fails.as_ref(), &key, b"body"),
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
            arbiter.write_via(first_write_fails.as_ref(), &key, b"later"),
            Err(ArbiterError::InputFailed)
        ));
        assert!(matches!(
            arbiter.claim(&key),
            Err(ArbiterError::InputFailed)
        ));
        assert_eq!(first_write_fails.records().len(), writes_after_failure);
        let restarted_key = PaneKey::new(&key.id, key.generation + 1);
        arbiter
            .register(&restarted_key)
            .expect("restart input with a new generation");
        arbiter
            .write_via(first_write_fails.as_ref(), &restarted_key, b"R")
            .expect("new generation accepts input");
        assert!(
            !arbiter
                .snapshot(&restarted_key)
                .expect("restarted state")
                .input_failed
        );
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
        use std::collections::HashMap;
        use std::sync::Arc;

        use super::super::InputArbiter;
        use crate::pty::conpty_test::{line_echo, open, read_until};
        use crate::pty::{serial_pty_test, PaneTable};

        /// A delivery is a bracketed paste and a delayed Enter; the harness
        /// must read the text alone, as one submitted line.
        #[test]
        fn a_paste_and_its_enter_reach_the_child_as_one_line() {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (key, output) = open(&table, &line_echo(), &HashMap::new(), 1 << 20);
            let arbiter = InputArbiter::new(5);
            arbiter.register(&key).expect("register pane");
            read_until(&table, &key, &output, "READY");

            arbiter
                .write_paste(&table, &key, b"pasted words")
                .expect("paste into the pane");
            let text = read_until(&table, &key, &output, "#");
            assert!(text.contains("GOT=[pasted words]"), "{text:?}");
            table.kill(&key).expect("kill the pane");
        }

        /// A human's typing reaches the child, Enter and all.
        #[test]
        fn human_typing_reaches_the_child() {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (key, output) = open(&table, &line_echo(), &HashMap::new(), 1 << 20);
            let arbiter = InputArbiter::new(5);
            arbiter.register(&key).expect("register pane");
            read_until(&table, &key, &output, "READY");

            arbiter
                .write(&table, &key, b"typed\r")
                .expect("type into the pane");
            let text = read_until(&table, &key, &output, "#");
            assert!(text.contains("GOT=[typed]"), "{text:?}");
            table.kill(&key).expect("kill the pane");
        }
    }
}
