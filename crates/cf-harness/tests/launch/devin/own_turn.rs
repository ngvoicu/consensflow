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
//! Every row is played on each shape of the wire (T-1423): Devin 3000.11's, which says
//! `working` as a turn begins and `blocked` where it waits or ends; Devin 3000.12.43's,
//! which says neither and says the message back live as the turn takes it in (T-1417); and
//! a log with both, which no Devin recorded has written, so that the rule hangs on what the
//! log holds and on no version.
//!
//! The invariant, held at every look of a row played with a look at every step: no step
//! with a turn of this window's open reads at rest, and every step with none open (nothing
//! given, its turn's end said) does, so nothing holds a window at rest for good, or at work.
//! The one look not held is the open of a window given a first message, before anything of
//! the message reached the store or the log. Rows left out, each for one reason:
//! - a turn that waits (`blocked`) or ended with nothing of it in the store: a turn that
//!   came to its end has its message in the store, and that is the "delivered" row;
//! - a compaction in the middle of a turn with a silent wire: Devin says `working` as a
//!   turn begins, before its first step, and a compaction comes at a step (T-660's
//!   probes; T-1401 did not count the order's other way as a blocker, and this table
//!   does not build for it: such a row reads at rest, see the result of T-1403);
//! - a compaction at rest with the wire saying a turn works or waits: a compaction while
//!   a turn is open is one of the mid-turn rows;
//! - `blocked` on 3000.12.43's shape, which says no such word;
//! - a dialog, a cancel or a message said back with nothing after where the turn is not one
//!   given since the open with nothing compacted under it: each is a step of a turn, and the
//!   compactions are rows of their own.

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

/// The shape of the launch's wire log: which Devin wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Devin 3000.11: `working` as a turn begins, then the message said back with neither its
    /// text's event type nor an event id; `blocked` where the turn waits and before its end.
    Words,
    /// Devin 3000.12.43: no word of its own on a turn; the message said back live as the turn
    /// takes it in (its event type, its client id, no event id); a dialog a line of its own.
    Unsaid,
    /// Both: 3000.12.43's lines and 3000.11's words.
    Both,
}

/// What the launch's wire log says at the look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wire {
    /// Nothing more than the window's configuration.
    Silent,
    /// The history a reopened window replays: stamped, in no stream; on 3000.12.43 each item
    /// with its event id, and the turn named with no cause.
    Replayed,
    /// What the TUI answers itself at once to a command of its own (`/bypass`).
    Instant,
    /// The turn's opening (3000.11's `working`, 3000.12.43's message said back) and nothing
    /// after it: a model's first call that says nothing for as long as it takes.
    SaidBack,
    /// The turn's opening, and its thought.
    Working,
    /// Then the dialog that asks the user to allow a call (3000.11 said `blocked` before it).
    Asks,
    /// Then Devin's word that it waits, before the turn's end (no 3000.12.43 says it).
    Blocked,
    /// Then the turn's end.
    Ended,
    /// Then the turn cancelled, as the app's Stop cancels it (T-1423).
    Cancelled,
}

