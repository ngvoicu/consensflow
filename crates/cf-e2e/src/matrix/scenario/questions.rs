//! LMX-11, questions: a worker's `cf ask` answered by the chief's `cf answer`; a question in the
//! last words of a worker's turn (T-1119), which is a question to the chief and no result; and an
//! advisor's answer, which comes back to the chief as its task's result.

use crate::live::report::Check;
use crate::matrix::observe::{Bind, Bindings, Is, Snapshot};
use crate::matrix::oracle::{self, Judged};

use super::kit::{answered, is, run, scenario};
use super::{Scenario, Step};

/// The chief's messages of `kind` from a session of the window that has the task bound to `name`.
fn from_window<'a>(judged: &'a Judged, name: &str, kind: &str) -> Vec<&'a serde_json::Value> {
    let window = judged
        .nth(name, 0)
        .and_then(|number| judged.snapshot.task(number))
        .and_then(|task| task["assignee"].as_str())
        .unwrap_or("none");
    judged
        .snapshot
        .chief_inbox
        .iter()
        .filter(|message| message["kind"] == kind && message["sender"] == window)
        .collect()
}

/// A chief may read or accept a result before its queued paste, or finish a
/// later question. Do not hold the scenario to a delivery that is now cancelled.
fn exchange_reached(snapshot: &Snapshot, bindings: &Bindings) -> bool {
    let Some(number) = bindings.nth("A", 0) else {
        return false;
    };
    let Some(task) = snapshot.task(number) else {
        return false;
    };
    let terminal = ["done", "accepted"].contains(&task["state"].as_str().unwrap_or_default());
    let answered_again = task["state"] == "waiting"
        && task["messages"].as_array().is_some_and(|messages| {
            messages.iter().any(|message| {
                message["kind"] == "answer"
                    && (message["state"] == "delivered" || message["state"] == "read")
            })
        });
    (terminal || answered_again) && crate::matrix::run::settled(snapshot)
}

/// Judge the question's history, rather than requiring a particular final task state.
fn exchange_checks(judged: &Judged, require_answer: bool) -> Vec<Check> {
    let asked = from_window(judged, "A", "question");
    let Some(question) = asked.first() else {
        return vec![Check::failed(
            "A",
            "its window asked a question",
            "no question",
        )];
    };
    let Some(number) = judged.nth("A", 0) else {
        return vec![];
    };
    let messages = judged
        .snapshot
        .task(number)
        .and_then(|task| task["messages"].as_array());
    let answers: Vec<_> = messages
        .into_iter()
        .flatten()
        .filter(|message| {
            message["kind"] == "answer"
                && message["replyTo"] == question["id"]
                && message["sender"] == "chief"
        })
        .collect();
    let events = &judged.snapshot.events;
    let waiting = events.iter().position(|event| {
        event["kind"] == "task.state"
            && event["data"]["task"] == number
            && event["data"]["to"] == "waiting"
    });
    let sent = answers.first().and_then(|answer| {
        events.iter().position(|event| {
            event["kind"] == "message.sent" && event["data"]["message"] == answer["id"]
        })
    });
    let working = sent.and_then(|sent| {
        events
            .iter()
            .enumerate()
            .skip(sent + 1)
            .find_map(|(at, event)| {
                (event["kind"] == "task.state"
                    && event["data"]["task"] == number
                    && event["data"]["from"] == "waiting"
                    && event["data"]["to"] == "working")
                    .then_some(at)
            })
    });
    let deliveries: Vec<_> = answers
        .first()
        .map(|answer| {
            events
                .iter()
                .enumerate()
                .filter_map(|(at, event)| {
                    let carried = event["data"]["carried"]
                        .as_array()
                        .is_some_and(|ids| ids.contains(&answer["id"]))
                        || event["data"]["withdrawnAfterHanded"]
                            .as_array()
                            .is_some_and(|ids| ids.contains(&answer["id"]));
                    ((event["kind"] == "delivery.confirmed"
                        && (event["data"]["message"] == answer["id"] || carried))
                        || (event["kind"] == "message.read"
                            && event["data"]["message"] == answer["id"]))
                        .then_some(at)
                })
                .collect()
        })
        .unwrap_or_default();
    let done = events.iter().position(|event| {
        event["kind"] == "task.state"
            && event["data"]["task"] == number
            && event["data"]["to"] == "done"
    });
    let explicit_finish = crate::matrix::observe::calls(events).iter().any(|call| {
        call.participant == "chief"
            && call.method == "POST"
            && call.path == format!("/api/tasks/{number}/finish")
            && call.error.is_none()
            && (200..300).contains(&call.status)
    });
    let results = from_window(judged, "A", "result");
    let result_recorded = results.is_empty()
        || done.is_some_and(|at| {
            results
                .iter()
                .all(|result| events[at]["data"]["result"] == result["id"])
        });
    let later = result_recorded
        && (done.is_none()
            || explicit_finish
            || done.is_some_and(|done| deliveries.iter().any(|at| *at < done)));
    let must_answer = (require_answer && !explicit_finish) || !answers.is_empty();
    vec![
        Check::new(
            "A",
            "its window asked a question",
            !asked.is_empty(),
            format!("{} question(s)", asked.len()),
        ),
        Check::new(
            "A",
            "the task waited at the question",
            waiting.is_some() && sent.is_none_or(|sent| waiting < Some(sent)),
            format!("waiting={waiting:?}, answer={sent:?}"),
        ),
        Check::new(
            "A",
            "the answer went in once",
            !must_answer || (answers.len() == 1 && deliveries.len() == 1),
            format!(
                "{} answer(s), {} receipt(s)",
                answers.len(),
                deliveries.len()
            ),
        ),
        Check::new(
            "A",
            "the task worked once the answer was queued",
            !must_answer || (sent.is_some() && working.is_some()),
            format!("answer={sent:?}, working={working:?}"),
        ),
        Check::new(
            "A",
            "no automatic result came from the question turn",
            later,
            format!("answer receipt={deliveries:?}, done={done:?}, chief finish={explicit_finish}"),
        ),
    ]
}

