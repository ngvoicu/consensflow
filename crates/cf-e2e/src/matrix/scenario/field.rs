//! LMX-12, the field reports of 2026-11-02 besides 6 (and 2, in `order`; 8, in `interrupts`),
//! and the pause that ended a window's work (T-1117): each its own scenario, its checks those
//! that fail on today's main as the field's board did, marked as known reds until the task
//! whose branch fixes them lands.
//!
//! - 1, tier fallback (T-1108): the last member of a task's tier leaves while the task waits.
//! - 3, accept a failed task (T-1111): the chief accepts what it checked, and the waiter goes.
//! - 4, a busy model (T-1112): a 529 from the provider waits and tries again, no failure.
//! - 5, notices (T-1113): a failure is told once, before what waits on it.
//! - 7, a brief that never began (T-1130): its window never took it; the task is paused and the
//!   chief told.
//! - the pause of a window whose turn ignores it while a job runs (T-1117): the window is kept.

use crate::live::report::Check;
use crate::matrix::observe::{Bind, Is};
use crate::matrix::oracle::{self, Answered, Judged};

use super::kit::{answered, brief_after, called, is, run, scenario, since, since_call};
use super::{Extra, Known, Scenario, Step};

const T1108: &str =
    "T-1108 (wt/tier-fallback, I-7): a task whose tier was left goes to the nearest";
const T1111: &str =
    "T-1111 (wt/accept-failed, I-7): a failed task is accepted on the words of what was checked";
const T1112: &str = "T-1112 (wt/transient-model-errors, I-7): a busy model waits and tries again, and fails nothing";
const T1113: &str =
    "T-1113 (wt/board-notices, I-7): a failure is told once, before what waits on it";
const T1130: &str = "T-1130 (wt/brief-never-began, accepted, in no integration yet): a brief its window never began pauses its task";
const T1117: &str = "T-1117 (wt/pause-keeps-work, I-7): a pause keeps a window that runs work";

/// The chief's notes (from nobody) whose words hold `words`, oldest first.
fn notes_with<'a>(judged: &'a Judged, words: &str) -> Vec<&'a serde_json::Value> {
    judged
        .snapshot
        .chief_inbox
        .iter()
        .filter(|message| message["kind"] == "note")
        .filter(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains(words))
        })
        .collect()
}

/// The lane of the window that has the task bound to `name`.
fn lane_of<'a>(judged: &'a Judged, name: &str) -> Option<&'a serde_json::Value> {
    let handle = judged
        .nth(name, 0)
        .and_then(|number| judged.snapshot.task(number))
        .and_then(|task| task["assignee"].as_str())?;
    judged
        .snapshot
        .lanes
        .iter()
        .find(|lane| lane["participant"]["handle"] == handle)
}

/// The steps that make A fail with B waiting on it: X at work, A (the worker's, whose window
/// exits at once) and B (A's waiter) added behind it, X accepted.
fn a_fails_with_a_waiter() -> Vec<Step> {
    vec![
        run!("X", "task add --advice --tier standard :: SLEEP 6 Reply with exactly: X_DONE", |snapshot, bindings| {
            is(snapshot, bindings, "X", 0, &["working"])
        }),
        run!(
            "A needs X, B needs A",
            "task add --tier standard --needs {X} :: Reply with exactly: NEVER_SHOWN\ntask add --test --tier standard --needs {X+1} :: Reply with exactly: B_DONE",
            |snapshot, bindings| {
                bindings.nth("A", 0).is_some() && bindings.nth("B", 0).is_some() && answered(snapshot, bindings, "X", 0)
            }
        ),
        run!("accept X", "task accept {X}", |snapshot, bindings| {
            is(snapshot, bindings, "A", 0, &["failed"]) && crate::matrix::run::settled(snapshot)
        }),
    ]
}

