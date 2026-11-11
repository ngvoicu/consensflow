//! A Devin window that began a turn of its own is never read at rest before that turn
//! ends (T-1403, from the reviews T-1397 and T-1401 of T-1395): not by what its record
//! held when it was opened again with no message (the old process's turn, `LeftOpen`),
//! and not by a chain Devin's compactor left holding nothing of the turn.
//!
//! The case the review found: a window opened again with no message on a session
//! compacted at rest (no message of the user's on its chain) is given a task, and Devin
//! compacts in the middle of that turn keeping none of its steps. The chain then holds
//! no message of the user's again, as at the open, and the window read the record
//! "unchanged" and at rest over the reader's own Unknown: a Stop owed was paid as to a
//! window at rest, with no key, and never owed again, while Devin worked on.
//!
//! One table, through the window's reader (`window.observe()`), over the real store
//! and the launch's own wire log: how the window was opened (with a first message, or
//! with none), its chain then (messages of the user's on it, or compacted to none), what
//! came after, and what the wire says at the look. Each row is run twice: with a look at
//! every step, and with looks only at the open and at the end (two looks that straddle
//! the delivery and the compaction never see the chain with the message on it).
//!
//! The invariant: no row with a turn of this window's open reads at rest, and every row
//! with none open (nothing given, its turn's end said) does, so nothing holds a window
//! at rest for good. Rows left out, each for one reason:
//! - a turn that waits (`blocked`) or ended with nothing of it in the store: a turn that
//!   came to its end has its message in the store, and that is the "delivered" row;
//! - a compaction in the middle of a turn with a silent wire: Devin says `working` as a
//!   turn begins, before its first step, and a compaction comes at a step (T-660's
//!   probes; T-1401 did not count the order's other way as a blocker, and this table
//!   does not build for it: such a row reads at rest, see the result of T-1403);
//! - a compaction at rest with the wire saying a turn works or waits: a compaction while
//!   a turn is open is one of the mid-turn rows.

use std::fs;
use std::rc::Rc;

use cf_harness::contract::Window;
use cf_harness::testing::{finished, EPOCH_MS};
use serde_json::{json, Value};

use super::fixtures::*;

const SESSION: &str = "wealthy-trick";

/// How the window was opened on the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opened {
    /// With a first message of its own (`--prompt-file`): the message is its turn.
    WithAMessage,
    /// With none (the board's view button, T-722).
    WithNone,
}

/// The session's main chain when the window was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtOpen {
    /// A finished turn: the user's message and Devin's answer, stored as its end.
    Messages,
    /// Compacted at rest (T-1388): the system's opening, the skills and the summary.
    Compacted,
    /// The old process's turn cut after a step whose call has no result (T-722): a record
    /// that reads its turn not ended by itself. Not an axis of the table: its own test.
    Cut,
}

/// What came after the open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    /// Nothing in the store.
    Nothing,
    /// A message of the user's taken (the first message, or one delivered since), and
    /// its turn's steps as the wire goes on.
    Delivered,
    /// Delivered, and a compaction in the middle of its turn that keeps its steps (T-652's
    /// shape: copies of the steps below the summary, the message read back by them).
    KeptSteps,
    /// Delivered, and a compaction in the middle of its turn that keeps no step of it
    /// (T-1401's): the summary alone.
    KeptNoStep,
    /// Delivered, and a compaction in the middle of its turn that keeps no step of it but
    /// a copy of the turn before's answer, which reads that turn's message back: the
    /// chain holds the same messages of the user's as at the open.
    KeptTheTurnBefore,
    /// A compaction at rest: with nothing given, or once the turn given ended.
    CompactedAtRest,
}

/// What the launch's wire log says at the look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wire {
    /// Nothing more than the window's configuration.
    Silent,
    /// The history a reopened window replays: stamped, in no stream.
    Replayed,
    /// What the TUI answers itself at once to a command of its own (`/bypass`).
    Instant,
    /// Devin's word that the turn works, and its thought.
    Working,
    /// Then Devin's word that it waits, before the turn's end.
    Blocked,
    /// Then the turn's end.
    Ended,
}

