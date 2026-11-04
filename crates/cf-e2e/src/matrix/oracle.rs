//! What a scenario is judged by: the ledger and the windows' own records, never what a model
//! said. For each task bound by what it is (its pool): one of it, accepted, of the kind and the
//! tier asked for, from the chief, done by the member seated for it in a window whose own
//! record holds the task's brief, with its result delivered to the chief; the fixture as the
//! checker reads it; and the human's asks in the chief's record, each with its turn.

use std::path::Path;

use crate::live::report::Check;
use crate::process::Run;
use crate::Result;

use super::human::Said;
use super::observe::{self, Bindings, Call, Snapshot, LOGICAL};
use super::scenario::{CHECK, GREET};
use super::seating::{Seating, TIER};

/// What the oracle reads of a window: the record of the participant `handle`, as a record of
/// `kind`. The rig's, in a run; a test's own in the oracle's tests.
pub trait Records {
    fn items(&self, kind: &str, handle: &str) -> Result<Vec<crate::live::ask::Item>>;

    /// The last `last` items of the transcript of task `number`, as the page's drawer and
    /// `cf task get --transcript --last` read it: ConsensFlow's copy of its window's record.
    fn transcript(&self, number: i64, last: usize) -> Result<Vec<serde_json::Value>> {
        let _ = (number, last);
        Ok(Vec::new())
    }
}

/// The checks of the tasks of `snapshot`, bound as `bindings` binds them, against `seating`.
pub fn tasks(
    seating: &Seating,
    snapshot: &Snapshot,
    bindings: &Bindings,
    records: &dyn Records,
) -> Vec<Check> {
    let mut checks = Vec::new();
    for (name, role) in LOGICAL {
        let seat = seating.seat(role);
        let pool = role.pool().unwrap_or_default();
        let who = format!("{name} ({} {})", role.name(), seat.who());
        let numbers = bindings.found.get(name).cloned().unwrap_or_default();
        checks.push(Check::new(
            &who,
            &format!("the chief put one task in the {pool} pool"),
            numbers.len() == 1,
            format!("the chief's tasks of the pool: {numbers:?}"),
        ));
        let Some(number) = numbers.first().copied() else {
            continue;
        };
        let Some(task) = snapshot.task(number) else {
            checks.push(Check::failed(
                &who,
                "the task is on the board",
                format!("no T-{number}"),
            ));
            continue;
        };
        checks.push(Check::new(
            &who,
            "it is accepted",
            task["state"] == "accepted",
            format!("T-{number} is {}", task["state"]),
        ));
        checks.push(Check::new(
            &who,
            &format!("it is {pool} work at {TIER}, asked by the chief"),
            task["pool"] == pool && task["tier"] == TIER && task["requester"] == "chief",
            format!(
                "T-{number}: pool {}, tier {}, requester {}",
                task["pool"], task["tier"], task["requester"]
            ),
        ));
        let assignee = task["assignee"].as_str().unwrap_or_default();
        let agent = snapshot.agent_of(assignee);
        checks.push(Check::new(
            &who,
            "it was assigned to the member seated for it",
            agent.as_deref() == Some(seat.agent().as_str()),
            format!(
                "T-{number}'s assignee @{assignee} is a session of {agent:?}, and {} sits there",
                seat.agent()
            ),
        ));
        let header = observe::brief_header(number);
        let did = records
            .items(seat.kind(), assignee)
            .map(|items| observe::user_item_with(&items, &header));
        checks.push(Check::new(
            &who,
            "its brief is in the record of the window of that member",
            matches!(did, Ok(Some(_))),
            match did {
                Ok(Some(at)) => format!(
                    "item {at} of @{assignee}'s {} record holds `{}`",
                    seat.kind(),
                    header.trim()
                ),
                Ok(None) => format!(
                    "@{assignee}'s {} record holds no `{}`",
                    seat.kind(),
                    header.trim()
                ),
                Err(failed) => format!("@{assignee}'s record could not be read: {failed}"),
            },
        ));
        let result = snapshot.delivered_result(number);
        checks.push(Check::new(
            &who,
            "its result was delivered to the chief",
            result.is_some_and(|result| {
                result["body"]
                    .as_str()
                    .is_some_and(|body| !body.trim().is_empty())
                    && result["sender"] == assignee
            }),
            result.map_or_else(
                || format!("no result of T-{number} delivered in the chief's inbox"),
                |result| {
                    format!(
                        "m-{} from @{}, {} characters",
                        result["id"],
                        result["sender"].as_str().unwrap_or_default(),
                        result["body"]
                            .as_str()
                            .map_or(0, |body| body.chars().count())
                    )
                },
            ),
        ));
    }
    checks
}

