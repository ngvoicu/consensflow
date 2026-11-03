//! The worker's verbs of a board in words every stand-in speaks (T-1207): the scenarios run with
//! the worker's seat shaped as Devin's or Codex's stand-in (`fake-devin`, `fake-codex`) besides
//! Claude Code's, where the older scenarios use words of the Claude-shaped stand-in alone
//! (`PACED`, `CLAUDE-JOB`, `REPLY … THEN`).
//!
//! - `seat-tell`: the chief tells a worker at work something; its answer comes back to the
//!   chief as an answer, and the task resumed goes on to its result.
//! - `seat-finish`: a worker's turn answers and leaves a command running; the chief finishes the
//!   task with what it said.

use crate::live::report::Check;
use crate::matrix::observe::{Bind, Is};
use crate::matrix::oracle::{self, Judged};

use super::kit::{answered, is, run, scenario, since};
use super::{Scenario, Step};

const A: &[Bind] = &[Bind {
    name: "A",
    is: Is::Pool("worker"),
    requester: "chief",
}];

/// The bodies of the chief's messages of `kind` about the task bound to `name`.
fn bodies(judged: &Judged, name: &str, kind: &str) -> Vec<String> {
    let number = judged.nth(name, 0);
    judged
        .snapshot
        .chief_inbox
        .iter()
        .filter(|message| {
            message["kind"] == kind && number.is_some_and(|n| message["taskNumber"] == n)
        })
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .collect()
}

/// The scenarios of the seats.
pub fn seats() -> Vec<Scenario> {
    vec![
        scenario(
            "seat-tell",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 40 Reply with exactly: NEVER_SAID",
                    |snapshot, bindings| since(snapshot, bindings, "A", "working")
                        .is_some_and(|seconds| seconds >= 5)
                ),
                // The words told are the worker's own call of `cf answer` (the tell is m-2 on the
                // board this starts), as an agent answers a tell: the Devin and Codex stand-ins
                // run a `CF` line, and answer no question by themselves.
                run!(
                    "tell A",
                    "tell {A} :: CF answer m-2 :: TOLD",
                    |snapshot, bindings| {
                        bindings.nth("A", 0).is_some_and(|number| {
                            snapshot.chief_inbox.iter().any(|message| {
                                message["kind"] == "answer" && message["taskNumber"] == number
                            })
                        }) && crate::matrix::run::settled(snapshot)
                    }
                ),
                run!(
                    "resume A",
                    "task resume {A} :: Reply with exactly: A_AGAIN",
                    |snapshot, bindings| {
                        answered(snapshot, bindings, "A", 0)
                            && crate::matrix::run::settled(snapshot)
                    }
                ),
            ],
            A,
            |judged| {
                let answers = bodies(judged, "A", "answer");
                let results = bodies(judged, "A", "result");
                vec![
                    Check::new(
                        "A",
                        "its window's answer to the tell reached the chief once",
                        answers.len() == 1 && answers[0].contains("TOLD"),
                        format!("{answers:?}"),
                    ),
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new(
                        "A",
                        "its result is the resumed turn's",
                        results.last().is_some_and(|body| body.contains("A_AGAIN"))
                            && !results.iter().any(|body| body.contains("NEVER_SAID")),
                        format!("{results:?}"),
                    ),
                ]
            },
            &[],
            &[],
        ),
        scenario(
            "seat-finish",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: BACKGROUND 300 W_ANSWERED",
                    |snapshot, bindings| {
                        is(snapshot, bindings, "A", 0, &["working", "done"])
                            && since(snapshot, bindings, "A", "working")
                                .is_some_and(|seconds| seconds >= 10)
                    }
                ),
                run!("finish A", "task finish {A}", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["done"])
                        && crate::matrix::run::settled(snapshot)
                }),
            ],
            A,
            |judged| {
                let results = bodies(judged, "A", "result");
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new(
                        "A",
                        "its one result is what its turn said",
                        results.len() == 1 && results[0].contains("W_ANSWERED"),
                        format!("{results:?}"),
                    ),
                ]
            },
            &[],
            &[],
        ),
    ]
}
