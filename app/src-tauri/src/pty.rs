use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

/// How long a pane whose output has ended waits for its program's exit to be
/// readable: a program closes its terminal before it has finished exiting,
/// and a large one takes a while to let go of its memory.
const EXIT_GRACE: Duration = Duration::from_secs(5);
/// How long ConPTY is given, once a pane's program has exited, to pass on
/// what the program printed last.
#[cfg(windows)]
const EXIT_FLUSH: Duration = Duration::from_secs(1);

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

#[cfg(test)]
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
    /// The window's program, when its process id is known.
    pub pid: Option<u32>,
    pub output: mpsc::Receiver<PaneOutput>,
}

/// The child environment carried by `pane.open`: inherit the parent, apply
/// `overlay`, then remove every validated name in `drop`.
pub struct PaneEnvironment<'a> {
    overlay: &'a HashMap<String, String>,
    drop: &'a [String],
}

impl<'a> PaneEnvironment<'a> {
    pub fn new(overlay: &'a HashMap<String, String>, drop: &'a [String]) -> Self {
        Self { overlay, drop }
    }
}

pub(crate) trait PaneInputWriter {
    fn write(&self, key: &PaneKey, bytes: &[u8]) -> Result<(), PaneError>;
}

#[derive(Debug)]
pub enum PaneError {
    EmptyArgv,
    Updating,
    UpdateBlocked,
    ProgramNotAbsolute(PathBuf),
    AlreadyOpen(PaneKey),
    NotFound(PaneKey),
    Pty(String),
    Io(io::Error),
    InvalidBacklog,
    InvalidEnvironmentVariableName(String),
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
            Self::Updating => write!(
                formatter,
                "ConsensFlow is installing an update; new panes cannot start"
            ),
            Self::UpdateBlocked => write!(
                formatter,
                "Close or suspend every open session and wait for pane cleanup before installing"
            ),
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
            Self::InvalidEnvironmentVariableName(name) => write!(
                formatter,
                "invalid environment variable name in dropEnv: {name:?}"
            ),
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

/// Rejects names that `std::process::Command` cannot represent. Keeping this
/// at the PTY boundary makes every caller refuse a malformed removal before
/// it can spawn a child.
pub fn validate_drop_env(names: &[String]) -> Result<(), PaneError> {
    for name in names {
        if name.is_empty() || name.bytes().any(|byte| byte == b'=' || byte == b'\0') {
            return Err(PaneError::InvalidEnvironmentVariableName(name.clone()));
        }
    }
    Ok(())
}

/// A launcher may have no terminal or advertise `dumb`; the pane is xterm.
/// A tool runner's inherited plain-output flags do not describe this terminal.
/// Explicit pane overrides and removals still win; no native theme is changed.
fn advertise_color_capability(
    command: &mut CommandBuilder,
    env: &HashMap<String, String>,
    drop_env: &[String],
) {
    for name in ["NO_COLOR", "FORCE_COLOR", "CLICOLOR_FORCE"] {
        if !env.contains_key(name) {
            command.env_remove(name);
        }
    }
    if !drop_env.iter().any(|name| name == "TERM")
        && !env.contains_key("TERM")
        && matches!(
            command.get_env("TERM").and_then(|value| value.to_str()),
            None | Some("" | "dumb")
        )
    {
        command.env("TERM", "xterm-256color");
    }
    if !drop_env.iter().any(|name| name == "COLORTERM")
        && !env.contains_key("COLORTERM")
        && matches!(
            command
                .get_env("COLORTERM")
                .and_then(|value| value.to_str()),
            None | Some("")
        )
    {
        command.env("COLORTERM", "truecolor");
    }
}

struct Pane {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn Child + Send + Sync>,
    process_group_id: Option<i32>,
    #[cfg(target_os = "macos")]
    process_tree: crate::process_tree::ProcessTree,
    #[cfg(windows)]
    job: crate::job_object::JobObject,
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

    /// Everything issued so far counts as read.
    fn acknowledge_all(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.unacked.clear();
        state.unacked_bytes = 0;
        self.ready.notify_all();
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        self.ready.notify_all();
    }
}

pub struct PaneTable {
    panes: Mutex<HashMap<PaneKey, Pane>>,
    #[cfg(test)]
    next_id: AtomicU64,
    updating: AtomicBool,
    teardown: Arc<Teardown>,
}

/// Panes whose child is still being torn down. A pane leaves `finishing` only
/// on positive evidence that its process is gone; anything unproven moves to
/// `unconfirmed`, which holds admission closed until ConsensFlow restarts.
#[derive(Default)]
struct Teardown {
    finishing: AtomicU64,
    unconfirmed: AtomicU64,
}

impl Teardown {
    fn begin(&self) {
        self.finishing.fetch_add(1, Ordering::AcqRel);
    }

    /// Raising `unconfirmed` before lowering `finishing` keeps the two counts
    /// from summing to zero mid-handover, so no installer slips through.
    fn resolve(&self, child_is_gone: bool) {
        if !child_is_gone {
            self.unconfirmed.fetch_add(1, Ordering::AcqRel);
        }
        self.finishing.fetch_sub(1, Ordering::AcqRel);
    }

