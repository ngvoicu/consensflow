//! The scenarios after the smoke (LMX-08 and LMX-12, T-1133's design, section 2): the core flow
//! of two tasks, and field report 6 in subcases each replayable alone. Each step's milestone is
//! what the board shows (a task's state, or the chief's call of the API answered, as a tooling
//! daemon writes it), never what the chief says, so a step is over whether the product did the
//! right thing or not, and the checks say which.
//!
//! On today's main some of field report 6's checks fail by design, until T-1118 (a follow-up of
//! a done task waits for its acceptance), T-1122 (`cancel` takes a done task, and a refusal names
//! the task that holds a window) and T-1128 (a window takes no follow-up while a task it did is
//! done and undecided) land in I-7: those are [`Known`], said apart and never passed.
//!
//! `cancel` is the control's planted red: the stand-in chief cancels the task it gave and asks
//! to send it back, so every run of it ends red at once, with the product's refusal.

use std::time::Duration;

use crate::live::report::Check;
use crate::matrix::observe::{self, Bind, Bindings, Is, Snapshot};
use crate::matrix::oracle::{self, Answered, Judged};

use super::{Known, Refused, Scenario, Step};

/// What the human answers a chief's questions with in these scenarios: do what was asked.
const FACTS: &[crate::matrix::machine::Fact] = &[
    crate::matrix::machine::Fact {
        asked_by: &["tier", "which member", "who should"],
        answer: "Every member of our staff is at tier standard.",
    },
    crate::matrix::machine::Fact {
        asked_by: &[
            "accept", "proceed", "go ahead", "shall i", "should i", "approve", "confirm", "cancel",
            "reopen",
        ],
        answer: "Yes: run exactly the command I gave, and tell me what it answered.",
    },
];

const NUDGE: &str = "Please run the command I gave last: the board does not show it yet.";

/// A real chief's deadline for one of these scenarios (the stand-ins' is the runner's own).
const DEADLINE: Duration = Duration::from_secs(20 * 60);

/// The work whose landing turns field report 6's known reds green.
const T1118: &str = "T-1118 (I-7): a follow-up of a done task waits for its acceptance";
const T1122: &str =
    "T-1122 (I-7): cancel takes a done task, and a refusal names the task holding the window";
const T1128: &str =
    "T-1128 (I-7): a window takes no follow-up while a task it did is done and undecided";

/// The task bound to `name`, the `nth`, as a stand-in's or a person's words name it.
fn t(bindings: &Bindings, name: &str, nth: usize) -> String {
    bindings.nth(name, nth).map_or_else(
        || format!("(no {name} task yet)"),
        |number| format!("T-{number}"),
    )
}

/// Whether the task bound to `name` (the `nth`) is in one of `states`.
fn is(snapshot: &Snapshot, bindings: &Bindings, name: &str, nth: usize, states: &[&str]) -> bool {
    bindings
        .nth(name, nth)
        .and_then(|number| snapshot.task(number))
        .is_some_and(|task| states.iter().any(|state| task["state"] == *state))
}

/// Whether the task bound to `name` (the `nth`) is done or accepted and its result was delivered.
fn answered(snapshot: &Snapshot, bindings: &Bindings, name: &str, nth: usize) -> bool {
    bindings.nth(name, nth).is_some_and(|number| {
        is(snapshot, bindings, name, nth, &["done", "accepted"])
            && snapshot.delivered_result(number).is_some()
    })
}

/// Whether the follow-up bound to `name` (the `nth`) is past what the product makes of it at
/// once, and the board at rest: on the board and waiting (`open`, as it should while its
/// predecessor is undecided), or given to its window and at work or over; not `queued` for a
/// window still to take it, which a look then would read too early.
fn left_the_queue(snapshot: &Snapshot, bindings: &Bindings, name: &str, nth: usize) -> bool {
    bindings.nth(name, nth).is_some()
        && !is(snapshot, bindings, name, nth, &["queued"])
        && crate::matrix::run::settled(snapshot)
}