#[derive(Debug, Clone, Copy)]
struct Row {
    opened: Opened,
    at_open: AtOpen,
    then: Then,
    wire: Wire,
}

impl Row {
    /// Whether a turn of this window's is open at the look: nothing given and no turn on
    /// the wire is none, and a turn whose end the wire said is over.
    fn turn_open(self) -> bool {
        let given = !matches!(self.then, Then::Nothing | Then::CompactedAtRest);
        match self.wire {
            Wire::Ended => false,
            Wire::Working | Wire::Blocked => true,
            Wire::Silent | Wire::Replayed | Wire::Instant => given,
        }
    }

    /// Whether the row is one that can happen (the module's doc says why each other one
    /// cannot).
    fn happens(self) -> bool {
        let compacted_in_the_turn = matches!(
            self.then,
            Then::KeptSteps | Then::KeptNoStep | Then::KeptTheTurnBefore
        );
        let quiet = matches!(self.wire, Wire::Silent | Wire::Replayed | Wire::Instant);
        match self.then {
            Then::Nothing => !matches!(self.wire, Wire::Blocked | Wire::Ended),
            Then::Delivered => true,
            _ if compacted_in_the_turn => !quiet,
            Then::CompactedAtRest => matches!(self.wire, Wire::Silent | Wire::Ended),
            _ => unreachable!(),
        }
    }
}

/// When the window is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Looks {
    /// After every step.
    EveryStep,
    /// At the open and at the end only.
    OpenAndEnd,
}

/// A row being played: its home, its store, its window.
struct Scene {
    home: Home,
    store: Store,
    /// None until it is opened.
    window: Option<Rc<dyn Window>>,
    looks: Looks,
    head: String,
    compactions: u32,
}

fn status(state: &str) -> Value {
    json!({
        "sessionId": SESSION,
        "update": {
            "sessionUpdate": "session_info_update",
            "_meta": {
                "cognition.ai/timestamp": 1_792_976_967_738_i64,
                "cognition.ai/eventType": "status_update",
                "cognition.ai/statusEnum": state,
            },
        },
    })
}

fn chunk(kind: &str, text: &str, meta: Value) -> Value {
    json!({
        "sessionId": SESSION,
        "update": {
            "sessionUpdate": kind,
            "content": { "type": "text", "text": text },
            "_meta": meta,
        },
    })
}

fn message(id: &str, role: &str, content: &str, more: Value) -> Value {
    let mut message = json!({
        "message_id": id,
        "role": role,
        "content": content,
        "metadata": { "created_at": "2026-10-13T10:00:00Z" },
    });
    if let (Some(message), Some(more)) = (message.as_object_mut(), more.as_object()) {
        for (key, value) in more {
            if key == "metadata" {
                for (name, field) in value.as_object().unwrap() {
                    message["metadata"][name] = field.clone();
                }
            } else {
                message.insert(key.clone(), value.clone());
            }
        }
    }
    message
}

/// The summary Devin's compactor begins a chain with.
fn summary(id: &str, source: &str, kept: u32) -> Value {
    message(
        id,
        "system",
        "The conversation so far.",
        json!({ "metadata": { "extensions": { "devin-rs/summary": { "source": source, "kept_turns": kept } } } }),
    )
}

/// The turn before the open: the user's message, and the answer Devin stored as its end.
const ASKED: &str = "asked-before";
const ANSWERED: &str = "answered-before";
/// The turn of the window's own: its message, a step that calls a tool, the tool's answer
/// and its answer.
const ASKS: &str = "asks-now";
const STEP: &str = "steps-now";
const TOOL: &str = "tool-now";
const ANSWERS: &str = "answers-now";

