//! What the runner reads of a board as it goes, and what the oracle judges at the end: the
//! ledger through the page's views (every task by its number, the chief's inbox), the daemon's
//! events, and each window's own record, read as the daemon reads it (`harness-ask records`).
//! Nothing here reads what a model said to judge it by: a record is read for the ConsensFlow
//! headers the product wrote into it, and for the human's own marker.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::live::ask::{self, Item};
use crate::live::members::{session_of, Members};
use crate::Result;

use super::seating::{Role, Seating};

/// The logical tasks of the smoke scenario and the position each is for ([`SMOKE`] binds them).
pub const LOGICAL: [(&str, Role); 4] = [
    ("advice", Role::Advisor),
    ("change", Role::Worker),
    ("review", Role::Reviewer),
    ("test", Role::Tester),
];

/// What the board holds at one moment.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Every task, as `task.get` reads it (its whole thread), by number, oldest first.
    pub tasks: Vec<Value>,
    /// The chief's inbox.
    pub chief_inbox: Vec<Value>,
    /// The board's lanes.
    pub lanes: Vec<Value>,
    /// The daemon's events so far.
    pub events: Vec<Value>,
}

impl Snapshot {
    /// The board of `members` now.
    pub fn of(members: &Members) -> Result<Self> {
        let board = members.board()?;
        let lanes = board["lanes"].as_array().cloned().unwrap_or_default();
        let newest = lanes
            .iter()
            .flat_map(|lane| lane["tasks"].as_array().cloned().unwrap_or_default())
            .filter_map(|task| task["number"].as_i64())
            .max()
            .unwrap_or(0);
        // A task whose session closed may be on no lane any more: every number is asked
        // until the first past the board's newest that is none.
        let mut tasks = Vec::new();
        for number in 1.. {
            let task = members.task(number)?;
            if !task.is_null() {
                tasks.push(task);
            } else if number > newest {
                break;
            }
        }
        Ok(Self {
            tasks,
            chief_inbox: members.inbox(Some("chief"))?,
            lanes,
            events: members.events(),
        })
    }

    /// The task `number`.
    pub fn task(&self, number: i64) -> Option<&Value> {
        self.tasks.iter().find(|task| task["number"] == number)
    }

    /// The result of the task `number` delivered into the chief's window, where one is.
    pub fn delivered_result(&self, number: i64) -> Option<&Value> {
        self.chief_inbox.iter().find(|message| {
            message["kind"] == "result"
                && message["taskNumber"] == number
                && message["state"] == "delivered"
        })
    }

    /// The agent the participant `handle` (a session of a member, `matrix-worker-amber`) is a
    /// session of, as the board's lanes say, and else as the daemon's `session.started` does.
    pub fn agent_of(&self, handle: &str) -> Option<String> {
        self.lanes
            .iter()
            .find(|lane| lane["participant"]["handle"] == handle)
            .and_then(|lane| {
                lane["participant"]["member"]
                    .as_str()
                    .or(lane["participant"]["agent"].as_str())
            })
            .map(str::to_owned)
            .or_else(|| {
                self.events.iter().find_map(|event| {
                    (event["kind"] == "session.started" && event["data"]["handle"] == handle)
                        .then(|| event["data"]["member"].as_str().map(str::to_owned))
                        .flatten()
                })
            })
    }