/// Whether the chief called `action` on the task bound to `name` at least `times` times, and
/// the board is at rest after it.
fn called(
    snapshot: &Snapshot,
    bindings: &Bindings,
    name: &str,
    action: &str,
    times: usize,
) -> bool {
    let Some(number) = bindings.nth(name, 0) else {
        return false;
    };
    let path = format!("/api/tasks/{number}/{action}");
    let made = observe::calls(&snapshot.events)
        .iter()
        .filter(|call| call.participant == "chief" && call.method == "POST" && call.path == path)
        .count();
    made >= times && crate::matrix::run::settled(snapshot)
}

/// The check that the brief of `later` (a name and its `nth`) went into its window only after
/// `earlier` (likewise) reached `state`.
fn brief_after(
    judged: &Judged,
    earlier: (&str, usize),
    state: &str,
    later: (&str, usize),
) -> Check {
    let events = &judged.snapshot.events;
    let what = format!("its brief went in after the {} was {state}", earlier.0);
    let reached = judged
        .nth(earlier.0, earlier.1)
        .and_then(|number| observe::reached_at(events, number, state));
    let brief = judged.nth(later.0, later.1).and_then(|number| {
        let message = observe::brief_message(events, number)?;
        Some((number, observe::delivered_at(events, message)?))
    });
    match (reached, brief) {
        (Some(at), Some((number, delivered))) => Check::new(
            later.0,
            &what,
            delivered >= at,
            format!("T-{number}'s brief went in {} ms after", delivered - at),
        ),
        (None, Some((number, _))) => Check::failed(
            later.0,
            &what,
            format!(
                "T-{number}'s brief went in, and the {} was never {state}",
                earlier.0
            ),
        ),
        (_, None) => Check::failed(
            later.0,
            &what,
            format!("no brief of the {} task went in", later.0),
        ),
    }
}

/// A step a worker's task begins with: the stand-in's reply is `reply`, one word.
fn worker_task(
    name: &'static str,
    asks: fn(&Bindings) -> String,
    stand_in: fn(&Bindings) -> String,
    reached: fn(&Snapshot, &Bindings) -> bool,
) -> Step {
    Step {
        name,
        asks,
        stand_in,
        reached,
    }
}

/// A scenario of this module's, with what they share.
fn scenario(
    name: &'static str,
    steps: Vec<Step>,
    binds: &'static [Bind],
    checks: fn(&Judged) -> Vec<Check>,
    refusals: &'static [Refused],
    known: &'static [Known],
) -> Scenario {
    Scenario {
        name,
        version: 1,
        fixture: Vec::new(),
        steps,
        deadline: DEADLINE,
        turns: 40,
        facts: FACTS,
        nudge: NUDGE,
        binds,
        order: &[],
        refusals,
        checks,
        known,
    }
}

const ADVICE: &[Bind] = &[Bind {
    name: "advice",
    is: Is::Pool("advisor"),
    requester: "chief",
}];

/// The control's planted red: the stand-in chief, in one turn, puts the advice task on the
/// board, cancels it, and asks to send it back, which the product refuses
/// (`invalid-transition`). The run ends at once, red, naming the cancelled task.
pub fn cancel() -> Scenario {
    scenario(
        "cancel",
        vec![Step {
            name: "advice",
            asks: |_| {
                "Put one advice task on the board with `cf task add --advice --tier standard \"Reply with exactly: ADVICE_OK\"`, cancel it at once with `cf task cancel` and its number, then ask to send it back with `cf task reopen` and its number and the words \"Once more.\". Tell me in one line what each command answered.".to_owned()
            },
            stand_in: |_| {
                "CF task add --advice --tier standard :: SLEEP 60 Reply with exactly: ADVICE_OK\nCF task cancel T-1\nCF task reopen T-1 :: Once more.".to_owned()
            },
            reached: |snapshot, bindings| answered(snapshot, bindings, "advice", 0),
        }],
        ADVICE,
        |judged| vec![oracle::state(judged, "advice", 0, &["cancelled"])],
        &[Refused {
            task: "advice",
            action: "reopen",
            code: "invalid-transition",
        }],
        &[],
    )
}

const AB: &[Bind] = &[
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
];

