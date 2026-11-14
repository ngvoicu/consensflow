//! The agent of a Codex window, as the stand-in's server plays it: a thread, whose
//! rollout is Codex's own record of it, and the turns the server is asked to take.
//!
//! The rollout is a JSONL file in `sessions/YYYY/MM/DD/` of `CODEX_HOME`, where the
//! daemon reads a thread's record (none is written where `CODEX_HOME` is not set,
//! so a case that is not the daemon's leaves nothing in anyone's home). As Codex's
//! is, it is begun by the thread's first turn and not by the thread: a thread that
//! has had none has no rollout, and a window resumed on it finds nothing
//! ([`Agent::resume`]). A message is a turn: the server is handed it as
//! `turn/start`, or as `thread/queue/add` while another runs, and answers at once;
//! the turn goes on beside the connections, one at a time, and writes what Codex
//! writes of one: it started, the message, the answer, it is complete.
//!
//! A thread is one server's at a time, as Codex's is: the server that begins its
//! rollout, or resumes it, holds it (a lock in `thread-locks/` of `CODEX_HOME`, out of
//! the way of what reads the rollouts, names its process),
//! and while that process lives another server asked to resume the thread answers
//! with it read-only, as Codex answered the window a second client opened on a thread
//! another held ("open in another app", kluris, 2026-10-26: [`Agent::hold`]).
//!
//! A thread keeps the instructions it began with. A thread starts with the
//! developer instructions its start names (`developerInstructions`, which the
//! window's broker gives the TUI's main start: the role's text), or else those its
//! server is configured with (`-c developer_instructions=…`), as Codex 0.164.0 does;
//! the first turn of the thread writes them into the rollout as the developer's
//! message. A thread it resumes goes on in the rollout it has, which already holds
//! that message, and is given no other: what its resume names (`developerInstructions`)
//! or the server is configured with now is read and unused by its turns, as the live
//! Codex 0.162.1 did with a chief resumed after its role changed (2026-10-13).
//!
//! A compaction is where those instructions are read again. Codex rebuilds a compacted
//! history from the session that compacts it: the developer message of its replacement
//! history is the instructions that session was given (its start's or its resume's
//! `developerInstructions`, else its configuration), not the one the thread began with
//! (`build_compaction_replacement_history`, codex-rs 0.167.0; T-1388's rollouts, Codex
//! 0.167.0: a window resumed with no instructions compacted to a history with no role,
//! one resumed with the role compacted to a history with it once). So the stand-in's
//! `compacted` record holds the developer message of the instructions its session has,
//! and after it that is what the thread holds. A message with the line `Say your role`
//! is answered from what the thread holds now: the sentence that names its ConsensFlow
//! role and the command it asks the chief with, or that it has none.
//!
//! What a turn does is what the stand-in agent's does: the `SLEEP <seconds>` line of
//! its message is how long it works, `Reply with exactly: X` is its answer, and
//! anything else is acknowledged; a line `CF <words> :: <text>` is a call of `cf`, answered with
//! what it printed. A turn at work stops when the TUI's Escape asks (`turn/interrupt`), as
//! Codex's does at one Escape: it ends `turn_aborted`, with no answer. A message whose line begins `COMPACT <seconds>` has
//! the thread compacted first, as Codex 0.165 compacts a long thread at the start of a
//! turn: the turn begins, nothing more is written for that long, then the compaction
//! and only after it the message (T-901, the field's rollout of 2026-10-28). A message that says `QUOTA-OUT` is refused for
//! the usage limit, as the stand-in agent's is (`CF_TEST_QUOTA_OUT` names the
//! member whose sessions are refused, while `CF_TEST_QUOTA_FILE`, when it is set,
//! is there): the turn works as long as it says and ends as Codex 0.163 ends one
//! its limit cut, with `usage_limit_exceeded` and no answer. One that says
//! `MODEL-BUSY` works as long as it says and fails as a real rollout's turn did when
//! its model was at capacity: `server_overloaded`, with the error's words and no
//! answer, a refusal for a moment that the task waits out (T-1112). One that says
//! `TURN-FAILS` fails the same way for a reason no wait helps (`bad_request`), which
//! fails its task. The server tells every connection a turn's start and its end
//! (`turn/started`, `turn/completed`), as Codex does, but for the first
//! `CF_TEST_CODEX_DROP_COMPLETED` ends, which it leaves unsaid, as a notice lost; a read
//! of the thread (`thread/read`) says whether a turn runs. The thread is the same in every run unless
//! `CF_TEST_CODEX_THREADS` says `unique`, which a window the daemon opens needs
//! (two of them in one `CODEX_HOME` would share a rollout). A read says of any thread it has
//! what Codex says of a TUI's main thread, or with `CF_TEST_CODEX_LINEAGE=sub-agent` of a
//! sub-agent's. The servers Codex opens
//! with its window are `CF_TEST_CODEX_SERVERS` of them, and the first starts
//! `CF_TEST_CODEX_WORKERS` processes of its own when the first turn's tools are
//! called ([`crate::servers`]).

use std::fs::{self};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{Identity, Machine, ThisMachine};
use cf_e2e::process::own_var;
use cf_e2e::{files, pattern};
use regex::Regex;
use serde_json::{json, Value};

use crate::servers::Plugins;

/// Says the turn `turn` completed once it is over, however it ends, as Codex does.
struct Completed<'a> {
    agent: &'a Agent,
    turn: &'a str,
}

impl Drop for Completed<'_> {
    fn drop(&mut self) {
        self.agent.running.store(false, Ordering::Relaxed);
        let unsaid = &self.agent.unsaid;
        if unsaid
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(1)
            })
            .is_ok()
        {
            return;
        }
        self.agent.notify("turn/completed", self.turn);
    }
}

/// The thread a run has, and the TUI sees, unless it is told to have one of its own.
pub const THREAD: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";

/// How long the agent thinks before its tools are called: past the daemon's pass
/// (a second), which is how late it may say a turn has begun.
const THINKING: Duration = Duration::from_secs(4);

