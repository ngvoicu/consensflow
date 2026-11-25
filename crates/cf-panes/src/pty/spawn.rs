//! Opening a window: its program started in a PTY of its own, with the environment it is given,
//! and put in the table.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

#[cfg(unix)]
use portable_pty::native_pty_system;
use portable_pty::{CommandBuilder, PtySize};

use cf_proto::agents::parent_session_variables;

use super::flow::{question_of, stream_output, terminal_of, ActivityReader, OutputFlow};
use super::output_end;
#[cfg(any(test, feature = "test-support"))]
use super::OpenedPane;
#[cfg(test)]
use super::TERMINAL_FAILS;
use super::{Pane, PaneEnvironment, PaneError, PaneKey, PaneTable, StreamedPane};
use crate::arbiter::OutputCount;
use crate::background::Baseline;
use crate::ended::EndedSlot;
use crate::journal::Journal;
use crate::screen::PaneScreen;

/// Rejects names that `std::process::Command` cannot represent. Keeping this
/// at the PTY boundary makes every caller refuse a malformed removal before
/// it can spawn a child.
pub(crate) fn validate_drop_env(names: &[String]) -> Result<(), PaneError> {
    for name in names {
        if name.is_empty() || name.bytes().any(|byte| byte == b'=' || byte == b'\0') {
            return Err(PaneError::InvalidEnvironmentVariableName(name.clone()));
        }
    }
    Ok(())
}

/// The name a pane's programs know their terminal by (`TERM_PROGRAM`).
const TERM_PROGRAM: &str = "ConsensFlow";

