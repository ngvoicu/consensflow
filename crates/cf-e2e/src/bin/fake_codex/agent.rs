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
//! What a turn does is what the stand-in agent's does: the `SLEEP <seconds>` line of
//! its message is how long it works, `Reply with exactly: X` is its answer, and
//! anything else is acknowledged. The thread is the same in every run unless
//! `CF_TEST_CODEX_THREADS` says `unique`, which a window the daemon opens needs
//! (two of them in one `CODEX_HOME` would share a rollout). The servers Codex opens
//! with its window are `CF_TEST_CODEX_SERVERS` of them, and the first starts
//! `CF_TEST_CODEX_WORKERS` processes of its own when the first turn's tools are
//! called ([`crate::servers`]).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::process::own_var;
use cf_e2e::{files, pattern};
use regex::Regex;
use serde_json::{json, Value};

use crate::servers::Plugins;

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
    plugins: Plugins,
    /// One turn at a time, in the order the messages came: what is queued waits.
    turns: Mutex<()>,
    /// Whether a turn has begun: the first of them is the one that calls the tools.
    begun: AtomicBool,
    /// The records the rollout holds, to number the next.
    records: AtomicU64,
}

/// What `lock` holds, whether or not a thread that held it panicked.
fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Agent {
    /// The agent a run is told to be by its environment. The servers its Codex
    /// opens with the window start now.
    pub fn start() -> Result<Arc<Self>, String> {
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
        Ok(Arc::new(Self {
            plugins: Plugins::open(
                number("CF_TEST_CODEX_SERVERS"),
                number("CF_TEST_CODEX_WORKERS"),
            )?,
            thread: Mutex::new(thread),
            home: own_var("CODEX_HOME").map(PathBuf::from),
            rollout: Mutex::new(None),
            turns: Mutex::new(()),
            begun: AtomicBool::new(false),
            records: AtomicU64::new(0),
        }))
    }

    /// The thread the server is on.
    pub fn thread(&self) -> String {
        held(&self.thread).clone()
    }

    /// The server is asked to resume `thread`: it goes on with it where the Codex
    /// home holds its rollout, which only a thread that had a turn has. Whether it
    /// does.
    pub fn resume(&self, thread: &str) -> bool {
        let ends = format!("-{thread}.jsonl");
        let sessions = self.home.as_ref().map(|home| home.join("sessions"));
        let Some(found) =
            sessions.and_then(|sessions| files::find(&sessions, &|name| name.ends_with(&ends)))
        else {
            return false;
        };
        // What the rollout holds is what its next record is numbered after.
        let holds = fs::read_to_string(&found).map_or(0, |text| text.lines().count());
        self.records
            .store(u64::try_from(holds).unwrap_or_default(), Ordering::Relaxed);
        thread.clone_into(&mut held(&self.thread));
        *held(&self.rollout) = Some(found);
        true
    }

    /// The server takes `text` as a turn, which goes on beside it.
    pub fn take(self: &Arc<Self>, text: &str) {
        let (agent, text) = (Arc::clone(self), text.to_owned());
        thread::spawn(move || agent.turn(&text));
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
                let _ = fs::create_dir_all(folder);
            }
            *rollout = Some(path);
        }
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
        thread::sleep(work.saturating_sub(began.elapsed()));
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
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| file.write_all(line.as_bytes()));
    }
}

/// The id of an item of the model's, as Codex numbers them.
fn item_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4())
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

/// How long the message `text` has the agent work, and what it says at the end.
fn what_it_says(text: &str) -> (Duration, String) {
    static SLEEP: OnceLock<Regex> = OnceLock::new();
    static EXACT: OnceLock<Regex> = OnceLock::new();

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_rollout_is_in_the_folder_of_its_day_and_its_name_holds_its_thread() {
        let path = rollout_of(Path::new("/home/.codex"), THREAD);
        let relative = path.strip_prefix("/home/.codex/sessions").unwrap();
        let parts: Vec<_> = relative.iter().map(|part| part.to_string_lossy()).collect();
        assert_eq!(parts.len(), 4, "{parts:?}");
        assert!(parts[3].starts_with("rollout-"), "{parts:?}");
        assert!(parts[3].ends_with(&format!("-{THREAD}.jsonl")), "{parts:?}");
    }
}