    /// The turns each window began, by its participant's handle: the more of two counts, each
    /// of which a fast turn can escape alone. One is the times the board saw the window go to
    /// `working` from anything else (the board samples a window's activity, and a turn of a
    /// second can fall between two looks); the other is what went into it, each of which begins
    /// a turn: the messages delivered into it (`delivery.confirmed`; a result or a question to
    /// the chief by the chief's inbox, a task's brief by its `task.assigned`), and, for the
    /// chief, the human's asks that went in (`asks`). A tool loop within one turn is one.
    pub fn turns(&self, asks: u32) -> BTreeMap<String, u32> {
        let mut was: BTreeMap<&str, &str> = BTreeMap::new();
        let mut working: BTreeMap<String, u32> = BTreeMap::new();
        let mut recipient: BTreeMap<i64, String> = BTreeMap::new();
        let mut assignees: BTreeMap<i64, String> = BTreeMap::new();
        for message in &self.chief_inbox {
            if let Some(id) = message["id"].as_i64() {
                recipient.insert(id, "chief".to_owned());
            }
        }
        let mut inputs: BTreeMap<String, u32> = BTreeMap::new();
        if asks > 0 {
            inputs.insert("chief".to_owned(), asks);
        }
        for event in &self.events {
            match event["kind"].as_str() {
                Some("window.activity") => {
                    let (Some(handle), Some(state)) =
                        (event["participant"].as_str(), event["state"].as_str())
                    else {
                        continue;
                    };
                    if state == "working" && was.get(handle) != Some(&"working") {
                        *working.entry(handle.to_owned()).or_default() += 1;
                    }
                    was.insert(handle, state);
                }
                Some("task.assigned" | "task.created") => {
                    let data = &event["data"];
                    let window = data["assignee"].as_str().or_else(|| data["to"].as_str());
                    if let (Some(id), Some(assignee)) = (data["message"].as_i64(), window) {
                        recipient.insert(id, assignee.to_owned());
                        if let Some(task) = data["task"].as_i64() {
                            assignees.insert(task, assignee.to_owned());
                        }
                    }
                }
                Some("task.state") => {
                    // A task sent back (`cf task reopen`) goes to the window that did it, with
                    // the chief's words.
                    let data = &event["data"];
                    let window = data["task"].as_i64().and_then(|task| assignees.get(&task));
                    if let (Some(id), Some(window)) = (data["message"].as_i64(), window) {
                        recipient.insert(id, window.clone());
                    }
                }
                Some("delivery.confirmed") => {
                    let to = event["data"]["message"]
                        .as_i64()
                        .and_then(|id| recipient.get(&id).cloned())
                        .unwrap_or_else(|| "unknown".to_owned());
                    *inputs.entry(to).or_default() += 1;
                }
                _ => {}
            }
        }
        let mut turns = working;
        for (handle, count) in inputs {
            let turn = turns.entry(handle).or_default();
            *turn = (*turn).max(count);
        }
        turns
    }

    /// [`Snapshot::turns`], by seat: each participant's counted for the seat of its agent,
    /// the chief's for the chief's.
    pub fn turns_by_seat(&self, seating: &Seating, asks: u32) -> BTreeMap<&'static str, u32> {
        let mut by_seat: BTreeMap<&'static str, u32> = BTreeMap::new();
        for (handle, count) in self.turns(asks) {
            let role = if handle == "chief" {
                Some(Role::Chief)
            } else {
                self.agent_of(&handle)
                    .and_then(|agent| seating.of_agent(&agent))
                    .map(|seat| seat.role)
            };
            let name = role.map_or("unseated", Role::name);
            *by_seat.entry(name).or_default() += count;
        }
        by_seat
    }

    /// Every turn of every window.
    pub fn total_turns(&self, asks: u32) -> u32 {
        self.turns(asks).values().sum()
    }
}

/// What a logical task is, as the board says it: the pool the chief put it in, or a follow-up
/// (`cf task add --after`) of another bound task, which is in no pool and goes to the session
/// that did the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Is {
    Pool(&'static str),
    After(&'static str),
    /// The chief's own step (`cf task add --self`): in no pool, the chief's, for the chief.
    Own,
}

/// How a logical task is bound: its name, what it is, and who asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bind {
    pub name: &'static str,
    pub is: Is,
    pub requester: &'static str,
}

/// The smoke scenario's tasks: each the chief's, in its pool.
pub const SMOKE: [Bind; 4] = [
    Bind {
        name: "advice",
        is: Is::Pool("advisor"),
        requester: "chief",
    },
    Bind {
        name: "change",
        is: Is::Pool("worker"),
        requester: "chief",
    },
    Bind {
        name: "review",
        is: Is::Pool("reviewer"),
        requester: "chief",
    },
    Bind {
        name: "test",
        is: Is::Pool("tester"),
        requester: "chief",
    },
];

