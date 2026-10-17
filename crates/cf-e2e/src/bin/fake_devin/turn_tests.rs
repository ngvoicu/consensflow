//! A turn of the window: the words of a message, the time it takes, and what it
//! writes in the store and on the wire, in order.

use std::thread;

use rusqlite::Connection;
use serde_json::Value;

use super::*;

#[test]
fn a_message_is_answered_with_the_word_it_asks_for_and_otherwise_acknowledged_by_its_first_line() {
    let quick = Pace::default();
    assert_eq!(
        plan_of("Reply with exactly: WORKER_OK", quick),
        Plan {
            work: Duration::ZERO,
            background: None,
            file: None,
            crash: None,
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
            background: None,
            file: None,
            crash: None,
            answer: "T5_DONE".to_owned()
        }
    );
    // A turn that works, and then ends its window's program.
    let crashing = plan_of("SLEEP 10 EXIT 3 fatal: gone", quick);
    assert_eq!(crashing.work, Duration::from_secs(10));
    assert_eq!(
        crashing.crash,
        Some(Crash {
            code: 3,
            words: "fatal: gone".to_owned()
        })
    );
    // A line that is no sleep is no wait.
    assert_eq!(plan_of("say SLEEP 5 x", quick).work, Duration::ZERO);
    assert_eq!(plan_of("SLEEP soon x", quick).work, Duration::ZERO);
}

#[test]
fn a_file_line_has_the_turns_tool_make_the_file_and_a_turn_that_goes_on_takes_no_wait_of_its_own() {
    let quick = Pace::default();
    let plan = plan_of(
        "[ConsensFlow m-1 · T-1 · task from @chief]\nSLEEP 600 FILE made.txt Reply with exactly: DONE",
        quick,
    );
    assert_eq!(
        plan,
        Plan {
            work: Duration::from_secs(600),
            background: None,
            file: Some("made.txt".to_owned()),
            crash: None,
            answer: "DONE".to_owned()
        }
    );
    assert_eq!(plan.going_on(quick).work, Duration::ZERO);
    // A line that is no file to make is none.
    assert_eq!(plan_of("say FILE x.txt y", quick).file, None);
}