impl Scene {
    fn new(row: Row, looks: Looks) -> Self {
        let home = Home::new();
        let store = home.store();
        store.begins(SESSION);
        store.writes(
            SESSION,
            "n-asked",
            None,
            &message(ASKED, "user", "Write the parser", json!({})),
        );
        let head = if row.at_open == AtOpen::Cut {
            store.writes(SESSION, "n-cut", Some("n-asked"), &cut_step());
            "n-cut"
        } else {
            store.writes(
                SESSION,
                "n-answered",
                Some("n-asked"),
                &message(
                    ANSWERED,
                    "assistant",
                    "Done.",
                    json!({ "metadata": { "finish_reason": "stop" } }),
                ),
            );
            "n-answered"
        };
        let mut scene = Self {
            home,
            store,
            window: None,
            looks,
            head: head.to_owned(),
            compactions: 0,
        };
        if row.at_open == AtOpen::Compacted {
            scene.compacts(&[]);
        }
        let adapter = scene.home.adapter();
        let request = Request {
            message: (row.opened == Opened::WithAMessage).then(|| TASK.to_owned()),
            ..Request::resumed(SESSION)
        };
        scene.window = Some(window(&adapter, &request));
        fs::create_dir_all(scene.home.folder()).unwrap();
        scene.home.selects(SESSION);
        scene
    }

    /// Devin's compactor makes a chain of its own: the system's opening, the summary, and
    /// copies of the messages `kept` (by their ids), each a new row.
    fn compacts(&mut self, kept: &[(&str, Value)]) {
        self.compactions += 1;
        let at = self.compactions;
        let node = |part: &str| format!("c{at}-{part}");
        self.store.writes(
            SESSION,
            &node("opening"),
            None,
            &message(
                &format!("opening-{at}"),
                "system",
                "The system's opening.",
                json!({}),
            ),
        );
        self.store.writes(
            SESSION,
            &node("summary"),
            Some(&node("opening")),
            &summary(
                &format!("summary-{at}"),
                "file_compactor",
                kept.len() as u32,
            ),
        );
        let mut parent = node("summary");
        for (index, (_, copy)) in kept.iter().enumerate() {
            let this = node(&format!("copy-{index}"));
            self.store.writes(SESSION, &this, Some(&parent), copy);
            parent = this;
        }
        self.head = parent;
        self.looks_now();
    }

    /// A row below the head.
    fn adds(&mut self, node: &str, written: &Value) {
        self.store
            .writes(SESSION, node, Some(&self.head.clone()), written);
        self.head = node.to_owned();
        self.looks_now();
    }

    fn says(&self, lines: &[Value]) {
        for line in lines {
            self.home.appends(&format!("{line}\n"));
        }
        self.looks_now();
    }

    fn looks_now(&self) {
        if self.looks == Looks::EveryStep && self.window.is_some() {
            let _ = self.at_rest();
        }
    }

    fn at_rest(&self) -> bool {
        let window = self.window.as_ref().expect("the window is open");
        finished(window.observe()).unwrap().settled
    }
}

fn asks() -> Value {
    let mut asks = message(ASKS, "user", TASK, json!({}));
    asks["metadata"]["created_at"] = json!(cf_base::time::iso(EPOCH_MS));
    asks
}

fn step() -> Value {
    message(
        STEP,
        "assistant",
        "",
        json!({
            "tool_calls": [{ "id": "call-now", "name": "exec" }],
            "metadata": { "request_id": "model-now-1", "finish_reason": null },
        }),
    )
}

fn tool() -> Value {
    message(TOOL, "tool", "ok", json!({ "tool_call_id": "call-now" }))
}

fn answers() -> Value {
    message(
        ANSWERS,
        "assistant",
        "Done again.",
        json!({ "metadata": { "request_id": "model-now-2", "finish_reason": "stop" } }),
    )
}

/// The step the old process's turn was cut after: a call with no result.
const CUT: &str = "cut-before";