/// Which task is which, bound by what each is ([`Bind`]), never by its number: the oldest that
/// fits. Several that fit are kept, for the oracle to say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bindings {
    /// The logical name of each task and its numbers, oldest first.
    pub found: BTreeMap<&'static str, Vec<i64>>,
}

impl Bindings {
    /// The tasks of `snapshot`, bound as the smoke scenario binds them ([`SMOKE`]).
    pub fn of(snapshot: &Snapshot) -> Self {
        Self::by(snapshot, &SMOKE)
    }

    /// The tasks of `snapshot`, bound by `binds`, in their order: a follow-up's is read
    /// after the task it follows is bound.
    pub fn by(snapshot: &Snapshot, binds: &[Bind]) -> Self {
        let mut bound = Self::default();
        for bind in binds {
            let numbers: Vec<i64> = snapshot
                .tasks
                .iter()
                .filter(|task| task["requester"] == bind.requester)
                .filter(|task| match bind.is {
                    Is::Pool(pool) => task["pool"] == pool,
                    Is::Own => task["pool"].is_null() && task["assignee"] == "chief",
                    Is::After(earlier) => {
                        let did = bound
                            .of_name(earlier)
                            .and_then(|number| snapshot.task(number))
                            .map(|earlier| earlier["assignee"].clone());
                        task["pool"].is_null()
                            && task["requester"] != task["assignee"]
                            && did.is_some_and(|did| !did.is_null() && task["assignee"] == did)
                            && bound.of_name(earlier) != task["number"].as_i64()
                    }
                })
                .filter_map(|task| task["number"].as_i64())
                .collect();
            if !numbers.is_empty() {
                bound.found.insert(bind.name, numbers);
            }
        }
        bound
    }

    /// The task bound to `name`, the `nth` (from 0) of those that fit.
    pub fn nth(&self, name: &str, nth: usize) -> Option<i64> {
        self.found
            .get(name)
            .and_then(|numbers| numbers.get(nth).copied())
    }

    /// The task bound to `name`: the oldest that fits.
    pub fn of_name(&self, name: &str) -> Option<i64> {
        self.found
            .get(name)
            .and_then(|numbers| numbers.first().copied())
    }

    /// The bindings, as the evidence has them.
    pub fn manifest(&self) -> Value {
        serde_json::to_value(&self.found).unwrap_or(Value::Null)
    }
}

/// A request a window's token made of the API, as a tooling daemon writes it
/// (`api.answered`, `cf_daemon::api::calls`): who, what, and how it was answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub participant: String,
    pub method: String,
    pub path: String,
    pub status: u64,
    /// The refusal's code; none where it was answered.
    pub error: Option<String>,
    /// The refusal's words, the product's own; none where it was answered.
    pub message: Option<String>,
    pub at_ms: Option<i64>,
}

/// The variable a tooling daemon is started with, that has it write [`Call`]s.
pub const TOOLING_API_CALLS: &str = "CONSENSFLOW_TOOLING_API_CALLS";

/// Every request of `events` a tooling daemon wrote, in order.
pub fn calls(events: &[Value]) -> Vec<Call> {
    events
        .iter()
        .filter(|event| event["kind"] == "api.answered")
        .map(|event| {
            let data = &event["data"];
            Call {
                participant: data["participant"].as_str().unwrap_or_default().to_owned(),
                method: data["method"].as_str().unwrap_or_default().to_owned(),
                path: data["path"].as_str().unwrap_or_default().to_owned(),
                status: data["status"].as_u64().unwrap_or_default(),
                error: data["error"].as_str().map(str::to_owned),
                message: data["message"].as_str().map(str::to_owned),
                at_ms: super::human::at_ms(event),
            }
        })
        .collect()
}