/// The agent of one window: its thread, its rollout and the servers it opened.
pub struct Agent {
    /// The thread it is on: the one it starts, or the one a window resumed.
    thread: Mutex<String>,
    /// The Codex home its rollouts are in; none where `CODEX_HOME` is not set.
    home: Option<PathBuf>,
    /// The rollout of its thread, once it has one: its first turn begins it.
    rollout: Mutex<Option<PathBuf>>,
    /// The developer instructions the server is configured with, which a thread it
    /// starts begins with where its start names none.
    configured: Option<String>,
    /// The developer instructions of this server's session of the thread: those its
    /// start or its resume names, or else those the server is configured with. A
    /// thread's first turn writes them, and a compaction rebuilds the thread's developer
    /// message from them; no turn of a resumed thread writes them.
    developer: Mutex<Option<String>>,
    /// The developer message the thread's history holds now, which is what its model
    /// reads: the one its first turn wrote, or the one its latest compaction rebuilt.
    holds: Mutex<Option<String>>,
    /// Whether the thread has its developer message in the rollout already: a thread
    /// writes it once, with its first turn, and a resumed one has it from before.
    instructed: AtomicBool,
    plugins: Plugins,
    /// One turn at a time, in the order the messages came: what is queued waits.
    turns: Mutex<()>,
    /// Whether a turn has begun: the first of them is the one that calls the tools.
    begun: AtomicBool,
    /// The records the rollout holds, to number the next.
    records: AtomicU64,
    /// The connections that hear what Codex says of its thread unasked: a turn's start
    /// and its end (`turn/started`, `turn/completed`), which the broker keeps the thread's
    /// idle by.
    listeners: Mutex<Vec<Sender<Value>>>,
    /// How many turn ends are still to go unsaid (`CF_TEST_CODEX_DROP_COMPLETED`): a
    /// `turn/completed` the broker never hears, as one lost.
    unsaid: AtomicU64,
    /// Whether a turn runs now, which a read of the thread says (`thread/read`).
    running: AtomicBool,
    /// Whether the turn at work was asked to stop (`turn/interrupt`, the TUI's Escape).
    interrupted: AtomicBool,
    /// The variables the thread's start gave its commands (`shell_environment_policy.set.*` of
    /// its `config`: ConsensFlow's URL, token and the rest), which Codex's memory job inherits
    /// with the rest of the config it clones.
    window_env: Mutex<Vec<(String, String)>>,
    /// The `cf` command Codex's memory job runs (`CF_TEST_CODEX_MEMORY_JOB`, the words of a
    /// `CF` line) when the backend is `memory-job`; none otherwise.
    memory_job: Option<Vec<String>>,
    /// Whether the job has run: it runs once, after the first turn.
    job_ran: AtomicBool,
}