    fn counts(&self) -> (u64, u64) {
        (
            self.finishing.load(Ordering::Acquire),
            self.unconfirmed.load(Ordering::Acquire),
        )
    }
}

/// The installer owns admission until restart, or until failure drops this guard.
pub struct UpdatePermit(Arc<PaneTable>);

impl Drop for UpdatePermit {
    fn drop(&mut self) {
        self.0.updating.store(false, Ordering::Release);
    }
}

impl PaneTable {
    pub fn new() -> Self {
        Self {
            panes: Mutex::new(HashMap::new()),
            #[cfg(test)]
            next_id: AtomicU64::new(1),
            updating: AtomicBool::new(false),
            teardown: Arc::new(Teardown::default()),
        }
    }

    pub fn begin_update(self: &Arc<Self>) -> Result<UpdatePermit, PaneError> {
        let panes = self.lock_panes()?;
        let (finishing, unconfirmed) = self.teardown.counts();
        if !panes.is_empty() || finishing != 0 || unconfirmed != 0 {
            return Err(PaneError::UpdateBlocked);
        }
        if self.updating.swap(true, Ordering::AcqRel) {
            return Err(PaneError::Updating);
        }
        Ok(UpdatePermit(Arc::clone(self)))
    }

    pub fn update_blockers(&self) -> Result<Vec<PaneKey>, PaneError> {
        let panes = self.lock_panes()?;
        let mut keys: Vec<_> = panes.keys().cloned().collect();
        let (finishing, unconfirmed) = self.teardown.counts();
        for i in 0..finishing {
            keys.push(PaneKey::new(format!("finishing-pane-cleanup-{i}"), 0));
        }
        // Named apart from the transient case: this one never clears on its own.
        for i in 0..unconfirmed {
            keys.push(PaneKey::new(format!("unconfirmed-pane-cleanup-{i}"), 0));
        }
        keys.sort();
        Ok(keys)
    }

    /// A test's pane, under a name of the table's own and with its raw
    /// output: the app opens every pane streamed, at the identity the daemon
    /// reserved for it (`open_streamed_at`).
    #[cfg(test)]
    pub fn open(
        &self,
        cwd: &Path,
        argv: &[String],
        env: &HashMap<String, String>,
        size: PtySize,
    ) -> Result<OpenedPane, PaneError> {
        let key = self.mint_key();
        let reader = self.open_at(key.clone(), cwd, argv, env, size)?;
        Ok(OpenedPane { key, reader })
    }

    #[cfg(test)]
    fn mint_key(&self) -> PaneKey {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        PaneKey::new(format!("pane-{id}"), 1)
    }

    #[cfg(test)]
    pub fn open_at(
        &self,
        key: PaneKey,
        cwd: &Path,
        argv: &[String],
        env: &HashMap<String, String>,
        size: PtySize,
    ) -> Result<Box<dyn Read + Send>, PaneError> {
        self.open_at_with_drop_env(key, cwd, argv, env, &[], size)
    }

    fn open_at_with_drop_env(
        &self,
        key: PaneKey,
        cwd: &Path,
        argv: &[String],
        env: &HashMap<String, String>,
        drop_env: &[String],
        size: PtySize,
    ) -> Result<Box<dyn Read + Send>, PaneError> {
        validate_drop_env(drop_env)?;
        let program = argv.first().ok_or(PaneError::EmptyArgv)?;
        let program_path = Path::new(program);
        if !program_path.is_absolute() {
            return Err(PaneError::ProgramNotAbsolute(program_path.to_path_buf()));
        }

        let mut panes = self.lock_panes()?;
        if self.updating.load(Ordering::Acquire) {
            return Err(PaneError::Updating);
        }
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
        for (name, value) in env {
            command.env(name, value);
        }
        // A pane inherits the app's launch environment. The frame overlays
        // role-specific values, then removes guards such as billing API keys.
        // Removal deliberately comes last so a guarded name cannot be put
        // back by the frame's `env` map.
        for name in drop_env {
            command.env_remove(name);
        }
        advertise_color_capability(&mut command, env, drop_env);
        #[cfg(target_os = "macos")]
        let mut process_tree = crate::process_tree::ProcessTree::new();
        #[cfg(target_os = "macos")]
        command.env(crate::process_tree::OWNER_ENV, &process_tree.marker);
        #[cfg(windows)]
        let job = crate::job_object::JobObject::new()?;
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| PaneError::Pty(error.to_string()))?;
        drop(pair.slave);
        // A window whose tree cannot be owned does not open: closing it could
        // not end what the harness starts.
        #[cfg(windows)]
        if let Err(error) = child
            .process_id()
            .ok_or_else(|| io::Error::other("the harness has no process id"))
            .and_then(|pid| job.assign(pid))
        {
            let _ = child.kill();
            return Err(PaneError::Io(error));
        }

