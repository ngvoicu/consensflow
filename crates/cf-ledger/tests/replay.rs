//! Every ledger the Node suite opened, replayed against this one (recorded from
//! Node's suites and fixed since: `tests/traces/README.md`): the same file to
//! start from, the same clock readings and the same calls, and each call's
//! answer or refusal, the events it logged and the clock readings it took, then
//! the database it left, compared exactly, but for the four columns of
//! migration 0011 that Node's ledger never writes (`cf_ledger::testing`). A
//! call this replay does not know fails its trace.
//!
//! A trace this ledger departs from Node's on purpose is named in
//! [`DEPARTED`], with the call where its answer first departs and why. It is
//! counted and printed instead of replayed, and a test of its own replays it:
//! the replay must agree with Node's recording up to that call, and depart at
//! it, and it fails when Node's recording replays clean again, so the
//! departure is taken off once Node does it too. A ledger trace is the calls
//! Node's dispatcher made, so what a departure changes of what a dispatcher
//! asks of the ledger is held by the engine's tests: by traces of their own for
//! the pause notes' (`crates/cf-engine/tests/departures/`), and by what each
//! asserts directly for the redesign's.

// The replay's own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use base64::Engine;
use cf_base::js;
use cf_base::time::{parse, Clock};
use cf_ledger::model::{parse_gate, parse_roles};
use cf_ledger::testing::{hold_apart_what_node_never_logs, hold_apart_what_node_never_writes};
use cf_ledger::{
    open_ledger, ChiefSwitch, Event, Ledger, LedgerError, NewMember, NewNote, NewProject,
    NewQuestion, NewTask, Options,
};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{json, Value};

/// What a replay found.
enum Outcome {
    /// Every call answered as Node's did: the methods called, one per call.
    Replayed(Vec<String>),
    /// Probes what only JavaScript could be handed: nothing to replay here.
    Skipped(String),
    Failed(String),
}

/// The traces this ledger departs from Node's recording of on purpose, by
/// their file's name without `.json.gz`, each with the call where its replay
/// first departs and the one thing that differs from Node's. Found by
/// replaying the recordings against this ledger, not by guessing: a trace is
/// here when its replay fails for a rule this ledger changed on purpose, and
/// only then. The receipt and stop redesign moved all of them but
/// `core-dispatcher-042` and `-074`, which the pause notes' withdrawal moved:
/// Node never withdraws a note that told a requester a task was paused, so
/// the notes this ledger withdraws when the task is resumed are, in Node's
/// recordings, queued still, and its dispatcher goes on to paste them.
/// Departed traces that Node's suite records only off Windows: the test
/// that writes `home-copies-004` is skipped there, as Windows holds an open
/// ledger's files (`tests/home-copies.test.mjs`). Elsewhere each must be there.
const RECORDED_OFF_WINDOWS: &[&str] = &["home-copies-004"];

const DEPARTED: &[(&str, usize, &str)] = &[
    ("core-api-006", 15, "the door's read `answerTo` is gone (a poll claims with `claim_answer`), and a choice answer lands queued, not read"),
    ("core-dispatcher-010", 121, REASON_PAUSE),
    ("core-dispatcher-042", 72, "the chief resumes T-1 before its window came back to take the note that T-1 is paused: the note is withdrawn, so the chief has no message waiting (`withWork`), where Node's has the note"),
    ("core-dispatcher-065", 147, KEPT_IN_BRIEF),
    ("core-dispatcher-074", 294, "the daemon resumes T-3 when its hold ends, and the note that said T-3 waits, which the chief had not been given, is withdrawn (`task`), where Node leaves it queued"),
    ("core-dispatcher-107", 122, KEPT_IN_BRIEF),
    ("core-page-014", 9, REASON_RELEASE),
    ("home-copies-001", 34, KEPT_IN_BRIEF),
    ("home-copies-004", 34, KEPT_IN_BRIEF),
    ("ledger-gate-006", 25, DOOR_READ),
    ("ledger-gate-007", 15, DOOR_READ),
    ("ledger-gate-008", 16, "the door's read `answerTo` is gone, and the choice answer the human approves lands queued, not read for the door"),
    ("ledger-messages-009", 9, "the door's read `answerTo` is gone, and a choice answer lands queued: its task waits until it is received"),
    ("ledger-messages-010", 11, READ_AT_ONCE),
    ("ledger-messages-011", 10, "the asker's own window answering first is received at once with the receipt `{window: true}` and its time; Node left both empty"),
    ("ledger-messages-013", 10, READ_AT_ONCE),
    ("ledger-messages-014", 10, READ_AT_ONCE),
    ("ledger-messages-015", 11, READ_AT_ONCE),
    ("ledger-messages-017", 10, DOOR_READ),
    ("ledger-messages-022", 12, DOOR_READ),
    ("ledger-projects-002", 14, KEPT_IN_BRIEF),
    ("ledger-schema-005", 15, KEPT_IN_BRIEF),
    ("ledger-schema-006", 18, KEPT_IN_BRIEF),
    ("ledger-schema-007", 15, KEPT_IN_BRIEF),
    ("ledger-schema-010", 14, KEPT_IN_BRIEF),
    ("ledger-schema-011", 15, KEPT_IN_BRIEF),
    ("ledger-schema-013", 14, KEPT_IN_BRIEF),
    ("ledger-schema-014", 15, KEPT_IN_BRIEF),
    ("ledger-schema-016", 14, KEPT_IN_BRIEF),
    ("ledger-schema-017", 15, KEPT_IN_BRIEF),
    ("ledger-schema-019", 15, KEPT_IN_BRIEF),
    ("ledger-schema-020", 14, KEPT_IN_BRIEF),
    ("ledger-schema-021", 15, KEPT_IN_BRIEF),
    ("ledger-schema-023", 14, KEPT_IN_BRIEF),
    ("ledger-schema-024", 15, KEPT_IN_BRIEF),
    ("ledger-schema-025", 15, KEPT_IN_BRIEF),
    ("ledger-schema-027", 14, KEPT_IN_BRIEF),
    ("ledger-staff-008", 13, KEPT_BY_PAUSE),
    ("ledger-staff-019", 18, KEPT_BY_PAUSE),
    ("ledger-tasks-005", 26, REASON_PAUSE),
    ("ledger-tasks-016", 10, KEPT_BY_PAUSE),
    ("ledger-tasks-018", 16, KEPT_BY_PAUSE),
    ("ledger-tasks-020", 19, REASON_PAUSE),
    ("ledger-tasks-023", 10, STOP_IN_EVENT),
    ("ledger-tasks-024", 13, STOP_IN_EVENT),
    ("ledger-tasks-026", 29, STOP_IN_EVENT),
    ("ledger-tiered-013", 13, KEPT_IN_BRIEF),
    ("ledger-tiered-014", 22, WINDOW_ENDED),
];