/// LMX-08, the core flow, as the design has it: A done and accepted; B added after A, done,
/// sent back while done, done again, and accepted; and sending the accepted A back refused.
pub fn core() -> Scenario {
    scenario(
        "core",
        vec![
            worker_task(
                "A",
                |_| {
                    "Put one task on the board for our worker with `cf task add --tier standard \"Reply with exactly: A_DONE\"`, and tell me in one line when its result is back. Do not accept it yet.".to_owned()
                },
                |_| "DISPATCH --tier standard Reply with exactly: A_DONE".to_owned(),
                |snapshot, bindings| answered(snapshot, bindings, "A", 0),
            ),
            Step {
                name: "accept A",
                asks: |bindings| {
                    format!(
                        "Accept {} with `cf task accept {}`, and tell me in one line.",
                        t(bindings, "A", 0),
                        t(bindings, "A", 0)
                    )
                },
                stand_in: |bindings| format!("CF task accept {}", t(bindings, "A", 0)),
                reached: |snapshot, bindings| is(snapshot, bindings, "A", 0, &["accepted"]),
            },
            Step {
                name: "B after A",
                asks: |bindings| {
                    format!("Give the window that did {} a follow-up with `cf task add --after {} \"Reply with exactly: B_DONE\"`, and tell me in one line when its result is back. Do not accept it yet.", t(bindings, "A", 0), t(bindings, "A", 0))
                },
                stand_in: |bindings| {
                    format!(
                        "CF task add --after {} :: Reply with exactly: B_DONE",
                        t(bindings, "A", 0)
                    )
                },
                reached: |snapshot, bindings| answered(snapshot, bindings, "B", 0),
            },
            Step {
                name: "reopen B",
                asks: |bindings| {
                    format!("Send {} back with `cf task reopen {} \"Reply with exactly: B_AGAIN\"`, and tell me in one line when its new result is back.", t(bindings, "B", 0), t(bindings, "B", 0))
                },
                stand_in: |bindings| {
                    format!(
                        "CF task reopen {} :: Reply with exactly: B_AGAIN",
                        t(bindings, "B", 0)
                    )
                },
                reached: |snapshot, bindings| {
                    bindings.nth("B", 0).is_some_and(|number| {
                        is(snapshot, bindings, "B", 0, &["done"]) && results(snapshot, number) >= 2
                    })
                },
            },
            Step {
                name: "accept B",
                asks: |bindings| {
                    format!(
                        "Accept {} with `cf task accept {}`, and tell me in one line.",
                        t(bindings, "B", 0),
                        t(bindings, "B", 0)
                    )
                },
                stand_in: |bindings| format!("CF task accept {}", t(bindings, "B", 0)),
                reached: |snapshot, bindings| is(snapshot, bindings, "B", 0, &["accepted"]),
            },
            Step {
                name: "reopen A",
                asks: |bindings| {
                    format!("Now try to send the accepted {} back with `cf task reopen {} \"Once more.\"`, and tell me in one line what it answered.", t(bindings, "A", 0), t(bindings, "A", 0))
                },
                stand_in: |bindings| {
                    format!("CF task reopen {} :: Once more.", t(bindings, "A", 0))
                },
                reached: |snapshot, bindings| called(snapshot, bindings, "A", "reopen", 1),
            },
        ],
        AB,
        |judged| {
            let mut checks = vec![
                oracle::state(judged, "A", 0, &["accepted"]),
                oracle::state(judged, "B", 0, &["accepted"]),
                oracle::chief_call(judged, "B", "reopen", 0, Answered::Done),
                oracle::result_kept(judged, "A"),
            ];
            // B went to A's window, and came back to it when it was sent back.
            let window = |name: &str| {
                judged
                    .nth(name, 0)
                    .and_then(|number| judged.snapshot.task(number))
                    .map(|task| task["assignee"].clone())
            };
            checks.push(Check::new(
                "B",
                "it went to the window that did A",
                window("A").is_some_and(|a| !a.is_null() && Some(a) == window("B")),
                format!("A: {:?}, B: {:?}", window("A"), window("B")),
            ));
            let two = judged
                .nth("B", 0)
                .map_or(0, |number| results(judged.snapshot, number));
            checks.push(Check::new(
                "B",
                "it has two results, its first and the one after it was sent back",
                two == 2,
                format!("{two} result(s) delivered"),
            ));
            checks
        },
        &[Refused {
            task: "A",
            action: "reopen",
            code: "invalid-transition",
        }],
        &[],
    )
}

/// How many results of task `number` were delivered to the chief.
fn results(snapshot: &Snapshot, number: i64) -> usize {
    snapshot
        .chief_inbox
        .iter()
        .filter(|message| {
            message["kind"] == "result"
                && message["taskNumber"] == number
                && message["state"] == "delivered"
        })
        .count()
}