/// The checks that no brief went in too early: the brief of each task bound to a name of
/// `later` was delivered into its window only once the task bound to `earlier` was done, as the
/// daemon's events time them.
pub fn after_done(
    snapshot: &Snapshot,
    bindings: &Bindings,
    earlier: &str,
    later: &[&str],
) -> Vec<Check> {
    after_state(snapshot, bindings, earlier, "done", later)
}

/// [`after_done`], for the task bound to `earlier` reaching `state` (a follow-up waits for its
/// predecessor's acceptance: field report 6).
pub fn after_state(
    snapshot: &Snapshot,
    bindings: &Bindings,
    earlier: &str,
    state: &str,
    later: &[&str],
) -> Vec<Check> {
    let events = &snapshot.events;
    let done = bindings
        .of_name(earlier)
        .and_then(|number| Some((number, observe::reached_at(events, number, state)?)));
    later
        .iter()
        .map(|name| {
            let what = format!("its brief went in after the {earlier} was {state}");
            let brief = bindings.of_name(name).and_then(|number| {
                let message = observe::brief_message(events, number)?;
                Some((number, message, observe::delivered_at(events, message)?))
            });
            match (done, brief) {
                (Some((before, done_ms)), Some((number, message, delivered_ms))) => Check::new(
                    name,
                    &what,
                    delivered_ms >= done_ms,
                    format!(
                        "T-{number}'s brief m-{message} was delivered {} ms after T-{before} was {state}",
                        delivered_ms - done_ms
                    ),
                ),
                (None, _) => Check::failed(name, &what, format!("no {earlier} task was {state}")),
                (_, None) => Check::failed(
                    name,
                    &what,
                    format!("no brief of the {name} task was delivered"),
                ),
            }
        })
        .collect()
}

/// What a scenario's checks are given: the seats, the board at its end, the tasks bound, the
/// windows' records and the project's folder.
pub struct Judged<'a> {
    pub seating: &'a Seating,
    pub snapshot: &'a Snapshot,
    pub bindings: &'a Bindings,
    pub records: &'a dyn Records,
    pub folder: &'a Path,
}

impl Judged<'_> {
    /// The task bound to `name`, the `nth` (from 0) of those that fit.
    pub fn nth(&self, name: &str, nth: usize) -> Option<i64> {
        self.bindings
            .found
            .get(name)
            .and_then(|numbers| numbers.get(nth).copied())
    }
}

/// The check that the task bound to `name` (the `nth`) is in one of `states`.
pub fn state(judged: &Judged, name: &str, nth: usize, states: &[&str]) -> Check {
    let what = format!("it is {}", states.join(" or "));
    match judged
        .nth(name, nth)
        .and_then(|number| Some((number, judged.snapshot.task(number)?)))
    {
        Some((number, task)) => Check::new(
            name,
            &what,
            states.iter().any(|state| task["state"] == *state),
            format!("T-{number} is {}", task["state"]),
        ),
        None => Check::failed(name, &what, format!("no {name} task was bound")),
    }
}

/// How many times the brief of the task bound to `name` (the `nth`) was delivered into a
/// window, as the daemon's events say: none for a task no window was given.
pub fn briefed(judged: &Judged, name: &str, nth: usize) -> Option<(i64, usize)> {
    let number = judged.nth(name, nth)?;
    let events = &judged.snapshot.events;
    let times = observe::brief_message(events, number).map_or(0, |message| {
        events
            .iter()
            .filter(|event| {
                event["kind"] == "delivery.confirmed" && event["data"]["message"] == message
            })
            .count()
    });
    Some((number, times))
}

/// The check that the task bound to `name` (the `nth`) went into no window: it waits.
pub fn never_briefed(judged: &Judged, name: &str, nth: usize) -> Check {
    let what = "no window was given its brief";
    match briefed(judged, name, nth) {
        Some((number, times)) => Check::new(
            name,
            what,
            times == 0 && observe::brief_message(&judged.snapshot.events, number).is_none(),
            format!("T-{number}'s brief went to a window {times} time(s)"),
        ),
        None => Check::failed(name, what, format!("no {name} task was bound")),
    }
}