/// The door's poll is a write now, `claim_answer`; `answerTo`, its read, went.
const DOOR_READ: &str =
    "the door's read `answerTo` is gone: a poll claims the answer with `claim_answer`";
/// An answer is received, not read at its creation.
const READ_AT_ONCE: &str =
    "a choice answer lands queued: Node marked it read and stamped it at its creation, two clock readings more";
/// What a window kept goes to the next window in the brief, not with the old one.
const KEPT_IN_BRIEF: &str =
    "a release carries what the old window kept into the task's brief, once (\"Kept from before\"); Node let it go with the window";
/// The reason a release gives the rows it carried.
const REASON_RELEASE: &str =
    "a release cancels the old window's kept rows with the reason `carried into T-n's brief for its next window`; Node gave none";
/// The reason a pause gives what the chief wrote and takes back.
const REASON_PAUSE: &str =
    "a chief's pause withdraws what the chief wrote for the window with the reason `withdrawn by @chief's pause`; Node gave none";
/// A pause keeps what is on its way, as a receipt alone resolves it.
const KEPT_BY_PAUSE: &str =
    "a pause or a hold keeps what is on its way (a question, a task message, a result); Node's `dropQueued` cancelled every queued row of the task";
/// A pause's event says which stop it asked.
const STOP_IN_EVENT: &str =
    "a pause's `task.state` event carries `stop`, the sequence of the stop it asked: Node's has no such key";
/// The reason a release gives what the old window still held for the human.
const WINDOW_ENDED: &str =
    "a release withdraws what the old window still held for the human with the reason `withdrawn: @x's window ended first`; Node gave none";

/// What Node's ledger drew in each call (the clock's readings, session
/// names), answered here in the same order; drawing past them is noted.
struct Draws<T> {
    left: Rc<RefCell<VecDeque<T>>>,
    overdrawn: Rc<Cell<bool>>,
}

impl<T> Draws<T> {
    fn new() -> Self {
        Self {
            left: Rc::new(RefCell::new(VecDeque::new())),
            overdrawn: Rc::new(Cell::new(false)),
        }
    }

    fn share(&self) -> Self {
        Self {
            left: Rc::clone(&self.left),
            overdrawn: Rc::clone(&self.overdrawn),
        }
    }

    fn draw(&self) -> Option<T> {
        let next = self.left.borrow_mut().pop_front();
        if next.is_none() {
            self.overdrawn.set(true);
        }
        next
    }

    /// After a call: why not, when it did not draw what Node's call drew.
    fn settle(&self, what: &str, recorded: usize) -> Option<String> {
        let left = self.left.borrow_mut().drain(..).count();
        if self.overdrawn.replace(false) {
            return Some(format!("{what} more than Node's {recorded} times"));
        }
        (left > 0).then(|| format!("{what} {} times, Node {recorded}", recorded - left))
    }
}

/// A clock that answers the readings Node recorded, in order.
struct Recorded(Draws<i64>);