/// What `lock` holds, whether or not a thread that held it panicked.
fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Agent {
    /// The agent a run is told to be by its environment, and `configured`, the
    /// developer instructions its server was given (none where it was given none).
    /// The servers its Codex opens with the window start now.
    pub fn start(configured: Option<String>) -> Result<Arc<Self>, String> {
        let number = |name: &str| {
            own_var(name)
                .and_then(|text| text.parse::<usize>().ok())
                .unwrap_or(0)
        };
        let thread = if own_var("CF_TEST_CODEX_THREADS").as_deref() == Some("unique") {
            uuid::Uuid::new_v4().to_string()
        } else {
            THREAD.to_owned()
        };
        let plugins = Plugins::open(
            number("CF_TEST_CODEX_SERVERS"),
            number("CF_TEST_CODEX_WORKERS"),
        )?;
        Ok(Arc::new(Self::on(
            thread,
            own_var("CODEX_HOME").map(PathBuf::from),
            configured,
            plugins,
        )))
    }

    /// The agent of a server on `thread`, its rollouts in `home` where there is one.
    fn on(
        thread: String,
        home: Option<PathBuf>,
        configured: Option<String>,
        plugins: Plugins,
    ) -> Self {
        Self {
            thread: Mutex::new(thread),
            home,
            rollout: Mutex::new(None),
            developer: Mutex::new(configured.clone()),
            holds: Mutex::new(None),
            configured,
            instructed: AtomicBool::new(false),
            plugins,
            turns: Mutex::new(()),
            begun: AtomicBool::new(false),
            records: AtomicU64::new(0),
            listeners: Mutex::new(Vec::new()),
            unsaid: AtomicU64::new(
                own_var("CF_TEST_CODEX_DROP_COMPLETED")
                    .and_then(|text| text.parse().ok())
                    .unwrap_or(0),
            ),
            running: AtomicBool::new(false),
            interrupted: AtomicBool::new(false),
            window_env: Mutex::new(Vec::new()),
            memory_job: memory_job_command(),
            job_ran: AtomicBool::new(false),
        }
    }

    /// The thread the server is on.
    pub fn thread(&self) -> String {
        held(&self.thread).clone()
    }

    /// The server starts its thread, as `params` (those of `thread/start`) ask: with
    /// the developer instructions they name, or those the server is configured with.
    pub fn started(&self, params: &Value) {
        *held(&self.developer) = self.instructions_of(params);
        *held(&self.window_env) = params["config"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(key, value)| {
                Some((
                    key.strip_prefix(WINDOW_ENV)?.to_owned(),
                    value.as_str()?.to_owned(),
                ))
            })
            .collect();
    }

    /// The server is asked to resume `thread`: it goes on with it where the Codex
    /// home holds its rollout, which only a thread that had a turn has. It goes on
    /// in that rollout, which keeps the developer message of the thread's first turn
    /// whatever the server is configured with now. Whether it does.
    pub fn resume(&self, thread: &str) -> bool {
        let Some(found) = self
            .home
            .as_ref()
            .and_then(|home| find_rollout(&home.join("sessions"), thread))
        else {
            return false;
        };
        // What the rollout holds is what its next record is numbered after, and
        // whether the thread began with a developer's message.
        let text = fs::read_to_string(&found).unwrap_or_default();
        self.records.store(
            u64::try_from(text.lines().count()).unwrap_or_default(),
            Ordering::Relaxed,
        );
        self.instructed
            .store(has_developer_message(&text), Ordering::Relaxed);
        *held(&self.holds) = developer_held(&text);
        thread.clone_into(&mut held(&self.thread));
        *held(&self.rollout) = Some(found);
        true
    }

    /// The developer instructions a start's or a resume's `params` give the session: those
    /// they name, or those the server is configured with.
    fn instructions_of(&self, params: &Value) -> Option<String> {
        params["developerInstructions"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| self.configured.clone())
    }

    /// The session of the thread a resume went on with has the developer instructions its
    /// `params` (those of `thread/resume`) give it: no turn writes them, and the thread's
    /// next compaction rebuilds its developer message from them.
    pub fn resumed_with(&self, params: &Value) {
        *held(&self.developer) = self.instructions_of(params);
    }

    /// The server takes `text` as a turn, which goes on beside it.
    pub fn take(self: &Arc<Self>, text: &str) {
        let (agent, text) = (Arc::clone(self), text.to_owned());
        thread::spawn(move || {
            agent.turn(&text);
            agent.consolidate_memories();
        });
    }

    /// Codex's memory job, as the server runs it after a turn (`memory-job`): a thread of its
    /// own, in the memories folder of the Codex home, that has the window's thread's config
    /// (its variables here, its role in `developer`) and runs the `cf` command the case gave
    /// it as the window's participant. It runs once, after the first turn; what it ran and
    /// what `cf` answered are in the log (`{"run":"memory-job"}`), the names of its variables
    /// and never a value.
    fn consolidate_memories(&self) {
        let (Some(words), Some(home)) = (self.memory_job.as_ref(), self.home.as_ref()) else {
            return;
        };
        if self.job_ran.swap(true, Ordering::Relaxed) {
            return;
        }
        let folder = home.join("memories");
        let _ = cf_e2e::door::make_dir(&folder);
        let thread = uuid::Uuid::new_v4().to_string();
        let window_env = held(&self.window_env).clone();
        let ran = cf_e2e::process::Run::new(cf_e2e::process::as_a_shell_finds("cf"))
            .args(words)
            .inheriting_env()
            .vars(window_env.iter().map(|(name, value)| (name, value)))
            .var("CODEX_THREAD_ID", &thread)
            .cwd(&folder)
            .unlimited()
            .run();
        let (code, output) = match ran {
            Ok(ran) => (ran.code, ran.output().trim().to_owned()),
            Err(failed) => (None, failed.to_string()),
        };
        super::record(json!({
            "run": "memory-job",
            "thread": thread,
            "cwd": folder,
            "developer": held(&self.developer).clone(),
            "variables": window_env.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            "code": code,
            "output": output,
        }));
    }

    /// The turn at work stops, as Codex's does at its TUI's Escape (`turn/interrupt`): it ends
    /// aborted, with no answer, and what was queued behind it goes on. Nothing where no turn
    /// runs.
    pub fn interrupt(&self) {
        if self.running.load(Ordering::Relaxed) {
            self.interrupted.store(true, Ordering::Relaxed);
        }
    }

    /// A connection hears, from now on, what Codex says of the thread unasked.
    pub fn listen(&self) -> Receiver<Value> {
        let (said, heard) = mpsc::channel();
        held(&self.listeners).push(said);
        heard
    }

    /// The thread as a read of it says it (`thread/read`'s `thread`): its id, whether a
    /// turn runs, and where it comes from, as Codex 0.167.0 says of a thread its TUI began
    /// on an app-server (`vscode`, no parent, the user's; T-1414), which the broker gives
    /// the window's role to at a resume.
    pub fn read(&self) -> Value {
        let status = if self.running.load(Ordering::Relaxed) {
            json!({ "type": "active", "activeFlags": [] })
        } else {
            json!({ "type": "idle" })
        };
        top_level(&self.thread(), status)
    }

    /// A read of `asked` (`thread/read`'s `threadId`, none for its own), as Codex answers it
    /// for any thread it keeps: its own thread as [`Agent::read`] says it, another it has a
    /// rollout of as idle, and none for a thread it has not (Codex's `thread not loaded`).
    pub fn read_of(&self, asked: Option<&str>) -> Option<Value> {
        let own = self.thread();
        match asked {
            None => Some(self.read()),
            Some(asked) if asked.eq_ignore_ascii_case(&own) => Some(self.read()),
            Some(asked) => self
                .home
                .as_ref()
                .and_then(|home| find_rollout(&home.join("sessions"), asked))
                .map(|_| top_level(asked, json!({ "type": "idle" }))),
        }
    }

    /// Tells every connection that listens `method` of the thread, as Codex notifies;
    /// one that has gone is told no more.
    fn notify(&self, method: &str, turn: &str) {
        self.notify_of(method, turn, None);
    }

    /// [`Agent::notify`], with the words the turn began on where it is said so: the stand-in's
    /// TUI draws them as Codex draws a message of the user's (`CF_TEST_CODEX_SCREEN`).
    fn notify_of(&self, method: &str, turn: &str, input: Option<&str>) {
        let mut params = json!({ "threadId": self.thread(), "turn": { "id": turn } });
        if let Some(input) = input {
            params["input"] = json!(input);
        }
        let notice = json!({ "method": method, "params": params });
        held(&self.listeners).retain(|listener| listener.send(notice.clone()).is_ok());
    }

    /// Whether this server holds its thread, which it takes where no other server that
    /// still runs does: the thread's lock names the one that holds it, by its process
    /// and when that began. A thread with no rollout yet is nobody's.
    pub fn hold(&self) -> bool {
        let Some(home) = self.home.as_ref().filter(|_| held(&self.rollout).is_some()) else {
            return true;
        };
        let locks = home.join("thread-locks");
        let _ = cf_e2e::door::make_dir(&locks);
        let lock = locks.join(self.thread());
        let ours = ThisMachine.identity(std::process::id()).ok().flatten();
        let holder = fs::read_to_string(&lock).ok().and_then(|text| {
            let (pid, started) = text.trim().split_once(' ')?;
            Some(Identity {
                pid: pid.parse().ok()?,
                started: started.parse().ok()?,
            })
        });
        if let Some(holder) = holder.filter(|holder| Some(*holder) != ours) {
            if ThisMachine.identity(holder.pid).ok().flatten() == Some(holder) {
                return false;
            }
        }
        if let Some(ours) = ours {
            let _ = cf_e2e::door::write(&lock, format!("{} {}\n", ours.pid, ours.started));
        }
        true
    }

    /// The thread's first turn begins its rollout, as Codex's does with what it ran
    /// in: a thread that has had none has no rollout to resume.
    fn begin_rollout(&self) {
        let thread = self.thread();
        {
            let mut rollout = held(&self.rollout);
            let Some(home) = self.home.as_ref().filter(|_| rollout.is_none()) else {
                return;
            };
            let path = rollout_of(home, &thread);
            if let Some(folder) = path.parent() {
                let _ = cf_e2e::door::make_dir(folder);
            }
            *rollout = Some(path);
        }
        self.hold();
        self.write(
            "session_meta",
            json!({ "session_id": thread, "id": thread, "originator": "codex-tui" }),
        );
    }

    /// A turn: it began with the message, works as long as the message says and ends
    /// with its answer.
    fn turn(&self, text: &str) {
        let _one_at_a_time = held(&self.turns);
        self.begin_rollout();
        let thread = self.thread();
        let began = Instant::now();
        let turn = uuid::Uuid::new_v4().to_string();
        self.running.store(true, Ordering::Relaxed);
        self.notify_of("turn/started", &turn, Some(text));
        let _completed = Completed {
            agent: self,
            turn: &turn,
        };
        let (message, answer) = (item_id(), item_id());
        self.write(
            "event_msg",
            json!({
                "type": "task_started",
                "turn_id": turn,
                "started_at": jiff::Timestamp::now().as_second(),
                "model_context_window": 258_400,
                "collaboration_mode_kind": "default",
            }),
        );
        if let Some(compacting) = compacts_for(text) {
            thread::sleep(compacting);
            // The history is rebuilt from the session's own instructions, which a
            // compaction installs as the thread's context: no first turn writes them after.
            let instructions = held(&self.developer).clone();
            let replacement: Vec<Value> = instructions
                .iter()
                .map(|instructions| developer_message(instructions))
                .chain([json!({ "type": "compaction", "encrypted_content": "stand-in" })])
                .collect();
            self.write(
                "compacted",
                json!({ "message": "", "replacement_history": replacement }),
            );
            self.instructed.store(true, Ordering::Relaxed);
            *held(&self.holds) = instructions;
        }
        // The first turn of a thread writes what the thread begins with: the
        // developer's instructions, which no later turn, and no resume, writes again.
        if !self.instructed.swap(true, Ordering::Relaxed) {
            if let Some(instructions) = held(&self.developer).clone() {
                self.write("response_item", developer_message(&instructions));
                *held(&self.holds) = Some(instructions);
            }
        }
        self.write(
            "response_item",
            json!({
                "type": "message",
                "id": message,
                "role": "user",
                "content": [{ "type": "input_text", "text": text }],
                "internal_chat_message_metadata_passthrough": { "turn_id": turn },
            }),
        );
        self.write(
            "event_msg",
            json!({
                "type": "item_completed",
                "thread_id": thread,
                "turn_id": turn,
                "item": {
                    "type": "UserMessage",
                    "id": uuid::Uuid::new_v4().to_string(),
                    "content": [{ "type": "text", "text": text, "text_elements": [] }],
                },
            }),
        );
        // The agent thinks, and then calls its tools: what a server starts when one
        // is called begins well after the daemon marked where the turn began.
        let first = !self.begun.swap(true, Ordering::Relaxed);
        if first && !self.plugins.is_empty() {
            thread::sleep(THINKING);
            self.plugins.call_a_tool();
        }
        let (work, said) = what_it_says(text);
        let said = if text.contains(SAY_YOUR_ROLE) {
            role_said(held(&self.holds).as_deref())
        } else {
            said
        };
        self.interrupted.store(false, Ordering::Relaxed);
        while began.elapsed() < work && !self.interrupted.load(Ordering::Relaxed) {
            thread::sleep(work.saturating_sub(began.elapsed()).min(INTERRUPT_LOOK));
        }
        // Stopped at its TUI's Escape: Codex ends the turn aborted, and says nothing more of it.
        if self.interrupted.swap(false, Ordering::Relaxed) {
            self.write(
                "event_msg",
                json!({ "type": "turn_aborted", "turn_id": turn, "reason": "interrupted" }),
            );
            return;
        }
        // A line `CF <words> :: <text>` is a call of `cf` by the agent's shell tool, answered
        // with what it printed, as the other stand-ins answer theirs (T-1207).
        let said = cf_in(text).map_or(said, |words| run_cf(&words));
        if refused(text) {
            self.write(
                "event_msg",
                json!({
                    "type": "task_complete",
                    "turn_id": turn,
                    "last_agent_message": null,
                    "error": {
                        "message": "You've hit your usage limit. Try again later.",
                        "codex_error_info": "usage_limit_exceeded",
                    },
                    "completed_at": jiff::Timestamp::now().as_second(),
                }),
            );
            return;
        }
        if text.contains("MODEL-BUSY") {
            self.write(
                "event_msg",
                json!({
                    "type": "task_complete",
                    "turn_id": turn,
                    "last_agent_message": null,
                    "error": {
                        "message": "The stand-in model is busy. Pick another model.",
                        "codex_error_info": "server_overloaded",
                    },
                    "completed_at": jiff::Timestamp::now().as_second(),
                }),
            );
            return;
        }
        // The turn Codex's safety filter refused, as the rollouts of kluris T-158 and T-163
        // recorded it (2026-10-27, Codex 0.164.0): the error's words and its kind.
        if text.contains("TURN-FILTERED") {
            self.write(
                "event_msg",
                json!({
                    "type": "task_complete",
                    "turn_id": turn,
                    "last_agent_message": null,
                    "error": {
                        "message": "This content was flagged for possible cybersecurity risk. If this seems wrong, try rephrasing your request. If you\u{2019}re doing authorized security work that requires more cyber permissive safeguards, apply for Daybreak access via https://platform.openai.com/settings/organization/status-and-access before retrying.",
                        "codex_error_info": "cyber_policy",
                    },
                    "completed_at": jiff::Timestamp::now().as_second(),
                }),
            );
            return;
        }
        if text.contains("TURN-FAILS") {
            self.write(
                "event_msg",
                json!({
                    "type": "task_complete",
                    "turn_id": turn,
                    "last_agent_message": null,
                    "error": {
                        "message": "The stand-in refused the request.",
                        "codex_error_info": "bad_request",
                    },
                    "completed_at": jiff::Timestamp::now().as_second(),
                }),
            );
            return;
        }
        self.write(
            "event_msg",
            json!({
                "type": "item_completed",
                "thread_id": thread,
                "turn_id": turn,
                "item": {
                    "type": "AgentMessage",
                    "id": answer,
                    "content": [{ "type": "Text", "text": said }],
                    "phase": "final_answer",
                },
            }),
        );
        self.write(
            "response_item",
            json!({
                "type": "message",
                "id": answer,
                "role": "assistant",
                "content": [{ "type": "output_text", "text": said }],
                "phase": "final_answer",
                "internal_chat_message_metadata_passthrough": { "turn_id": turn },
            }),
        );
        self.write(
            "event_msg",
            json!({
                "type": "task_complete",
                "turn_id": turn,
                "last_agent_message": said,
                "completed_at": jiff::Timestamp::now().as_second(),
            }),
        );
    }

    /// A record of `kind` to the rollout, whole in one write, so that a reader that
    /// follows the file never sees half of one.
    fn write(&self, kind: &str, payload: Value) {
        let Some(path) = held(&self.rollout).clone() else {
            return;
        };
        let record = json!({
            "timestamp": jiff::Timestamp::now().strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            "ordinal": self.records.fetch_add(1, Ordering::Relaxed),
            "type": kind,
            "payload": payload,
        });
        let line = format!("{record}\n");
        let _ = cf_e2e::door::append(&path)
            .map_err(std::io::Error::from)
            .and_then(|mut file| file.write_all(line.as_bytes()));
    }
}