/// The check that the task bound to `name` (the `nth`) was delivered into its window once.
pub fn briefed_once(judged: &Judged, name: &str, nth: usize) -> Check {
    let what = "its brief was delivered once";
    match briefed(judged, name, nth) {
        Some((number, times)) => Check::new(
            name,
            what,
            times == 1,
            format!("T-{number}'s brief was delivered {times} time(s)"),
        ),
        None => Check::failed(name, what, format!("no {name} task was bound")),
    }
}

/// What a call of the chief's is to be answered with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answered {
    /// Done: no refusal.
    Done,
    /// Refused with the code, its words holding each of the texts (`T-{n}` of a bound task is
    /// written as `{name}` and stands for its number).
    Refused(&'static str, &'static [&'static str]),
}

/// The check that the chief's `nth` (from 0) call of `action` on the task bound to `name` was
/// answered as `expected`, as a tooling daemon wrote it: the product's own answer and words,
/// never a model's.
pub fn chief_call(
    judged: &Judged,
    name: &str,
    action: &str,
    nth: usize,
    expected: Answered,
) -> Check {
    let label = match expected {
        Answered::Done => format!("the chief's {} {action} of it was done", ordinal(nth)),
        Answered::Refused(code, words) => format!(
            "the chief's {} {action} of it was refused with {code}{}",
            ordinal(nth),
            if words.is_empty() {
                String::new()
            } else {
                format!(", naming {}", words.join(" and "))
            }
        ),
    };
    let Some(number) = judged.nth(name, 0) else {
        return Check::failed(name, &label, format!("no {name} task was bound"));
    };
    let path = format!("/api/tasks/{number}/{action}");
    let calls: Vec<Call> = observe::calls(&judged.snapshot.events)
        .into_iter()
        .filter(|call| call.participant == "chief" && call.method == "POST" && call.path == path)
        .collect();
    let Some(call) = calls.get(nth) else {
        return Check::failed(
            name,
            &label,
            format!("the chief made {} call(s) of POST {path}", calls.len()),
        );
    };
    let said = format!(
        "POST {path} answered {} {}{}",
        call.status,
        call.error.as_deref().unwrap_or("ok"),
        call.message
            .as_deref()
            .map_or_else(String::new, |message| format!(": {message}"))
    );
    let held = match expected {
        Answered::Done => call.error.is_none() && call.status < 400,
        Answered::Refused(code, words) => {
            let message = call.message.as_deref().unwrap_or_default();
            call.error.as_deref() == Some(code)
                && words
                    .iter()
                    .all(|word| message.contains(&named(judged, word)))
        }
    };
    Check::new(name, &label, held, said)
}

/// `word` with each `{name}` of a bound task written as `T-<its number>`.
fn named(judged: &Judged, word: &str) -> String {
    let mut text = word.to_owned();
    for (name, numbers) in &judged.bindings.found {
        if let Some(number) = numbers.first() {
            text = text.replace(&format!("{{{name}}}"), &format!("T-{number}"));
        }
    }
    text
}

fn ordinal(nth: usize) -> &'static str {
    ["first", "second", "third"]
        .get(nth)
        .copied()
        .unwrap_or("later")
}

/// The check that the result of the task bound to `name` is kept: the message its `done`
/// named is in the chief's inbox, a result of that task, with words.
pub fn result_kept(judged: &Judged, name: &str) -> Check {
    let what = "its result is kept";
    let Some(number) = judged.nth(name, 0) else {
        return Check::failed(name, what, format!("no {name} task was bound"));
    };
    let result = judged.snapshot.events.iter().find_map(|event| {
        (event["kind"] == "task.state"
            && event["data"]["task"] == number
            && event["data"]["to"] == "done")
            .then(|| event["data"]["result"].as_i64())
            .flatten()
    });
    let Some(id) = result else {
        return Check::failed(
            name,
            what,
            format!("T-{number} was never done with a result"),
        );
    };
    let kept = judged
        .snapshot
        .chief_inbox
        .iter()
        .find(|message| message["id"] == id);
    Check::new(
        name,
        what,
        kept.is_some_and(|message| {
            message["kind"] == "result"
                && message["taskNumber"] == number
                && message["body"]
                    .as_str()
                    .is_some_and(|body| !body.is_empty())
        }),
        kept.map_or_else(
            || format!("m-{id}, T-{number}'s result, is not in the chief's inbox"),
            |message| format!("m-{id} is {} {}", message["kind"], message["state"]),
        ),
    )
}