/// A launcher may have no terminal or advertise `dumb`; the pane is xterm.
/// A tool runner's inherited plain-output flags do not describe this terminal.
/// The pane is ConsensFlow's terminal, whatever terminal launched the app: a
/// program that knows no terminal by name takes it for the bare console
/// (Devin on Windows warned "Windows Console Host (conhost) has limited
/// support", 2026-10-05). Explicit pane overrides and removals still win; no
/// native theme is changed.
fn advertise_terminal(
    command: &mut CommandBuilder,
    env: &HashMap<String, String>,
    drop_env: &[String],
) {
    let set_by_frame =
        |name: &str| env.contains_key(name) || drop_env.iter().any(|dropped| dropped == name);
    if !set_by_frame("TERM_PROGRAM") {
        command.env("TERM_PROGRAM", TERM_PROGRAM);
        if !set_by_frame("TERM_PROGRAM_VERSION") {
            command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        }
    }
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

/// A pane inherits nothing of a session the host was started in
/// ([`parent_session_variables`]): an app, a `tauri dev` or a test run started from an
/// agent's shell holds its session's variables (T-971), and one started from a board's
/// window that board's (T-868); a window given them is taken for that session, or reaches
/// into it. What the frame's own `env` sets is the window's, and stays.
fn forget_parent_sessions(command: &mut CommandBuilder, env: &HashMap<String, String>) {
    for name in parent_session_variables() {
        if !env.contains_key(name) {
            command.env_remove(name);
        }
    }
}
impl PaneTable {
    /// A test's pane, under a name of the table's own and with its raw
    /// output: the app opens every pane streamed, at the identity the daemon
    /// reserved for it (`open_streamed_at_while`).
    #[cfg(any(test, feature = "test-support"))]
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

    #[cfg(any(test, feature = "test-support"))]
    fn mint_key(&self) -> PaneKey {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        PaneKey::new(format!("pane-{id}"), 1)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn open_at(
        &self,
        key: PaneKey,
        cwd: &Path,
        argv: &[String],
        env: &HashMap<String, String>,
        size: PtySize,
    ) -> Result<Box<dyn Read + Send>, PaneError> {
        self.open_at_with_drop_env(
            key,
            cwd,
            argv,
            PaneEnvironment::new(env, &[]),
            size,
            &|| true,
        )
    }

    fn open_at_with_drop_env(
        &self,
        key: PaneKey,
        cwd: &Path,
        argv: &[String],
        environment: PaneEnvironment<'_>,
        size: PtySize,
        owns: &dyn Fn() -> bool,
    ) -> Result<Box<dyn Read + Send>, PaneError> {
        let (env, drop_env) = (environment.overlay, environment.drop);
        validate_drop_env(drop_env)?;
        let program = argv.first().ok_or(PaneError::EmptyArgv)?;
        let program_path = Path::new(program);
        if !program_path.is_absolute() {
            return Err(PaneError::ProgramNotAbsolute(program_path.to_path_buf()));
        }

        #[cfg(any(test, feature = "test-support"))]
        self.wait_while_held();
        let mut panes = self.lock_panes()?;
        if self.updating.load(Ordering::Acquire) {
            return Err(PaneError::Updating);
        }
        if !owns() {
            return Err(PaneError::Fenced(key));
        }
        if panes.contains_key(&key) {
            return Err(PaneError::AlreadyOpen(key));
        }
        if self.uncancel(&key) {
            return Err(PaneError::Cancelled(key));
        }

        // A window whose tree cannot be owned does not open: closing it could not end
        // what the harness starts. On Windows the harness is started in its job.
        #[cfg(windows)]
        let job = Arc::new(crate::job_object::JobObject::new()?);
        #[cfg(unix)]
        let pty_system = native_pty_system();
        #[cfg(windows)]
        let pty_system = super::conpty::ConPty::joining(Arc::clone(&job));
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
        forget_parent_sessions(&mut command, env);
        advertise_terminal(&mut command, env, drop_env);
        // The clock is watched from before the program starts.
        #[cfg(target_os = "macos")]
        let clock = super::turn::watch().open();
        #[cfg(target_os = "macos")]
        let mut process_tree = crate::process_tree::ProcessTree::new();
        #[cfg(target_os = "macos")]
        command.env(crate::process_tree::OWNER_ENV, &process_tree.marker);
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| PaneError::Spawn {
                message: error.to_string(),
                // The spawner's own error is text with the program's path in it. The system's
                // name for why the program's file cannot be reached is its own answer about that
                // file, from the table of libuv's names.
                code: std::fs::metadata(program_path)
                    .err()
                    .as_ref()
                    .map(cf_base::file::error_code),
            })?;
        drop(pair.slave);

        #[cfg(unix)]
        let process_group_id = child.process_id().and_then(|pid| i32::try_from(pid).ok());
        #[cfg(target_os = "macos")]
        if let Some(pid) = process_group_id {
            process_tree.attach(pid);
        }
        #[cfg(not(unix))]
        let process_group_id = None;
        let last_activity = Arc::new(Mutex::new(Instant::now()));
        let counts = Arc::new(OutputCount::default());
        let reader = Box::new(ActivityReader {
            inner: reader,
            last_activity: Arc::clone(&last_activity),
            counts: Arc::clone(&counts),
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
                baseline: Baseline::default(),
                #[cfg(target_os = "macos")]
                clock,
                output_flow: None,
                journal: None,
                active_writes: Arc::new(AtomicU64::new(0)),
                alive: true,
                last_activity,
                counts,
                screen: Arc::new(PaneScreen::new(size.rows, size.cols)),
                ended: Arc::new(EndedSlot::default()),
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

    /// Opens a streamed pane at the identity the daemon reserved for it: the daemon
    /// mints pane ids, and the host owns the program living at that key.
    #[cfg(any(test, feature = "test-support"))]
    pub fn open_streamed_at(
        self: &Arc<Self>,
        key: PaneKey,
        cwd: &Path,
        argv: &[String],
        environment: PaneEnvironment<'_>,
        size: PtySize,
        backlog_bytes: usize,
    ) -> Result<StreamedPane, PaneError> {
        self.open_streamed_at_while(&|| true, key, cwd, argv, environment, size, backlog_bytes)
    }

    /// `open_streamed_at` for a window whose opener may stop owning it: `owns` is
    /// asked at the table's insertion, with the table's lock held, and a window it refuses is
    /// not opened and has no program started ([`PaneError::Fenced`]). The daemon's launch
    /// that is slow to reach the table (a loaded Mac) is refused where it would have
    /// inserted a pane of a daemon that has stopped. Since the lock is held from this look to
    /// the insertion, whoever then lists the table, a close of everything that daemon had
    /// among them, sees a window this let in, and one that comes after is refused.
    // The arguments of `open_streamed_at`, and who still owns the window.
    #[allow(clippy::too_many_arguments)]
    pub fn open_streamed_at_while(
        self: &Arc<Self>,
        owns: &dyn Fn() -> bool,
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

        let reader = self.open_at_with_drop_env(key.clone(), cwd, argv, environment, size, owns)?;
        let flow = Arc::new(OutputFlow::new(backlog_bytes));
        let journal = self
            .journals
            .load(std::sync::atomic::Ordering::Acquire)
            .then(|| Arc::new(Journal::new((size.rows, size.cols))));
        #[cfg(target_os = "macos")]
        let tree;
        let (pid, screen, ended, terminal, counts) = {
            let mut panes = self.lock_panes()?;
            let pane = panes
                .get_mut(&key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            pane.output_flow = Some(Arc::clone(&flow));
            pane.journal.clone_from(&journal);
            #[cfg(target_os = "macos")]
            {
                tree = pane.process_tree.clone();
            }
            (
                pane.child.process_id(),
                Arc::clone(&pane.screen),
                Arc::clone(&pane.ended),
                terminal_of(pane),
                Arc::clone(&pane.counts),
            )
        };
        // A window that fails here lets go of its output before it is ended: on Windows a
        // console closed while its output is held open and unread may wait for ever (`conpty`).
        #[cfg(test)]
        let terminal = if TERMINAL_FAILS.get() {
            Err(PaneError::Pty("planted: no terminal".to_owned()))
        } else {
            terminal
        };
        let terminal = match terminal {
            Ok(terminal) => terminal,
            Err(error) => return Err(self.abandon_open(&key, reader, error)),
        };
        let question = match question_of(&terminal) {
            Ok(question) => question,
            Err(error) => {
                drop(terminal);
                return Err(self.abandon_open(&key, reader, error));
            }
        };
        #[cfg(windows)]
        if let Err(error) = self.watch_exit(&key) {
            // Let go before the window is ended: a console closed while its output is held
            // open and unread may wait for ever (`conpty`).
            drop((terminal, question));
            return Err(self.abandon_open(&key, reader, error));
        }
        // Before the thread that reads: a look from here on asks the terminal.
        counts.watch(question);

        let (sender, output) = mpsc::channel();
        let output_key = key.clone();
        let (table, watched, slot) = (Arc::downgrade(self), key.clone(), Arc::clone(&ended));
        let gone = Box::new(move |at_an_end| {
            table
                .upgrade()
                .is_none_or(|table| table.program_gone(&watched, &slot, at_an_end))
        });
        let read = output_end::Output::new(reader, terminal, gone);
        let reader_counts = Arc::clone(&counts);
        std::thread::spawn(move || stream_output(output_key, read, flow, sender, reader_counts));
        Ok(StreamedPane {
            key,
            pid,
            output,
            screen,
            counts,
            journal,
            ended,
            owed: self.exits.owe(),
            #[cfg(target_os = "macos")]
            tree,
        })
    }

    /// A window that failed to open after it was in the table: its output is let go, it is
    /// ended, and what is returned is why it did not open.
    pub(super) fn abandon_open(
        &self,
        key: &PaneKey,
        reader: Box<dyn Read + Send>,
        error: PaneError,
    ) -> PaneError {
        drop(reader);
        match self.kill(key) {
            Ok(()) => error,
            Err(cleanup) => PaneError::OpenNotUndone {
                cause: Box::new(error),
                cleanup: Box::new(cleanup),
            },
        }
    }
}