#[test]
fn a_message_that_asks_for_no_time_works_as_long_as_the_pace_says_and_one_that_does_ask_as_it_says()
{
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

#[test]
fn a_background_line_leaves_a_command_running_that_long_and_does_what_the_words_after_it_say() {
    let quick = Pace::default();
    let plan = plan_of(
        "[ConsensFlow m-1 · T-1 · task from @chief]\nBACKGROUND 600 Reply with exactly: LEFT_IT",
        quick,
    );
    assert_eq!(
        plan,
        Plan {
            work: Duration::ZERO,
            background: Some(600),
            file: None,
            crash: None,
            answer: "LEFT_IT".to_owned()
        }
    );
    // A line that is no command left running is none.
    assert_eq!(plan_of("say BACKGROUND 5 x", quick).background, None);
    assert_eq!(plan_of("BACKGROUND soon x", quick).background, None);
    // Its turn works as long as the pace says, as any that asks for no time of its own.
    let slow = Pace::of(None, Some("14000"));
    assert_eq!(
        plan_of("BACKGROUND 600 hello", slow).work,
        Duration::from_secs(14)
    );
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

#[cfg(unix)]
#[test]
fn a_turn_that_leaves_a_command_running_ends_and_the_command_goes_on_reporting_on_the_wire() {
    let mut case = case();
    let jobs = case.folder.path().join("jobs");
    case.window = case
        .window
        .leaving_jobs_in(jobs.clone(), 4242, Duration::ZERO);
    case.window
        .take("BACKGROUND 2 Reply with exactly: LEFT_IT")
        .unwrap();
    // The command's shell is a job of the window, as the rig finds one.
    let pid: u32 = std::fs::read_to_string(jobs.join("job-4242-1.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(cf_e2e::process::is_alive(pid), "its command runs");
    let updates = |wire: &[Value]| {
        wire.iter()
            .filter(|record| record["update"]["sessionUpdate"] == "tool_call_update")
            .count()
    };
    // The turn ended with the answer, and what the command says comes after it.
    let wire = case.wire();
    let ended = wire
        .iter()
        .position(|record| record["cause"] == "complete")
        .unwrap();
    assert!(wire[..ended]
        .iter()
        .any(|record| record["update"]["sessionUpdate"] == "tool_call"));
    let gone = (0..100).any(|_| {
        thread::sleep(Duration::from_millis(100));
        !cf_e2e::process::is_alive(pid)
    });
    assert!(gone, "the command ran its two seconds");
    // Nothing is said once it has ended, which the thread that says it sees within a
    // look.
    thread::sleep(Duration::from_millis(600));
    let wire = case.wire();
    let after = &wire[ended + 1..];
    assert!(updates(after) > 3, "{after:?}");
    assert_eq!(updates(&wire[..=ended]), 0);
    thread::sleep(Duration::from_millis(600));
    assert_eq!(case.wire().len(), wire.len());
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

/// Where on `wire` the first record is that `holds`.
#[cfg(unix)]
fn first_that(wire: &[Value], holds: impl Fn(&Value) -> bool) -> Option<usize> {
    wire.iter().position(holds)
}

#[cfg(unix)]
#[test]
fn a_turn_that_leaves_a_command_runs_it_once_the_model_has_spoken_and_the_agent_has_thought_and_the_command_reports_after_the_answer(
) {
    let first_word = Duration::from_millis(800);
    let mut case = case_at(Pace {
        first_word,
        work: Duration::ZERO,
    });
    let folder = case.folder.path().to_path_buf();
    let jobs = folder.join("jobs");
    case.window = case
        .window
        .leaving_jobs_in(jobs.clone(), 4242, Duration::ZERO);
    let taking = thread::spawn(move || {
        case.window
            .take("BACKGROUND 2 Reply with exactly: LEFT_IT")
            .unwrap();
        case
    });
    // The message is put to the agent first, and the model has not spoken: the wire
    // shows no thought of the agent's, and the window has no command.
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
    assert!(!jobs.join("job-4242-1.pid").exists(), "nor its command");
    let case = taking.join().unwrap();

    // The command goes on saying what it prints, on the wire, once the turn has ended.
    let reported = |record: &Value| record["update"]["sessionUpdate"] == "tool_call_update";
    let waited = std::time::Instant::now();
    let wire = loop {
        let wire = case.wire();
        if first_that(&wire, reported).is_some() {
            break wire;
        }
        assert!(waited.elapsed() < Duration::from_secs(10), "{wire:?}");
        thread::sleep(Duration::from_millis(50));
    };
    let order = [
        first_that(&wire, |record| record["method"] == "session/prompt"),
        first_that(&wire, |record| {
            record["update"]["sessionUpdate"] == "agent_thought_chunk"
        }),
        first_that(&wire, |record| {
            record["update"]["sessionUpdate"] == "tool_call"
        }),
        first_that(&wire, |record| {
            record["update"]["sessionUpdate"] == "agent_message_chunk"
        }),
        first_that(&wire, |record| record["cause"] == "complete"),
        first_that(&wire, reported),
    ];
    assert!(order.iter().all(Option::is_some), "{order:?}");
    assert!(
        order.windows(2).all(|pair| pair[0] < pair[1]),
        "the prompt, the thought, the tool, the answer, its end and the command's words, in that order: {order:?}"
    );
    // Its command ran its two seconds, and is gone with them.
    let pid: u32 = std::fs::read_to_string(jobs.join("job-4242-1.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let gone = (0..100).any(|_| {
        thread::sleep(Duration::from_millis(100));
        !cf_e2e::process::is_alive(pid)
    });
    assert!(gone, "the command ran its two seconds");
}