/// The scenarios of LMX-11.
pub fn questions() -> Vec<Scenario> {
    vec![
        // The worker asks with `cf ask`; the chief answers with `cf answer`; the worker goes on
        // and its result comes after the answer.
        scenario(
            "question-ask-answer",
            vec![Step {
                human: None,
                restarts: false,
                name: "A asks",
                asks: |_| "Put one task on the board for our worker with `cf task add --tier standard \"Ask the chief with cf ask which format to use, JSON or text, then reply with exactly the format you were told.\"`. When its question comes, answer it with `cf answer` and the word JSON. Tell me in one line when its result is back.".to_owned(),
                stand_in: |_| "CF task add --tier standard :: CF ask :: Which format, JSON or text? REPLY JSON".to_owned(),
                reached: exchange_reached,
            }],
            &[Bind { name: "A", is: Is::Pool("worker"), requester: "chief" }],
            |judged| {
                exchange_checks(judged, true)
            },
            &[],
            &[],
        ),
        // A worker's turn that ends asking: a question to the chief, the task waiting on it, and
        // no result.
        scenario(
            "question-in-last-words",
            vec![run!(
                "A",
                "task add --tier standard :: SAY Should I continue with the live A/B runs and the packet/socket capture, or stop here so you can review the findings first?",
                |snapshot, bindings| {
                    bindings.nth("A", 0).is_some()
                        && !is(snapshot, bindings, "A", 0, &["queued", "working"])
                        && crate::matrix::run::settled(snapshot)
                }
            )],
            &[Bind { name: "A", is: Is::Pool("worker"), requester: "chief" }],
            |judged| {
                exchange_checks(judged, false)
            },
            &[],
            &[],
        ),
        // The chief asks an advisor; the advisor's answer comes back as the advice task's result.
        scenario(
            "question-advisor-answer",
            vec![run!(
                "advice",
                "task add --advice --tier standard :: Which is safer to change first, the parser or the printer? Reply with exactly: THE_PRINTER",
                |snapshot, bindings| answered(snapshot, bindings, "advice", 0)
            )],
            &[Bind { name: "advice", is: Is::Pool("advisor"), requester: "chief" }],
            |judged| {
                let results = from_window(judged, "advice", "result");
                vec![
                    oracle::state(judged, "advice", 0, &["done"]),
                    Check::new(
                        "advice",
                        "the advisor's answer is its one result, delivered",
                        results.len() == 1
                            && results[0]["state"] == "delivered"
                            && results[0]["body"].as_str().is_some_and(|body| body.contains("THE_PRINTER")),
                        format!("{:?}", results.iter().map(|message| (message["state"].clone(), message["body"].clone())).collect::<Vec<_>>()),
                    ),
                    Check::new("advice", "it asked the chief nothing", from_window(judged, "advice", "question").is_empty(), "no question"),
                ]
            },
            &[],
            &[],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matrix::{oracle::Records, seating::Seating};
    use serde_json::json;
    use std::{collections::BTreeMap, path::Path};

    struct NoRecords;
    impl Records for NoRecords {
        fn items(&self, _: &str, _: &str) -> crate::Result<Vec<crate::live::ask::Item>> {
            Ok(vec![])
        }
    }

    fn board() -> Snapshot {
        let question = json!({"id": 2, "kind": "question", "sender": "worker", "taskNumber": 1, "state": "read"});
        let answer = json!({"id": 3, "kind": "answer", "sender": "chief", "replyTo": 2, "state": "delivered"});
        Snapshot {
            tasks: vec![
                json!({"number": 1, "state": "accepted", "assignee": "worker", "messages": [question, answer]}),
            ],
            chief_inbox: vec![
                question,
                json!({"id": 4, "kind": "result", "sender": "worker", "taskNumber": 1, "state": "cancelled"}),
            ],
            events: vec![
                json!({"kind":"task.state", "data":{"task":1,"from":"working","to":"waiting"}}),
                json!({"kind":"message.sent", "data":{"message":3,"kind":"answer","from":"chief","to":"worker"}}),
                json!({"kind":"task.state", "data":{"task":1,"from":"waiting","to":"working"}}),
                json!({"kind":"delivery.confirmed", "data":{"message":3}}),
                json!({"kind":"task.state", "data":{"task":1,"from":"working","to":"done","result":4}}),
            ],
            lanes: vec![json!({"participant":{"handle":"chief"},"activity":{"state":"idle"}})],
        }
    }

    fn checks(snapshot: &Snapshot, answer: bool) -> Vec<Check> {
        let seating = Seating::stand_ins();
        let bindings = Bindings {
            found: BTreeMap::from([("A", vec![1])]),
        };
        exchange_checks(
            &Judged {
                seating: &seating,
                snapshot,
                bindings: &bindings,
                records: &NoRecords,
                folder: Path::new("."),
            },
            answer,
        )
    }

    fn failed(snapshot: &Snapshot, answer: bool) -> Vec<String> {
        checks(snapshot, answer)
            .into_iter()
            .filter(|check| !check.ok)
            .map(|check| check.what)
            .collect()
    }

    #[test]
    fn t1778_oracle_distinguishes_a_later_result_from_an_automatic_question_result() {
        let mut snapshot = board();
        assert!(failed(&snapshot, true).is_empty());
        assert!(failed(&snapshot, false).is_empty());
        let bindings = Bindings {
            found: BTreeMap::from([("A", vec![1])]),
        };
        assert!(exchange_reached(&snapshot, &bindings));
        snapshot.events.swap(3, 4);
        assert!(failed(&snapshot, false)
            .iter()
            .any(|what| what.contains("automatic result")));
        snapshot.events.push(json!({"kind":"api.answered","data":{"participant":"chief","method":"POST","path":"/api/tasks/1/finish","status":200,"error":null}}));
        assert!(
            failed(&snapshot, false).is_empty(),
            "explicit Finish is the chief's override"
        );
    }

    #[test]
    fn t1778_oracle_requires_waiting_working_and_one_answer_receipt() {
        let mut snapshot = board();
        snapshot.events.remove(0);
        assert!(failed(&snapshot, true)
            .iter()
            .any(|what| what.contains("waited")));
        let mut snapshot = board();
        snapshot.events.remove(2);
        assert!(failed(&snapshot, true)
            .iter()
            .any(|what| what.contains("worked")));
        let mut snapshot = board();
        snapshot
            .events
            .push(json!({"kind":"delivery.confirmed","data":{"message":3}}));
        assert!(failed(&snapshot, true)
            .iter()
            .any(|what| what.contains("once")));
    }

    #[test]
    fn t1778_a_repeated_question_is_a_milestone_and_the_chief_can_finish_it() {
        let mut snapshot = board();
        snapshot.tasks[0]["state"] = json!("waiting");
        snapshot.chief_inbox.pop();
        snapshot.events.pop();
        snapshot
            .events
            .push(json!({"kind":"task.state","data":{"task":1,"from":"working","to":"waiting"}}));
        let bindings = Bindings {
            found: BTreeMap::from([("A", vec![1])]),
        };
        assert!(exchange_reached(&snapshot, &bindings));
        assert!(failed(&snapshot, true).is_empty());

        snapshot.tasks[0]["state"] = json!("done");
        snapshot.chief_inbox.push(
            json!({"id":4,"kind":"result","sender":"worker","taskNumber":1,"state":"delivered"}),
        );
        snapshot.events.push(
            json!({"kind":"task.state","data":{"task":1,"from":"waiting","to":"done","result":4}}),
        );
        snapshot.events.push(json!({"kind":"api.answered","data":{"participant":"chief","method":"POST","path":"/api/tasks/1/finish","status":200,"error":null}}));
        assert!(exchange_reached(&snapshot, &bindings));
        assert!(failed(&snapshot, true).is_empty());
    }

    #[test]
    fn t1778_oracle_accepts_the_chiefs_finish_of_an_unanswered_question() {
        let mut snapshot = board();
        snapshot.tasks[0]["messages"] = json!([snapshot.chief_inbox[0]]);
        snapshot.events = vec![
            json!({"kind":"task.state","data":{"task":1,"from":"working","to":"waiting"}}),
            json!({"kind":"task.state","data":{"task":1,"from":"waiting","to":"done","result":4}}),
            json!({"kind":"api.answered","data":{"participant":"chief","method":"POST","path":"/api/tasks/1/finish","status":200,"error":null}}),
        ];
        assert!(failed(&snapshot, true).is_empty());
        snapshot.events.pop();
        assert!(failed(&snapshot, true)
            .iter()
            .any(|what| what.contains("automatic result")));
    }
}