const DF: &[Bind] = &[
    Bind {
        name: "D",
        is: Is::Pool("worker"),
        requester: "chief",
    },
    Bind {
        name: "F",
        is: Is::After("D"),
        requester: "chief",
    },
];

/// The step every field report 6 subcase begins with: D, done without doing what it was asked.
fn done_without_work() -> Step {
    worker_task(
        "D",
        |_| {
            "Put one task on the board for our worker with `cf task add --tier standard \"Do not do anything: reply only that you stopped before doing any work.\"`, and tell me in one line when its result is back. Do not accept it.".to_owned()
        },
        |_| "DISPATCH --tier standard Reply with exactly: STOPPED_BEFORE_ANY_WORK".to_owned(),
        |snapshot, bindings| answered(snapshot, bindings, "D", 0),
    )
}

/// The step that adds F after the done and undecided D.
fn follow_up() -> Step {
    Step {
        name: "F after D",
        asks: |bindings| {
            format!("Without accepting {}, put its follow-up on the board with `cf task add --after {} \"Reply with exactly: F_DONE\"`, and tell me in one line when it is on the board.", t(bindings, "D", 0), t(bindings, "D", 0))
        },
        stand_in: |bindings| {
            format!(
                "CF task add --after {} :: Reply with exactly: F_DONE",
                t(bindings, "D", 0)
            )
        },
        reached: |snapshot, bindings| left_the_queue(snapshot, bindings, "F", 0),
    }
}

/// A step where the chief runs `cf task <action>` on a bound task.
macro_rules! act {
    ($step:literal, $action:literal, $name:literal, $times:literal) => {
        Step {
            name: $step,
            asks: |bindings| {
                format!(
                    "Run `cf task {} {}`, and tell me in one line what it answered.",
                    $action,
                    t(bindings, $name, 0)
                )
            },
            stand_in: |bindings| format!("CF task {} {}", $action, t(bindings, $name, 0)),
            reached: |snapshot, bindings| called(snapshot, bindings, $name, $action, $times),
        }
    };
    // An action that takes words (`reopen`): the stand-in's last word, whole.
    ($step:literal, $action:literal, $name:literal, $times:literal, $words:literal) => {
        Step {
            name: $step,
            asks: |bindings| {
                format!(
                    "Run `cf task {} {} \"{}\"`, and tell me in one line what it answered.",
                    $action,
                    t(bindings, $name, 0),
                    $words
                )
            },
            stand_in: |bindings| {
                format!(
                    "CF task {} {} :: {}",
                    $action,
                    t(bindings, $name, 0),
                    $words
                )
            },
            reached: |snapshot, bindings| called(snapshot, bindings, $name, $action, $times),
        }
    };
}