/// The message that carried task `number`'s brief into its window, the first: its id, as the
/// daemon's events say it.
pub fn brief_message(events: &[Value], number: i64) -> Option<i64> {
    // A pool's task is given its window by `task.assigned`; a follow-up queued for its window
    // at once (`--after`, `--to`) by `task.created`; one that waited on the board (`--after` a
    // task at work, `--self --needs`) by `task.state` from `open`. The first is the brief: a
    // reopen's or a resume's words come later.
    events.iter().find_map(|event| {
        let data = &event["data"];
        let gives = match event["kind"].as_str().unwrap_or_default() {
            "task.assigned" | "task.created" => true,
            "task.state" => data["from"] == "open",
            _ => false,
        };
        (gives && data["task"] == number)
            .then(|| data["message"].as_i64())
            .flatten()
    })
}

/// When the message `id` was confirmed delivered, first, as the daemon's events say it.
pub fn delivered_at(events: &[Value], id: i64) -> Option<i64> {
    events.iter().find_map(|event| {
        (event["kind"] == "delivery.confirmed" && event["data"]["message"] == id)
            .then(|| super::human::at_ms(event))
            .flatten()
    })
}

/// When task `number` first went to `state`, as the daemon's events say it.
pub fn reached_at(events: &[Value], number: i64, state: &str) -> Option<i64> {
    events.iter().find_map(|event| {
        (event["kind"] == "task.state"
            && event["data"]["task"] == number
            && event["data"]["to"] == state)
            .then(|| super::human::at_ms(event))
            .flatten()
    })
}

/// Every native session the participant `handle`'s conversations were bound to, oldest first:
/// a window opened again (a restart, a reopened task) has a conversation of its own.
pub fn sessions_of(events: &[Value], handle: &str) -> Vec<String> {
    events
        .iter()
        .filter(|event| {
            event["kind"] == "conversation.started" && event["data"]["participant"] == handle
        })
        .filter_map(|started| {
            let conversation = &started["data"]["conversation"];
            events.iter().find_map(|bound| {
                (bound["kind"] == "conversation.bound"
                    && bound["data"]["conversation"] == *conversation)
                    .then(|| bound["data"]["nativeSession"].as_str().map(str::to_owned))
                    .flatten()
            })
        })
        .collect()
}

/// The pane id of the window of the participant `handle` in `members`' project.
pub fn pane_id(members: &Members, handle: &str) -> String {
    format!("p{}-{handle}", members.project_id())
}

/// The native session of the participant `handle`'s window: the one its command line names
/// (Claude's `--session-id`), or the one the daemon's events say it was bound to (a harness
/// given no id on its command line: Codex, Devin, Muse, Pi).
pub fn session(members: &Members, events: &[Value], handle: &str) -> Option<String> {
    let id = pane_id(members, handle);
    members
        .rig()
        .open_frames()
        .into_iter()
        .rev()
        .find(|frame| frame["id"] == id.as_str())
        .and_then(|frame| session_of(&frame))
        .or_else(|| bound_session(events, handle))
}

/// The native session the daemon's events say the newest conversation of `handle` was bound
/// to (`conversation.started`, then `conversation.bound`).
pub fn bound_session(events: &[Value], handle: &str) -> Option<String> {
    let started = events.iter().rev().find(|event| {
        event["kind"] == "conversation.started" && event["data"]["participant"] == handle
    })?;
    let conversation = &started["data"]["conversation"];
    let bound = events.iter().rev().find(|event| {
        event["kind"] == "conversation.bound" && event["data"]["conversation"] == *conversation
    })?;
    bound["data"]["nativeSession"].as_str().map(str::to_owned)
}

/// The record of the window of `handle`, read as a record of `kind` (the seat's harness): its
/// items, none where it has no session or no record yet.
pub fn record(members: &Members, events: &[Value], kind: &str, handle: &str) -> Result<Vec<Item>> {
    let Some(session) = session(members, events, handle) else {
        return Ok(Vec::new());
    };
    Ok(ask::records(kind, &session, members.rig().environment())?
        .map(|record| record.items)
        .unwrap_or_default())
}