/// The checks of the fixture in `folder`: its checker as it was, the code changed, and the
/// oracle's own copy of the checker passing on it.
pub fn fixture(folder: &Path) -> Vec<Check> {
    let read = |name: &str| std::fs::read_to_string(folder.join(name)).unwrap_or_default();
    let (check, greet) = (read("check.cjs"), read("greet.cjs"));
    let checked = Run::new("node")
        .args(["-e", CHECK])
        .cwd(folder)
        .inheriting_env()
        .run();
    vec![
        Check::new(
            "fixture",
            "check.cjs is as it was",
            check == CHECK,
            format!("{} bytes", check.len()),
        ),
        Check::new(
            "fixture",
            "greet.cjs was changed",
            greet != GREET && !greet.is_empty(),
            crate::live::cut(&greet, 160),
        ),
        match checked {
            Ok(ran) => Check::new(
                "fixture",
                "the checker passes on it",
                ran.code == Some(0) && ran.stdout.trim() == "OK",
                format!(
                    "exit {:?}: {}",
                    ran.code,
                    crate::live::cut(ran.output().trim(), 160)
                ),
            ),
            Err(failed) => Check::failed("fixture", "the checker passes on it", failed.to_string()),
        },
    ]
}

/// The checks of the human's asks: each in the chief's record, with the chief's turn after it.
pub fn asks(said: &[Said], steps: usize) -> Vec<Check> {
    let asked = said
        .iter()
        .filter(|said| !["answer", "nudge"].contains(&said.step))
        .count();
    let mut checks = vec![Check::new(
        "human",
        "every step was asked",
        asked == steps,
        format!(
            "{asked} of {steps} asked, and {} answer(s) and nudge(s) besides",
            said.len() - asked
        ),
    )];
    for (index, ask) in said.iter().enumerate() {
        checks.push(Check::new(
            "human",
            &format!(
                "ask {} ({}) went into the chief's record and began its turn",
                index + 1,
                ask.step
            ),
            ask.went_in(),
            ask.failed.clone().unwrap_or_else(|| {
                format!(
                    "in its record after {} ms, its turn after {} ms{}",
                    ask.in_record_ms.unwrap_or_default() - ask.typed_ms,
                    ask.turn_ms.unwrap_or_default() - ask.typed_ms,
                    if ask.enter_again {
                        ", with Enter again once"
                    } else {
                        ""
                    }
                )
            }),
        ));
    }
    checks
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{json, Value};

    use super::*;
    use crate::live::ask::Item;
    use crate::matrix::seating::Role;

    /// Records in which each handle's holds the brief of the tasks it is given.
    struct Briefs(BTreeMap<String, Vec<i64>>);

    impl Records for Briefs {
        fn items(&self, _kind: &str, handle: &str) -> Result<Vec<Item>> {
            Ok(self
                .0
                .get(handle)
                .map(|numbers| {
                    numbers
                        .iter()
                        .map(|number| Item {
                            id: String::new(),
                            role: "user".to_owned(),
                            text: format!(
                                "[ConsensFlow m-9{} · T-{number} · task from @chief]\nbrief",
                                number
                            ),
                            complete: false,
                        })
                        .collect()
                })
                .unwrap_or_default())
        }
    }

    /// A board on which each logical task was done by its seat and accepted.
    fn board() -> (Snapshot, Briefs) {
        let mut snapshot = Snapshot::default();
        let mut briefs = BTreeMap::new();
        for (index, (_, role)) in LOGICAL.iter().enumerate() {
            let number = i64::try_from(index).unwrap() + 1;
            let handle = format!("{}-h{number}", role.agent());
            snapshot.tasks.push(json!({
                "number": number, "state": "accepted", "pool": role.pool(), "tier": "standard",
                "requester": "chief", "assignee": handle,
            }));
            snapshot
                .lanes
                .push(json!({ "participant": { "handle": handle, "agent": role.agent() } }));
            snapshot.chief_inbox.push(json!({
                "id": 100 + number, "kind": "result", "taskNumber": number, "state": "delivered",
                "sender": handle, "body": "done",
            }));
            briefs.insert(handle, vec![number]);
        }
        (snapshot, Briefs(briefs))
    }

    fn failed(checks: &[Check]) -> Vec<String> {
        checks
            .iter()
            .filter(|check| !check.ok)
            .map(|check| format!("{}: {}", check.who, check.what))
            .collect()
    }

    #[test]
    fn a_board_where_each_seat_did_its_task_passes_every_check() {
        let (snapshot, briefs) = board();
        let checks = tasks(
            &Seating::stand_ins(),
            &snapshot,
            &Bindings::of(&snapshot),
            &briefs,
        );
        assert_eq!(checks.len(), 4 * 6);
        assert_eq!(failed(&checks), Vec::<String>::new());
    }

    #[test]
    fn a_task_done_by_another_member_is_a_wrong_assignee() {
        let (mut snapshot, briefs) = board();
        // The review went to the tester's window.
        snapshot.tasks[2]["assignee"] = json!("matrix-tester-h4");
        let checks = tasks(
            &Seating::stand_ins(),
            &snapshot,
            &Bindings::of(&snapshot),
            &briefs,
        );
        let failed = failed(&checks);
        assert!(
            failed.contains(
                &"review (reviewer stand-in): it was assigned to the member seated for it"
                    .to_owned()
            ),
            "{failed:?}"
        );
    }

    #[test]
    fn a_result_never_delivered_or_from_another_window_is_missing() {
        let (mut snapshot, briefs) = board();
        snapshot.chief_inbox[1]["state"] = json!("pending");
        snapshot.chief_inbox[3]["sender"] = json!("chief");
        let checks = tasks(
            &Seating::stand_ins(),
            &snapshot,
            &Bindings::of(&snapshot),
            &briefs,
        );
        assert_eq!(
            failed(&checks),
            [
                "change (worker stand-in): its result was delivered to the chief",
                "test (tester stand-in): its result was delivered to the chief"
            ]
        );
    }

    #[test]
    fn two_tasks_of_a_pool_a_task_not_accepted_and_a_brief_in_no_record_each_fail() {
        let (mut snapshot, mut briefs) = board();
        snapshot
            .tasks
            .push(json!({ "number": 5, "pool": "advisor", "requester": "chief" }));
        snapshot.tasks[1]["state"] = json!("done");
        briefs.0.insert("matrix-tester-h4".to_owned(), Vec::new());
        let checks = tasks(
            &Seating::stand_ins(),
            &snapshot,
            &Bindings::of(&snapshot),
            &briefs,
        );
        assert_eq!(
            failed(&checks),
            [
                "advice (advisor stand-in): the chief put one task in the advisor pool",
                "change (worker stand-in): it is accepted",
                "test (tester stand-in): its brief is in the record of the window of that member"
            ]
        );
    }

    #[test]
    fn nothing_bound_is_one_failed_check_for_each_task() {
        let checks = tasks(
            &Seating::stand_ins(),
            &Snapshot::default(),
            &Bindings::default(),
            &Briefs(BTreeMap::new()),
        );
        assert_eq!(checks.len(), 4);
        assert!(checks.iter().all(|check| !check.ok));
        let _ = Role::Chief;
    }

    fn stamped(ms: i64, kind: &str, data: Value) -> Value {
        json!({ "at": jiff::Timestamp::from_millisecond(ms).unwrap().to_string(), "kind": kind, "data": data })
    }

    /// The change done at 100 ms, the review's brief (m-5) delivered at `review_ms`.
    fn timed(review_ms: i64) -> Snapshot {
        let (mut snapshot, _) = board();
        snapshot.events = vec![
            stamped(100, "task.state", json!({ "task": 2, "to": "done" })),
            stamped(110, "task.assigned", json!({ "task": 3, "message": 5 })),
            stamped(review_ms, "delivery.confirmed", json!({ "message": 5 })),
        ];
        snapshot
    }

    #[test]
    fn a_brief_delivered_before_the_task_it_waits_for_was_done_is_too_early() {
        let in_time = timed(150);
        let checks = after_done(&in_time, &Bindings::of(&in_time), "change", &["review"]);
        assert_eq!(failed(&checks), Vec::<String>::new());
        assert!(
            checks[0]
                .detail
                .contains("delivered 50 ms after T-2 was done"),
            "{}",
            checks[0].detail
        );
        let early = timed(60);
        let checks = after_done(&early, &Bindings::of(&early), "change", &["review"]);
        assert_eq!(
            failed(&checks),
            ["review: its brief went in after the change was done"]
        );
        // A task with no brief delivered, and one that waits on nothing done, fail too.
        let checks = after_done(&in_time, &Bindings::of(&in_time), "change", &["test"]);
        assert_eq!(
            failed(&checks),
            ["test: its brief went in after the change was done"]
        );
        let checks = after_done(&in_time, &Bindings::of(&in_time), "advice", &["review"]);
        assert!(checks[0].detail.contains("no advice task was done"));
    }

    #[test]
    fn a_refusal_is_judged_by_the_code_and_the_words_the_daemon_answered_and_nothing_else() {
        let (mut snapshot, briefs) = board();
        let answered = |path: &str, status: u64, error: Option<&str>, message: Option<&str>| {
            json!({ "kind": "api.answered", "data": {
                "participant": "chief", "method": "POST", "path": path,
                "status": status, "error": error, "message": message,
            } })
        };
        snapshot.events = vec![
            answered("/api/tasks/1/accept", 200, None, None),
            answered(
                "/api/tasks/1/reopen",
                409,
                Some("session-busy"),
                Some("@w is still on its work: T-3 is paused"),
            ),
            answered("/api/tasks/2/cancel", 200, None, None),
        ];
        let bindings = Bindings::of(&snapshot);
        let seating = Seating::stand_ins();
        let judged = Judged {
            seating: &seating,
            snapshot: &snapshot,
            bindings: &bindings,
            records: &briefs,
            folder: Path::new("."),
        };
        assert!(
            chief_call(
                &judged,
                "advice",
                "reopen",
                0,
                Answered::Refused("session-busy", &[])
            )
            .ok
        );
        assert!(
            chief_call(
                &judged,
                "advice",
                "reopen",
                0,
                Answered::Refused("session-busy", &["{review}", "paused"])
            )
            .ok
        );
        let wrong = chief_call(
            &judged,
            "advice",
            "reopen",
            0,
            Answered::Refused("waited-for", &[]),
        );
        assert!(
            !wrong.ok
                && wrong
                    .detail
                    .contains("answered 409 session-busy: @w is still"),
            "{}",
            wrong.detail
        );
        // The code right and the words naming another task: no pass.
        assert!(
            !chief_call(
                &judged,
                "advice",
                "reopen",
                0,
                Answered::Refused("session-busy", &["{change}"])
            )
            .ok
        );
        assert!(chief_call(&judged, "change", "cancel", 0, Answered::Done).ok);
        assert!(!chief_call(&judged, "advice", "reopen", 0, Answered::Done).ok);
        let none = chief_call(&judged, "advice", "reopen", 1, Answered::Done);
        assert!(
            !none.ok && none.detail.contains("made 1 call(s)"),
            "{}",
            none.detail
        );
    }

    #[test]
    fn the_fixture_passes_once_the_change_is_made_and_not_before() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("check.cjs"), CHECK).unwrap();
        std::fs::write(folder.path().join("greet.cjs"), GREET).unwrap();
        let before = fixture(folder.path());
        assert_eq!(
            failed(&before),
            [
                "fixture: greet.cjs was changed",
                "fixture: the checker passes on it"
            ]
        );
        std::fs::write(
            folder.path().join("greet.cjs"),
            "module.exports = (name) => `Hello, ${name}!`;\n",
        )
        .unwrap();
        assert_eq!(failed(&fixture(folder.path())), Vec::<String>::new());
        // A checker the worker rewrote to pass is no pass.
        std::fs::write(folder.path().join("greet.cjs"), GREET).unwrap();
        std::fs::write(folder.path().join("check.cjs"), "console.log('OK')").unwrap();
        assert_eq!(
            failed(&fixture(folder.path())),
            [
                "fixture: check.cjs is as it was",
                "fixture: greet.cjs was changed",
                "fixture: the checker passes on it"
            ]
        );
    }

    #[test]
    fn an_ask_that_never_went_in_fails_and_a_step_never_asked_fails() {
        let ask = |in_record: Option<i64>, turn: Option<i64>| Said {
            step: "advice",
            marker: "[m]".to_owned(),
            text: String::new(),
            typed_ms: 0,
            in_record_ms: in_record,
            turn_ms: turn,
            enter_again: false,
            failed: in_record.is_none().then(|| "never".to_owned()),
        };
        let checks = asks(&[ask(Some(10), Some(20)), ask(None, None)], 4);
        let failed = failed(&checks);
        assert_eq!(
            failed,
            [
                "human: every step was asked",
                "human: ask 2 (advice) went into the chief's record and began its turn"
            ]
        );
        let _: Value = json!(null);
    }
}