/// What a thread's `config` names a variable of its commands with.
const WINDOW_ENV: &str = "shell_environment_policy.set.";

/// The words of the `cf` command Codex's memory job runs: the server runs it when its backend is
/// `memory-job`, and `CF_TEST_CODEX_MEMORY_JOB` names it as a `CF` line does
/// (`task add --tier standard :: the brief`).
fn memory_job_command() -> Option<Vec<String>> {
    if own_var("CF_TEST_CODEX_BACKEND").as_deref() != Some("memory-job") {
        return None;
    }
    cf_in(&format!("CF {}", own_var("CF_TEST_CODEX_MEMORY_JOB")?))
}

/// Whether the usage limit refuses this window the message `text`: it says
/// `QUOTA-OUT`, the window is a session of the member `CF_TEST_QUOTA_OUT` names,
/// and the account is the refused one (`CF_TEST_QUOTA_FILE`, when it is set, is
/// still there).
fn refused(text: &str) -> bool {
    let member = own_var("CF_TEST_QUOTA_OUT").unwrap_or_default();
    let participant = own_var("CONSENSFLOW_PARTICIPANT").unwrap_or_default();
    let account = own_var("CF_TEST_QUOTA_FILE").is_none_or(|file| Path::new(&file).exists());
    text.contains("QUOTA-OUT")
        && !member.is_empty()
        && account
        && (participant == member || participant.starts_with(&format!("{member}-")))
}