#[derive(Debug, Clone, Copy)]
struct Row {
    shape: Shape,
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
            Wire::Ended | Wire::Cancelled => false,
            Wire::SaidBack | Wire::Working | Wire::Asks | Wire::Blocked => true,
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
        if self.shape == Shape::Unsaid && self.wire == Wire::Blocked {
            return false;
        }
        if matches!(self.wire, Wire::Asks | Wire::Cancelled) {
            return self.then == Then::Delivered;
        }
        if self.wire == Wire::SaidBack {
            return matches!(self.then, Then::Nothing | Then::Delivered);
        }
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
    /// Whether a turn of the window's own is open now: from its message taken, in the store
    /// or said on the wire, to its end said.
    open: std::cell::Cell<bool>,
    /// What the step just played was, for a look that reads it wrong.
    step: std::cell::RefCell<String>,
    /// The looks that read wrong: the step, and what it read.
    wrong: std::cell::RefCell<Vec<String>>,
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
            open: std::cell::Cell::new(false),
            step: std::cell::RefCell::new("the open".to_owned()),
            wrong: std::cell::RefCell::new(Vec::new()),
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
        *self.step.borrow_mut() = format!("compaction {at}");
        self.looks_now();
    }

    /// A row below the head.
    fn adds(&mut self, node: &str, written: &Value) {
        self.store
            .writes(SESSION, node, Some(&self.head.clone()), written);
        self.head = node.to_owned();
        if node == "n-asks" {
            self.open.set(true);
        }
        *self.step.borrow_mut() = format!("store {node}");
        self.looks_now();
    }

    /// Lines of the wire, `what` they are, which open the window's own turn or end it.
    fn says_that(&self, what: &str, lines: &[Value], opens: Option<bool>) {
        for line in lines {
            self.home.appends(&format!("{line}\n"));
        }
        if let Some(open) = opens {
            self.open.set(open);
        }
        *self.step.borrow_mut() = format!("wire {what}");
        self.looks_now();
    }

    fn says(&self, what: &str, lines: &[Value]) {
        self.says_that(what, lines, None);
    }

    /// A look after the step just played, held to whether a turn of the window's own is open.
    fn looks_now(&self) {
        if self.looks == Looks::EveryStep && self.window.is_some() {
            let read = self.at_rest();
            if read == self.open.get() {
                self.wrong.borrow_mut().push(format!(
                    "after {}: {}",
                    self.step.borrow(),
                    if read { "at rest" } else { "at work" }
                ));
            }
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

/// The user's message as Devin 3000.12.43 says it back live as its turn takes it in.
fn said_back_live() -> Value {
    json!({
        "sessionId": SESSION,
        "update": {
            "sessionUpdate": "user_message_chunk",
            "content": { "type": "text", "text": TASK },
            "_meta": {
                "cognition.ai/clientMessageId": "request-now",
                "cognition.ai/eventType": "user_message",
                "cognition.ai/message": TASK,
                "cognition.ai/messageSubIndex": 0,
                "cognition.ai/timestamp": "2026-11-10T20:35:14.000000+00:00",
            },
        },
    })
}

/// The user's message as Devin 3000.11 says it back, live or replayed: its client id, its
/// part and its stamp alone.
fn said_back_old(stamp: &str) -> Value {
    json!({
        "sessionId": SESSION,
        "update": {
            "sessionUpdate": "user_message_chunk",
            "content": { "type": "text", "text": TASK },
            "_meta": {
                "cognition.ai/clientMessageId": "request-now",
                "cognition.ai/messageSubIndex": 0,
                "cognition.ai/timestamp": stamp,
            },
        },
    })
}

/// The dialog that asks the user to allow the turn's call, a line of its own.
fn asked() -> Value {
    json!({
        "sessionId": SESSION,
        "toolCall": {
            "toolCallId": "exec:0#call-now",
            "_meta": { "cognition.ai/editableCommand": "cargo test" },
        },
        "options": [
            { "optionId": "allow_once", "name": "Allow", "kind": "allow_once" },
            { "optionId": "reject_once", "name": "Reject", "kind": "reject_once" },
        ],
    })
}

impl Shape {
    /// The lines a turn opens with.
    fn opening(self) -> Vec<Value> {
        match self {
            Self::Words => vec![
                status("working"),
                said_back_old("2026-11-10T20:35:14.000000+00:00"),
            ],
            Self::Unsaid => vec![said_back_live()],
            Self::Both => vec![status("working"), said_back_live()],
        }
    }

    /// The turn's opening, and its thought.
    fn working(self) -> Vec<Value> {
        let mut lines = self.opening();
        lines.push(chunk(
            "agent_thought_chunk",
            "Reading the next file.",
            json!({
                "cognition.ai/timestamp": "2026-11-10T20:35:15.000000+00:00",
                "cognition.ai/streamingMessageId": "stream-now",
            }),
        ));
        lines
    }

    fn says_it_waits(self) -> Vec<Value> {
        match self {
            Self::Unsaid => Vec::new(),
            Self::Words | Self::Both => vec![status("blocked")],
        }
    }

    fn ended(self) -> Vec<Value> {
        let mut lines = self.says_it_waits();
        lines.extend([
            json!({ "sessionId": SESSION, "turnClientMessageId": "request-now",
                "turnRequestId": "model-now-2" }),
            json!({ "sessionId": SESSION, "cause": "complete", "stats": {} }),
        ]);
        lines
    }

    /// The turn cancelled: 3000.11's as the kit recorded it (no `blocked`), 3000.12.43's as
    /// the app's Stop over its thoughts did (T-1423 run 5).
    fn cancelled(self) -> Vec<Value> {
        match self {
            Self::Words => vec![
                json!({ "sessionId": SESSION, "turnClientMessageId": "request-now",
                    "turnRequestId": "model-now-2" }),
                json!({ "sessionId": SESSION, "cause": "cancelled", "stats": {} }),
            ],
            Self::Unsaid | Self::Both => {
                let mut lines = self.says_it_waits();
                lines.extend([
                    json!({ "sessionId": SESSION, "turnClientMessageId": "request-now" }),
                    json!({ "cause": "cancelled", "stats": {}, "sessionId": SESSION }),
                    json!({ "durationMs": 4886, "blockIndex": 0, "sessionId": SESSION }),
                ]);
                lines
            }
        }
    }

    fn asks(self) -> Vec<Value> {
        let mut lines = self.says_it_waits();
        lines.push(asked());
        lines
    }

    /// What a reopened window replays of the turn before.
    fn replayed(self) -> Vec<Value> {
        match self {
            Self::Words => vec![
                said_back_old("2026-10-13T10:00:00.000000+00:00"),
                chunk(
                    "agent_message_chunk",
                    "Done.",
                    json!({ "cognition.ai/timestamp": "2026-10-13T10:00:05.000000+00:00" }),
                ),
            ],
            Self::Unsaid | Self::Both => {
                let mut user = said_back_live();
                user["update"]["_meta"]["cognition.ai/eventId"] = json!("3");
                vec![
                    user,
                    chunk(
                        "agent_message_chunk",
                        "Done.",
                        json!({ "cognition.ai/timestamp": "2026-10-13T10:00:05.000000+00:00",
                            "cognition.ai/eventId": "5" }),
                    ),
                    json!({ "sessionId": SESSION, "turnClientMessageId": "request-before" }),
                ]
            }
        }
    }
}

/// Plays `row` with `looks`: what the window reads at the end, and the looks before it that
/// read wrong.
fn play(row: Row, looks: Looks) -> (bool, Vec<String>) {
    let shape = row.shape;
    let mut scene = Scene::new(row, looks);
    let at_open = scene.at_rest();
    if row.opened == Opened::WithNone {
        assert!(
            at_open,
            "{row:?}: a window given nothing is at rest at its open"
        );
    }
    // A turn given: 3000.11 stores the message, then says it works; 3000.12.43 says the
    // message back first, and stores it 0.5 to 27 s after (T-1423).
    let delivers = |scene: &mut Scene, lines: Vec<Value>| {
        if shape == Shape::Words {
            scene.adds("n-asks", &asks());
            if !lines.is_empty() {
                scene.says_that("the opening", &lines, Some(true));
            }
        } else {
            if !lines.is_empty() {
                scene.says_that("the opening", &lines, Some(true));
            }
            scene.adds("n-asks", &asks());
        }
    };
    match row.then {
        Then::Nothing | Then::CompactedAtRest => {
            if row.then == Then::CompactedAtRest && row.wire == Wire::Ended {
                // A turn of its own, ended, then the compaction at rest.
                delivers(&mut scene, shape.working());
                scene.adds("n-answers", &answers());
                scene.says_that("the end", &shape.ended(), Some(false));
            }
            match row.wire {
                Wire::Replayed => scene.says("the replay", &shape.replayed()),
                Wire::Instant => scene.says(
                    "the TUI's own answer",
                    &[chunk(
                        "agent_message_chunk",
                        "Bypass mode on.",
                        json!({
                            "cognition.ai/instant": true,
                            "cognition.ai/streamingMessageId": "stream-instant",
                        }),
                    )],
                ),
                Wire::Working => scene.says_that("the opening", &shape.working(), Some(true)),
                Wire::SaidBack => scene.says_that("the opening", &shape.opening(), Some(true)),
                _ => {}
            }
            if row.then == Then::CompactedAtRest {
                scene.compacts(&[]);
            }
        }
        _ => match row.wire {
            Wire::Silent => delivers(&mut scene, Vec::new()),
            Wire::SaidBack => delivers(&mut scene, shape.opening()),
            _ => {
                delivers(&mut scene, shape.working());
                scene.adds("n-step", &step());
                if row.wire == Wire::Asks {
                    scene.says("the dialog", &shape.asks());
                } else {
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
                        Wire::Blocked => scene.says("blocked", &[status("blocked")]),
                        Wire::Ended => {
                            // Its answer, below whatever the chain holds, unless the
                            // compaction kept nothing of the turn at its end.
                            if row.then != Then::KeptNoStep {
                                scene.adds("n-answers", &answers());
                            }
                            scene.says_that("the end", &shape.ended(), Some(false));
                        }
                        Wire::Cancelled => {
                            scene.says_that("the cancel", &shape.cancelled(), Some(false));
                        }
                        _ => {}
                    }
                }
            }
        },
    }
    let wrong = scene.wrong.borrow().clone();
    (scene.at_rest(), wrong)
}

fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for shape in [Shape::Words, Shape::Unsaid, Shape::Both] {
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
                        Wire::SaidBack,
                        Wire::Working,
                        Wire::Asks,
                        Wire::Blocked,
                        Wire::Ended,
                        Wire::Cancelled,
                    ] {
                        let row = Row {
                            shape,
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
    }
    rows
}

#[test]
fn a_window_that_began_a_turn_of_its_own_is_never_at_rest_before_that_turn_ends() {
    let mut table = Vec::new();
    let mut wrong = Vec::new();
    for row in rows() {
        for looks in [Looks::EveryStep, Looks::OpenAndEnd] {
            let (read, steps) = play(row, looks);
            let meant = !row.turn_open();
            let right = read == meant && steps.is_empty();
            let line = format!(
                "{:<6} {:<12} {:<9} {:<17} {:<9} {:<10} reads {:<7} {}{}",
                format!("{:?}", row.shape),
                format!("{:?}", row.opened),
                format!("{:?}", row.at_open),
                format!("{:?}", row.then),
                format!("{:?}", row.wire),
                format!("{looks:?}"),
                if read { "at rest" } else { "at work" },
                if right { "" } else { "WRONG" },
                if steps.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", steps.join("; "))
                },
            );
            if !right {
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
    for shape in [Shape::Words, Shape::Unsaid, Shape::Both] {
        let row = Row {
            shape,
            opened: Opened::WithNone,
            at_open: AtOpen::Compacted,
            then: Then::KeptNoStep,
            // 3000.12.43 says no `blocked`: its turn goes on in silence.
            wire: if shape == Shape::Unsaid {
                Wire::Working
            } else {
                Wire::Blocked
            },
        };
        assert!(rows()
            .iter()
            .any(|listed| format!("{listed:?}") == format!("{row:?}")));
        for looks in [Looks::EveryStep, Looks::OpenAndEnd] {
            let (read, wrong) = play(row, looks);
            assert!(!read, "{row:?} {looks:?}: a turn of its own is open");
            assert!(wrong.is_empty(), "{row:?} {looks:?}: {wrong:?}");
        }
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
    for shape in [Shape::Words, Shape::Unsaid, Shape::Both] {
        let row = Row {
            shape,
            opened: Opened::WithNone,
            at_open: AtOpen::Cut,
            then: Then::KeptTheTurnBefore,
            wire: Wire::Ended,
        };
        for looks in [Looks::EveryStep, Looks::OpenAndEnd] {
            let mut scene = Scene::new(row, looks);
            assert!(scene.at_rest(), "{shape:?} {looks:?}: at rest at its open");
            scene.says_that("the opening", &shape.opening(), Some(true));
            scene.adds("n-asks", &asks());
            scene.adds("n-step", &step());
            scene.adds("n-tool", &tool());
            scene.compacts(&[(CUT, cut_step())]);
            scene.says_that("the end", &shape.ended(), Some(false));
            assert!(
                scene.at_rest(),
                "{shape:?} {looks:?}: the turn of its own ended"
            );
            assert!(
                scene.wrong.borrow().is_empty(),
                "{shape:?} {looks:?}: {:?}",
                scene.wrong.borrow()
            );
        }
    }
}
