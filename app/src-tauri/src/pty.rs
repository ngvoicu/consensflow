use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PaneKey {
    pub id: String,
    pub generation: u64,
}

impl PaneKey {
    pub fn new(id: impl Into<String>, generation: u64) -> Self {
        Self {
            id: id.into(),
            generation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneInfo {
    pub id: String,
    pub generation: u64,
    pub alive: bool,
    pub idle_ms: u64,
}

pub struct OpenedPane {
    pub key: PaneKey,
    pub reader: Box<dyn Read + Send>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneOutput {
    pub key: PaneKey,
    pub seq: u64,
    pub bytes: Vec<u8>,
}

pub struct StreamedPane {
    pub key: PaneKey,
    pub output: mpsc::Receiver<PaneOutput>,
}

pub(crate) trait PaneInputWriter {
    fn write(&self, key: &PaneKey, bytes: &[u8]) -> Result<(), PaneError>;
}

#[derive(Debug)]
pub enum PaneError {
    EmptyArgv,
    ProgramNotAbsolute(PathBuf),
    AlreadyOpen(PaneKey),
    NotFound(PaneKey),
    Pty(String),
    Io(io::Error),
    InvalidBacklog,
    NoOutputStream(PaneKey),
    FutureAck {
        key: PaneKey,
        seq: u64,
        highest_issued: u64,
    },
    LockPoisoned,
}

impl fmt::Display for PaneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyArgv => write!(formatter, "argv must contain an absolute program path"),
            Self::ProgramNotAbsolute(path) => {
                write!(formatter, "argv[0] must be absolute: {}", path.display())
            }
            Self::AlreadyOpen(key) => write!(
                formatter,
                "pane {} generation {} is already open",
                key.id, key.generation
            ),
            Self::NotFound(key) => write!(
                formatter,
                "pane {} generation {} is not open",
                key.id, key.generation
            ),
            Self::Pty(message) => write!(formatter, "PTY error: {message}"),
            Self::Io(error) => write!(formatter, "PTY I/O error: {error}"),
            Self::InvalidBacklog => write!(formatter, "backlogBytes must be greater than zero"),
            Self::NoOutputStream(key) => write!(
                formatter,
                "pane {} generation {} has no output stream",
                key.id, key.generation
            ),
            Self::FutureAck {
                key,
                seq,
                highest_issued,
            } => write!(
                formatter,
                "pane {} generation {} ack {} exceeds highest issued seq {}",
                key.id, key.generation, seq, highest_issued
            ),
            Self::LockPoisoned => write!(formatter, "pane table lock is poisoned"),
        }
    }
}

impl std::error::Error for PaneError {}

impl From<io::Error> for PaneError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

struct Pane {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn Child + Send + Sync>,
    process_group_id: Option<i32>,
    output_flow: Option<Arc<OutputFlow>>,
    active_writes: Arc<AtomicU64>,
    alive: bool,
    last_activity: Arc<Mutex<Instant>>,
}

struct OutputFlow {
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
    fn new(backlog_bytes: usize) -> Self {
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

    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        self.ready.notify_all();
    }
}

pub struct PaneTable {
    panes: Mutex<HashMap<PaneKey, Pane>>,
    next_id: AtomicU64,
}

impl PaneTable {
    pub fn new() -> Self {
        Self {
            panes: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn open(
        &self,
        cwd: &Path,
        argv: &[String],
        env: &HashMap<String, String>,
        size: PtySize,
    ) -> Result<OpenedPane, PaneError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let key = PaneKey::new(format!("pane-{id}"), 1);
        let reader = self.open_at(key.clone(), cwd, argv, env, size)?;
        Ok(OpenedPane { key, reader })
    }

    pub fn open_at(
        &self,
        key: PaneKey,
        cwd: &Path,
        argv: &[String],
        env: &HashMap<String, String>,
        size: PtySize,
    ) -> Result<Box<dyn Read + Send>, PaneError> {
        let program = argv.first().ok_or(PaneError::EmptyArgv)?;
        let program_path = Path::new(program);
        if !program_path.is_absolute() {
            return Err(PaneError::ProgramNotAbsolute(program_path.to_path_buf()));
        }

        let mut panes = self.lock_panes()?;
        if panes.contains_key(&key) {
            return Err(PaneError::AlreadyOpen(key));
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size)
            .map_err(|error| PaneError::Pty(error.to_string()))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| PaneError::Pty(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| PaneError::Pty(error.to_string()))?;

        let mut command = CommandBuilder::new(program);
        command.args(&argv[1..]);
        command.cwd(cwd);
        command.env_clear();
        for (name, value) in env {
            command.env(name, value);
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| PaneError::Pty(error.to_string()))?;
        drop(pair.slave);

        #[cfg(unix)]
        let process_group_id = child.process_id().and_then(|pid| i32::try_from(pid).ok());
        #[cfg(not(unix))]
        let process_group_id = None;
        let last_activity = Arc::new(Mutex::new(Instant::now()));
        let reader = Box::new(ActivityReader {
            inner: reader,
            last_activity: Arc::clone(&last_activity),
        });

        panes.insert(
            key,
            Pane {
                master: pair.master,
                writer: Arc::new(Mutex::new(writer)),
                child,
                process_group_id,
                output_flow: None,
                active_writes: Arc::new(AtomicU64::new(0)),
                alive: true,
                last_activity,
            },
        );
        Ok(reader)
    }

    pub fn open_streamed(
        &self,
        cwd: &Path,
        argv: &[String],
        env: &HashMap<String, String>,
        size: PtySize,
        backlog_bytes: usize,
    ) -> Result<StreamedPane, PaneError> {
        if backlog_bytes == 0 {
            return Err(PaneError::InvalidBacklog);
        }

        let OpenedPane { key, reader } = self.open(cwd, argv, env, size)?;
        let flow = Arc::new(OutputFlow::new(backlog_bytes));
        self.lock_panes()?
            .get_mut(&key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?
            .output_flow = Some(Arc::clone(&flow));

        let (sender, output) = mpsc::channel();
        let output_key = key.clone();
        std::thread::spawn(move || stream_output(output_key, reader, flow, sender));
        Ok(StreamedPane { key, output })
    }

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

    pub fn write(&self, key: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
        let (writer, last_activity, active_writes) = {
            let panes = self.lock_panes()?;
            let pane = panes
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            pane.active_writes.fetch_add(1, Ordering::AcqRel);
            (
                Arc::clone(&pane.writer),
                Arc::clone(&pane.last_activity),
                Arc::clone(&pane.active_writes),
            )
        };
        let _active_write = ActiveWrite(active_writes);
        let mut writer = writer.lock().map_err(|_| PaneError::LockPoisoned)?;
        writer.write_all(bytes)?;
        writer.flush()?;
        *last_activity.lock().map_err(|_| PaneError::LockPoisoned)? = Instant::now();
        Ok(())
    }

    pub fn write_paste(
        &self,
        key: &PaneKey,
        body: &[u8],
        enter_delay_ms: u64,
    ) -> Result<(), PaneError> {
        write_paste_via(self, key, body, enter_delay_ms)
    }

    pub fn resize(&self, key: &PaneKey, rows: u16, cols: u16) -> Result<(), PaneError> {
        let mut panes = self.lock_panes()?;
        let pane = panes
            .get_mut(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?;
        pane.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| PaneError::Pty(error.to_string()))?;
        *pane
            .last_activity
            .lock()
            .map_err(|_| PaneError::LockPoisoned)? = Instant::now();
        Ok(())
    }

    pub fn kill(&self, key: &PaneKey) -> Result<(), PaneError> {
        let mut pane = self
            .lock_panes()?
            .remove(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?;
        close_output(&pane);
        if pane.active_writes.load(Ordering::Acquire) == 0 {
            terminate(&mut pane)
        } else {
            terminate_detached(pane)
        }
    }

    pub fn list(&self) -> Result<Vec<PaneInfo>, PaneError> {
        let mut panes = self.lock_panes()?;
        let now = Instant::now();
        let mut listed = Vec::with_capacity(panes.len());
        for (key, pane) in panes.iter_mut() {
            let alive = pane.child.try_wait()?.is_none();
            if pane.alive && !alive {
                *pane
                    .last_activity
                    .lock()
                    .map_err(|_| PaneError::LockPoisoned)? = now;
            }
            pane.alive = alive;
            let last_activity = *pane
                .last_activity
                .lock()
                .map_err(|_| PaneError::LockPoisoned)?;
            let idle_ms = now.duration_since(last_activity).as_millis();
            listed.push(PaneInfo {
                id: key.id.clone(),
                generation: key.generation,
                alive,
                idle_ms: idle_ms.min(u128::from(u64::MAX)) as u64,
            });
        }
        listed
            .sort_by(|left, right| (&left.id, left.generation).cmp(&(&right.id, right.generation)));
        Ok(listed)
    }

    #[cfg(test)]
    fn process_group_id(&self, key: &PaneKey) -> Result<i32, PaneError> {
        self.lock_panes()?
            .get(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?
            .process_group_id
            .ok_or_else(|| PaneError::Pty("the PTY has no process group".to_string()))
    }

    fn lock_panes(&self) -> Result<MutexGuard<'_, HashMap<PaneKey, Pane>>, PaneError> {
        self.panes.lock().map_err(|_| PaneError::LockPoisoned)
    }
}

impl PaneInputWriter for PaneTable {
    fn write(&self, key: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
        PaneTable::write(self, key, bytes)
    }
}

pub(crate) fn write_paste_via<W: PaneInputWriter + ?Sized>(
    writer: &W,
    key: &PaneKey,
    body: &[u8],
    enter_delay_ms: u64,
) -> Result<(), PaneError> {
    let mut bracketed = Vec::with_capacity(12 + body.len());
    bracketed.extend_from_slice(b"\x1b[200~");
    bracketed.extend_from_slice(body);
    bracketed.extend_from_slice(b"\x1b[201~");
    writer.write(key, &bracketed)?;
    thread_sleep_ms(enter_delay_ms);
    writer.write(key, b"\r")
}

struct ActiveWrite(Arc<AtomicU64>);

impl Drop for ActiveWrite {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct ActivityReader {
    inner: Box<dyn Read + Send>,
    last_activity: Arc<Mutex<Instant>>,
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

fn thread_sleep_ms(milliseconds: u64) {
    if milliseconds > 0 {
        std::thread::sleep(Duration::from_millis(milliseconds));
    }
}

impl Default for PaneTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
pub(crate) fn serial_pty_test() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|error| error.into_inner())
}

impl Drop for PaneTable {
    fn drop(&mut self) {
        let panes = match self.panes.get_mut() {
            Ok(panes) => panes,
            Err(poisoned) => poisoned.into_inner(),
        };
        for pane in panes.values_mut() {
            close_output(pane);
            let _ = terminate(pane);
        }
    }
}

fn stream_output(
    key: PaneKey,
    mut reader: Box<dyn Read + Send>,
    flow: Arc<OutputFlow>,
    sender: mpsc::Sender<PaneOutput>,
) {
    const MAX_CHUNK_BYTES: usize = 8 * 1024;

    while let Some(allowance) = flow.read_allowance() {
        let mut bytes = vec![0; allowance.min(MAX_CHUNK_BYTES)];
        match reader.read(&mut bytes) {
            Ok(0) | Err(_) => break,
            Ok(byte_count) => {
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
        }
    }
    let mut discarded = [0; MAX_CHUNK_BYTES];
    while reader
        .read(&mut discarded)
        .is_ok_and(|byte_count| byte_count > 0)
    {}
    flow.close();
}

fn close_output(pane: &Pane) {
    if let Some(flow) = &pane.output_flow {
        flow.close();
    }
}

fn terminate(pane: &mut Pane) -> Result<(), PaneError> {
    let child_exited = signal_for_termination(pane)?;

    if !child_exited {
        pane.child.wait()?;
    }
    pane.alive = false;
    Ok(())
}

fn terminate_detached(mut pane: Pane) -> Result<(), PaneError> {
    let child_exited = signal_for_termination(&mut pane)?;
    if child_exited {
        pane.alive = false;
        return Ok(());
    }

    std::thread::Builder::new()
        .name("consensflow-pty-reaper".to_string())
        .spawn(move || {
            let _ = pane.child.wait();
        })?;
    Ok(())
}

fn signal_for_termination(pane: &mut Pane) -> Result<bool, PaneError> {
    let mut child_exited = pane.child.try_wait()?.is_some();

    #[cfg(unix)]
    if let Some(process_group_id) = pane.process_group_id {
        if let Err(error) = signal_process_group(process_group_id) {
            if is_permission_denied(&error) {
                let deadline = Instant::now() + Duration::from_millis(100);
                while !child_exited && Instant::now() < deadline {
                    child_exited = pane.child.try_wait()?.is_some();
                    if !child_exited {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
            if !child_exited || !is_permission_denied(&error) {
                return Err(error);
            }
        }
    }

    #[cfg(not(unix))]
    if !child_exited {
        pane.child.kill()?;
    }

    Ok(child_exited)
}

#[cfg(unix)]
fn is_permission_denied(error: &PaneError) -> bool {
    matches!(error, PaneError::Io(error) if error.kind() == io::ErrorKind::PermissionDenied)
}

#[cfg(unix)]
fn signal_process_group(process_group_id: i32) -> Result<(), PaneError> {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }

    const SIGKILL: i32 = 9;
    // SAFETY: portable-pty created the child as its own session leader. A
    // negative pid addresses that process group, and SIGKILL needs no handler.
    if unsafe { kill(-process_group_id, SIGKILL) } == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    const ESRCH: i32 = 3;
    if error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(ESRCH) {
        Ok(())
    } else {
        Err(PaneError::Io(error))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Read};
    use std::path::Path;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use portable_pty::PtySize;

    use super::{serial_pty_test, OpenedPane, PaneError, PaneKey, PaneTable};

    fn terminal_size(rows: u16, cols: u16) -> PtySize {
        PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn shell(script: &str) -> Vec<String> {
        vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()]
    }

    fn open_shell(table: &PaneTable, script: &str) -> OpenedPane {
        table
            .open(
                Path::new("/tmp"),
                &shell(script),
                &HashMap::new(),
                terminal_size(40, 120),
            )
            .expect("open shell in a PTY")
    }

    fn read_to_end(mut reader: Box<dyn Read + Send>) -> Vec<u8> {
        let (sender, receiver) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let mut output = Vec::new();
            let result = reader.read_to_end(&mut output).map(|_| output);
            let _ = sender.send(result);
        });
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("PTY reader did not reach EOF")
            .expect("read PTY output")
    }

    #[cfg(unix)]
    fn process_exists(pid: i32) -> bool {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }

        // SAFETY: signal 0 does not deliver a signal; it only checks whether
        // the process exists and is signalable by this process.
        let result = unsafe { kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1)
    }

    #[test]
    fn open_reads_hello_then_eof() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let OpenedPane { key, reader } = open_shell(&table, "printf hello");

        assert_eq!(read_to_end(reader), b"hello");
        assert_eq!(
            table.list().expect("list panes"),
            vec![super::PaneInfo {
                id: key.id,
                generation: key.generation,
                alive: false,
                idle_ms: 0,
            }]
        );
    }

    #[test]
    fn pane_table_keys_same_id_by_generation() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let argv = shell("sleep 1000");
        let env = HashMap::new();
        let first = PaneKey::new("worker", 1);
        let second = PaneKey::new("worker", 2);

        let first_reader = table
            .open_at(
                first.clone(),
                Path::new("/tmp"),
                &argv,
                &env,
                terminal_size(24, 80),
            )
            .expect("open first generation");
        let second_reader = table
            .open_at(
                second.clone(),
                Path::new("/tmp"),
                &argv,
                &env,
                terminal_size(24, 80),
            )
            .expect("open second generation");

        let listed = table.list().expect("list panes");
        assert_eq!(listed.len(), 2);
        assert!(listed
            .iter()
            .any(|pane| pane.id == "worker" && pane.generation == 1));
        assert!(listed
            .iter()
            .any(|pane| pane.id == "worker" && pane.generation == 2));

        table.kill(&first).expect("kill first generation");
        table.kill(&second).expect("kill second generation");
        assert_eq!(read_to_end(first_reader), b"");
        assert_eq!(read_to_end(second_reader), b"");
    }

    #[test]
    fn resize_reaches_the_child_terminal() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let OpenedPane { key, reader } = table
            .open(
                Path::new("/tmp"),
                &shell("sleep 0.1; stty size"),
                &HashMap::new(),
                terminal_size(10, 20),
            )
            .expect("open shell in a PTY");

        table.resize(&key, 24, 80).expect("resize PTY");

        assert_eq!(
            String::from_utf8(read_to_end(reader))
                .expect("UTF-8 stty output")
                .trim(),
            "24 80"
        );
    }

