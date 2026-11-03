//! LMX-09, follow-ups and order: `--after` a window at work (and, in `core` and field report 6,
//! a done and undecided one and an accepted one), `--needs` given more than once (field report 2,
//! T-1110), `--before`, and `--self` with `--needs`. Each checks the ledger's states and that no
//! brief reached a window before what it waits for was accepted.

use crate::live::report::Check;
use crate::matrix::observe::{Bind, Is};
use crate::matrix::oracle::{self, Answered};

use super::kit::{
    answered, brief_after, called, is, needs, result_says, run, same_window, scenario,
};
use super::{Known, Scenario, Step};

/// The work whose landing turns field report 2's known reds green.
const T1110: &str = "T-1110 (wt/cf-repeated-flags, I-7): every --needs given is kept";

/// The scenarios of LMX-09.
pub fn order() -> Vec<Scenario> {
    vec![
        // B, added after A while A's window is at work, waits on the board for A's acceptance,
        // then goes into A's window.
        scenario(
            "order-after-working",
            vec![
                run!("A", "task add --tier standard :: SLEEP 12 Reply with exactly: A_DONE", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["working"])
                }),
                run!("B after A", "task add --after {A} :: Reply with exactly: B_DONE", |snapshot, bindings| {
                    bindings.nth("B", 0).is_some() && answered(snapshot, bindings, "A", 0)
                }),
                run!("accept A", "task accept {A}", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["accepted"]) && answered(snapshot, bindings, "B", 0)
                }),
            ],
            &[
                Bind { name: "A", is: Is::Pool("worker"), requester: "chief" },
                Bind { name: "B", is: Is::After("A"), requester: "chief" },
            ],
            |judged| {
                vec![
                    needs(judged, "B", &["A"]),
                    brief_after(judged, ("A", 0), "accepted", ("B", 0)),
                    oracle::briefed_once(judged, "B", 0),
                    same_window(judged, "A", "B"),
                    result_says(judged, "B", "B_DONE"),
                ]
            },
            &[],
            &[],
        ),
        // Field report 2: C needs A and B, each named with its own --needs. Main kept only the
        // last, so C went in when B was accepted, before A was.
        scenario(
            "field2-needs-repeated",
            vec![
                run!(
                    "A and B",
                    "task add --tier standard :: Reply with exactly: A_DONE\ntask add --test --tier standard :: Reply with exactly: B_DONE",
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0) && answered(snapshot, bindings, "B", 0)
                ),
                run!(
                    "C needs A and B",
                    "task add --advice --tier standard --needs {A} --needs {B} :: Reply with exactly: C_DONE",
                    |snapshot, bindings| bindings.nth("C", 0).is_some() && crate::matrix::run::settled(snapshot)
                ),
// C, waiting for A as well, must stay on the board: a C that goes in now went in on B
                // alone. Its window is given the time to take it.
                run!("accept B", "task accept {B}", |snapshot, bindings| {
                    is(snapshot, bindings, "B", 0, &["accepted"])
                        && super::kit::since(snapshot, bindings, "B", "accepted").is_some_and(|seconds| seconds >= 15)
                        && crate::matrix::run::settled(snapshot)
                }),
                run!("accept A", "task accept {A}", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["accepted"]) && answered(snapshot, bindings, "C", 0)
                }),
            ],
            &[
                Bind { name: "A", is: Is::Pool("worker"), requester: "chief" },
                Bind { name: "B", is: Is::Pool("tester"), requester: "chief" },
                Bind { name: "C", is: Is::Pool("advisor"), requester: "chief" },
            ],
            |judged| {
                vec![
                    needs(judged, "C", &["A", "B"]),
                    brief_after(judged, ("A", 0), "accepted", ("C", 0)),
                    brief_after(judged, ("B", 0), "accepted", ("C", 0)),
                    oracle::briefed_once(judged, "C", 0),
                ]
            },
            &[],
            &[
                Known { who: "C", what: "it needs A and B", until: T1110 },
                Known { who: "C", what: "its brief went in after the A was accepted", until: T1110 },
            ],
        ),
        // A waits for W; B is added --before A, so A waits for B too, and goes in only when both
        // are accepted.
        scenario(
            "order-before",
            vec![
                run!("W", "task add --tier standard :: SLEEP 10 Reply with exactly: W_DONE", |snapshot, bindings| {
                    is(snapshot, bindings, "W", 0, &["working"])
                }),
                run!("A needs W", "task add --advice --tier standard --needs {W} :: Reply with exactly: A_DONE", |_, bindings| {
                    bindings.nth("A", 0).is_some()
                }),
                run!("B before A", "task add --test --tier standard --before {A} :: Reply with exactly: B_DONE", |snapshot, bindings| {
                    answered(snapshot, bindings, "B", 0) && answered(snapshot, bindings, "W", 0)
                }),
                run!("accept W", "task accept {W}", |snapshot, bindings| {
                    is(snapshot, bindings, "W", 0, &["accepted"]) && crate::matrix::run::settled(snapshot)
                }),
                run!("accept B", "task accept {B}", |snapshot, bindings| {
                    is(snapshot, bindings, "B", 0, &["accepted"]) && answered(snapshot, bindings, "A", 0)
                }),
            ],
            &[
                Bind { name: "W", is: Is::Pool("worker"), requester: "chief" },
                Bind { name: "A", is: Is::Pool("advisor"), requester: "chief" },
                Bind { name: "B", is: Is::Pool("tester"), requester: "chief" },
            ],
            |judged| {
                vec![
                    needs(judged, "A", &["W", "B"]),
                    brief_after(judged, ("W", 0), "accepted", ("A", 0)),
                    brief_after(judged, ("B", 0), "accepted", ("A", 0)),
                    oracle::briefed_once(judged, "A", 0),
                ]
            },
            &[],
            &[],
        ),
        // The chief's own step, --self --needs A: its brief comes back to the chief when A is
        // accepted, and the chief finishes it with `cf task done`, which accepts it.
        scenario(
            "order-self-needs",
            vec![
                run!("A", "task add --tier standard :: Reply with exactly: A_DONE", |snapshot, bindings| {
                    answered(snapshot, bindings, "A", 0)
                }),
                Step {
                    human: None,
                    restarts: false,
                    name: "own step",
                    asks: |bindings| {
                        format!(
                            "Put your own step on the board with `cf task add --self --needs {a} \"Finish this step with cf task done and its number.\"`; when its brief comes back to you, finish it with `cf task done` and its number and the words \"step finished\". Tell me in one line when it is on the board.",
                            a = super::kit::t(bindings, "A", 0)
                        )
                    },
                    // The step is the next task: its number is A's and one.
                    stand_in: |bindings| {
                        let own = bindings.nth("A", 0).map_or(0, |number| number + 1);
                        format!(
                            "CF task add --self --needs {} :: CF task done T-{own} :: step finished",
                            super::kit::t(bindings, "A", 0)
                        )
                    },
                    reached: |snapshot, bindings| {
                        bindings.nth("S", 0).is_some() && crate::matrix::run::settled(snapshot)
                    },
                },
                run!("accept A", "task accept {A}", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["accepted"])
                        && is(snapshot, bindings, "S", 0, &["done", "accepted"])
                        && called(snapshot, bindings, "S", "done", 1)
                }),
            ],
            &[
                Bind { name: "A", is: Is::Pool("worker"), requester: "chief" },
                Bind { name: "S", is: Is::Own, requester: "chief" },
            ],
            |judged| {
                vec![
                    needs(judged, "S", &["A"]),
                    brief_after(judged, ("A", 0), "accepted", ("S", 0)),
                    oracle::chief_call(judged, "S", "done", 0, Answered::Done),
                    oracle::state(judged, "S", 0, &["accepted"]),
                    own_brief_to_the_chief(judged),
                ]
            },
            &[],
            &[],
        ),
    ]
}

/// The check that the chief's own step went to the chief and to no member's window.
fn own_brief_to_the_chief(judged: &crate::matrix::oracle::Judged) -> Check {
    let task = judged
        .nth("S", 0)
        .and_then(|number| judged.snapshot.task(number));
    Check::new(
        "S",
        "it is the chief's own, in no pool",
        task.is_some_and(|task| task["assignee"] == "chief" && task["pool"].is_null()),
        format!(
            "{:?}",
            task.map(|task| (task["assignee"].clone(), task["pool"].clone()))
        ),
    )
}