impl Clock for Recorded {
    fn now_ms(&mut self) -> i64 {
        self.0.draw().unwrap_or_default()
    }
}

fn traces() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("traces")
}

/// Every recorded trace, by its file's name without `.json.gz`, in that order.
fn recorded() -> Vec<(String, String)> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(traces())
        .expect("the traces: they are fixed recordings (tests/traces/README.md)")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "gz"))
        .collect();
    names.sort();
    assert!(
        !names.is_empty(),
        "no traces: they are fixed recordings (tests/traces/README.md)"
    );
    names
        .iter()
        .map(|name| {
            let mut text = String::new();
            flate2::read::GzDecoder::new(std::fs::File::open(name).unwrap())
                .read_to_string(&mut text)
                .unwrap();
            let stem = name.file_name().unwrap().to_string_lossy();
            (stem.trim_end_matches(".json.gz").to_string(), text)
        })
        .collect()
}

/// The call where a trace's answer first departs, and why, when it is named
/// in [`DEPARTED`].
fn departed(trace: &str) -> Option<(usize, &'static str)> {
    DEPARTED
        .iter()
        .find(|(name, ..)| *name == trace)
        .map(|(_, at, why)| (*at, *why))
}

/// What is wrong with a departed trace, or none when it is held as it should
/// be: replayed, it agrees with Node's recording up to call `at`, and departs
/// there.
fn departure(node: &str, at: usize) -> Option<String> {
    match replay(node) {
        Outcome::Failed(why) if why.starts_with(&format!("call {at} (")) => None,
        Outcome::Failed(why) => Some(format!("it departs elsewhere than at call {at}: {why}")),
        Outcome::Replayed(_) => {
            Some("it replays as Node recorded it: take it off DEPARTED".to_owned())
        }
        Outcome::Skipped(why) => Some(format!("it is skipped: {why}")),
    }
}

#[test]
fn every_ledger_the_node_suite_opened_answers_here_as_it_answered_there() {
    let traces = recorded();
    let (mut replayed, mut skipped, mut failed) = (0, BTreeMap::<String, usize>::new(), Vec::new());
    let mut calls = BTreeMap::<String, usize>::new();
    let mut left = Vec::new();
    for (trace, text) in &traces {
        if let Some((at, why)) = departed(trace) {
            left.push(format!("  {trace} at call {at}: {why}"));
            continue;
        }
        match replay(text) {
            Outcome::Replayed(methods) => {
                replayed += 1;
                for method in methods {
                    *calls.entry(method).or_default() += 1;
                }
            }
            Outcome::Skipped(why) => *skipped.entry(why).or_default() += 1,
            Outcome::Failed(why) => failed.push(format!("{trace}: {why}")),
        }
    }
    let skipped_count: usize = skipped.values().sum();
    println!(
        "{replayed} traces replayed, {} departed, {skipped_count} skipped, {} failed of {}",
        left.len(),
        failed.len(),
        traces.len()
    );
    for line in &left {
        println!("{line}");
    }
    for (why, count) in &skipped {
        println!("  skipped {count}: {why}");
    }
    let replayed_calls: Vec<String> = calls
        .iter()
        .map(|(method, count)| format!("{method} {count}"))
        .collect();
    println!("calls replayed: {}", replayed_calls.join(", "));
    assert!(
        failed.is_empty(),
        "{} traces answered otherwise:\n{}",
        failed.len(),
        failed.join("\n")
    );
    assert!(replayed > 0, "nothing replayed");
}