/// The id of an item of the model's, as Codex numbers them.
fn item_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4())
}

/// The rollout of `thread` below `sessions`, Codex's folder of them: the first file
/// named as Codex names one, ending with the thread, as the daemon finds one.
fn find_rollout(sessions: &Path, thread: &str) -> Option<PathBuf> {
    let ends = format!("-{thread}.jsonl");
    files::find(sessions, &|name| name.ends_with(&ends))
}

/// The line of a message that asks the agent what its role is, which it answers from the
/// developer message its thread holds ([`role_said`]).
const SAY_YOUR_ROLE: &str = "Say your role";

/// A developer's message of `text`, as a rollout and a replacement history hold one.
fn developer_message(text: &str) -> Value {
    json!({
        "type": "message",
        "id": item_id(),
        "role": "developer",
        "content": [{ "type": "input_text", "text": text }],
    })
}

/// What an agent whose thread holds the developer message `holds` says when asked its role:
/// the sentence that names its ConsensFlow role and the command it asks the chief with, each
/// as its instructions say them, or that they say none, as Codex's model answered with no
/// role in its history (T-1388: "No command is specified for asking the chief a question.").
fn role_said(holds: Option<&str>) -> String {
    static ROLE: OnceLock<Regex> = OnceLock::new();
    static ASK: OnceLock<Regex> = OnceLock::new();
    let holds = holds.unwrap_or_default();
    let role = pattern::once(&ROLE, r"Your ConsensFlow role is [\w-]+")
        .find(holds)
        .map_or("No ConsensFlow role is given", |found| found.as_str());
    let ask = pattern::once(&ASK, r#"(\S*cf(?:\.exe)?) ask ""#)
        .captures(holds)
        .map_or_else(
            || "No command is specified for asking the chief a question.".to_owned(),
            |found| format!("Ask the chief with: {} ask", &found[1]),
        );
    format!("{role}. {ask}")
}

/// The developer message the rollout `text` has its thread hold now: the one its latest
/// compaction rebuilt (none where it rebuilt none), or else the last one written.
fn developer_held(text: &str) -> Option<String> {
    let said = |message: &Value| message["content"][0]["text"].as_str().map(str::to_owned);
    let mut holds = None;
    for record in text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
    {
        if record["type"] == "compacted" {
            holds = record["payload"]["replacement_history"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|item| item["role"] == "developer")
                .find_map(said);
        } else if record["type"] == "response_item"
            && record["payload"]["type"] == "message"
            && record["payload"]["role"] == "developer"
        {
            holds = said(&record["payload"]);
        }
    }
    holds
}

/// Whether the rollout `text` has given its thread its context: a developer's message, what
/// a thread begins with, or a compaction, which installs the context in its place.
fn has_developer_message(text: &str) -> bool {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|record| {
            record["type"] == "compacted"
                || record["type"] == "response_item"
                    && record["payload"]["type"] == "message"
                    && record["payload"]["role"] == "developer"
        })
}

