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
//! A thread keeps the instructions it began with. The server is given the role's
//! text as Codex is (`-c developer_instructions=…`), and the first turn of a thread
//! it starts writes it into the rollout as the developer's message. A thread it
//! resumes goes on in the rollout it has, which already holds that message, and is
//! given no other: what the server is configured with now is read and unused, as the
//! live Codex 0.162.1 did with a chief resumed after its role changed (2026-10-13).
//!
//! What a turn does is what the stand-in agent's does: the `SLEEP <seconds>` line of
//! its message is how long it works, `Reply with exactly: X` is its answer, and
//! anything else is acknowledged. A message that says `QUOTA-OUT` is refused for
//! the usage limit, as the stand-in agent's is (`CF_TEST_QUOTA_OUT` names the
//! member whose sessions are refused, while `CF_TEST_QUOTA_FILE`, when it is set,
//! is there): the turn works as long as it says and ends as Codex 0.163 ends one
//! its limit cut, with `usage_limit_exceeded` and no answer. The thread is the same in every run unless
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

/// What `lock` holds, whether or not a thread that held it panicked.
fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
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
        Self {
            thread: Mutex::new(thread),
            home,
            rollout: Mutex::new(None),
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

    #[test]
    fn a_thread_that_had_no_turn_has_no_rollout_and_no_instructions_yet_and_cannot_be_resumed() {
        let home = tempfile::tempdir().unwrap();
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
