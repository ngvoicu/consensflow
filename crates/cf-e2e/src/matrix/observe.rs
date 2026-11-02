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

/// The logical tasks of a scenario, by what they are: the pool a task for it is put in.
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

    /// The tasks the chief put in `pool`, oldest first.
    pub fn of_pool(&self, pool: &str) -> Vec<&Value> {
        self.tasks
            .iter()
            .filter(|task| task["pool"] == pool && task["requester"] == "chief")
            .collect()
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
            .and_then(|lane| lane["participant"]["agent"].as_str())
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
                Some("task.assigned") => {
                    let data = &event["data"];
                    if let (Some(id), Some(assignee)) =
                        (data["message"].as_i64(), data["assignee"].as_str())
                    {
                        recipient.insert(id, assignee.to_owned());
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

/// Which task is which, bound by what each is (the pool the chief put it in), never by its
/// number: the oldest of each pool. Several of one pool are kept, for the oracle to say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bindings {
    /// The logical name of each task and its numbers, oldest first.
    pub found: BTreeMap<&'static str, Vec<i64>>,
}

impl Bindings {
    /// The tasks of `snapshot`, bound.
    pub fn of(snapshot: &Snapshot) -> Self {
        let mut found = BTreeMap::new();
        for (name, role) in LOGICAL {
            let numbers: Vec<i64> = snapshot
                .of_pool(role.pool().unwrap_or_default())
                .iter()
                .filter_map(|task| task["number"].as_i64())
                .collect();
            if !numbers.is_empty() {
                found.insert(name, numbers);
            }
        }
        Self { found }
    }

    /// The task bound to `name`: the oldest of its pool.
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
                ("matrix-worker-a".to_owned(), 1),
                ("unknown".to_owned(), 1)
            ])
        );
        assert_eq!(snapshot.turns(0)["chief"], 2);
        assert_eq!(snapshot.total_turns(1), 5);
        assert_eq!(
            snapshot.turns_by_seat(&Seating::stand_ins(), 1),
            BTreeMap::from([("chief", 3), ("worker", 1), ("unseated", 1)])
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
    }
}
