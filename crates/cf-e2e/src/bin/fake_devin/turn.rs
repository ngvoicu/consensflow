//! A turn of the window: what a message asks of it, and how it ends. It speaks
//! the words the other stand-in does: a line `SLEEP <seconds> <words>` is a turn
//! that works that long before it does what the words say, and `Reply with
//! exactly: X` is answered with X, as anything else is acknowledged. What Devin
//! writes of a turn is written in its order: the message in the store and on the
//! wire, the agent's thought, and then the answer (the store first, so that a
//! reader that sees the turn end on the wire finds the answer in the store) or,
//! where an Escape stops the turn, the cancel and what was said so far.
//!
//! A turn takes the time a model takes ([`Pace`]): from the message put to the
//! agent to its first word, in which the turn is begun and its work is not to be
//! seen, and the work itself, which no words but a `SLEEP` set. What a message
//! the window is handed while a turn is at work comes to is the window's
//! (`fake_devin.rs`): it waits, and is written to the store when its turn begins.

use std::io::{self, Write};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use cf_e2e::pattern;
use regex::Regex;

use crate::said;
use crate::stop::Stop;
use crate::store::Store;
use crate::wire::Wire;

/// The variables that set a window's [`Pace`], in milliseconds: nothing in a
/// person's environment sets them, and a window with neither takes no time at all.
pub const FIRST_WORD_VARIABLE: &str = "FAKE_DEVIN_FIRST_WORD_MS";
pub const WORK_VARIABLE: &str = "FAKE_DEVIN_WORK_MS";

/// How long a turn takes where its message does not say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pace {
    /// From the message put to the agent to the agent's first word (what Devin
    /// measures as `ttftMs`): the turn is begun, its message is in the store and on
    /// the wire, and nothing of its work is. A look at the window finds it at rest.
    pub first_word: Duration,
    /// How long a turn that asks for no `SLEEP` works before it answers.
    pub work: Duration,
}

impl Pace {
    /// The pace the two variables name: each a number of milliseconds, or none.
    pub fn of(first_word: Option<&str>, work: Option<&str>) -> Self {
        let named = |ms: Option<&str>| {
            ms.and_then(|ms| ms.parse().ok())
                .map_or(Duration::ZERO, Duration::from_millis)
        };
        Self {
            first_word: named(first_word),
            work: named(work),
        }
    }
}

/// What a turn is asked to do.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// How long it works before it answers.
    pub work: Duration,
    /// What it answers.
    pub answer: String,
}

/// What the message `text` asks of a turn, which works as long as `pace` says
/// where the message says nothing of it.
pub fn plan_of(text: &str, pace: Pace) -> Plan {
    static SLEEP: OnceLock<Regex> = OnceLock::new();
    static EXACT: OnceLock<Regex> = OnceLock::new();
    let (work, words) = match pattern::once(&SLEEP, r"(?m)^SLEEP (\d+) (.+)$").captures(text) {
        Some(slow) => (
            Duration::from_secs(slow[1].parse().unwrap_or(0)),
            slow[2].to_owned(),
        ),
        None => (pace.work, text.to_owned()),
    };
    let answer = match pattern::once(&EXACT, r"Reply with exactly: (\S+)").captures(&words) {
        Some(exact) => exact[1].to_owned(),
        None => format!("noted: {}", words.split('\n').next().unwrap_or_default()),
    };
    Plan { work, answer }
}

/// The window's turns, one at a time, in the order the messages came.
pub struct Window<W: Write> {
    store: Store,
    wire: Wire,
    stop: Arc<Stop>,
    /// What the window says on its screen.
    screen: W,
    pace: Pace,
    turns: u64,
}

impl<W: Write> Window<W> {
    pub fn new(store: Store, wire: Wire, stop: Arc<Stop>, screen: W, pace: Pace) -> Self {
        Self {
            store,
            wire,
            stop,
            screen,
            pace,
            turns: 0,
        }
    }

    /// Says `text` on the window's screen.
    fn say(&mut self, text: &str) -> io::Result<()> {
        self.screen.write_all(text.as_bytes())?;
        self.screen.flush()
    }

    /// The window takes the message `text`: its turn, from the message to the
    /// answer or to the Escape that stops it.
    pub fn take(&mut self, text: &str) -> Result<(), String> {
        self.stop.begin();
        let done = self.turn(text);
        self.stop.end();
        done
    }