/// The steps that make A fail in a turn of its window, its provider refusing it for good,
/// with B waiting on it: the path a task that fails at work takes (its window's launch failing
/// is another).
fn a_fails_in_a_turn_with_a_waiter() -> Vec<Step> {
    vec![
        run!("X", "task add --advice --tier standard :: SLEEP 6 Reply with exactly: X_DONE", |snapshot, bindings| {
            is(snapshot, bindings, "X", 0, &["working"])
        }),
        run!(
            "A needs X, B needs A",
            "task add --tier standard --needs {X} :: MODEL-GONE\ntask add --test --tier standard --needs {X+1} :: Reply with exactly: B_DONE",
            |snapshot, bindings| {
                bindings.nth("A", 0).is_some() && bindings.nth("B", 0).is_some() && answered(snapshot, bindings, "X", 0)
            }
        ),
        run!("accept X", "task accept {X}", |snapshot, bindings| {
            is(snapshot, bindings, "A", 0, &["failed"]) && crate::matrix::run::settled(snapshot)
        }),
    ]
}

const XAB: &[Bind] = &[
    Bind {
        name: "X",
        is: Is::Pool("advisor"),
        requester: "chief",
    },
    Bind {
        name: "A",
        is: Is::Pool("worker"),
        requester: "chief",
    },
    Bind {
        name: "B",
        is: Is::Pool("tester"),
        requester: "chief",
    },
];