        #[cfg(unix)]
        let process_group_id = child.process_id().and_then(|pid| i32::try_from(pid).ok());
        #[cfg(target_os = "macos")]
        if let Some(pid) = process_group_id {
            process_tree.attach(pid);
        }
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
                #[cfg(target_os = "macos")]
                process_tree,
                #[cfg(windows)]
                job,
                output_flow: None,
                active_writes: Arc::new(AtomicU64::new(0)),
                alive: true,
                last_activity,
            },
        );
        Ok(reader)
    }

    #[cfg(test)]
    pub fn open_streamed(
        self: &Arc<Self>,
        cwd: &Path,
        argv: &[String],
        environment: PaneEnvironment<'_>,
        size: PtySize,
        backlog_bytes: usize,
    ) -> Result<StreamedPane, PaneError> {
        self.open_streamed_at(self.mint_key(), cwd, argv, environment, size, backlog_bytes)
    }

    /// Opens a streamed pane at the app-owned identity Node already reserved.
    /// The store mints pane ids; Rust owns the process living at that key.
    pub fn open_streamed_at(
        self: &Arc<Self>,
        key: PaneKey,
        cwd: &Path,
        argv: &[String],
        environment: PaneEnvironment<'_>,
        size: PtySize,
        backlog_bytes: usize,
    ) -> Result<StreamedPane, PaneError> {
        if backlog_bytes == 0 {
            return Err(PaneError::InvalidBacklog);
        }

        let reader = self.open_at_with_drop_env(
            key.clone(),
            cwd,
            argv,
            environment.overlay,
            environment.drop,
            size,
        )?;
        let flow = Arc::new(OutputFlow::new(backlog_bytes));
        let pid = {
            let mut panes = self.lock_panes()?;
            let pane = panes
                .get_mut(&key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            pane.output_flow = Some(Arc::clone(&flow));
            pane.child.process_id()
        };
        #[cfg(windows)]
        self.watch_exit(&key)?;

        let (sender, output) = mpsc::channel();
        let output_key = key.clone();
        std::thread::spawn(move || stream_output(output_key, reader, flow, sender));
        Ok(StreamedPane { key, pid, output })
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

    /// Ends a pane and takes it out of the table. One that is gone already,
    /// killed or retired when its program ended, is closed: the caller cannot
    /// tell the two apart, and does not need to.
    pub fn kill(&self, key: &PaneKey) -> Result<(), PaneError> {
        let mut panes = self.lock_panes()?;
        let Some(pane) = panes.remove(key) else {
            return Ok(());
        };
        self.tear_down(panes, pane)
    }

    /// A pane whose program has ended leaves the table by a kill's rules:
    /// what the harness left running ends with it, its master goes and its
    /// child is reaped, and a write still in progress hands the exit to the
    /// reaper. Nothing else would take it out: the daemon closes a window
    /// that ended on its own without a `pane.kill`, and the pane kept its
    /// master, a zombie child and the update blocked until the app quit.
    ///
    /// `true` once the pane is out of the table (now, or already); `false`
    /// while its program still runs, so the pane stays for a `pane.kill`. The
    /// output ends a moment before the exit can be read, so it looks again
    /// for a while before it says so.
    pub fn retire_exited(&self, key: &PaneKey) -> Result<bool, PaneError> {
        self.retire_exited_within(key, EXIT_GRACE)
    }

    fn retire_exited_within(&self, key: &PaneKey, grace: Duration) -> Result<bool, PaneError> {
        let deadline = Instant::now() + grace;
        loop {
            let mut panes = self.lock_panes()?;
            let Some(pane) = panes.get_mut(key) else {
                return Ok(true);
            };
            if has_exited(pane)? {
                if let Some(pane) = panes.remove(key) {
                    self.tear_down(panes, pane)?;
                }
                return Ok(true);
            }
            drop(panes);
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Ends a pane just taken out of the table. Its teardown is counted before
    /// the table's lock goes, so no installer is admitted in between.
    fn tear_down(
        &self,
        panes: MutexGuard<'_, HashMap<PaneKey, Pane>>,
        mut pane: Pane,
    ) -> Result<(), PaneError> {
        self.teardown.begin();
        drop(panes);
        close_output(&pane);
        if pane.active_writes.load(Ordering::Acquire) == 0 {
            let result = terminate(&mut pane);
            self.teardown.resolve(result.is_ok());
            result
        } else {
            terminate_detached(pane, Arc::clone(&self.teardown))
        }
    }

    /// The built-in ConPTY keeps a pane's output open after its program exits
    /// (node-pty closes it itself a second after the exit), so the end of the
    /// output, which says on a Unix PTY that the window has ended, never came:
    /// the chief's `/exit` or a crash went unseen. One watcher per pane waits
    /// on the program, gives the pseudoconsole a moment to pass on what it
    /// printed last, and retires the pane; its master's drop closes the
    /// pseudoconsole, the output ends, and `pane.exit` follows as anywhere.
    /// A window whose end could not be seen does not open.
    #[cfg(windows)]
    fn watch_exit(self: &Arc<Self>, key: &PaneKey) -> Result<(), PaneError> {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
        };

        struct Program(HANDLE);
        // SAFETY: a process handle is a kernel handle, usable from any thread.
        unsafe impl Send for Program {}
        impl Drop for Program {
            fn drop(&mut self) {
                // SAFETY: the handle is ours and closed once.
                unsafe { CloseHandle(self.0) };
            }
        }

        let pid = self
            .lock_panes()?
            .get(key)
            .and_then(|pane| pane.child.process_id());
        let watched = pid
            .ok_or_else(|| io::Error::other("the harness has no process id"))
            .and_then(|pid| {
                // SAFETY: a wait-only handle; the pane's own child handle keeps
                // the pid from being reused while the pane is open.
                let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                if handle.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let program = Program(handle);
                let table = Arc::downgrade(self);
                let key = key.clone();
                std::thread::Builder::new()
                    .name("consensflow-pty-exit".to_string())
                    .spawn(move || {
                        // SAFETY: the handle stays open until `program` drops.
                        unsafe { WaitForSingleObject(program.0, INFINITE) };
                        drop(program);
                        std::thread::sleep(EXIT_FLUSH);
                        if let Some(table) = table.upgrade() {
                            let _ = table.retire_exited(&key);
                        }
                    })
                    .map(|_| ())
            });
        if let Err(error) = watched {
            let _ = self.kill(key);
            return Err(PaneError::Io(error));
        }
        Ok(())
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

    pub fn process_group_id(&self, key: &PaneKey) -> Result<i32, PaneError> {
        self.lock_panes()?
            .get(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?
            .process_group_id
            .ok_or_else(|| PaneError::Pty("the PTY has no process group".to_string()))
    }

    fn lock_panes(&self) -> Result<MutexGuard<'_, HashMap<PaneKey, Pane>>, PaneError> {
        self.panes.lock().map_err(|_| PaneError::LockPoisoned)
    }

    /// The table as a thread that panicked holding its lock leaves it: every
    /// operation on it fails from then on, a kill's included.
    #[cfg(test)]
    pub(crate) fn poison(&self) {
        std::thread::scope(|scope| {
            let poisoned = scope
                .spawn(|| {
                    let _held = self.panes.lock();
                    panic!("the pane table's lock is poisoned");
                })
                .join();
            assert!(poisoned.is_err());
        });
    }
}

impl PaneInputWriter for PaneTable {
    fn write(&self, key: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
        PaneTable::write(self, key, bytes)
    }
}

/// A paste in brackets, then its Enter as a write of its own, once
/// `before_enter` returns: a harness takes an Enter that arrives with the
/// paste into it.
pub(crate) fn write_paste_via<W: PaneInputWriter + ?Sized>(
    writer: &W,
    key: &PaneKey,
    body: &[u8],
    before_enter: impl FnOnce(),
) -> Result<(), PaneError> {
    let mut bracketed = Vec::with_capacity(12 + body.len());
    bracketed.extend_from_slice(b"\x1b[200~");
    bracketed.extend_from_slice(body);
    bracketed.extend_from_slice(b"\x1b[201~");
    writer.write(key, &bracketed)?;
    before_enter();
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

impl Default for PaneTable {
    fn default() -> Self {
        Self::new()
    }
}

/// The PTY tests' lock, one test at a time, held with a watchdog: a test that
/// hung holding it (three arbiter tests on a macOS runner, 2026-09-30) kept
/// the others waiting until the CI step's 15 minutes ran out, and nobody
/// learnt which. After 60 s, far past any PTY test's run, the watchdog names
/// the test on stderr (past the test's capture) and aborts, as the recorder's
/// ready read does.
#[cfg(test)]
pub(crate) struct SerialPtyTest {
    _lock: MutexGuard<'static, ()>,
    done: Arc<AtomicBool>,
}

#[cfg(test)]
impl Drop for SerialPtyTest {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
pub(crate) fn serial_pty_test() -> SerialPtyTest {
    use std::io::Write as _;
    static LOCK: Mutex<()> = Mutex::new(());
    let lock = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let done = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&done);
    let test = std::thread::current()
        .name()
        .unwrap_or("a PTY test")
        .to_string();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            if finished.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = std::io::stderr().write_all(
            format!("{test} has held the PTY tests' lock for 60 s: it hung\n").as_bytes(),
        );
        std::process::abort();
    });
    SerialPtyTest { _lock: lock, done }
}

/// Whether a pid is still there — signal 0 delivers nothing and only asks.
#[cfg(all(test, unix))]
pub(crate) fn process_exists(pid: i32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }

    // SAFETY: signal 0 does not deliver a signal; it only checks whether
    // the process exists and is signalable by this process.
    let result = unsafe { kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1)
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

/// `ECHILD` says the kernel has no such child left to reap, which is proof the
/// process is gone rather than a doubt. Every other error leaves its fate open.
#[cfg(unix)]
fn already_reaped(error: &io::Error) -> bool {
    const ECHILD: i32 = 10;
    error.raw_os_error() == Some(ECHILD)
}

#[cfg(not(unix))]
fn already_reaped(_error: &io::Error) -> bool {
    false
}

fn has_exited(pane: &mut Pane) -> Result<bool, PaneError> {
    match pane.child.try_wait() {
        Ok(status) => Ok(status.is_some()),
        Err(error) if already_reaped(&error) => Ok(true),
        Err(error) => Err(PaneError::Io(error)),
    }
}

fn wait_for_exit(pane: &mut Pane) -> Result<(), PaneError> {
    match pane.child.wait() {
        Ok(_) => Ok(()),
        Err(error) if already_reaped(&error) => Ok(()),
        Err(error) => Err(PaneError::Io(error)),
    }
}

fn terminate(pane: &mut Pane) -> Result<(), PaneError> {
    let child_exited = signal_for_termination(pane, false)?;

    if !child_exited {
        wait_for_exit(pane)?;
    }
    pane.alive = false;
    Ok(())
}

fn terminate_detached(mut pane: Pane, teardown: Arc<Teardown>) -> Result<(), PaneError> {
    let child_exited = match signal_for_termination(&mut pane, true) {
        Ok(exited) => exited,
        Err(error) => {
            teardown.resolve(false);
            return Err(error);
        }
    };
    if child_exited {
        pane.alive = false;
        teardown.resolve(true);
        return Ok(());
    }

    let reaper = Arc::clone(&teardown);
    if let Err(error) = std::thread::Builder::new()
        .name("consensflow-pty-reaper".to_string())
        .spawn(move || reaper.resolve(wait_for_exit(&mut pane).is_ok()))
    {
        teardown.resolve(false);
        return Err(PaneError::Io(error));
    }
    Ok(())
}

/// `detached`: the caller does not wait for the exit (a write is still in
/// progress in the pane), and a reaper confirms it later.
fn signal_for_termination(pane: &mut Pane, detached: bool) -> Result<bool, PaneError> {
    #[cfg(not(target_os = "macos"))]
    let _ = detached;
    #[cfg(target_os = "macos")]
    pane.process_tree.terminate()?;

    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut child_exited = has_exited(pane)?;

    #[cfg(unix)]
    if let Some(process_group_id) = pane.process_group_id {
        if let Err(error) = signal_process_group(process_group_id) {
            // macOS answers EPERM when a member of the group cannot take the
            // signal: here one already killed with the tree above, whose exit
            // waits on the pane's blocked write. A caller that does not wait
            // hands that exit to the reaper; one that waits keeps the check.
            #[cfg(target_os = "macos")]
            if detached && is_permission_denied(&error) {
                return Ok(child_exited);
            }
            if is_permission_denied(&error) {
                let deadline = Instant::now() + Duration::from_millis(100);
                while !child_exited && Instant::now() < deadline {
                    child_exited = has_exited(pane)?;
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

    #[cfg(windows)]
    if !child_exited {
        pane.job.terminate()?;
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

/// ConPTY helpers the Windows tests of the pane table and the arbiter share.
#[cfg(all(test, windows))]
pub(crate) mod conpty_test {
    use std::collections::HashMap;
    use std::sync::mpsc::Receiver;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use portable_pty::PtySize;

    use super::{PaneEnvironment, PaneKey, PaneOutput, PaneTable};

    pub fn system(program: &str) -> String {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        format!(r"{root}\System32\{program}")
    }

    pub fn powershell(script: &str) -> Vec<String> {
        vec![
            system(r"WindowsPowerShell\v1.0\powershell.exe"),
            "-NoProfile".into(),
            "-Command".into(),
            script.into(),
        ]
    }

    pub fn open(
        table: &Arc<PaneTable>,
        argv: &[String],
        env: &HashMap<String, String>,
        backlog: usize,
    ) -> (PaneKey, Receiver<PaneOutput>) {
        let streamed = table
            .open_streamed(
                &std::env::temp_dir(),
                argv,
                PaneEnvironment::new(env, &[]),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                backlog,
            )
            .expect("open a ConPTY pane");
        (streamed.key, streamed.output)
    }

    /// Everything the pane printed until `needle`, acknowledged as read.
    /// ConPTY wraps the child's text in its own escape sequences, and its
    /// output does not end when the child does, so a test reads for text. It
    /// also opens by asking where the cursor is and holds the child's output
    /// until a terminal answers; the page's xterm does, and so does this.
    pub fn read_until(
        table: &PaneTable,
        key: &PaneKey,
        output: &Receiver<PaneOutput>,
        needle: &str,
    ) -> String {
        // A minute: a busy runner's PowerShell has twice taken longer than 20 s
        // to print its READY (2026-10-01 and 10-02); output on time costs nothing.
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut seen = String::new();
        while !seen.contains(needle) {
            let left = deadline.saturating_duration_since(Instant::now());
            let chunk = output
                .recv_timeout(left)
                .unwrap_or_else(|error| panic!("no {needle:?} in the pane ({error}): {seen:?}"));
            table.ack(key, chunk.seq).expect("ack output");
            if chunk.bytes.windows(4).any(|bytes| bytes == b"\x1b[6n") {
                table
                    .write(key, b"\x1b[1;1R")
                    .expect("answer the cursor query");
            }
            seen.push_str(&String::from_utf8_lossy(&chunk.bytes));
        }
        seen
    }

    /// A child that says READY, reads one line and prints it back as `GOT=[…]#`.
    pub fn line_echo() -> Vec<String> {
        powershell(
            "Write-Output READY; $line = [Console]::In.ReadLine(); Write-Output \"GOT=[$line]#\"",
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    #[cfg(unix)]
    use std::io::{BufRead, BufReader, Read};
    use std::path::Path;
    use std::sync::Arc;
    #[cfg(unix)]
    use std::thread;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    use portable_pty::PtySize;

    #[cfg(unix)]
    use super::{
        process_exists, serial_pty_test, write_paste_via, OpenedPane, PaneEnvironment, PaneKey,
    };
    use super::{PaneError, PaneTable};

    fn terminal_size(rows: u16, cols: u16) -> PtySize {
        PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    #[cfg(unix)]
    fn shell(script: &str) -> Vec<String> {
        vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()]
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    }

    #[cfg(unix)]
    fn child_process_id(table: &PaneTable, key: &PaneKey) -> i32 {
        table
            .panes
            .lock()
            .unwrap()
            .get(key)
            .and_then(|pane| pane.child.process_id())
            .expect("the pane child reports a process id") as i32
    }

    #[cfg(unix)]
    #[test]
    fn a_child_reaped_outside_the_table_still_reopens_update_admission() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let pane = open_shell(&table, "exec /bin/cat");
        let pid = child_process_id(&table, &pane.key);

        // Someone else reaps the child first, so the table's own wait sees
        // ECHILD. That is proof the process is gone, not a doubt to hold on to.
        assert_eq!(unsafe { kill(pid, 9) }, 0, "signal the pane child");
        let mut status = 0;
        assert_eq!(
            unsafe { waitpid(pid, &mut status, 0) },
            pid,
            "reap the pane child outside the table"
        );

        table
            .kill(&pane.key)
            .expect("closing a pane whose child is already reaped succeeds");
        assert!(
            table.update_blockers().unwrap().is_empty(),
            "a reaped child leaves no cleanup blocker behind"
        );
        assert!(
            table.begin_update().is_ok(),
            "update admission reopens once every child is gone"
        );
    }

    #[test]
    fn an_unproven_teardown_holds_admission_closed_under_its_own_name() {
        let table = Arc::new(PaneTable::new());
        table.teardown.begin();
        table.teardown.resolve(false);

        assert!(
            matches!(table.begin_update(), Err(PaneError::UpdateBlocked)),
            "a child that never proved it stopped keeps admission closed"
        );
        assert_eq!(
            table
                .update_blockers()
                .unwrap()
                .iter()
                .map(|key| key.id.clone())
                .collect::<Vec<_>>(),
            ["unconfirmed-pane-cleanup-0"],
            "the blocker names itself so the dialog can ask for a restart"
        );
    }

    #[cfg(unix)]
    #[test]
    fn update_installation_blocks_all_pane_launches_and_failure_restores_admission() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let pane = open_shell(&table, "exec /bin/cat");
        assert!(
            table.begin_update().is_err(),
            "any open pane prevents installation"
        );
        table.kill(&pane.key).unwrap();
        let permit = table.begin_update().unwrap();
        let launched = table.open_at(
            PaneKey::new("racing-worker", 7),
            Path::new("/tmp"),
            &shell("exec /bin/cat"),
            &HashMap::new(),
            terminal_size(40, 120),
        );
        assert!(matches!(launched, Err(PaneError::Updating)));
        assert!(table.list().unwrap().is_empty());
        assert!(
            table.begin_update().is_err(),
            "only one installer may hold admission"
        );
        drop(permit); // failed install/panic: admission must reopen automatically
        let pane = open_shell(&table, "exec /bin/cat");
        table.kill(&pane.key).unwrap();
        assert!(table.begin_update().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn update_installation_and_real_spawn_share_one_atomic_boundary() {
        let _serial = serial_pty_test();
        for _ in 0..20 {
            let table = Arc::new(PaneTable::new());
            let start = Arc::new(std::sync::Barrier::new(2));
            let child_table = table.clone();
            let child_start = start.clone();
            let launch = thread::spawn(move || {
                child_start.wait();
                child_table.open(
                    Path::new("/tmp"),
                    &shell("exec /bin/cat"),
                    &HashMap::new(),
                    terminal_size(40, 120),
                )
            });
            start.wait();
            let install = table.begin_update();
            let opened = launch.join().unwrap();
            assert_ne!(
                install.is_ok(),
                opened.is_ok(),
                "exactly one side is admitted"
            );
            if let Ok(pane) = opened {
                table.kill(&pane.key).unwrap();
            }
        }
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn streamed_open_at_preserves_the_store_reserved_identity() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let key = PaneKey::new("reserved-worker", 7);
        let streamed = table
            .open_streamed_at(
                key.clone(),
                Path::new("/tmp"),
                &shell("printf reserved"),
                PaneEnvironment::new(&HashMap::new(), &[]),
                terminal_size(24, 80),
                1024,
            )
            .expect("open the store-reserved pane identity");

        assert_eq!(streamed.key, key);
        let mut bytes = Vec::<u8>::new();
        while let Ok(chunk) = streamed.output.recv_timeout(Duration::from_secs(1)) {
            bytes.extend_from_slice(&chunk.bytes);
            table
                .ack(&key, chunk.seq)
                .expect("ack reserved pane output");
        }
        assert_eq!(bytes, b"reserved");
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    /// A program that ended takes its pane out of the table as a kill would:
    /// its child reaped, its master gone and installation admitted again.
    #[cfg(unix)]
    #[test]
    fn a_pane_whose_program_ended_retires() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let OpenedPane { key, reader } = open_shell(&table, "printf done");
        let pid = child_process_id(&table, &key);

        assert_eq!(read_to_end(reader), b"done");
        assert!(table.retire_exited(&key).expect("retire the pane"));
        assert!(table.list().expect("list").is_empty());
        assert!(!process_exists(pid), "the ended child was not reaped");
        assert!(
            table.begin_update().is_ok(),
            "an ended window holds up no update"
        );
        assert!(
            table.retire_exited(&key).expect("retire again"),
            "a pane that is gone already is out of the table"
        );
    }

    /// A pane whose program still runs is never ended on its own, whatever
    /// its output did: it waits for a kill.
    #[cfg(unix)]
    #[test]
    fn a_pane_whose_program_runs_on_is_not_retired() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let OpenedPane {
            key,
            reader: _reader,
        } = open_shell(&table, "exec /bin/sleep 30");
        let pid = child_process_id(&table, &key);

        assert!(!table
            .retire_exited_within(&key, Duration::from_millis(100))
            .expect("look at the pane"));
        assert!(process_exists(pid), "a running program was ended");
        assert_eq!(table.list().expect("list").len(), 1);
        table.kill(&key).expect("kill the pane");
        assert!(table.list().expect("list").is_empty());
    }

    #[cfg(target_os = "macos")]
    fn detached_child(
        table: &PaneTable,
        parent_exits: bool,
        clear_environment: bool,
    ) -> (PaneKey, i32) {
        let script = format!(
            "import subprocess,time; p=subprocess.Popen(['/bin/sleep','60'], start_new_session=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL{}); print(p.pid, flush=True); {}",
            if clear_environment { ", env={}" } else { "" },
            if parent_exits { "" } else { "time.sleep(60)" },
        );
        let OpenedPane { key, reader } = table
            .open(
                Path::new("/tmp"),
                &["/usr/bin/python3".into(), "-c".into(), script],
                &HashMap::new(),
                terminal_size(24, 80),
            )
            .expect("launch a parent with a detached child");
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let pid = line.trim().parse::<i32>().expect("native child pid");
        assert_ne!(
            unsafe { libc::getpgid(pid) },
            table.process_group_id(&key).unwrap()
        );
        if parent_exits {
            let mut tail = Vec::new();
            reader.read_to_end(&mut tail).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while table.list().unwrap().iter().any(|p| p.alive) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
        }
        (key, pid)
    }

    #[cfg(target_os = "macos")]
    fn stopped_with_cleanup(pid: i32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let stopped = !process_exists(pid);
        // A failing regression must not leave the test's own sleeper behind.
        if !stopped {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
        stopped
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn close_stops_detached_children_but_preserves_other_panes() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let (foreign, foreign_pid) = detached_child(&table, false, false);
        let (own, own_pid) = detached_child(&table, false, false);
        table.kill(&own).unwrap();
        let own_stopped = stopped_with_cleanup(own_pid);
        let foreign_alive = process_exists(foreign_pid);
        table.kill(&foreign).unwrap();
        let foreign_stopped = stopped_with_cleanup(foreign_pid);
        assert!(own_stopped, "detached child survived pane close");
        assert!(foreign_alive, "closing a pane killed another pane's child");
        assert!(
            foreign_stopped,
            "second pane's child survived its own close"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn close_stops_reparented_children_after_the_root_exits() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let (key, pid) = detached_child(&table, true, false);
        assert!(process_exists(pid), "fixture needs a live orphan");
        table.kill(&key).unwrap();
        assert!(
            stopped_with_cleanup(pid),
            "reparented child survived pane close"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn close_stops_attached_descendants_even_with_a_cleared_environment() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let (key, pid) = detached_child(&table, false, true);
        table.kill(&key).unwrap();
        assert!(
            stopped_with_cleanup(pid),
            "owned child with empty environment survived"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn drop_stops_detached_children() {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let (_, pid) = detached_child(&table, false, false);
        drop(table);
        assert!(
            stopped_with_cleanup(pid),
            "detached child survived table drop"
        );
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn child_environment_is_inherited_then_overlaid() {
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
            .expect("open child without an overlay");
        assert_eq!(read_to_end(reader), b"inherited");

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

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn a_paste_through_the_table_writes_brackets_then_delayed_enter() {
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
        write_paste_via(&table, &key, b"body", || {
            std::thread::sleep(Duration::from_millis(25));
        })
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

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn reader_activity_resets_idle_then_silence_increases_it() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let streamed = table
            .open_streamed(
                Path::new("/tmp"),
                // A second before its output: the child starts as the pane
                // opens, and a busy runner once took over 100 ms to return
                // from opening it, so a quarter second came before the look.
                &shell("/bin/sleep 1; printf x; /bin/sleep 1000"),
                PaneEnvironment::new(&HashMap::new(), &[]),
                terminal_size(24, 80),
                1024,
            )
            .expect("open delayed-output pane");

        thread::sleep(Duration::from_millis(150));
        let idle_before_output = table.list().expect("list before output")[0].idle_ms;
        let output = streamed
            .output
            .recv_timeout(Duration::from_secs(5))
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

    #[cfg(unix)]
    #[test]
    fn unacked_yes_output_is_bounded_then_resumes_after_ack() {
        let _pty_guard = serial_pty_test();
        const BACKLOG_BYTES: usize = 128;

        let table = Arc::new(PaneTable::new());
        let streamed = table
            .open_streamed(
                Path::new("/tmp"),
                &["/usr/bin/yes".to_string()],
                PaneEnvironment::new(&HashMap::new(), &[]),
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

    #[cfg(unix)]
    #[test]
    fn ack_rejects_future_sequences_and_cumulative_progress_stays_bounded() {
        const BACKLOG_BYTES: usize = 12;

        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let streamed = table
            .open_streamed(
                Path::new("/tmp"),
                &shell(
                    "printf 1111; /bin/sleep 0.1; printf 2222; /bin/sleep 0.1; \
                     printf 3333; /bin/sleep 0.1; printf 4444; /bin/sleep 0.1; \
                     printf 5555; /bin/sleep 0.1; printf 6666; /bin/sleep 1000",
                ),
                PaneEnvironment::new(&HashMap::new(), &[]),
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

    #[cfg(unix)]
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
                PaneEnvironment::new(&HashMap::new(), &[]),
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
        use std::collections::HashMap;
        use std::sync::mpsc::RecvTimeoutError;
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        use portable_pty::PtySize;

        use super::super::conpty_test::{line_echo, open, powershell, read_until, system};
        use super::super::{serial_pty_test, PaneEnvironment, PaneTable};

        fn number_after(text: &str, marker: &str) -> u32 {
            let at = text.find(marker).expect("marker printed") + marker.len();
            text[at..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .expect("a number after the marker")
        }

        fn running(pid: u32) -> bool {
            // SAFETY: a query-only handle, closed before returning.
            unsafe {
                let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if process.is_null() {
                    return false;
                }
                let mut code = 0u32;
                let read = GetExitCodeProcess(process, &mut code);
                CloseHandle(process);
                read != 0 && code == STILL_ACTIVE as u32
            }
        }

        #[test]
        fn the_environment_and_a_resize_reach_the_child() {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let env = HashMap::from([("CF_PANE_VALUE".to_string(), "overlaid".to_string())]);
            let (key, output) = open(
                &table,
                &powershell(
                    "Write-Output \"ENV=$env:CF_PANE_VALUE\"; [Console]::In.ReadLine() | Out-Null; \
                     Write-Output \"SIZE=$([Console]::WindowWidth)#\"",
                ),
                &env,
                1 << 20,
            );
            assert!(read_until(&table, &key, &output, "ENV=overlaid").contains("ENV=overlaid"));
            table.resize(&key, 30, 100).expect("resize the pane");
            table.write(&key, b"\r").expect("go on");
            let text = read_until(&table, &key, &output, "#");
            assert_eq!(number_after(&text, "SIZE="), 100, "{text:?}");
            table.kill(&key).expect("kill the pane");
        }

        #[test]
        fn kill_ends_everything_the_harness_started() {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let ping = system("PING.EXE");
            let (key, output) = open(
                &table,
                &powershell(&format!(
                    "$p = Start-Process -PassThru -WindowStyle Hidden -FilePath '{ping}' -ArgumentList '-n','1000','127.0.0.1'; \
                     Write-Output \"PID=$($p.Id)#\"; Start-Sleep 1000"
                )),
                &HashMap::new(),
                1 << 20,
            );
            let text = read_until(&table, &key, &output, "#");
            let started = number_after(&text, "PID=");
            assert!(running(started), "the detached child is not running");

            table.kill(&key).expect("kill the pane");
            let deadline = Instant::now() + Duration::from_secs(5);
            while running(started) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                !running(started),
                "a child the harness started survived its window"
            );
        }

        /// ConPTY keeps the output open after the program ends, so without
        /// the exit watcher this pane's output never ended, and the end of a
        /// pane's output is what sends `pane.exit`.
        #[test]
        fn a_program_that_exits_on_its_own_ends_its_output_and_leaves_the_table() {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (key, output) = open(
                &table,
                &powershell("Write-Output 'BYE#'"),
                &HashMap::new(),
                1 << 20,
            );
            read_until(&table, &key, &output, "BYE#");
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match output.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    // The pane may have left the table by the time this
                    // chunk is read, and an ack then has nothing to free.
                    Ok(chunk) => {
                        let _ = table.ack(&key, chunk.seq);
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        panic!("the output went on after the program exited")
                    }
                }
            }
            assert!(
                table.list().expect("list the panes").is_empty(),
                "the ended pane stayed in the table"
            );
        }

        /// A pane names its program's process: PowerShell's own `$PID`.
        #[test]
        fn a_pane_names_its_programs_process() {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let streamed = table
                .open_streamed(
                    &std::env::temp_dir(),
                    &powershell("Write-Output \"PID=$PID#\"; Start-Sleep 1000"),
                    PaneEnvironment::new(&HashMap::new(), &[]),
                    PtySize {
                        rows: 24,
                        cols: 80,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                    1 << 20,
                )
                .expect("open a ConPTY pane");
            let text = read_until(&table, &streamed.key, &streamed.output, "#");
            assert_eq!(streamed.pid, Some(number_after(&text, "PID=")), "{text:?}");
            table.kill(&streamed.key).expect("kill the pane");
        }

        #[test]
        fn input_reaches_the_child() {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (key, output) = open(&table, &line_echo(), &HashMap::new(), 1 << 20);
            read_until(&table, &key, &output, "READY");
            table
                .write(&key, b"typed words\r")
                .expect("type into the pane");
            assert!(read_until(&table, &key, &output, "#").contains("GOT=[typed words]"));
            table.kill(&key).expect("kill the pane");
        }

        #[test]
        fn unacked_output_is_bounded_then_resumes_after_ack() {
            const BACKLOG_BYTES: usize = 128;
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (key, output) = open(
                &table,
                &[
                    system("cmd.exe"),
                    "/d".into(),
                    "/c".into(),
                    "for /L %i in (1,0,2) do @echo y".into(),
                ],
                &HashMap::new(),
                BACKLOG_BYTES,
            );
            let mut received = 0;
            let mut last_seq = 0;
            while received < BACKLOG_BYTES {
                let chunk = output
                    .recv_timeout(Duration::from_secs(10))
                    .expect("output up to the backlog");
                if chunk.bytes.windows(4).any(|bytes| bytes == b"\x1b[6n") {
                    table
                        .write(&key, b"\x1b[1;1R")
                        .expect("answer the cursor query");
                }
                received += chunk.bytes.len();
                last_seq = chunk.seq;
            }
            assert_eq!(received, BACKLOG_BYTES);
            assert!(matches!(
                output.recv_timeout(Duration::from_millis(200)),
                Err(RecvTimeoutError::Timeout)
            ));
            table.ack(&key, last_seq).expect("ack the backlog");
            let resumed = output
                .recv_timeout(Duration::from_secs(5))
                .expect("output resumes after the ack");
            assert_eq!(resumed.seq, last_seq + 1);
            table.kill(&key).expect("kill the pane");
        }
    }
}