    #[test]
    fn kill_closes_the_reader_and_reaps_the_process_group() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let OpenedPane { key, reader } =
            open_shell(&table, "sleep 1000 & printf '%s\\n' \"$!\"; sleep 1000");
        let process_group = table.process_group_id(&key).expect("read process group id");
        let mut reader = BufReader::new(reader);
        let mut child_pid = String::new();
        reader
            .read_line(&mut child_pid)
            .expect("read background child pid");
        let child_pid = child_pid.trim().parse::<i32>().expect("numeric child pid");
        assert!(process_exists(process_group));
        assert!(process_exists(child_pid));

        table.kill(&key).expect("kill process group");

        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).expect("reader reaches EOF");
        let deadline = Instant::now() + Duration::from_secs(2);
        while (process_exists(process_group) || process_exists(child_pid))
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process_exists(process_group),
            "process group survived kill"
        );
        assert!(!process_exists(child_pid), "background child survived kill");
    }

    #[cfg(unix)]
    #[test]
    fn kill_after_natural_exit_is_cleanup_success() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let OpenedPane { key, reader } = open_shell(&table, "printf done");

        assert_eq!(read_to_end(reader), b"done");
        table
            .kill(&key)
            .expect("a gone process group is already clean");
        assert!(table.list().expect("list after cleanup").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn dropping_a_live_table_reaps_its_entire_process_group() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let OpenedPane { key, reader } =
            open_shell(&table, "sleep 1000 & printf '%s\\n' \"$!\"; sleep 1000");
        let process_group = table.process_group_id(&key).expect("read process group id");
        let mut reader = BufReader::new(reader);
        let mut child_pid = String::new();
        reader
            .read_line(&mut child_pid)
            .expect("read background child pid");
        let child_pid = child_pid.trim().parse::<i32>().expect("numeric child pid");
        assert!(process_exists(process_group));
        assert!(process_exists(child_pid));

        drop(table);

        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).expect("reader reaches EOF");
        let deadline = Instant::now() + Duration::from_secs(2);
        while (process_exists(process_group) || process_exists(child_pid))
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process_exists(process_group),
            "process group survived Drop"
        );
        assert!(!process_exists(child_pid), "background child survived Drop");
    }

    #[test]
    fn environment_map_reaches_the_child() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let mut env = HashMap::new();
        env.insert("PANE_TEST_VALUE".to_string(), "from-env-map".to_string());
        let OpenedPane { reader, .. } = table
            .open(
                Path::new("/tmp"),
                &shell("printf %s \"$PANE_TEST_VALUE\""),
                &env,
                terminal_size(24, 80),
            )
            .expect("open shell in a PTY");

        assert_eq!(read_to_end(reader), b"from-env-map");
    }

    #[test]
    fn child_environment_is_replaced_instead_of_inherited() {
        let _pty_guard = serial_pty_test();
        const SENTINEL: &str = "CONSENSFLOW_PTY_TEST_LAUNCHER_SENTINEL_7FC9A1";

        struct RemoveSentinel;
        impl Drop for RemoveSentinel {
            fn drop(&mut self) {
                std::env::remove_var(SENTINEL);
            }
        }

        std::env::set_var(SENTINEL, "launcher-only");
        let _remove_sentinel = RemoveSentinel;
        let table = PaneTable::new();
        let script = format!(
            "if [ \"${{{SENTINEL}+present}}\" = present ]; then printf inherited; else printf absent; fi"
        );
        let OpenedPane { reader, .. } = table
            .open(
                Path::new("/tmp"),
                &shell(&script),
                &HashMap::new(),
                terminal_size(24, 80),
            )
            .expect("open child without the sentinel");
        assert_eq!(read_to_end(reader), b"absent");

        let mut supplied = HashMap::new();
        supplied.insert(SENTINEL.to_string(), "role-value".to_string());
        let OpenedPane { reader, .. } = table
            .open(
                Path::new("/tmp"),
                &shell(&format!("printf %s \"${SENTINEL}\"")),
                &supplied,
                terminal_size(24, 80),
            )
            .expect("open child with the supplied sentinel");
        assert_eq!(read_to_end(reader), b"role-value");
    }

    #[test]
    fn raw_mode_recorder_receives_exact_bytes() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let OpenedPane { key, mut reader } =
            open_shell(&table, "stty raw -echo; printf ready; od -An -tx1 -N 5");
        let mut ready = [0_u8; 5];
        reader.read_exact(&mut ready).expect("raw recorder ready");
        assert_eq!(&ready, b"ready");

        table
            .write(&key, &[0x00, 0x09, 0x0d, 0x1b, 0xff])
            .expect("write raw bytes");

        let hex = String::from_utf8(read_to_end(reader)).expect("UTF-8 od output");
        assert_eq!(hex.split_whitespace().collect::<String>(), "00090d1bff");
    }

    #[cfg(unix)]
    #[test]
    fn blocked_write_does_not_stall_other_panes_or_kill() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let OpenedPane {
            key: blocked_key,
            reader: _blocked_reader,
        } = open_shell(&table, "sleep 1000");
        let blocked_process_group = table
            .process_group_id(&blocked_key)
            .expect("blocked pane process group");
        let OpenedPane {
            key: responsive_key,
            mut reader,
        } = open_shell(&table, "stty raw -echo; printf ready; od -An -tx1 -N 1");
        let mut ready = [0_u8; 5];
        reader
            .read_exact(&mut ready)
            .expect("responsive pane ready");
        assert_eq!(&ready, b"ready");

        let (started_sender, started_receiver) = std::sync::mpsc::channel();
        let (blocked_sender, blocked_receiver) = std::sync::mpsc::channel();
        let blocked_table = Arc::clone(&table);
        let writer_key = blocked_key.clone();
        let blocked_writer = thread::spawn(move || {
            started_sender.send(()).expect("announce blocked write");
            let result = blocked_table.write(&writer_key, &vec![b'x'; 8 * 1024 * 1024]);
            let _ = blocked_sender.send(result);
        });
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("blocked writer started");
        thread::sleep(Duration::from_millis(50));
        assert!(matches!(
            blocked_receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));

        let (responsive_sender, responsive_receiver) = std::sync::mpsc::channel();
        let responsive_table = Arc::clone(&table);
        let writer_key = responsive_key.clone();
        let responsive_writer = thread::spawn(move || {
            let _ = responsive_sender.send(responsive_table.write(&writer_key, b"R"));
        });
        let (kill_sender, kill_receiver) = std::sync::mpsc::channel();
        let kill_table = Arc::clone(&table);
        let kill_key = blocked_key.clone();
        let killer = thread::spawn(move || {
            let _ = kill_sender.send(kill_table.kill(&kill_key));
        });

        let responsive_result = responsive_receiver.recv_timeout(Duration::from_millis(500));
        let kill_result = kill_receiver.recv_timeout(Duration::from_millis(500));
        let completed_before_emergency_cleanup = responsive_result.is_ok() && kill_result.is_ok();
        if responsive_result.is_err() || kill_result.is_err() {
            let _ = super::signal_process_group(blocked_process_group);
        }

        let responsive_result = responsive_result
            .or_else(|_| responsive_receiver.recv_timeout(Duration::from_secs(2)))
            .expect("responsive pane write eventually finishes");
        let kill_result = kill_result
            .or_else(|_| kill_receiver.recv_timeout(Duration::from_secs(2)))
            .expect("blocked pane kill eventually finishes");
        let _ = blocked_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("blocked write unblocked after kill");
        blocked_writer.join().expect("blocked writer thread");
        responsive_writer.join().expect("responsive writer thread");
        killer.join().expect("killer thread");

        assert!(
            completed_before_emergency_cleanup,
            "responsiveness or kill completed only after emergency process-group cleanup"
        );
        responsive_result.expect("unrelated pane remains writable");
        kill_result.expect("kill is independent of the blocked writer");
        assert_eq!(
            String::from_utf8(read_to_end(reader))
                .expect("responsive recorder output")
                .split_whitespace()
                .collect::<String>(),
            "52"
        );
        let _ = table.kill(&responsive_key);
    }

    #[test]
    fn pane_table_write_paste_writes_brackets_then_delayed_enter() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let OpenedPane { key, mut reader } = open_shell(
            &table,
            "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 17",
        );
        let mut ready = [0_u8; 5];
        reader.read_exact(&mut ready).expect("raw recorder ready");
        assert_eq!(&ready, b"ready");

        let started = Instant::now();
        table
            .write_paste(&key, b"body", 25)
            .expect("write paste through PaneTable");

        assert!(started.elapsed() >= Duration::from_millis(25));
        assert_eq!(
            String::from_utf8(read_to_end(reader))
                .expect("UTF-8 od output")
                .split_whitespace()
                .collect::<String>(),
            "1b5b3230307e626f64791b5b3230317e0d"
        );
    }

    #[test]
    fn open_refuses_a_bare_program_name() {
        let table = PaneTable::new();
        let result = table.open(
            Path::new("/tmp"),
            &[
                "sh".to_string(),
                "-c".to_string(),
                "printf nope".to_string(),
            ],
            &HashMap::new(),
            terminal_size(24, 80),
        );

        assert!(matches!(result, Err(PaneError::ProgramNotAbsolute(_))));
        assert!(table.list().expect("list panes").is_empty());
    }

    fn fill_backlog(
        output: &std::sync::mpsc::Receiver<super::PaneOutput>,
        backlog_bytes: usize,
    ) -> (usize, u64) {
        let mut received_bytes = 0;
        let mut last_seq = 0;
        while received_bytes < backlog_bytes {
            let chunk = output
                .recv_timeout(Duration::from_secs(1))
                .expect("receive output up to backlog limit");
            assert!(chunk.seq > last_seq);
            received_bytes += chunk.bytes.len();
            last_seq = chunk.seq;
        }
        (received_bytes, last_seq)
    }

    fn assert_output_closes(output: std::sync::mpsc::Receiver<super::PaneOutput>) {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match output.recv_timeout(Duration::from_millis(20)) {
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
                Err(error) => panic!("output did not close after kill: {error}"),
            }
        }
    }

    #[test]
    fn reader_activity_resets_idle_then_silence_increases_it() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let streamed = table
            .open_streamed(
                Path::new("/tmp"),
                &shell("/bin/sleep 0.25; printf x; /bin/sleep 1000"),
                &HashMap::new(),
                terminal_size(24, 80),
                1024,
            )
            .expect("open delayed-output pane");

        thread::sleep(Duration::from_millis(150));
        let idle_before_output = table.list().expect("list before output")[0].idle_ms;
        let output = streamed
            .output
            .recv_timeout(Duration::from_secs(1))
            .expect("receive delayed output");
        assert_eq!(output.bytes, b"x");
        table.ack(&streamed.key, output.seq).expect("ack output");
        let idle_after_output = table.list().expect("list after output")[0].idle_ms;
        assert!(
            idle_after_output < idle_before_output,
            "nonempty output did not reset idle time: before={idle_before_output} after={idle_after_output}"
        );

        thread::sleep(Duration::from_millis(120));
        let idle_after_silence = table.list().expect("list after silence")[0].idle_ms;
        assert!(idle_after_silence >= idle_after_output + 80);
        table.kill(&streamed.key).expect("kill delayed-output pane");
    }

    #[test]
    fn unacked_yes_output_is_bounded_then_resumes_after_ack() {
        let _pty_guard = serial_pty_test();
        const BACKLOG_BYTES: usize = 128;

        let table = PaneTable::new();
        let streamed = table
            .open_streamed(
                Path::new("/tmp"),
                &["/usr/bin/yes".to_string()],
                &HashMap::new(),
                terminal_size(24, 80),
                BACKLOG_BYTES,
            )
            .expect("open ack-gated yes");
        let key = streamed.key;
        let output = streamed.output;

        let (received_bytes, last_seq) = fill_backlog(&output, BACKLOG_BYTES);
        assert_eq!(received_bytes, BACKLOG_BYTES);
        assert!(matches!(
            output.recv_timeout(Duration::from_millis(40)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));

        table.ack(&key, last_seq).expect("ack bounded output");
        let resumed = output
            .recv_timeout(Duration::from_secs(1))
            .expect("reader resumes after ack");
        assert_eq!(resumed.seq, last_seq + 1);
        assert!(!resumed.bytes.is_empty());

        table.kill(&key).expect("kill streamed pane");
        assert_output_closes(output);
    }

    #[test]
    fn ack_rejects_future_sequences_and_cumulative_progress_stays_bounded() {
        const BACKLOG_BYTES: usize = 12;

        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let streamed = table
            .open_streamed(
                Path::new("/tmp"),
                &shell(
                    "printf 1111; /bin/sleep 0.1; printf 2222; /bin/sleep 0.1; \
                     printf 3333; /bin/sleep 0.1; printf 4444; /bin/sleep 0.1; \
                     printf 5555; /bin/sleep 0.1; printf 6666; /bin/sleep 1000",
                ),
                &HashMap::new(),
                terminal_size(24, 80),
                BACKLOG_BYTES,
            )
            .expect("open chunked output pane");
        let key = streamed.key;
        let output = streamed.output;

        let first = output
            .recv_timeout(Duration::from_secs(1))
            .expect("first output chunk");
        let second = output
            .recv_timeout(Duration::from_secs(1))
            .expect("second output chunk");
        let third = output
            .recv_timeout(Duration::from_secs(1))
            .expect("third output chunk");
        assert_eq!(
            [first.bytes, second.bytes, third.bytes].concat(),
            b"111122223333"
        );
        assert!(matches!(
            output.recv_timeout(Duration::from_millis(150)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));

        assert!(table.ack(&key, third.seq + 1).is_err());
        assert!(matches!(
            output.recv_timeout(Duration::from_millis(150)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(matches!(
            table.ack(&PaneKey::new(&key.id, key.generation + 1), second.seq),
            Err(PaneError::NotFound(_))
        ));

        table.ack(&key, second.seq).expect("cumulative ack");
        let mut resumed_bytes = Vec::new();
        while resumed_bytes.len() < 8 {
            resumed_bytes.extend(
                output
                    .recv_timeout(Duration::from_secs(1))
                    .expect("output released by cumulative ack")
                    .bytes,
            );
        }
        assert_eq!(resumed_bytes, b"44445555");
        table.ack(&key, second.seq).expect("duplicate ack");
        table.ack(&key, first.seq).expect("older ack");
        assert!(matches!(
            output.recv_timeout(Duration::from_millis(150)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));

        table.ack(&key, third.seq).expect("partial cumulative ack");
        let sixth = output
            .recv_timeout(Duration::from_secs(1))
            .expect("sixth output chunk");
        assert_eq!(sixth.bytes, b"6666");
        table.kill(&key).expect("kill chunked output pane");
        assert_output_closes(output);
    }

    #[test]
    fn input_remains_responsive_while_output_waits_for_ack() {
        let _pty_guard = serial_pty_test();
        const BACKLOG_BYTES: usize = 64;
        const INPUT_BYTES: usize = 1 + 511 * 1024;
        const FLOW_TIMEOUT: Duration = Duration::from_secs(10);

        let table = Arc::new(PaneTable::new());
        let streamed = table
            .open_streamed(
                Path::new("/tmp"),
                &shell(
                    "/bin/stty raw -echo; printf READY; \
                     /usr/bin/yes flood | /usr/bin/head -c 65536; \
                     first=$(/usr/bin/od -An -tx1 -N 1); \
                     /bin/dd of=/dev/null bs=1024 count=511 2>/dev/null; \
                     printf 'INPUT:%s' \"$first\"; /bin/sleep 1000",
                ),
                &HashMap::new(),
                terminal_size(24, 80),
                BACKLOG_BYTES,
            )
            .expect("open raw flooding input consumer");
        let key = streamed.key;
        let output = streamed.output;
        let mut initial_output = Vec::new();
        let mut last_seq = 0;
        while initial_output.len() < BACKLOG_BYTES {
            let chunk = output
                .recv_timeout(Duration::from_secs(1))
                .expect("fill raw child output window");
            last_seq = chunk.seq;
            initial_output.extend(chunk.bytes);
        }
        assert!(initial_output.starts_with(b"READY"));
        assert!(matches!(
            output.recv_timeout(Duration::from_millis(100)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));

        let OpenedPane {
            key: responsive_key,
            reader: mut responsive_reader,
        } = open_shell(
            &table,
            "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 1",
        );
        let mut responsive_ready = [0_u8; 5];
        responsive_reader
            .read_exact(&mut responsive_ready)
            .expect("responsive raw pane ready");
        assert_eq!(&responsive_ready, b"ready");

        let (write_sender, write_receiver) = std::sync::mpsc::channel();
        let writer_table = Arc::clone(&table);
        let writer_key = key.clone();
        let writer = thread::spawn(move || {
            let mut bytes = vec![b'Z'; INPUT_BYTES];
            bytes[0] = b'Z';
            let _ = write_sender.send(writer_table.write(&writer_key, &bytes));
        });
        thread::sleep(Duration::from_millis(50));
        assert!(matches!(
            write_receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));

        let started = Instant::now();
        table
            .write(&responsive_key, b"R")
            .expect("write to unrelated pane while the flood cycle is blocked");
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(
            String::from_utf8(read_to_end(responsive_reader))
                .expect("responsive recorder output")
                .split_whitespace()
                .collect::<String>(),
            "52"
        );

        let consumer_table = Arc::clone(&table);
        let consumer_key = key.clone();
        let consumer = thread::spawn(move || {
            let mut terminal_output = initial_output;
            consumer_table
                .ack(&consumer_key, last_seq)
                .expect("release initial output window");
            let deadline = Instant::now() + FLOW_TIMEOUT;
            loop {
                let compact = String::from_utf8_lossy(&terminal_output)
                    .split_whitespace()
                    .collect::<String>();
                if compact.contains("INPUT:5a") {
                    return terminal_output;
                }
                assert!(Instant::now() < deadline, "raw child did not consume input");
                let chunk = output
                    .recv_timeout(Duration::from_secs(1))
                    .expect("receive flood output after ack");
                consumer_table
                    .ack(&consumer_key, chunk.seq)
                    .expect("ack flood output");
                terminal_output.extend(chunk.bytes);
            }
        });

        write_receiver
            .recv_timeout(FLOW_TIMEOUT)
            .expect("blocked input write resumes after output acks")
            .expect("write complete input body");
        writer.join().expect("input writer thread");
        let terminal_output = consumer.join().expect("output consumer thread");
        assert!(String::from_utf8_lossy(&terminal_output)
            .split_whitespace()
            .collect::<String>()
            .contains("INPUT:5a"));

        table.kill(&key).expect("kill streamed pane");
        table.kill(&responsive_key).expect("remove responsive pane");
    }

    #[cfg(windows)]
    mod windows {
        #[test]
        #[ignore = "macOS-first PTY contract"]
        fn open_resize_and_environment_contract() {
            panic!("implement with the Windows PTY backend");
        }

        #[test]
        #[ignore = "macOS-first PTY contract"]
        fn kill_process_group_contract() {
            panic!("implement with the Windows PTY backend");
        }

        #[test]
        #[ignore = "macOS-first PTY contract"]
        fn raw_input_contract() {
            panic!("implement with the Windows PTY backend");
        }

        #[test]
        #[ignore = "macOS-first PTY contract"]
        fn ack_gated_backpressure_contract() {
            panic!("implement with the Windows PTY backend");
        }
    }
}