    fn turn(&mut self, text: &str) -> Result<(), String> {
        self.turns += 1;
        let client = format!("client-{}", self.turns);
        let stream = format!("stream-{}", self.turns);
        let plan = plan_of(text, self.pace);
        self.store.user(text, &client)?;
        self.wire.prompt(self.turns + 1, text).map_err(said)?;
        self.say("working (esc twice to interrupt)\n")
            .map_err(said)?;
        // The turn is begun, and its agent has not said a word: a model's time,
        // in which the wire shows none of its work.
        let mut stopped = self.stop.work(self.pace.first_word);
        if !stopped {
            self.wire.thinking(&client).map_err(said)?;
            stopped = self.stop.work(plan.work);
        }
        if stopped {
            // The TUI asks its agent to cancel, which keeps what it said so far.
            self.wire.cancel().map_err(said)?;
            self.wire
                .said("Interrupted", &stream, &client)
                .map_err(said)?;
            self.store.assistant("Interrupted", false)?;
            self.wire.ended("cancelled", &client).map_err(said)?;
            return self.say("Interrupted\n").map_err(said);
        }
        self.wire
            .said(&plan.answer, &stream, &client)
            .map_err(said)?;
        self.store.assistant(&plan.answer, true)?;
        self.wire.ended("complete", &client).map_err(said)?;
        self.say(&format!("{}\n", plan.answer)).map_err(said)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use rusqlite::Connection;
    use serde_json::Value;

    use super::*;

    #[test]
    fn a_message_is_answered_with_the_word_it_asks_for_and_otherwise_acknowledged_by_its_first_line(
    ) {
        let quick = Pace::default();
        assert_eq!(
            plan_of("Reply with exactly: WORKER_OK", quick),
            Plan {
                work: Duration::ZERO,
                answer: "WORKER_OK".to_owned()
            }
        );
        assert_eq!(
            plan_of(
                "[ConsensFlow m-4 · T-1 · task from @chief]\nReply with exactly: ONE_DONE",
                quick
            )
            .answer,
            "ONE_DONE"
        );
        assert_eq!(plan_of("hello\nsecond", quick).answer, "noted: hello");
    }

    #[test]
    fn a_sleep_line_works_that_long_before_it_does_what_the_words_after_it_say() {
        let quick = Pace::default();
        let plan = plan_of(
            "[ConsensFlow m-1 · T-5 · task from @chief]\nSLEEP 120 Reply with exactly: T5_DONE",
            quick,
        );
        assert_eq!(
            plan,
            Plan {
                work: Duration::from_secs(120),
                answer: "T5_DONE".to_owned()
            }
        );
        // A line that is no sleep is no wait.
        assert_eq!(plan_of("say SLEEP 5 x", quick).work, Duration::ZERO);
        assert_eq!(plan_of("SLEEP soon x", quick).work, Duration::ZERO);
    }

    #[test]
    fn a_message_that_asks_for_no_time_works_as_long_as_the_pace_says_and_one_that_does_ask_as_it_says(
    ) {
        let slow = Pace::of(Some("3000"), Some("14000"));
        assert_eq!(slow.first_word, Duration::from_secs(3));
        assert_eq!(plan_of("hello", slow).work, Duration::from_secs(14));
        assert_eq!(
            plan_of("SLEEP 2 hello", slow).work,
            Duration::from_secs(2),
            "its own words come before the pace"
        );
        // What is no number of milliseconds is no time, as a window with no variable takes none.
        assert_eq!(Pace::of(None, None), Pace::default());
        assert_eq!(Pace::of(Some("soon"), Some("-4")), Pace::default());
    }

    /// A window on a store and a wire of a folder of its own, and what it said on its screen.
    struct Case {
        folder: tempfile::TempDir,
        window: Window<Vec<u8>>,
    }

    fn case() -> Case {
        case_at(Pace::default())
    }

    fn case_at(pace: Pace) -> Case {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(&folder.path().join("sessions.db"), "calm-river").unwrap();
        let wire = Wire::open(&folder.path().join("wire.jsonl"), "calm-river").unwrap();
        let window = Window::new(store, wire, Arc::new(Stop::default()), Vec::new(), pace);
        Case { folder, window }
    }

    impl Case {
        fn nodes(&self) -> Vec<Value> {
            let db = Connection::open(self.folder.path().join("sessions.db")).unwrap();
            let mut rows = db
                .prepare("SELECT chat_message FROM message_nodes ORDER BY row_id")
                .unwrap();
            rows.query_map([], |row| {
                let message: String = row.get(0)?;
                Ok(serde_json::from_str(&message).unwrap())
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
        }

        fn wire(&self) -> Vec<Value> {
            std::fs::read_to_string(self.folder.path().join("wire.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    #[test]
    fn a_turn_that_is_answered_is_a_user_and_an_assistant_message_that_ends_it_and_a_complete_on_the_wire(
    ) {
        let mut case = case();
        case.window.take("Reply with exactly: DONE").unwrap();
        let nodes = case.nodes();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0]["role"], "user");
        assert_eq!(nodes[1]["content"], "DONE");
        assert_eq!(nodes[1]["metadata"]["finish_reason"], "stop");
        let wire = case.wire();
        assert_eq!(wire.last().unwrap()["cause"], "complete");
        assert_eq!(
            wire.last().unwrap()["turnClientMessageId"],
            nodes[0]["metadata"]["extensions"]["chisel/client-message-id"]
        );
        assert!(wire
            .iter()
            .any(|record| record["update"]["sessionUpdate"] == "agent_thought_chunk"));
        let screen = String::from_utf8(case.window.screen.clone()).unwrap();
        assert_eq!(screen, "working (esc twice to interrupt)\nDONE\n");
    }

    #[test]
    fn a_turn_that_is_stopped_is_cancelled_on_the_wire_and_leaves_what_was_said_with_no_end_to_its_turn(
    ) {
        let mut case = case();
        let stop = Arc::clone(&case.window.stop);
        let presser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            stop.press();
        });
        let started = std::time::Instant::now();
        case.window
            .take("SLEEP 30 Reply with exactly: LATE")
            .unwrap();
        presser.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(20));
        let nodes = case.nodes();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[1]["content"], "Interrupted");
        assert!(nodes[1]["metadata"].get("finish_reason").is_none());
        let wire = case.wire();
        let cancel = wire
            .iter()
            .position(|record| record["method"] == "session/cancel")
            .unwrap();
        assert_eq!(wire[cancel + 2]["cause"], "cancelled");
        assert!(wire.iter().all(|record| record["cause"] != "complete"));
        let screen = String::from_utf8(case.window.screen.clone()).unwrap();
        assert_eq!(screen, "working (esc twice to interrupt)\nInterrupted\n");
    }

    #[test]
    fn a_window_takes_one_message_after_another_each_a_client_of_its_own() {
        let mut case = case();
        case.window.take("Reply with exactly: ONE").unwrap();
        case.window.take("Reply with exactly: TWO").unwrap();
        let nodes = case.nodes();
        assert_eq!(nodes.len(), 4);
        let clients: Vec<&Value> = [0, 2]
            .iter()
            .map(|at| &nodes[*at]["metadata"]["extensions"]["chisel/client-message-id"])
            .collect();
        assert_eq!(clients, ["client-1", "client-2"]);
    }

    /// Whether the wire of `folder` holds a record of the thought of an agent at work.
    fn thinking(folder: &std::path::Path) -> bool {
        std::fs::read_to_string(folder.join("wire.jsonl"))
            .unwrap_or_default()
            .lines()
            .any(|line| line.contains("agent_thought_chunk"))
    }

    #[test]
    fn a_turn_is_begun_for_a_model_s_time_before_the_agent_says_a_word_and_shows_no_work_in_it() {
        let first_word = Duration::from_millis(800);
        let mut case = case_at(Pace {
            first_word,
            work: Duration::ZERO,
        });
        let folder = case.folder.path().to_path_buf();
        let taking = thread::spawn(move || {
            let started = std::time::Instant::now();
            case.window.take("Reply with exactly: DONE").unwrap();
            (case, started.elapsed())
        });
        // The message is put to the agent first, and its store has it: the turn is begun.
        let prompt = || {
            std::fs::read_to_string(folder.join("wire.jsonl"))
                .unwrap_or_default()
                .contains("session/prompt")
        };
        let waited = std::time::Instant::now();
        while !prompt() {
            assert!(waited.elapsed() < Duration::from_secs(10), "no prompt");
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!thinking(&folder), "its work is not to be seen yet");
        let (case, took) = taking.join().unwrap();
        assert!(took >= first_word, "{took:?}");
        let nodes = case.nodes();
        assert_eq!(nodes[0]["role"], "user");
        assert!(thinking(&folder), "and is, once the model has spoken");
    }

    #[test]
    fn a_turn_that_is_stopped_before_the_agent_says_a_word_is_cancelled_with_no_work_on_the_wire() {
        let mut case = case_at(Pace {
            first_word: Duration::from_secs(30),
            work: Duration::ZERO,
        });
        let stop = Arc::clone(&case.window.stop);
        let presser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            stop.press();
        });
        let started = std::time::Instant::now();
        case.window.take("Reply with exactly: LATE").unwrap();
        presser.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(20));
        assert!(!thinking(case.folder.path()));
        let wire = case.wire();
        assert!(wire
            .iter()
            .any(|record| record["method"] == "session/cancel"));
        assert_eq!(wire.last().unwrap()["cause"], "cancelled");
        assert_eq!(case.nodes()[1]["content"], "Interrupted");
    }
}