/// A trace is named in [`DEPARTED`] because it differs from Node's, and
/// for no other reason: one that no longer differs, that differs elsewhere
/// than at its call, or that is not there, is a line to take out or to
/// correct, not a test left passing.
#[test]
fn every_departed_trace_is_there_and_still_departs() {
    let traces = recorded();
    let mut wrong = Vec::new();
    for (trace, at, why) in DEPARTED {
        match traces.iter().find(|(name, _)| name == trace) {
            // Not there where its Node test does not run: that is its platform's say.
            None if cfg!(windows) && RECORDED_OFF_WINDOWS.contains(trace) => {}
            None => wrong.push(format!("{trace} is not a recorded trace ({why})")),
            Some((_, text)) => {
                wrong.extend(departure(text, *at).map(|wrong| format!("{trace}: {wrong} ({why})")))
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

fn replay(text: &str) -> Outcome {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    // The recorder wrote the ledger's path as «ledger»: this replay's goes back.
    let path = serde_json::to_string(&file.display().to_string()).unwrap();
    let trace: Value =
        serde_json::from_str(&text.replace("«ledger»", &path[1..path.len() - 1])).unwrap();
    let calls = trace["calls"].as_array().cloned().unwrap_or_default();
    // A trace is replayed only whole: a call this replay does not know fails it.
    if let Some(unknown) = calls
        .iter()
        .enumerate()
        .find_map(|(at, call)| unknown_call(at, call))
    {
        return Outcome::Failed(unknown);
    }
    if let Some(probe) = calls.iter().find_map(probe_of_javascript) {
        return Outcome::Skipped(probe);
    }
    let held = start_from(&trace["initial"], &file);
    let readings = Draws::<i64>::new();
    let names = Draws::<String>::new();
    let told = Rc::new(RefCell::new(Vec::<Event>::new()));
    let events = Rc::clone(&told);
    let drawn = names.share();
    let opened = open_ledger(
        &file,
        Options {
            clock: Box::new(Recorded(readings.share())),
            names: Box::new(move || drawn.draw().unwrap_or_default()),
            trace: Box::new(move |event| events.borrow_mut().push(event.clone())),
        },
    );
    drop(held);
    let mut ledger = match (opened, trace.get("openError")) {
        (Err(error), Some(expected)) => {
            return compare(
                "the opening",
                &failure(&error),
                &json!({ "$error": expected }),
            )
            .map_or(Outcome::Replayed(Vec::new()), Outcome::Failed);
        }
        (Ok(_), Some(expected)) => {
            return Outcome::Failed(format!("opened, where Node was refused: {expected}"))
        }
        (Err(error), None) => {
            return Outcome::Failed(format!("refused, where Node opened: {error}"))
        }
        (Ok(ledger), None) => Some(ledger),
    };
    for (at, call) in calls.iter().enumerate() {
        let method = call["method"].as_str().unwrap_or_default();
        let recorded = call["clock"].as_array().unwrap();
        readings.left.borrow_mut().extend(
            recorded
                .iter()
                .map(|at| parse(at.as_str().unwrap()).expect("a clock reading Node wrote")),
        );
        let named = call["names"].as_array().unwrap();
        names.left.borrow_mut().extend(
            named
                .iter()
                .map(|name| name.as_str().expect("a name Node drew").to_string()),
        );
        let answer = match (method, ledger.take()) {
            (_, None) => {
                return Outcome::Failed(format!("call {at} ({method}) after the ledger closed"))
            }
            ("close", Some(open)) => open
                .close()
                .map_or_else(|error| failure(&error), |()| undefined()),
            (_, Some(mut open)) => {
                let answered = answer(&mut open, call);
                ledger = Some(open);
                match answered {
                    Ok(answered) => answered,
                    Err(why) => return Outcome::Failed(format!("call {at} ({method}): {why}")),
                }
            }
        };
        let settled = [
            readings.settle("read the clock", recorded.len()),
            names.settle("drew a session name", named.len()),
        ];
        if let Some(why) = settled.into_iter().flatten().next() {
            return Outcome::Failed(format!("call {at} ({method}) {why}"));
        }
        let mut logged: Vec<Value> = told
            .borrow_mut()
            .drain(..)
            .map(|event| json!({ "at": event.at, "project": event.project, "kind": event.kind, "data": event.data }))
            .collect();
        hold_apart_what_node_never_logs(&mut logged);
        if let Some(why) = compare(&format!("call {at} ({method})"), &answer, &call["result"]) {
            return Outcome::Failed(why);
        }
        if let Some(why) = compare(
            &format!("call {at} ({method}) logged"),
            &json!(logged),
            &call["events"],
        ) {
            return Outcome::Failed(why);
        }
        if method == "close" {
            let (ours, theirs) = (held_apart(dump(&file)), held_apart(trace["final"].clone()));
            if let Some(why) = compare(
                &format!("call {at} ({method}), the database it left,"),
                &ours,
                &theirs,
            ) {
                return Outcome::Failed(why);
            }
        }
    }
    Outcome::Replayed(
        calls
            .iter()
            .map(|call| call["method"].as_str().unwrap_or_default().to_string())
            .collect(),
    )
}

/// The call `at` when this replay does not know it, said as the failure it
/// is; none for a method it does.
fn unknown_call(at: usize, call: &Value) -> Option<String> {
    let method = call["method"].as_str().unwrap_or_default();
    const DONE: &[&str] = &[
        "createProject",
        "project",
        "projects",
        "setProjectState",
        "deleteProject",
        "setGate",
        "suspendForRestart",
        "forgetResume",
        "events",
        "integrity",
        "close",
        "addMember",
        "refreshMemberTiers",
        "setRoles",
        "removeMember",
        "lastStaff",
        "holdsWork",
        "candidates",
        "members",
        "endSession",
        "markOut",
        "markBack",
        "startConversation",
        "bindConversation",
        "copyTranscript",
        "transcript",
        "chiefHistory",
        "chiefOpenWork",
        "switchChief",
        "historyRead",
        "lastSwitch",
        "endConversation",
        "currentConversation",
        "copiedItemWith",
        "followConversation",
        "message",
        "createTask",
        "assignTask",
        "releaseTask",
        "checkRelease",
        "recordResult",
        "acceptTask",
        "pauseTask",
        "holdTask",
        "clearHold",
        "heldTasksDue",
        "pausedTask",
        "toldSincePaused",
        "resumeTask",
        "reopenTask",
        "cancelTask",
        "failTask",
        "deleteTasks",
        "task",
        "activeTask",
        "lastTask",
        "note",
        "ask",
        "answer",
        "nextDelivery",
        "withWork",
        "beginDelivery",
        "confirmDelivery",
        "cancelMessage",
        "retryDelivery",
        "failDelivery",
        "inFlight",
        "pending",
        "markRead",
        "approveMessage",
        "declineMessage",
        "inbox",
        "board",
        "openTasks",
        "taskThatFits",
        "latestTranscript",
        "latestMessages",
    ];
    (!DONE.contains(&method))
        .then(|| format!("call {at} ({method}) is one this replay does not know"))
}

/// A probe of what only JavaScript could be handed, which the Rust signature
/// rules out: the one thing a trace is skipped for.
fn probe_of_javascript(call: &Value) -> Option<String> {
    (call["method"] == "copyTranscript" && !call["args"][1].is_array())
        .then(|| "hands copyTranscript items that are no list".into())
}

/// A value a call passed, as the recorder wrote it: `undefined` is no value,
/// so an argument holding it is missing, an object leaves its key out, and
/// a list holds null in its place, as `JSON.stringify` writes it.
fn revive(value: &Value) -> Option<Value> {
    match value {
        Value::Object(fields) if fields.contains_key("$undefined") => None,
        Value::Object(fields) => Some(Value::Object(
            fields
                .iter()
                .filter_map(|(key, item)| Some((key.clone(), revive(item)?)))
                .collect(),
        )),
        Value::Array(items) => Some(Value::Array(
            items
                .iter()
                .map(|item| revive(item).unwrap_or(Value::Null))
                .collect(),
        )),
        other => Some(other.clone()),
    }
}

/// Argument `at`, when the call passed one.
fn arg(args: &[Option<Value>], at: usize) -> Option<&Value> {
    args.get(at).and_then(Option::as_ref)
}

/// Field `name` of argument `at`, an object.
fn field<'a>(args: &'a [Option<Value>], at: usize, name: &str) -> Option<&'a Value> {
    arg(args, at).and_then(|value| value.get(name))
}

fn text(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).expect("text")
}

fn integer(value: Option<&Value>) -> i64 {
    value.and_then(Value::as_i64).expect("an integer")
}

/// One call made here, and its answer as the recorder wrote Node's; why
/// not, when it asked its function argument otherwise than Node's call did.
fn answer(ledger: &mut Ledger, call: &Value) -> Result<Value, String> {
    let method = call["method"].as_str().unwrap_or_default();
    let args: Vec<Option<Value>> = call["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(revive)
        .collect();
    let args = args.as_slice();
    let id = || integer(arg(args, 0));
    let answered: Result<Value, LedgerError> = match method {
        "createProject" => NewProject::from_json(arg(args, 0).expect("a request"))
            .and_then(|request| encode(ledger.create_project(&request))),
        "project" => encode(ledger.project(id())),
        "projects" => encode(ledger.projects()),
        "setProjectState" => encode(ledger.set_project_state(id(), &js::text(arg(args, 1)))),
        "deleteProject" => encode(ledger.delete_project(id())),
        "setGate" => parse_gate(arg(args, 1)).and_then(|gate| encode(ledger.set_gate(id(), gate))),
        "suspendForRestart" => encode(ledger.suspend_for_restart()),
        "forgetResume" => ledger.forget_resume(id()).map(|()| undefined()),
        "events" => {
            let after = field(args, 1, "after").and_then(Value::as_i64);
            let limit = field(args, 1, "limit").and_then(Value::as_i64);
            encode(ledger.events(id(), after.unwrap_or(0), limit.unwrap_or(500)))
        }
        "integrity" => encode(ledger.integrity()),
        "addMember" => NewMember::from_json(arg(args, 1).expect("a member"))
            .and_then(|member| encode(ledger.add_member(id(), &member))),
        "refreshMemberTiers" => return tiers(ledger, &call["callbacks"]),
        "setRoles" => parse_roles(arg(args, 2))
            .and_then(|roles| encode(ledger.set_roles(id(), text(arg(args, 1)), &roles))),
        "removeMember" => encode(ledger.remove_member(id(), text(arg(args, 1)))),
        "lastStaff" => encode(ledger.last_staff()),
        "holdsWork" => encode(ledger.has_task_in_hand(id())),
        "candidates" => encode(ledger.candidates(id(), integer(arg(args, 1)))),
        "members" => encode(ledger.members(id(), text(arg(args, 1)))),
        "endSession" => {
            encode(ledger.end_session(id(), text(arg(args, 1)), text(field(args, 2, "by"))))
        }
        "markOut" => encode(ledger.mark_out(
            id(),
            text(field(args, 1, "until")),
            text(field(args, 1, "reason")),
        )),
        "markBack" => encode(ledger.mark_back(id(), text(field(args, 1, "because")))),
        "startConversation" => {
            encode(ledger.start_conversation(id(), text(field(args, 1, "harness"))))
        }
        "bindConversation" => encode(ledger.bind_conversation(id(), text(arg(args, 1)))),
        "copyTranscript" => {
            let items = arg(args, 1).and_then(Value::as_array).expect("a list");
            let from = field(args, 2, "from").and_then(Value::as_i64);
            encode(ledger.copy_transcript(id(), items, from.unwrap_or(0)))
        }
        "transcript" => {
            let limit = field(args, 2, "limit").and_then(Value::as_u64);
            encode(ledger.transcript(
                id(),
                integer(arg(args, 1)),
                limit.map(|limit| usize::try_from(limit).unwrap()),
            ))
        }
        "chiefHistory" => encode(ledger.chief_history(id())),
        "chiefOpenWork" => encode(ledger.chief_open_work(id())),
        "switchChief" => ChiefSwitch::from_json(arg(args, 1).expect("a switch"))
            .and_then(|switch| encode(ledger.switch_chief(id(), &switch))),
        "historyRead" => ledger
            .history_read(
                id(),
                integer(field(args, 1, "page")),
                field(args, 1, "find").and_then(Value::as_str),
                field(args, 1, "tools")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
            .map(|()| undefined()),
        "lastSwitch" => encode(ledger.last_switch(id())),
        "endConversation" => encode(ledger.end_conversation(id())),
        "currentConversation" => encode(ledger.current_conversation(id())),
        "copiedItemWith" => encode(ledger.copied_item_with(id(), text(arg(args, 1)))),
        "followConversation" => encode(ledger.follow_conversation(
            id(),
            text(field(args, 1, "harness")),
            text(field(args, 1, "nativeSession")),
        )),
        "message" => encode(ledger.message(id())),
        "createTask" => NewTask::from_json(arg(args, 1).expect("a task"))
            .and_then(|task| encode(ledger.create_task(id(), &task))),
        "assignTask" => {
            encode(ledger.assign_task(id(), integer(arg(args, 1)), integer(arg(args, 2))))
        }
        "releaseTask" => encode(ledger.release_task(
            id(),
            integer(arg(args, 1)),
            text(field(args, 2, "because")),
        )),
        "checkRelease" => ledger
            .check_release(id(), integer(arg(args, 1)))
            .map(|()| undefined()),
        "recordResult" => {
            encode(ledger.record_result(id(), integer(arg(args, 1)), text(field(args, 2, "body"))))
        }
        "acceptTask" => {
            encode(ledger.accept_task(id(), integer(arg(args, 1)), text(field(args, 2, "by"))))
        }
        "pauseTask" => encode(ledger.pause_task(
            id(),
            integer(arg(args, 1)),
            field(args, 2, "by").and_then(Value::as_str),
            field(args, 2, "because").and_then(Value::as_str),
        )),
        "holdTask" => encode(ledger.hold_task(
            id(),
            integer(arg(args, 1)),
            text(field(args, 2, "until")),
            text(field(args, 2, "because")),
        )),
        "clearHold" => {
            encode(ledger.clear_hold(id(), integer(arg(args, 1)), text(field(args, 2, "because"))))
        }
        "heldTasksDue" => encode(ledger.held_tasks_due(text(arg(args, 0)))),
        "pausedTask" => encode(ledger.paused_task(id())),
        "toldSincePaused" => encode(ledger.told_since_paused(id(), integer(arg(args, 1)))),
        "resumeTask" => encode(ledger.resume_task(
            id(),
            integer(arg(args, 1)),
            field(args, 2, "by").and_then(Value::as_str),
            text(field(args, 2, "body")),
        )),
        "reopenTask" => encode(ledger.reopen_task(
            id(),
            integer(arg(args, 1)),
            text(field(args, 2, "by")),
            text(field(args, 2, "body")),
        )),
        "cancelTask" => {
            encode(ledger.cancel_task(id(), integer(arg(args, 1)), text(field(args, 2, "by"))))
        }
        "failTask" => {
            encode(ledger.fail_task(id(), integer(arg(args, 1)), text(field(args, 2, "reason"))))
        }
        "deleteTasks" => {
            let numbers: Vec<i64> = arg(args, 1)
                .and_then(Value::as_array)
                .expect("task numbers")
                .iter()
                .map(|number| integer(Some(number)))
                .collect();
            encode(ledger.delete_tasks(id(), &numbers))
        }
        "task" => encode(ledger.task(id(), integer(arg(args, 1)))),
        "activeTask" => encode(
            ledger.active_task(
                id(),
                field(args, 1, "queued")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        ),
        "lastTask" => encode(ledger.last_task(id())),
        "note" => encode(
            ledger.note(
                id(),
                &NewNote {
                    from: field(args, 1, "from")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    to: text(field(args, 1, "to")).to_string(),
                    body: text(field(args, 1, "body")).to_string(),
                    task: field(args, 1, "task").and_then(Value::as_i64),
                },
            ),
        ),
        "ask" => encode(
            ledger.ask(
                id(),
                &NewQuestion {
                    from: field(args, 1, "from")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    to: text(field(args, 1, "to")).to_string(),
                    body: field(args, 1, "body")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    task: field(args, 1, "task").and_then(Value::as_i64),
                    questions: field(args, 1, "questions").cloned(),
                    urgent: field(args, 1, "urgent")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                },
            ),
        ),
        "answer" => encode(ledger.answer(
            id(),
            integer(field(args, 1, "from")),
            field(args, 1, "body"),
            field(args, 1, "choices"),
        )),
        "nextDelivery" => encode(ledger.next_delivery(id())),
        // A Set, as the recorder wrote it.
        "withWork" => ledger.with_work(id()).map(|ids| json!({ "$set": ids })),
        "beginDelivery" => encode(ledger.begin_delivery(id()).map(|begun| begun.message)),
        "confirmDelivery" => encode(ledger.confirm_delivery(id(), arg(args, 1))),
        "cancelMessage" => encode(ledger.cancel_message(id(), text(arg(args, 1)))),
        "retryDelivery" => encode(
            ledger.retry_delivery(
                id(),
                text(arg(args, 1)),
                field(args, 2, "refund")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        ),
        "failDelivery" => encode(ledger.fail_delivery(id(), text(arg(args, 1)))),
        "inFlight" => encode(ledger.in_flight()),
        "pending" => encode(ledger.pending(id())),
        "markRead" => encode(ledger.mark_read(id())),
        "approveMessage" => encode(ledger.approve_message(id(), text(field(args, 1, "by")))),
        "declineMessage" => encode(ledger.decline_message(id(), text(field(args, 1, "by")))),
        "inbox" => encode(
            ledger.inbox(
                id(),
                field(args, 1, "limit")
                    .and_then(Value::as_i64)
                    .unwrap_or(100),
            ),
        ),
        "board" => encode(ledger.board(id())),
        "openTasks" => encode(ledger.open_tasks(id())),
        "taskThatFits" => encode(ledger.task_that_fits(id(), integer(arg(args, 1)))),
        "latestTranscript" => {
            let limit = field(args, 2, "limit").and_then(Value::as_u64);
            encode(ledger.latest_transcript(
                id(),
                integer(arg(args, 1)),
                limit.map(|limit| usize::try_from(limit).unwrap()),
            ))
        }
        "latestMessages" => encode(
            ledger.latest_messages(
                id(),
                field(args, 1, "unread")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        ),
        other => unreachable!("{other} is checked before the replay"),
    };
    Ok(answered.unwrap_or_else(|error| failure(&error)))
}

/// `refreshMemberTiers`, its `tierOf` answered as Node's was: the same
/// agents asked in the same order, each answered what Node's answered.
fn tiers(ledger: &mut Ledger, callbacks: &Value) -> Result<Value, String> {
    let mut recorded = callbacks
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter();
    let mut wrong = None;
    let answered = ledger.refresh_member_tiers(|agent| {
        let asked = json!([agent]);
        match recorded.next() {
            Some(entry) if entry["args"] == asked => entry["result"].as_str().map(str::to_string),
            Some(entry) => {
                wrong.get_or_insert(format!("asked tierOf {asked}, Node {}", entry["args"]));
                None
            }
            None => {
                wrong.get_or_insert(format!("asked tierOf {asked} past Node's last"));
                None
            }
        }
    });
    if let Some(why) = wrong {
        return Err(why);
    }
    if let Some(entry) = recorded.next() {
        return Err(format!("never asked tierOf {}, as Node did", entry["args"]));
    }
    Ok(encode(answered).unwrap_or_else(|error| failure(&error)))
}

fn encode<T: Serialize>(answered: Result<T, LedgerError>) -> Result<Value, LedgerError> {
    answered.map(|value| serde_json::to_value(value).unwrap())
}

fn undefined() -> Value {
    json!({ "$undefined": true })
}

/// A refusal as the recorder wrote one: what the ledger refused, or what SQLite said.
fn failure(error: &LedgerError) -> Value {
    match error {
        LedgerError::Refused(refusal) => json!({ "$error": {
            "name": "LedgerError", "code": refusal.code, "status": refusal.status, "message": refusal.message,
        }}),
        other => json!({ "$error": { "name": "Error", "message": other.to_string() } }),
    }
}

/// `actual` against what Node answered: exactly, key order and all; for an
/// error SQLite raised, its message.
fn compare(what: &str, actual: &Value, expected: &Value) -> Option<String> {
    let (actual, expected) = match (actual.get("$error"), expected.get("$error")) {
        (Some(ours), Some(theirs)) if theirs["name"] != "LedgerError" => (
            json!({ "error": ours["message"] }),
            json!({ "error": theirs["message"] }),
        ),
        _ => (actual.clone(), expected.clone()),
    };
    let (ours, theirs) = (actual.to_string(), expected.to_string());
    (ours != theirs).then(|| {
        let at = ours
            .bytes()
            .zip(theirs.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let from = at.saturating_sub(80);
        let near = |text: &str| {
            text.get(from..(at + 160).min(text.len()))
                .unwrap_or_default()
                .to_string()
        };
        format!(
            "{what} differs at byte {at}:\n    here: …{}…\n    node: …{}…",
            near(&ours),
            near(&theirs)
        )
    })
}

/// The file a trace's ledger found, made again: a database from its dump,
/// bytes as they were, or one another holder keeps (returned, held until the
/// ledger has tried to open it).
fn start_from(initial: &Value, file: &Path) -> Option<Connection> {
    if let Some(database) = initial.get("database") {
        restore(database, file);
    } else if let Some(bytes) = initial.get("bytes").and_then(Value::as_str) {
        std::fs::write(
            file,
            base64::engine::general_purpose::STANDARD
                .decode(bytes)
                .unwrap(),
        )
        .unwrap();
    } else if initial.get("heldHere").is_some() {
        let holder = Connection::open(file).unwrap();
        holder
            .execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE; COMMIT")
            .unwrap();
        return Some(holder);
    }
    None
}

/// A database made again from a recorded dump: its tables, rows (each value
/// as SQLite quoted it, so of its own storage class), indexes, the sequence
/// of its AUTOINCREMENT ids, and its version.
fn restore(database: &Value, file: &Path) {
    let db = Connection::open(file).unwrap();
    // Tables come back in name order, so a row may come before the one it
    // references; and a test may have written rows past the checks on purpose.
    db.execute_batch("PRAGMA foreign_keys = OFF; PRAGMA ignore_check_constraints = ON")
        .unwrap();
    let schema = database["schema"].as_array().unwrap();
    let sql = |entry: &Value| entry["sql"].as_str().unwrap().to_string();
    for entry in schema.iter().filter(|entry| {
        sql(entry).starts_with("CREATE TABLE") && entry["name"] != "sqlite_sequence"
    }) {
        db.execute_batch(&sql(entry)).unwrap();
    }
    let tables = database["tables"].as_object().unwrap();
    for (table, contents) in tables
        .iter()
        .filter(|(table, _)| *table != "sqlite_sequence")
    {
        insert(&db, table, contents);
    }
    for entry in schema
        .iter()
        .filter(|entry| sql(entry).starts_with("CREATE") && !sql(entry).starts_with("CREATE TABLE"))
    {
        db.execute_batch(&sql(entry)).unwrap();
    }
    if let Some(sequence) = tables.get("sqlite_sequence") {
        db.execute_batch("DELETE FROM sqlite_sequence").unwrap();
        insert(&db, "sqlite_sequence", sequence);
    }
    db.execute_batch(&format!(
        "PRAGMA user_version = {}",
        database["userVersion"]
    ))
    .unwrap();
}

fn insert(db: &Connection, table: &str, contents: &Value) {
    let columns: Vec<String> = contents["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| format!("\"{}\"", column.as_str().unwrap()))
        .collect();
    for row in contents["rows"].as_array().unwrap() {
        let values: Vec<&str> = row
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        db.execute_batch(&format!(
            "INSERT INTO \"{table}\" ({}) VALUES ({})",
            columns.join(", "),
            values.join(", ")
        ))
        .unwrap();
    }
}

/// A dump with what only this ledger writes held apart.
fn held_apart(mut dump: Value) -> Value {
    if let Some(Value::Object(tables)) = dump.get_mut("tables") {
        hold_apart_what_node_never_writes(tables);
    }
    dump
}

/// The database a ledger left, as the recorder dumps it: its version, its
/// schema, and each table's rows in rowid order, each value as SQLite quotes it.
fn dump(file: &Path) -> Value {
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let schema: Vec<Value> = db
        .prepare("SELECT name, sql FROM sqlite_master WHERE type IN ('table', 'index') AND sql IS NOT NULL ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "sql": row.get::<_, String>(1)? })))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut tables = serde_json::Map::new();
    for name in names {
        let columns: Vec<String> = db
            .prepare(&format!("PRAGMA table_info(\"{name}\")"))
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let quoted = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect::<Vec<_>>()
            .join(", ");
        let rows: Vec<Value> = db
            .prepare(&format!("SELECT {quoted} FROM \"{name}\" ORDER BY rowid"))
            .unwrap()
            .query_map([], |row| {
                (0..columns.len())
                    .map(|at| row.get::<_, String>(at))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .map(|row| json!(row.unwrap()))
            .collect();
        tables.insert(name, json!({ "columns": columns, "rows": rows }));
    }
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    json!({ "userVersion": version, "schema": schema, "tables": tables })
}