/// Field report 6's subcases (LMX-12), each a scenario of its own.
pub fn field6() -> Vec<Scenario> {
    vec![
        // D returns without doing its work; F, added after it, stays on the board, given to no
        // window, while D is done and undecided.
        scenario(
            "field6-done-predecessor",
            vec![done_without_work(), follow_up()],
            DF,
            |judged| {
                vec![
                    oracle::state(judged, "D", 0, &["done"]),
                    oracle::result_kept(judged, "D"),
                    oracle::state(judged, "F", 0, &["open"]),
                    oracle::never_briefed(judged, "F", 0),
                ]
            },
            &[],
            &[
                Known {
                    who: "F",
                    what: "it is open",
                    until: T1118,
                },
                Known {
                    who: "F",
                    what: "no window was given its brief",
                    until: T1118,
                },
            ],
        ),
        // D's acceptance releases F, which then goes into the window that did D once.
        scenario(
            "field6-acceptance-releases",
            vec![
                done_without_work(),
                follow_up(),
                Step {
                    name: "accept D",
                    asks: |bindings| {
                        format!("Accept {} with `cf task accept {}`, and tell me in one line when the follow-up's result is back.", t(bindings, "D", 0), t(bindings, "D", 0))
                    },
                    stand_in: |bindings| format!("CF task accept {}", t(bindings, "D", 0)),
                    reached: |snapshot, bindings| {
                        is(snapshot, bindings, "D", 0, &["accepted"])
                            && answered(snapshot, bindings, "F", 0)
                    },
                },
            ],
            DF,
            |judged| {
                vec![
                    oracle::state(judged, "D", 0, &["accepted"]),
                    brief_after(judged, ("D", 0), "accepted", ("F", 0)),
                    oracle::briefed_once(judged, "F", 0),
                ]
            },
            &[],
            &[Known {
                who: "F",
                what: "its brief went in after the D was accepted",
                until: T1118,
            }],
        ),
        // With nothing waiting for it, a done D is cancelled, not accepted, and its result kept.
        scenario(
            "field6-cancel-done",
            vec![done_without_work(), act!("cancel D", "cancel", "D", 1)],
            &DF[..1],
            |judged| {
                vec![
                    oracle::chief_call(judged, "D", "cancel", 0, Answered::Done),
                    oracle::state(judged, "D", 0, &["cancelled"]),
                    oracle::result_kept(judged, "D"),
                ]
            },
            &[],
            &[
                Known {
                    who: "D",
                    what: "the chief's first cancel of it was done",
                    until: T1122,
                },
                Known {
                    who: "D",
                    what: "it is cancelled",
                    until: T1122,
                },
            ],
        ),
        // While F waits for D, cancelling D is refused (`waited-for`); F cancelled, D is.
        scenario(
            "field6-cancel-with-waiter",
            vec![
                done_without_work(),
                follow_up(),
                act!("cancel D", "cancel", "D", 1),
                act!("cancel F", "cancel", "F", 1),
                act!("cancel D again", "cancel", "D", 2),
            ],
            DF,
            |judged| {
                vec![
                    oracle::chief_call(
                        judged,
                        "D",
                        "cancel",
                        0,
                        Answered::Refused("waited-for", &[]),
                    ),
                    oracle::chief_call(judged, "F", "cancel", 0, Answered::Done),
                    oracle::chief_call(judged, "D", "cancel", 1, Answered::Done),
                    oracle::state(judged, "F", 0, &["cancelled"]),
                    oracle::state(judged, "D", 0, &["cancelled"]),
                    oracle::result_kept(judged, "D"),
                ]
            },
            &[],
            &[
                Known {
                    who: "D",
                    what: "the chief's first cancel of it was refused with waited-for",
                    until: T1118,
                },
                Known {
                    who: "F",
                    what: "the chief's first cancel of it was done",
                    until: T1118,
                },
                Known {
                    who: "F",
                    what: "it is cancelled",
                    until: T1118,
                },
                Known {
                    who: "D",
                    what: "the chief's second cancel of it was done",
                    until: T1122,
                },
                Known {
                    who: "D",
                    what: "it is cancelled",
                    until: T1122,
                },
            ],
        ),
        // A accepted; a newer D in A's window done and undecided; F added after A waits on the
        // board until D is accepted.
        scenario(
            "field6-accepted-bypass",
            vec![
                worker_task(
                    "A",
                    |_| {
                        "Put one task on the board for our worker with `cf task add --tier standard \"Reply with exactly: A_DONE\"`, and tell me in one line when its result is back.".to_owned()
                    },
                    |_| "DISPATCH --tier standard Reply with exactly: A_DONE".to_owned(),
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0),
                ),
                Step {
                    name: "accept A",
                    asks: |bindings| {
                        format!(
                            "Accept {} with `cf task accept {}`, and tell me in one line.",
                            t(bindings, "A", 0),
                            t(bindings, "A", 0)
                        )
                    },
                    stand_in: |bindings| format!("CF task accept {}", t(bindings, "A", 0)),
                    reached: |snapshot, bindings| is(snapshot, bindings, "A", 0, &["accepted"]),
                },
                Step {
                    name: "D after A",
                    asks: |bindings| {
                        format!("Give the window that did {} a follow-up with `cf task add --after {} \"Reply only that you stopped before doing any work.\"`, and tell me in one line when its result is back. Do not accept it.", t(bindings, "A", 0), t(bindings, "A", 0))
                    },
                    stand_in: |bindings| {
                        format!(
                            "CF task add --after {} :: Reply with exactly: STOPPED_BEFORE_ANY_WORK",
                            t(bindings, "A", 0)
                        )
                    },
                    reached: |snapshot, bindings| answered(snapshot, bindings, "after A", 0),
                },
                Step {
                    name: "F after A",
                    asks: |bindings| {
                        format!("Without accepting that follow-up, give the window another one with `cf task add --after {} \"Reply with exactly: F_DONE\"`, and tell me in one line when it is on the board.", t(bindings, "A", 0))
                    },
                    stand_in: |bindings| {
                        format!(
                            "CF task add --after {} :: Reply with exactly: F_DONE",
                            t(bindings, "A", 0)
                        )
                    },
                    reached: |snapshot, bindings| left_the_queue(snapshot, bindings, "after A", 1),
                },
                Step {
                    name: "accept D",
                    asks: |bindings| {
                        format!("Accept {} with `cf task accept {}`, and tell me in one line when the other follow-up's result is back.", t(bindings, "after A", 0), t(bindings, "after A", 0))
                    },
                    stand_in: |bindings| format!("CF task accept {}", t(bindings, "after A", 0)),
                    reached: |snapshot, bindings| {
                        is(snapshot, bindings, "after A", 0, &["accepted"])
                            && answered(snapshot, bindings, "after A", 1)
                    },
                },
            ],
            &[
                Bind {
                    name: "A",
                    is: Is::Pool("worker"),
                    requester: "chief",
                },
                Bind {
                    name: "after A",
                    is: Is::After("A"),
                    requester: "chief",
                },
            ],
            |judged| {
                vec![
                    oracle::state(judged, "after A", 0, &["accepted"]),
                    brief_after(judged, ("after A", 0), "accepted", ("after A", 1)),
                    oracle::briefed_once(judged, "after A", 1),
                ]
            },
            &[],
            &[Known {
                who: "after A",
                what: "its brief went in after the after A was accepted",
                until: T1128,
            }],
        ),
        // The done D and the paused P in one window: sending D back is refused `session-busy`,
        // naming P, paused; cancelling D keeps its result and leaves P paused. The shape is built
        // through the follow-up door main leaves open (T-1118 closes it: then P waits on the
        // board, and this subcase is to prove the shape unreachable instead).
        scenario(
            "field6-paused-holder",
            vec![
                done_without_work(),
                Step {
                    name: "P after D",
                    asks: |bindings| {
                        format!("Without accepting {}, give its window a follow-up with `cf task add --after {} \"Work for two minutes, then reply with exactly: P_DONE\"`, and tell me in one line when it is at work.", t(bindings, "D", 0), t(bindings, "D", 0))
                    },
                    stand_in: |bindings| {
                        format!(
                            "CF task add --after {} :: SLEEP 120 Reply with exactly: P_DONE",
                            t(bindings, "D", 0)
                        )
                    },
                    reached: |snapshot, bindings| is(snapshot, bindings, "P", 0, &["working"]),
                },
                Step {
                    name: "pause P",
                    asks: |bindings| {
                        format!(
                            "Pause it with `cf task pause {}`, and tell me in one line.",
                            t(bindings, "P", 0)
                        )
                    },
                    stand_in: |bindings| format!("CF task pause {}", t(bindings, "P", 0)),
                    reached: |snapshot, bindings| is(snapshot, bindings, "P", 0, &["paused"]),
                },
                act!("reopen D", "reopen", "D", 1, "Once more."),
                act!("cancel D", "cancel", "D", 1),
            ],
            &[
                Bind {
                    name: "D",
                    is: Is::Pool("worker"),
                    requester: "chief",
                },
                Bind {
                    name: "P",
                    is: Is::After("D"),
                    requester: "chief",
                },
            ],
            |judged| {
                vec![
                    oracle::chief_call(
                        judged,
                        "D",
                        "reopen",
                        0,
                        Answered::Refused("session-busy", &["{P}", "paused"]),
                    ),
                    oracle::chief_call(judged, "D", "cancel", 0, Answered::Done),
                    oracle::result_kept(judged, "D"),
                    oracle::state(judged, "P", 0, &["paused"]),
                ]
            },
            &[],
            &[
                Known {
                    who: "D",
                    what: "the chief's first reopen of it was refused with session-busy",
                    until: T1122,
                },
                Known {
                    who: "D",
                    what: "the chief's first cancel of it was done",
                    until: T1122,
                },
            ],
        ),
    ]
}
