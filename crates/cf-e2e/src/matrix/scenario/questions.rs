//! LMX-11, questions: a worker's `cf ask` answered by the chief's `cf answer`; a question in the
//! last words of a worker's turn (T-1119), which is a question to the chief and no result; and an
//! advisor's answer, which comes back to the chief as its task's result.

use crate::live::report::Check;
use crate::matrix::observe::{Bind, Is};
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
                reached: |snapshot, bindings| answered(snapshot, bindings, "A", 0),
            }],
            &[Bind { name: "A", is: Is::Pool("worker"), requester: "chief" }],
            |judged| {
                let asked = from_window(judged, "A", "question");
                let question = asked.first().map(|message| message["id"].clone());
                let calls = crate::matrix::observe::calls(&judged.snapshot.events);
                let answers: Vec<_> = calls
                    .iter()
                    .filter(|call| call.participant == "chief" && call.path == "/api/answers" && call.error.is_none())
                    .collect();
                vec![
                    Check::new("A", "its window asked the chief one question", asked.len() == 1, format!("{} question(s)", asked.len())),
                    Check::new(
                        "A",
                        "the question was delivered to the chief once",
                        asked.first().is_some_and(|message| message["state"] == "delivered" || message["state"] == "read"),
                        format!("{:?}", asked.first().map(|message| message["state"].clone())),
                    ),
                    Check::new("chief", "the chief answered it with cf answer", answers.len() == 1, format!("{} answer call(s); question {question:?}", answers.len())),
                    oracle::state(judged, "A", 0, &["done"]),
                    oracle::result_kept(judged, "A"),
                ]
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
                let results = from_window(judged, "A", "result").len();
                let asked = from_window(judged, "A", "question").len();
                vec![
                    oracle::state(judged, "A", 0, &["waiting"]),
                    Check::new("A", "its last words reached the chief as a question", asked == 1, format!("{asked} question(s)")),
                    Check::new("A", "they are no result", results == 0, format!("{results} result(s)")),
                ]
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