/// Where Codex keeps the rollout of `thread`, begun now, in the Codex home `home`.
fn rollout_of(home: &Path, thread: &str) -> PathBuf {
    let now = jiff::Timestamp::now();
    home.join("sessions")
        .join(now.strftime("%Y/%m/%d").to_string())
        .join(format!(
            "rollout-{}-{thread}.jsonl",
            now.strftime("%Y-%m-%dT%H-%M-%S")
        ))
}

/// How long the thread is compacted for before the message `text` is taken, where it
/// says: its line `COMPACT <seconds> <words>`.
fn compacts_for(text: &str) -> Option<Duration> {
    static COMPACT: OnceLock<Regex> = OnceLock::new();
    pattern::once(&COMPACT, r"(?m)^COMPACT (\d+) ")
        .captures(text)
        .and_then(|compact| compact[1].parse().ok())
        .map(Duration::from_secs)
}

/// How often a turn at work looks whether it was asked to stop.
const INTERRUPT_LOOK: Duration = Duration::from_millis(50);

/// The words of the message's line `CF <words> :: <text>` (after a `SLEEP <seconds>` too): each
/// word, and the text, spaces and all, as one last word, as the other stand-ins read theirs.
fn cf_in(text: &str) -> Option<Vec<String>> {
    static SLEEP: OnceLock<Regex> = OnceLock::new();
    text.lines().find_map(|line| {
        let line = pattern::once(&SLEEP, r"^(?:SLEEP \d+ )+").replace(line, "");
        let rest = line.strip_prefix("CF ")?;
        let mut parts = rest.split(" :: ");
        let mut words: Vec<String> = parts
            .next()
            .unwrap_or_default()
            .split(' ')
            .map(str::to_owned)
            .collect();
        let said: Vec<&str> = parts.collect();
        if !said.is_empty() {
            words.push(said.join(" :: "));
        }
        Some(words)
    })
}

/// `cf` as an agent runs it: the one first on the window's `PATH`, with the window's own
/// environment. What it printed and said, trimmed.
fn run_cf(words: &[String]) -> String {
    match cf_e2e::process::Run::new(cf_e2e::process::as_a_shell_finds("cf"))
        .args(words)
        .inheriting_env()
        .unlimited()
        .run()
    {
        Ok(ran) => ran.output().trim().to_owned(),
        Err(failed) => failed.to_string(),
    }
}

/// How long the message `text` has the agent work, and what it says at the end.
fn what_it_says(text: &str) -> (Duration, String) {
    static SLEEP: OnceLock<Regex> = OnceLock::new();
    static EXACT: OnceLock<Regex> = OnceLock::new();
    static COMPACT: OnceLock<Regex> = OnceLock::new();

    if let Some(compact) = pattern::once(&COMPACT, r"(?m)^COMPACT \d+ (.+)$").captures(text) {
        return what_it_says(&compact[1]);
    }
    if let Some(slow) = pattern::once(&SLEEP, r"(?m)^SLEEP (\d+) (.+)$").captures(text) {
        let seconds = slow[1].parse().unwrap_or(0);
        let (_, said) = what_it_says(&slow[2]);
        return (Duration::from_secs(seconds), said);
    }
    let said = pattern::once(&EXACT, r"Reply with exactly: (\S+)")
        .captures(text)
        .map_or_else(
            || format!("noted: {}", text.split('\n').next().unwrap_or_default()),
            |exact| exact[1].to_owned(),
        );
    (Duration::ZERO, said)
}

/// `thread` as Codex 0.167.0 reads a thread its TUI began on an app-server: `vscode`, no
/// parent, the user's (T-1414), with `status`. With `CF_TEST_CODEX_LINEAGE=sub-agent`, as it
/// reads a sub-agent's thread (`subAgent.thread_spawn`, its parent, `subagent`).
fn top_level(thread: &str, status: Value) -> Value {
    if own_var("CF_TEST_CODEX_LINEAGE").as_deref() == Some("sub-agent") {
        return json!({
            "id": thread,
            "forkedFromId": null,
            "parentThreadId": PARENT,
            "source": { "subAgent": { "thread_spawn": { "parent_thread_id": PARENT, "depth": 1 } } },
            "threadSource": "subagent",
            "status": status,
            "turns": [],
        });
    }
    json!({
        "id": thread,
        "forkedFromId": null,
        "parentThreadId": null,
        "source": "vscode",
        "threadSource": "user",
        "status": status,
        "turns": [],
    })
}

/// The thread a sub-agent's is under, with `CF_TEST_CODEX_LINEAGE=sub-agent`.
const PARENT: &str = "01a17a05-6f24-7f01-b73d-6e76c4aa01c5";

#[cfg(test)]
mod tests {
    use super::*;

