//! The agent of a Codex window, as the stand-in's server plays it: a thread, whose
//! rollout is Codex's own record of it, and the turns the server is asked to take.
//!
//! The rollout is a JSONL file in `sessions/YYYY/MM/DD/` of `CODEX_HOME`, where the
//! daemon reads a thread's record (none is written where `CODEX_HOME` is not set,
//! so a case that is not the daemon's leaves nothing in anyone's home). A message is
//! a turn: the server is handed it as `turn/start`, or as `thread/queue/add` while
//! another runs, and answers at once; the turn goes on beside the connections, one
//! at a time, and writes what Codex writes of one: it started, the message, the
//! answer, it is complete.
//!
//! A thread keeps the instructions it began with. The server is given the role's
//! text as Codex is (`-c developer_instructions=…`), and the first turn of a thread
//! it starts writes it into the rollout as the developer's message. A thread it
//! resumes goes on in the rollout it has, which already holds that message, and is
//! given no other: what the server is configured with now is read and unused, as the
//! live Codex 0.162.1 did with a chief resumed after its role changed (2026-10-13).
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

use cf_e2e::pattern;
use cf_e2e::process::own_var;
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
    /// The thread the server is on: the one it starts, or the one it resumes.
    thread: Mutex<String>,
    /// The `CODEX_HOME` the rollouts are in, none where a case leaves none.
    home: Option<PathBuf>,
    rollout: Mutex<Option<PathBuf>>,
    /// The developer instructions the server is configured with, which a thread it
    /// starts begins with.
    developer: Option<String>,
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
}

impl Agent {
    /// The agent a run is told to be by its environment, and `developer`, the
    /// developer instructions its server was given (none where it was given none).
    /// The servers its Codex opens with the window start now.
    pub fn start(developer: Option<String>) -> Result<Arc<Self>, String> {
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
            developer,
            plugins,
        )))
    }

    /// The agent of a server on `thread`, its rollouts in `home` where there is one.
    fn on(
        thread: String,
        home: Option<PathBuf>,
        developer: Option<String>,
        plugins: Plugins,
    ) -> Self {
        let rollout = home.as_deref().map(|home| rollout_of(home, &thread));
        Self {
            thread: Mutex::new(thread),
            home,
            rollout: Mutex::new(rollout),
            developer,
            instructed: AtomicBool::new(false),
            plugins,
            turns: Mutex::new(()),
            begun: AtomicBool::new(false),
            records: AtomicU64::new(0),
        }
    }

    /// The thread the server is on.
    pub fn thread(&self) -> String {
        held(&self.thread).clone()
    }

    /// The server starts the thread: its rollout begins, as Codex's does with what
    /// it ran in.
    pub fn thread_started(&self) {
        if let Some(path) = held(&self.rollout).as_ref() {
            if let Some(folder) = path.parent() {
                let _ = fs::create_dir_all(folder);
            }
        }
        let thread = self.thread();
        self.write(
            "session_meta",
            json!({ "session_id": thread, "id": thread, "originator": "codex-tui" }),
        );
    }

    /// The server resumes `thread`: it goes on in the rollout the thread has, which
    /// keeps the developer message of its first turn whatever the server is
    /// configured with now. False where no rollout holds the thread, as Codex says
    /// of one that never had a turn.
    pub fn thread_resumed(&self, thread: &str) -> bool {
        let rollout = match &self.home {
            Some(home) => match find_rollout(&home.join("sessions"), thread) {
                Some(found) => Some(found),
                None => return false,
            },
            None => None,
        };
        let (records, instructed) = rollout.as_deref().map_or((0, false), |path| {
            let text = fs::read_to_string(path).unwrap_or_default();
            (text.lines().count() as u64, has_developer_message(&text))
        });
        *held(&self.thread) = thread.to_owned();
        *held(&self.rollout) = rollout;
        self.records.store(records, Ordering::Relaxed);
        self.instructed.store(instructed, Ordering::Relaxed);
        true
    }

    /// The server takes `text` as a turn, which goes on beside it.
    pub fn take(self: &Arc<Self>, text: &str) {
        let (agent, text) = (Arc::clone(self), text.to_owned());
        thread::spawn(move || agent.turn(&text));
    }

    /// A turn: it began with the message, works as long as the message says and ends
    /// with its answer.
    fn turn(&self, text: &str) {
        let _one_at_a_time = held(&self.turns);
        let began = Instant::now();
        let thread = self.thread();
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
        // The first turn of a thread writes what the thread begins with: the
        // developer's instructions, which no later turn, and no resume, writes again.
        if !self.instructed.swap(true, Ordering::Relaxed) {
            if let Some(instructions) = &self.developer {
                self.write(
                    "response_item",
                    json!({
                        "type": "message",
                        "id": item_id(),
                        "role": "developer",
                        "content": [{ "type": "input_text", "text": instructions }],
                    }),
                );
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
            .open(path)
            .and_then(|mut file| file.write_all(line.as_bytes()));
    }
}

/// The id of an item of the model's, as Codex numbers them.
fn item_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4())
}

/// What `lock` holds, one that a panic poisoned too: a thread that ended in one
/// has left the state it was in.
fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The rollout of `thread` below `folder`, Codex's `sessions`: the first file whose
/// name holds it, as the daemon finds one.
fn find_rollout(folder: &Path, thread: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(folder).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_rollout(&path, thread) {
                return Some(found);
            }
        } else if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains(thread))
        {
            return Some(path);
        }
    }
    None
}

/// Whether the rollout `text` has a developer's message: what a thread begins with.
fn has_developer_message(text: &str) -> bool {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|record| {
            record["type"] == "response_item"
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
        let home = tempfile::tempdir().unwrap();
        let agent = server(home.path(), "thread-1", "the role as it was");
        agent.thread_started();
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
    fn a_resumed_thread_goes_on_in_its_rollout_with_the_instructions_it_began_with_whatever_its_server_has_now(
    ) {
        let home = tempfile::tempdir().unwrap();
        let before = server(home.path(), "thread-1", "the role as it was");
        before.thread_started();
        before.turn("first");
        let began = records(home.path(), "thread-1").len();

        // A window later: a server of its own, configured with the role as it is
        // now, resumes the thread.
        let after = server(home.path(), "thread-2", "the role as it is now");
        assert!(after.thread_resumed("thread-1"));
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

    #[test]
    fn a_thread_that_had_no_turn_has_no_instructions_yet_and_a_thread_with_no_rollout_cannot_be_resumed(
    ) {
        let home = tempfile::tempdir().unwrap();
        let before = server(home.path(), "thread-1", "the role as it was");
        before.thread_started();
        // Its rollout is begun, and holds no developer message: no turn has been.
        let after = server(home.path(), "thread-2", "the role as it is now");
        assert!(after.thread_resumed("thread-1"));
        after.turn("first");
        assert_eq!(
            developers(&records(home.path(), "thread-1")),
            ["the role as it is now"],
            "its first turn is the one that writes them"
        );
        assert!(!after.thread_resumed("thread-9"), "no rollout holds it");
        assert_eq!(
            after.thread(),
            "thread-1",
            "and the server stays where it was"
        );
    }
}
