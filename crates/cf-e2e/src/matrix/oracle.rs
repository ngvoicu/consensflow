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
use super::observe::{self, Bindings, Snapshot, LOGICAL};
use super::scenario::{CHECK, GREET};
use super::seating::{Seating, TIER};

/// What the oracle reads of a window: the record of the participant `handle`, as a record of
/// `kind`. The rig's, in a run; a test's own in the oracle's tests.
pub trait Records {
    fn items(&self, kind: &str, handle: &str) -> Result<Vec<crate::live::ask::Item>>;
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
    let mut checks = vec![Check::new(
        "human",
        "every step was asked",
        said.len() == steps,
        format!("{} of {steps} asked", said.len()),
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