    /// A `CF` line of a message is the words of a call of `cf`, its text one last word; after a
    /// `SLEEP` too; none where no line begins so (T-1207).
    #[test]
    fn a_cf_line_is_the_words_of_a_call_of_cf() {
        let brief =
            "[ConsensFlow m-1 · T-1 · task from @chief]\nCF ask :: Which format, JSON or text?";
        assert_eq!(
            cf_in(brief),
            Some(vec![
                "ask".to_owned(),
                "Which format, JSON or text?".to_owned()
            ])
        );
        assert_eq!(
            cf_in("SLEEP 8 CF task finish T-1"),
            Some(vec![
                "task".to_owned(),
                "finish".to_owned(),
                "T-1".to_owned()
            ])
        );
        assert_eq!(cf_in("Reply with exactly: DONE"), None);
        assert_eq!(cf_in("Run CF ask :: no"), None);
    }

    #[test]
    fn a_message_that_says_sleep_works_that_long_and_one_that_asks_for_a_word_is_answered_with_it()
    {
        assert_eq!(
            what_it_says(
                "[ConsensFlow m-4 · T-1 · task from @chief]\nSLEEP 15 Reply with exactly: DONE"
            ),
            (Duration::from_secs(15), "DONE".to_owned())
        );
        assert_eq!(
            what_it_says("Reply with exactly: WORKER_OK"),
            (Duration::ZERO, "WORKER_OK".to_owned())
        );
        assert_eq!(
            what_it_says("hello\nsecond"),
            (Duration::ZERO, "noted: hello".to_owned())
        );
    }

    #[test]
    fn a_message_that_says_compact_has_the_thread_compacted_that_long_first() {
        let brief =
            "[ConsensFlow m-4 · T-1 · task from @chief]\nCOMPACT 30 Reply with exactly: DONE";
        assert_eq!(compacts_for(brief), Some(Duration::from_secs(30)));
        assert_eq!(what_it_says(brief), (Duration::ZERO, "DONE".to_owned()));
        assert_eq!(compacts_for("Reply with exactly: DONE"), None);
    }

    #[test]
    fn a_rollout_is_in_the_folder_of_its_day_and_its_name_holds_its_thread() {
        let path = rollout_of(Path::new("/home/.codex"), THREAD);
        let relative = path.strip_prefix("/home/.codex/sessions").unwrap();
        let parts: Vec<_> = relative.iter().map(|part| part.to_string_lossy()).collect();
        assert_eq!(parts.len(), 4, "{parts:?}");
        assert!(parts[3].starts_with("rollout-"), "{parts:?}");
        assert!(parts[3].ends_with(&format!("-{THREAD}.jsonl")), "{parts:?}");
    }

    /// The server of a window, on `thread`, in the Codex home `home`, configured
    /// with `developer` as its developer instructions.
    fn server(home: &Path, thread: &str, developer: &str) -> Agent {
        Agent::on(
            thread.to_owned(),
            Some(home.to_path_buf()),
            Some(developer.to_owned()),
            Plugins::open(0, 0).unwrap(),
        )
    }