fn cut_step() -> Value {
    message(
        CUT,
        "assistant",
        "",
        json!({
            "tool_calls": [{ "id": "call-before", "name": "exec" }],
            "metadata": { "request_id": "model-before", "finish_reason": null },
        }),
    )
}

/// A copy of the turn before's answer, as a compaction keeps it.
fn answered_before() -> Value {
    message(
        ANSWERED,
        "assistant",
        "Done.",
        json!({ "metadata": { "finish_reason": "stop" } }),
    )
}

/// Plays `row` with `looks`, and what the window reads at the end.
fn play(row: Row, looks: Looks) -> bool {
    let mut scene = Scene::new(row, looks);
    let at_open = scene.at_rest();
    if row.opened == Opened::WithNone {
        assert!(
            at_open,
            "{row:?}: a window given nothing is at rest at its open"
        );
    }
    let working = || {
        vec![
            status("working"),
            chunk(
                "agent_thought_chunk",
                "Reading the next file.",
                json!({
                    "cognition.ai/timestamp": "2026-11-10T20:35:15.000000+00:00",
                    "cognition.ai/streamingMessageId": "stream-now",
                }),
            ),
        ]
    };
    let ended = || {
        vec![
            status("blocked"),
            json!({ "sessionId": SESSION, "turnClientMessageId": "request-now" }),
            json!({ "sessionId": SESSION, "cause": "complete", "stats": {} }),
        ]
    };
    match row.then {
        Then::Nothing | Then::CompactedAtRest => {
            if row.then == Then::CompactedAtRest && row.wire == Wire::Ended {
                // A turn of its own, ended, then the compaction at rest.
                scene.adds("n-asks", &asks());
                scene.says(&working());
                scene.adds("n-answers", &answers());
                scene.says(&ended());
            }
            match row.wire {
                Wire::Replayed => scene.says(&[chunk(
                    "agent_message_chunk",
                    "Done.",
                    json!({ "cognition.ai/timestamp": "2026-10-13T10:00:05.000000+00:00" }),
                )]),
                Wire::Instant => scene.says(&[chunk(
                    "agent_message_chunk",
                    "Bypass mode on.",
                    json!({
                        "cognition.ai/instant": true,
                        "cognition.ai/streamingMessageId": "stream-instant",
                    }),
                )]),
                Wire::Working => scene.says(&working()),
                Wire::Silent | Wire::Ended | Wire::Blocked => {}
            }
            if row.then == Then::CompactedAtRest {
                scene.compacts(&[]);
            }
        }
        _ => {
            scene.adds("n-asks", &asks());
            if row.wire != Wire::Silent {
                scene.says(&working());
                scene.adds("n-step", &step());
                scene.adds("n-tool", &tool());
                match row.then {
                    Then::KeptSteps => scene.compacts(&[(STEP, step()), (TOOL, tool())]),
                    Then::KeptNoStep => scene.compacts(&[]),
                    Then::KeptTheTurnBefore => {
                        scene.compacts(&[(ANSWERED, answered_before())]);
                    }
                    _ => {}
                }
                match row.wire {
                    Wire::Blocked => scene.says(&[status("blocked")]),
                    Wire::Ended => {
                        // Its answer, below whatever the chain holds, unless the compaction
                        // kept nothing of the turn at its end.
                        if row.then != Then::KeptNoStep {
                            scene.adds("n-answers", &answers());
                        }
                        scene.says(&ended());
                    }
                    _ => {}
                }
            }
        }
    }
    scene.at_rest()
}

fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for opened in [Opened::WithAMessage, Opened::WithNone] {
        for at_open in [AtOpen::Messages, AtOpen::Compacted] {
            for then in [
                Then::Nothing,
                Then::Delivered,
                Then::KeptSteps,
                Then::KeptNoStep,
                Then::KeptTheTurnBefore,
                Then::CompactedAtRest,
            ] {
                for wire in [
                    Wire::Silent,
                    Wire::Replayed,
                    Wire::Instant,
                    Wire::Working,
                    Wire::Blocked,
                    Wire::Ended,
                ] {
                    let row = Row {
                        opened,
                        at_open,
                        then,
                        wire,
                    };
                    // The replay and the TUI's own answer are looked at where nothing was
                    // given: they are no turn, and nothing else is said of them.
                    let only_at_rest = matches!(wire, Wire::Replayed | Wire::Instant);
                    if row.happens() && (!only_at_rest || then == Then::Nothing) {
                        rows.push(row);
                    }
                }
            }
        }
    }
    rows
}

#[test]
fn a_window_that_began_a_turn_of_its_own_is_never_at_rest_before_that_turn_ends() {
    let mut table = Vec::new();
    let mut wrong = Vec::new();
    for row in rows() {
        for looks in [Looks::EveryStep, Looks::OpenAndEnd] {
            let read = play(row, looks);
            let meant = !row.turn_open();
            let line = format!(
                "{:<12} {:<9} {:<17} {:<8} {:<10} reads {:<7} {}",
                format!("{:?}", row.opened),
                format!("{:?}", row.at_open),
                format!("{:?}", row.then),
                format!("{:?}", row.wire),
                format!("{looks:?}"),
                if read { "at rest" } else { "at work" },
                if read == meant { "" } else { "WRONG" },
            );
            if read != meant {
                wrong.push(line.clone());
            }
            table.push(line);
        }
    }
    eprintln!("{} rows:\n{}", table.len(), table.join("\n"));
    assert!(
        wrong.is_empty(),
        "{} of {} rows read wrong:\n{}",
        wrong.len(),
        table.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_case_the_review_found_is_a_row_of_the_table() {
    // T-1401: opened again with no message on a chain compacted at rest, a task delivered,
    // a compaction in the middle of its turn that keeps no step, and Devin's `blocked`
    // before the turn's end.
    let row = Row {
        opened: Opened::WithNone,
        at_open: AtOpen::Compacted,
        then: Then::KeptNoStep,
        wire: Wire::Blocked,
    };
    assert!(rows()
        .iter()
        .any(|listed| format!("{listed:?}") == format!("{row:?}")));
    for looks in [Looks::EveryStep, Looks::OpenAndEnd] {
        assert!(!play(row, looks), "{looks:?}: a turn of its own is open");
    }
}

#[test]
fn a_turn_of_its_own_that_ended_leaves_the_window_at_rest_on_a_chain_put_back_as_at_its_open() {
    // The old process's turn was cut after a step whose call has no result, and the window
    // is opened on it with no message: at rest, though the record alone reads that turn
    // not ended. A task is delivered and worked; a compaction in the middle of the turn
    // keeps a copy of the cut step alone, which reads the old message back, so the chain
    // holds the user's messages it held at the open; and the turn's end is said with
    // nothing stored after the compaction. The window's turn is over, and the record has
    // nothing of a turn of its own open: at rest, not held at work by the old cut turn.
    let row = Row {
        opened: Opened::WithNone,
        at_open: AtOpen::Cut,
        then: Then::KeptTheTurnBefore,
        wire: Wire::Ended,
    };
    for looks in [Looks::EveryStep, Looks::OpenAndEnd] {
        let mut scene = Scene::new(row, looks);
        assert!(scene.at_rest(), "{looks:?}: at rest at its open");
        scene.adds("n-asks", &asks());
        scene.says(&[status("working")]);
        scene.adds("n-step", &step());
        scene.adds("n-tool", &tool());
        scene.compacts(&[(CUT, cut_step())]);
        scene.says(&[
            status("blocked"),
            json!({ "sessionId": SESSION, "turnClientMessageId": "request-now" }),
            json!({ "sessionId": SESSION, "cause": "complete", "stats": {} }),
        ]);
        assert!(scene.at_rest(), "{looks:?}: the turn of its own ended");
    }
}