/// Where in `items` the first user item holding `words` is.
pub fn user_item_with(items: &[Item], words: &str) -> Option<usize> {
    items
        .iter()
        .position(|item| item.role == "user" && item.text.contains(words))
}

/// Whether `items` have an item of the assistant's after the first user item holding `words`.
pub fn answered_after(items: &[Item], words: &str) -> bool {
    user_item_with(items, words)
        .is_some_and(|at| items[at + 1..].iter().any(|item| item.role == "assistant"))
}

/// The header ConsensFlow writes at the top of the brief of task `number` given by the chief.
pub fn brief_header(number: i64) -> String {
    format!(" · T-{number} · task from @chief]")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn item(role: &str, text: &str) -> Item {
        Item {
            id: String::new(),
            role: role.to_owned(),
            text: text.to_owned(),
            complete: false,
        }
    }

    #[test]
    fn a_window_began_the_more_of_its_working_looks_and_what_went_into_it() {
        let activity = |handle: &str, state: &str| json!({ "kind": "window.activity", "participant": handle, "state": state });
        let confirmed =
            |message: i64| json!({ "kind": "delivery.confirmed", "data": { "message": message } });
        let snapshot = Snapshot {
            events: vec![
                activity("chief", "starting"),
                activity("chief", "idle"),
                activity("chief", "working"),
                activity("chief", "working"),
                json!({ "kind": "task.assigned", "data": { "task": 1, "assignee": "matrix-worker-a", "message": 3 } }),
                confirmed(3),
                activity("matrix-worker-a", "working"),
                json!({ "kind": "message.sent", "participant": "chief", "state": "working" }),
                activity("chief", "idle"),
                confirmed(4),
                confirmed(5),
                confirmed(9),
                json!({ "kind": "task.state", "data": { "task": 1, "to": "queued", "by": "chief", "message": 12 } }),
                confirmed(12),
                activity("chief", "working"),
                activity("matrix-worker-a", "closed"),
            ],
            chief_inbox: vec![json!({ "id": 4 }), json!({ "id": 5 })],
            lanes: vec![
                json!({ "participant": { "handle": "matrix-worker-a", "agent": "matrix-worker" } }),
            ],
            ..Snapshot::default()
        };
        // The chief: 2 looks at working, and 2 deliveries and 1 ask into it.
        assert_eq!(
            snapshot.turns(1),
            BTreeMap::from([
                ("chief".to_owned(), 3),
                ("matrix-worker-a".to_owned(), 2),
                ("unknown".to_owned(), 1)
            ])
        );
        assert_eq!(snapshot.turns(0)["chief"], 2);
        assert_eq!(snapshot.total_turns(1), 6);
        assert_eq!(
            snapshot.turns_by_seat(&Seating::stand_ins(), 1),
            BTreeMap::from([("chief", 3), ("worker", 2), ("unseated", 1)])
        );
    }

    #[test]
    fn tasks_are_bound_by_their_pool_oldest_first_and_never_by_their_number() {
        let task = |number: i64, pool: &str, requester: &str| json!({ "number": number, "pool": pool, "requester": requester });
        let snapshot = Snapshot {
            tasks: vec![
                task(1, "worker", "chief"),
                task(2, "advisor", "chief"),
                task(3, "reviewer", "matrix-worker-a"),
                task(4, "tester", "chief"),
                task(5, "worker", "chief"),
            ],
            ..Snapshot::default()
        };
        let bound = Bindings::of(&snapshot);
        assert_eq!(bound.of_name("advice"), Some(2));
        assert_eq!(bound.of_name("change"), Some(1));
        assert_eq!(bound.found["change"], [1, 5]);
        // A review a member asked for is not the chief's.
        assert_eq!(bound.of_name("review"), None);
        assert_eq!(bound.of_name("test"), Some(4));
    }

    fn at(ms: i64) -> String {
        jiff::Timestamp::from_millisecond(ms).unwrap().to_string()
    }

    #[test]
    fn a_follow_up_is_bound_to_the_task_whose_window_it_went_to_and_never_by_its_number() {
        let snapshot = Snapshot {
            tasks: vec![
                json!({ "number": 1, "pool": "worker", "requester": "chief", "assignee": "w-a" }),
                json!({ "number": 2, "pool": "worker", "requester": "chief", "assignee": "w-b" }),
                json!({ "number": 3, "pool": null, "requester": "chief", "assignee": "w-b" }),
                json!({ "number": 4, "pool": null, "requester": "chief", "assignee": "w-a" }),
                json!({ "number": 5, "pool": null, "requester": "w-a", "assignee": "w-a" }),
            ],
            ..Snapshot::default()
        };
        let binds = [
            Bind {
                name: "a",
                is: Is::Pool("worker"),
                requester: "chief",
            },
            Bind {
                name: "b",
                is: Is::After("a"),
                requester: "chief",
            },
            Bind {
                name: "none",
                is: Is::After("missing"),
                requester: "chief",
            },
        ];
        let bound = Bindings::by(&snapshot, &binds);
        assert_eq!(bound.found["a"], [1, 2]);
        // The follow-up of T-1 is the one that went to T-1's window, and a member's own step is none.
        assert_eq!(bound.found["b"], [4]);
        assert!(!bound.found.contains_key("none"));
    }

    #[test]
    fn a_call_a_tooling_daemon_wrote_is_read_with_its_refusal_code() {
        let events = vec![
            json!({ "at": at(1_000), "kind": "api.answered", "data": { "participant": "chief", "method": "POST", "path": "/api/tasks/1/reopen", "status": 409, "error": "not-done" } }),
            json!({ "at": at(2_000), "kind": "api.answered", "data": { "participant": "chief", "method": "GET", "path": "/api/staff", "status": 200, "error": null } }),
            json!({ "kind": "task.state", "data": {} }),
        ];
        assert_eq!(
            calls(&events),
            [
                Call {
                    participant: "chief".to_owned(),
                    method: "POST".to_owned(),
                    path: "/api/tasks/1/reopen".to_owned(),
                    status: 409,
                    error: Some("not-done".to_owned()),
                    message: None,
                    at_ms: Some(1_000)
                },
                Call {
                    participant: "chief".to_owned(),
                    method: "GET".to_owned(),
                    path: "/api/staff".to_owned(),
                    status: 200,
                    error: None,
                    message: None,
                    at_ms: Some(2_000)
                },
            ]
        );
    }

    #[test]
    fn a_brief_its_delivery_and_a_tasks_states_are_timed_by_the_events() {
        let events = vec![
            json!({ "at": at(10), "kind": "task.assigned", "data": { "task": 2, "message": 7 } }),
            json!({ "at": at(20), "kind": "delivery.confirmed", "data": { "message": 7 } }),
            json!({ "at": at(25), "kind": "delivery.confirmed", "data": { "message": 7 } }),
            json!({ "at": at(30), "kind": "task.state", "data": { "task": 2, "to": "done" } }),
            json!({ "at": at(40), "kind": "task.state", "data": { "task": 2, "to": "done" } }),
        ];
        assert_eq!(brief_message(&events, 2), Some(7));
        assert_eq!(brief_message(&events, 3), None);
        let created = vec![
            json!({ "kind": "task.created", "data": { "task": 5, "to": "w-a", "message": 9 } }),
        ];
        assert_eq!(brief_message(&created, 5), Some(9));
        let released = vec![
            json!({ "kind": "task.opened", "data": { "task": 2, "to": "w-a", "needs": [1] } }),
            json!({ "kind": "task.state", "data": { "task": 2, "from": "open", "to": "queued", "message": 3 } }),
            json!({ "kind": "task.state", "data": { "task": 2, "from": "done", "to": "queued", "message": 7 } }),
        ];
        assert_eq!(brief_message(&released, 2), Some(3));
        assert_eq!(delivered_at(&events, 7), Some(20));
        assert_eq!(reached_at(&events, 2, "done"), Some(30));
        assert_eq!(reached_at(&events, 2, "accepted"), None);
    }

    #[test]
    fn each_conversation_of_a_window_is_one_of_its_sessions() {
        let events = vec![
            json!({ "kind": "conversation.started", "data": { "participant": "h", "conversation": 1 } }),
            json!({ "kind": "conversation.bound", "data": { "conversation": 1, "nativeSession": "s1" } }),
            json!({ "kind": "conversation.started", "data": { "participant": "other", "conversation": 2 } }),
            json!({ "kind": "conversation.bound", "data": { "conversation": 2, "nativeSession": "s2" } }),
            json!({ "kind": "conversation.started", "data": { "participant": "h", "conversation": 3 } }),
            json!({ "kind": "conversation.bound", "data": { "conversation": 3, "nativeSession": "s3" } }),
        ];
        assert_eq!(sessions_of(&events, "h"), ["s1", "s3"]);
        assert!(sessions_of(&events, "nobody").is_empty());
    }

    #[test]
    fn a_participants_agent_is_its_lanes_and_else_the_events() {
        let snapshot = Snapshot {
            lanes: vec![json!({ "participant": { "handle": "h1", "agent": "matrix-tester" } })],
            events: vec![
                json!({ "kind": "session.started", "data": { "handle": "h2", "member": "matrix-advisor", "role": "advisor" } }),
            ],
            ..Snapshot::default()
        };
        assert_eq!(snapshot.agent_of("h1").as_deref(), Some("matrix-tester"));
        assert_eq!(snapshot.agent_of("h2").as_deref(), Some("matrix-advisor"));
        assert_eq!(snapshot.agent_of("h3"), None);
    }

    #[test]
    fn a_session_bound_by_the_events_is_the_newest_conversations() {
        let events = vec![
            json!({ "kind": "conversation.started", "data": { "participant": "h", "conversation": 1 } }),
            json!({ "kind": "conversation.bound", "data": { "conversation": 1, "nativeSession": "old" } }),
            json!({ "kind": "conversation.started", "data": { "participant": "h", "conversation": 2 } }),
            json!({ "kind": "conversation.bound", "data": { "conversation": 2, "nativeSession": "new" } }),
        ];
        assert_eq!(bound_session(&events, "h").as_deref(), Some("new"));
        assert_eq!(bound_session(&events, "other"), None);
    }

    #[test]
    fn a_result_counts_only_delivered_and_of_its_own_task() {
        let snapshot = Snapshot {
            chief_inbox: vec![
                json!({ "kind": "result", "taskNumber": 1, "state": "pending" }),
                json!({ "kind": "note", "taskNumber": 2, "state": "delivered" }),
                json!({ "kind": "result", "taskNumber": 3, "state": "delivered" }),
            ],
            ..Snapshot::default()
        };
        assert!(snapshot.delivered_result(1).is_none());
        assert!(snapshot.delivered_result(2).is_none());
        assert!(snapshot.delivered_result(3).is_some());
    }

    #[test]
    fn a_user_item_is_found_by_its_words_and_an_assistants_is_not() {
        let items = [
            item("assistant", "[matrix ask 1/4]"),
            item("user", "noise"),
            item(
                "user",
                "[ConsensFlow m-4 · T-2 · task from @chief]\nWRITE x",
            ),
        ];
        assert_eq!(user_item_with(&items, "[matrix ask 1/4]"), None);
        assert_eq!(user_item_with(&items, &brief_header(2)), Some(2));
        assert_eq!(user_item_with(&items, &brief_header(12)), None);
        // The assistant's item before the user's is no answer to it, and one after it is.
        assert!(!answered_after(&items, "[matrix ask 1/4]"));
        assert!(!answered_after(&items, &brief_header(2)));
        let answered = [
            item("user", "[m] ask"),
            item("user", "more"),
            item("assistant", "done"),
        ];
        assert!(answered_after(&answered, "[m]"));
        assert!(!answered_after(&answered[..2], "[m]"));
    }
}