    /// The records of the rollout of `thread`, in order.
    fn records(home: &Path, thread: &str) -> Vec<Value> {
        let rollout = find_rollout(&home.join("sessions"), thread).expect("a rollout");
        fs::read_to_string(rollout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// What the developer's messages of `records` say.
    fn developers(records: &[Value]) -> Vec<&str> {
        records
            .iter()
            .filter(|record| record["payload"]["role"] == "developer")
            .filter_map(|record| record["payload"]["content"][0]["text"].as_str())
            .collect()
    }

    #[test]
    fn a_thread_begins_with_the_developer_instructions_its_server_has_and_writes_them_once() {
        let home = cf_e2e::door::made("cf-test-").unwrap();
        let agent = server(home.path(), "thread-1", "the role as it was");
        agent.turn("first");
        agent.turn("second");
        let written = records(home.path(), "thread-1");
        assert_eq!(developers(&written), ["the role as it was"]);
        // Before the first message of the thread, and none between its turns.
        let kinds: Vec<(&str, &str)> = written
            .iter()
            .map(|record| {
                (
                    record["type"].as_str().unwrap(),
                    record["payload"]["role"].as_str().unwrap_or("-"),
                )
            })
            .collect();
        assert_eq!(kinds[0], ("session_meta", "-"));
        assert_eq!(kinds[2], ("response_item", "developer"));
        assert_eq!(kinds[3], ("response_item", "user"));
    }

    #[test]
    fn a_thread_whose_start_names_developer_instructions_begins_with_them_in_place_of_its_servers()
    {
        let home = cf_e2e::door::made("cf-test-").unwrap();
        let agent = server(
            home.path(),
            "thread-1",
            "what the server is configured with",
        );
        agent.started(&json!({ "developerInstructions": "the role its start names" }));
        agent.turn("first");
        assert_eq!(
            developers(&records(home.path(), "thread-1")),
            ["the role its start names"]
        );
        // A start that names none, or names them as null, begins with the server's.
        let other = cf_e2e::door::made("cf-test-").unwrap();
        let agent = server(
            other.path(),
            "thread-2",
            "what the server is configured with",
        );
        agent.started(&json!({ "developerInstructions": null }));
        agent.turn("first");
        assert_eq!(
            developers(&records(other.path(), "thread-2")),
            ["what the server is configured with"]
        );
    }

    #[test]
    fn a_resumed_thread_goes_on_in_its_rollout_with_the_instructions_it_began_with_whatever_its_server_has_now(
    ) {
        let home = cf_e2e::door::made("cf-test-").unwrap();
        let before = server(home.path(), "thread-1", "the role as it was");
        before.turn("first");
        let began = records(home.path(), "thread-1").len();

        // A window later: a server of its own, configured with the role as it is
        // now, resumes the thread.
        let after = server(home.path(), "thread-2", "the role as it is now");
        assert!(after.resume("thread-1"));
        assert_eq!(after.thread(), "thread-1");
        after.turn("again");
        let written = records(home.path(), "thread-1");
        assert_eq!(
            developers(&written),
            ["the role as it was"],
            "the thread keeps what it began with, and is given no other"
        );
        assert!(written.len() > began, "it went on in the rollout it had");
        let ordinals: Vec<u64> = written
            .iter()
            .map(|record| record["ordinal"].as_u64().unwrap())
            .collect();
        assert!(
            ordinals.windows(2).all(|pair| pair[0] < pair[1]),
            "the records go on being numbered: {ordinals:?}"
        );
        assert!(
            find_rollout(&home.path().join("sessions"), "thread-2").is_none(),
            "no rollout of the id the server began with"
        );
    }

    /// A role as a window's broker gives it: Codex's own, then ConsensFlow's, with the
    /// command that asks the chief.
    const ROLE: &str = "Codex's own instructions.\n\n  Your ConsensFlow role is worker. \
        Follow it.\n\n    /app/bin/cf ask \"…\"   a question to the chief; then end your turn";

    /// What the agent said last in `records`.
    fn answer(records: &[Value]) -> &str {
        records
            .iter()
            .rev()
            .find(|record| record["payload"]["role"] == "assistant")
            .and_then(|record| record["payload"]["content"][0]["text"].as_str())
            .unwrap_or_default()
    }

    /// The developer's messages of the replacement history of the `compacted` records.
    fn rebuilt(records: &[Value]) -> Vec<Vec<&str>> {
        records
            .iter()
            .filter(|record| record["type"] == "compacted")
            .map(|record| {
                record["payload"]["replacement_history"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|item| item["role"] == "developer")
                    .filter_map(|item| item["content"][0]["text"].as_str())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_compaction_rebuilds_the_developer_message_from_the_instructions_of_the_session_that_compacts(
    ) {
        // A first window: its thread began with the role, and a compaction keeps it once.
        let home = cf_e2e::door::made("cf-test-").unwrap();
        let first = server(home.path(), "thread-1", "the configured");
        first.started(&json!({ "developerInstructions": ROLE }));
        first.turn("Reply with exactly: ONE");
        first.turn("COMPACT 0 Say your role");
        let written = records(home.path(), "thread-1");
        assert_eq!(
            developers(&written),
            [ROLE],
            "written once, by the first turn"
        );
        assert_eq!(rebuilt(&written), [vec![ROLE]]);
        assert_eq!(
            answer(&written),
            "Your ConsensFlow role is worker. Ask the chief with: /app/bin/cf ask"
        );

        // Resumed by a window that gives no instructions, as the broker did before T-1394:
        // the thread keeps its role until the compaction, which rebuilds none (T-1388's
        // run-codex-1794337068, Codex 0.167.0).
        let lost = Agent::on(
            "thread-9".to_owned(),
            Some(home.path().to_path_buf()),
            None,
            Plugins::open(0, 0).unwrap(),
        );
        assert!(lost.resume("thread-1"));
        lost.resumed_with(&json!({ "threadId": "thread-1", "developerInstructions": null }));
        lost.turn("Say your role");
        let written = records(home.path(), "thread-1");
        assert!(answer(&written).starts_with("Your ConsensFlow role is worker."));
        lost.turn("COMPACT 0 Say your role");
        let written = records(home.path(), "thread-1");
        assert_eq!(rebuilt(&written).last(), Some(&Vec::<&str>::new()));
        assert_eq!(
            answer(&written),
            "No ConsensFlow role is given. No command is specified for asking the chief a question."
        );

        // Resumed by a window that gives the role: no turn writes it, and the compaction
        // rebuilds it once (T-1388's run-codex-1794335426).
        let kept = server(home.path(), "thread-8", "the configured");
        assert!(kept.resume("thread-1"));
        kept.resumed_with(&json!({ "threadId": "thread-1", "developerInstructions": ROLE }));
        kept.turn("Say your role");
        let written = records(home.path(), "thread-1");
        assert_eq!(
            developers(&written),
            [ROLE],
            "no turn of a resumed thread writes it"
        );
        assert!(
            answer(&written).starts_with("No ConsensFlow role"),
            "it holds none yet"
        );
        kept.turn("COMPACT 0 Say your role");
        let written = records(home.path(), "thread-1");
        assert_eq!(rebuilt(&written).last(), Some(&vec![ROLE]));
        assert_eq!(developers(&written), [ROLE]);
        assert_eq!(
            answer(&written),
            "Your ConsensFlow role is worker. Ask the chief with: /app/bin/cf ask"
        );

        // And a resume that names none has the session's configured instructions.
        let configured = server(home.path(), "thread-7", "the configured");
        assert!(configured.resume("thread-1"));
        assert_eq!(
            held(&configured.holds).as_deref(),
            Some(ROLE),
            "read from the rollout"
        );
        configured.resumed_with(&json!({ "threadId": "thread-1" }));
        configured.turn("COMPACT 0 Reply with exactly: TWO");
        assert_eq!(
            rebuilt(&records(home.path(), "thread-1")).last(),
            Some(&vec!["the configured"])
        );
    }

    #[test]
    fn a_thread_that_had_no_turn_has_no_rollout_and_no_instructions_yet_and_cannot_be_resumed() {
        let home = cf_e2e::door::made("cf-test-").unwrap();
        let before = server(home.path(), "thread-1", "the role as it was");
        // Nothing was said in it: its server has written no rollout of it, and the
        // server of another window finds nothing to go on in.
        let after = server(home.path(), "thread-2", "the role as it is now");
        assert!(!after.resume("thread-1"), "no turn, no rollout");
        assert_eq!(
            after.thread(),
            "thread-2",
            "and the server stays where it was"
        );

        // Its first turn is the one that begins the rollout and writes the
        // instructions its server had, and then it can be resumed.
        before.turn("first");
        assert_eq!(
            developers(&records(home.path(), "thread-1")),
            ["the role as it was"]
        );
        assert!(after.resume("thread-1"));
        assert!(!after.resume("thread-9"), "no rollout holds it");
        assert_eq!(
            after.thread(),
            "thread-1",
            "and the server stays where it was"
        );
    }
}