/// The worker's window exits at once, its harness logged in nowhere: its task fails.
const WORKER_FAILS: &[(&str, &str)] = &[
    ("CF_TEST_NO_LOGIN_EXITS", "matrix-worker"),
    ("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "8000"),
];

/// The scenarios of LMX-12 that are not in `order`, `interrupts` or `phase2`.
pub fn field() -> Vec<Scenario> {
    vec![
        // 1: T waits for A, for a complex tester; the only complex tester leaves the staff.
        scenario(
            "field1-tier-fallback",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 10 Reply with exactly: A_DONE",
                    |snapshot, bindings| { is(snapshot, bindings, "A", 0, &["working"]) }
                ),
                run!(
                    "T for a complex tester",
                    "task add --test --tier complex --needs {A} :: Reply with exactly: T_DONE",
                    |snapshot, bindings| { is(snapshot, bindings, "T", 0, &["open"]) }
                ),
                Step {
                    human: Some(|members| {
                        members
                            .page(
                                "member.remove",
                                serde_json::json!({ "agent": "matrix-tester-complex" }),
                            )
                            .map(|_| ())
                    }),
                    restarts: false,
                    name: "the complex tester leaves the staff",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, bindings| answered(snapshot, bindings, "A", 0),
                },
                run!("accept A", "task accept {A}", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["accepted"])
                        && (answered(snapshot, bindings, "T", 0)
                            || since(snapshot, bindings, "A", "accepted")
                                .is_some_and(|seconds| seconds >= 20))
                }),
            ],
            &[
                Bind {
                    name: "A",
                    is: Is::Pool("worker"),
                    requester: "chief",
                },
                Bind {
                    name: "T",
                    is: Is::Pool("tester"),
                    requester: "chief",
                },
            ],
            |judged| {
                let to = lane_of(judged, "T")
                    .and_then(|lane| lane["participant"]["agent"].as_str())
                    .unwrap_or("none");
                let retiered = judged
                    .snapshot
                    .events
                    .iter()
                    .any(|event| event["kind"] == "task.retiered");
                let events = &judged.snapshot.events;
                let t = judged.nth("T", 0).unwrap_or(0);
                let opened_complex = events.iter().any(|event| {
                    event["kind"] == "task.opened"
                        && event["data"]["task"] == t
                        && event["data"]["tier"] == "complex"
                });
                let left = events.iter().any(|event| {
                    event["kind"] == "member.left"
                        && event["data"]["handle"] == "matrix-tester-complex"
                });
                vec![
                    Check::new(
                        "T",
                        "it was opened for a complex tester",
                        opened_complex,
                        format!("T-{t}"),
                    ),
                    Check::new(
                        "staff",
                        "the complex tester left the staff while T waited",
                        left,
                        "member.left",
                    ),
                    oracle::state(judged, "T", 0, &["done"]),
                    Check::new(
                        "T",
                        "it went to the standard tester",
                        to == "matrix-tester",
                        format!("its window is a session of {to}"),
                    ),
                    Check::new(
                        "T",
                        "the board logged that it changed its tier",
                        retiered,
                        "task.retiered",
                    ),
                ]
            },
            &[],
            &[
                Known {
                    who: "T",
                    what: "it is done",
                    until: T1108,
                },
                Known {
                    who: "T",
                    what: "it went to the standard tester",
                    until: T1108,
                },
                Known {
                    who: "T",
                    what: "the board logged that it changed its tier",
                    until: T1108,
                },
            ],
        )
        .with_extra(&[Extra {
            agent: "matrix-tester-complex",
            role: "tester",
            tier: "complex",
        }]),
        // 3: A fails; the chief accepts it on the words of what it checked, and B, which waits
        // on it, goes.
        scenario(
            "field3-accept-failed",
            {
                let mut steps = a_fails_with_a_waiter();
                steps.push(run!(
                    "accept A",
                    "task accept {A} :: I checked it by hand: the work is in",
                    // B, released by A's acceptance, is given the time to go in and answer.
                    |snapshot, bindings| {
                        answered(snapshot, bindings, "B", 0)
                            || (called(snapshot, bindings, "A", "accept", 1)
                                && since_call(snapshot, bindings, "A", "accept")
                                    .is_some_and(|seconds| seconds >= 15))
                    }
                ));
                steps
            },
            XAB,
            |judged| {
                vec![
                    oracle::chief_call(judged, "A", "accept", 0, Answered::Done),
                    oracle::state(judged, "A", 0, &["accepted"]),
                    oracle::state(judged, "B", 0, &["done"]),
                    brief_after(judged, ("A", 0), "accepted", ("B", 0)),
                ]
            },
            &[],
            &[
                Known {
                    who: "A",
                    what: "the chief's first accept of it was done",
                    until: T1111,
                },
                Known {
                    who: "A",
                    what: "it is accepted",
                    until: T1111,
                },
                Known {
                    who: "B",
                    what: "it is done",
                    until: T1111,
                },
                Known {
                    who: "B",
                    what: "its brief went in after the A was accepted",
                    until: T1111,
                },
            ],
        )
        .ending(&["A"])
        .with_vars(WORKER_FAILS, &[]),
        // 4: the worker's provider refuses its turn for a moment (a 529).
        scenario(
            "field4-busy-model",
            vec![run!(
                "A",
                "task add --tier standard :: MODEL-BUSY",
                |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["failed"])
                        || answered(snapshot, bindings, "A", 0)
                }
            )],
            &[Bind {
                name: "A",
                is: Is::Pool("worker"),
                requester: "chief",
            }],
            |judged| {
                let failed = judged.nth("A", 0).is_some_and(|number| {
                    crate::matrix::observe::reached_at(&judged.snapshot.events, number, "failed")
                        .is_some()
                });
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new(
                        "A",
                        "it never failed",
                        !failed,
                        if failed { "it failed" } else { "it did not" },
                    ),
                    Check::new(
                        "A",
                        "the chief was told its model is busy",
                        !notes_with(judged, "its model is busy").is_empty(),
                        format!(
                            "{} such note(s)",
                            notes_with(judged, "its model is busy").len()
                        ),
                    ),
                ]
            },
            &[],
            &[
                Known {
                    who: "A",
                    what: "it is done",
                    until: T1112,
                },
                Known {
                    who: "A",
                    what: "it never failed",
                    until: T1112,
                },
                Known {
                    who: "A",
                    what: "the chief was told its model is busy",
                    until: T1112,
                },
            ],
        )
        .ending(&["A"]),
        // 5: A fails in a turn with B waiting on it; the chief is told of the failure once, first.
        scenario(
            "field5-notices",
            a_fails_in_a_turn_with_a_waiter(),
            XAB,
            |judged| {
                let (a, b) = (
                    judged.nth("A", 0).unwrap_or(0),
                    judged.nth("B", 0).unwrap_or(0),
                );
                let about: Vec<&serde_json::Value> = judged
                    .snapshot
                    .chief_inbox
                    .iter()
                    .filter(|message| message["kind"] == "note")
                    .filter(|message| {
                        message["body"].as_str().is_some_and(|body| {
                            body.contains(&format!("T-{a} failed"))
                                || body.contains(&format!("T-{a}, which failed"))
                        })
                    })
                    .collect();
                // In the order they were written, which is the order they go in.
                let mut about = about;
                about.sort_by_key(|note| note["id"].as_i64());
                let first = about
                    .first()
                    .and_then(|note| note["body"].as_str())
                    .unwrap_or_default();
                vec![
                    Check::new(
                        "A",
                        "the first note of its failure says it failed",
                        first.starts_with(&format!("T-{a} failed")),
                        crate::live::cut(first, 160),
                    ),
                    Check::new(
                        "A",
                        "one note tells its failure and what waits on it",
                        about.len() == 1 && first.contains(&format!("T-{b}")),
                        format!("{} note(s) of it", about.len()),
                    ),
                ]
            },
            &[],
            &[
                Known {
                    who: "A",
                    what: "the first note of its failure says it failed",
                    until: T1113,
                },
                Known {
                    who: "A",
                    what: "one note tells its failure and what waits on it",
                    until: T1113,
                },
            ],
        )
        .ending(&["A"]),
        // 7: B's window never begins its brief (still in its prompt's hooks).
        scenario(
            "field7-never-began",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: Reply with exactly: A_DONE",
                    |snapshot, bindings| { answered(snapshot, bindings, "A", 0) }
                ),
                run!(
                    "B after A",
                    "task add --after {A} :: HOOKS 3600 Reply with exactly: TWO",
                    |_, bindings| { bindings.nth("B", 0).is_some() }
                ),
                run!("accept A", "task accept {A}", |snapshot, bindings| {
                    is(snapshot, bindings, "B", 0, &["paused"])
                        || since(snapshot, bindings, "B", "working")
                            .is_some_and(|seconds| seconds >= 75)
                }),
            ],
            &[
                Bind {
                    name: "A",
                    is: Is::Pool("worker"),
                    requester: "chief",
                },
                Bind {
                    name: "B",
                    is: Is::After("A"),
                    requester: "chief",
                },
            ],
            |judged| {
                let b = judged.nth("B", 0).unwrap_or(0);
                let told = notes_with(
                    judged,
                    &format!("T-{b} is paused: its window began no turn"),
                );
                vec![
                    oracle::state(judged, "B", 0, &["paused"]),
                    Check::new(
                        "B",
                        "the chief was told its window began no turn",
                        told.len() == 1,
                        format!("{} note(s)", told.len()),
                    ),
                ]
            },
            &[],
            &[
                Known {
                    who: "B",
                    what: "it is paused",
                    until: T1130,
                },
                Known {
                    who: "B",
                    what: "the chief was told its window began no turn",
                    until: T1130,
                },
            ],
        )
        .with_vars(&[], &[("CONSENSFLOW_ARRIVAL_TIMEOUT_MS", "15000")]),
        // A pause of a turn that ignores it while a job of its runs: the window is kept.
        scenario(
            "pause-keeps-work",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: JOB 120 HANG 120 Reply with exactly: NEVER",
                    |snapshot, bindings| { is(snapshot, bindings, "A", 0, &["working"]) }
                ),
                run!("pause A", "task pause {A}", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["paused"])
                        && since(snapshot, bindings, "A", "paused")
                            .is_some_and(|seconds| seconds >= 25)
                }),
            ],
            &[Bind {
                name: "A",
                is: Is::Pool("worker"),
                requester: "chief",
            }],
            |judged| {
                let lane = lane_of(judged, "A");
                let kept = lane.is_some_and(|lane| {
                    !lane["pane"].is_null() && lane["activity"]["state"] != "closed"
                });
                vec![
                    oracle::state(judged, "A", 0, &["paused"]),
                    Check::new(
                        "A",
                        "its window, which runs a job, is kept",
                        kept,
                        format!(
                            "{:?}",
                            lane.map(|lane| (
                                lane["pane"].clone(),
                                lane["activity"]["state"].clone()
                            ))
                        ),
                    ),
                    Check::new(
                        "A",
                        "the chief was told the window is kept for its work",
                        !notes_with(judged, "keeps the window open").is_empty(),
                        format!(
                            "{} such note(s)",
                            notes_with(judged, "keeps the window open").len()
                        ),
                    ),
                ]
            },
            &[],
            &[
                Known {
                    who: "A",
                    what: "its window, which runs a job, is kept",
                    until: T1117,
                },
                Known {
                    who: "A",
                    what: "the chief was told the window is kept for its work",
                    until: T1117,
                },
            ],
        ),
    ]
}
