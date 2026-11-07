//! LMX-10, interrupts: `pause` and `resume` mid-turn and mid-tool (a job of its turn's running),
//! `tell` then `resume` (field report 8, T-1139), `cancel` of an open, a working, a paused and a
//! done task, `edit` of an open task, and `finish` of a working, a waiting and a paused task
//! (T-1119). Each checks the ledger's states and the product's own answers to the chief.

use crate::live::report::Check;
use crate::matrix::observe::{self, Bind, Is};
use crate::matrix::oracle::{self, Answered, Judged};

use super::kit::{answered, called, is, result_says, run, scenario};
use super::{Scenario, Step};

const ONE: &[Bind] = &[Bind {
    name: "A",
    is: Is::Pool("worker"),
    requester: "chief",
}];

/// The check that the task bound to `name` went to `working` after the chief's `action` of it,
/// and before it was next done: the board showed its window at work on the words.
fn working_after(judged: &Judged, name: &str, action: &str) -> Check {
    let what = format!("it showed working after the chief's {action}, before it was done");
    let Some(number) = judged.nth(name, 0) else {
        return Check::failed(name, &what, format!("no {name} task was bound"));
    };
    let events = &judged.snapshot.events;
    let path = format!("/api/tasks/{number}/{action}");
    let Some(at) = observe::calls(events)
        .iter()
        .find(|call| call.participant == "chief" && call.path == path && call.error.is_none())
        .and_then(|call| call.at_ms)
    else {
        return Check::failed(
            name,
            &what,
            format!("the chief's {action} of T-{number} was not done"),
        );
    };
    let states: Vec<(String, i64)> = events
        .iter()
        .filter(|event| event["kind"] == "task.state" && event["data"]["task"] == number)
        .filter_map(|event| {
            Some((
                event["data"]["to"].as_str()?.to_owned(),
                super::super::human::at_ms(event)?,
            ))
        })
        .filter(|(_, when)| *when >= at)
        .collect();
    let working = states.iter().position(|(state, _)| state == "working");
    let done = states.iter().position(|(state, _)| state == "done");
    Check::new(
        name,
        &what,
        working.is_some_and(|working| done.is_none_or(|done| working < done)),
        format!(
            "after the {action}: {}",
            states
                .iter()
                .map(|(state, _)| state.as_str())
                .collect::<Vec<_>>()
                .join(" → ")
        ),
    )
}

/// The check that the task bound to `name` went to `working` within `ms` of the chief's
/// `action` of it, before it was next done: the board showed its window at work at once.
fn working_within(judged: &Judged, name: &str, action: &str, ms: i64) -> Check {
    let what = format!(
        "it showed working within {} s of the chief's {action}",
        ms / 1000
    );
    let Some(number) = judged.nth(name, 0) else {
        return Check::failed(name, &what, format!("no {name} task was bound"));
    };
    let events = &judged.snapshot.events;
    let path = format!("/api/tasks/{number}/{action}");
    let Some(at) = observe::calls(events)
        .iter()
        .find(|call| call.participant == "chief" && call.path == path && call.error.is_none())
        .and_then(|call| call.at_ms)
    else {
        return Check::failed(
            name,
            &what,
            format!("the chief's {action} of T-{number} was not done"),
        );
    };
    let next: Vec<(String, i64)> = events
        .iter()
        .filter(|event| event["kind"] == "task.state" && event["data"]["task"] == number)
        .filter_map(|event| {
            Some((
                event["data"]["to"].as_str()?.to_owned(),
                super::super::human::at_ms(event)?,
            ))
        })
        .filter(|(_, when)| *when >= at)
        .collect();
    let working = next
        .iter()
        .find(|(state, _)| state == "working")
        .map(|(_, when)| when - at);
    Check::new(
        name,
        &what,
        working.is_some_and(|after| after <= ms),
        format!(
            "after the {action}: {}",
            next.iter()
                .map(|(state, when)| format!("{state} at +{} ms", when - at))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
}

/// The check that the window of the task bound to `name` was opened once: a pause and a resume
/// went on in the window that had the task, never a new one.
fn one_window(judged: &Judged, name: &str) -> Check {
    let what = "its work went on in the window it had, opened once";
    let Some(task) = judged
        .nth(name, 0)
        .and_then(|number| judged.snapshot.task(number))
    else {
        return Check::failed(name, what, format!("no {name} task was bound"));
    };
    let handle = task["assignee"].as_str().unwrap_or_default();
    let sessions = observe::sessions_of(&judged.snapshot.events, handle);
    Check::new(
        name,
        what,
        sessions.len() == 1,
        format!("@{handle} had {} conversation(s)", sessions.len()),
    )
}

/// The scenarios of LMX-10.
pub fn interrupts() -> Vec<Scenario> {
    vec![
        // Paused mid-turn, resumed with words: the same window takes them and answers.
        scenario(
            "pause-resume-mid-turn",
            vec![
                run!("A", "task add --tier standard :: SLEEP 30 Reply with exactly: A_DONE", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["working"])
                }),
                run!("pause A", "task pause {A}", |snapshot, bindings| is(snapshot, bindings, "A", 0, &["paused"])),
                run!("resume A", "task resume {A} :: Reply with exactly: A_AGAIN", |snapshot, bindings| {
                    answered(snapshot, bindings, "A", 0)
                }),
            ],
            ONE,
            |judged| {
                vec![
                    oracle::chief_call(judged, "A", "pause", 0, Answered::Done),
                    oracle::chief_call(judged, "A", "resume", 0, Answered::Done),
                    result_says(judged, "A", "A_AGAIN"),
                    working_after(judged, "A", "resume"),
                ]
            },
            &[],
            &[],
        ),
        // Paused while a job its turn started runs: the pause stops the turn, and the window,
        // which runs work, stays; the resume goes on in it (T-1117's rule).
        scenario(
            "pause-resume-mid-tool",
            vec![
                run!("A", "task add --tier standard :: JOB 90 SLEEP 30 Reply with exactly: A_DONE", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["working"])
                }),
                run!("pause A", "task pause {A}", |snapshot, bindings| is(snapshot, bindings, "A", 0, &["paused"])),
                run!("resume A", "task resume {A} :: Reply with exactly: A_AGAIN", |snapshot, bindings| {
                    answered(snapshot, bindings, "A", 0)
                }),
            ],
            ONE,
            |judged| {
                vec![
                    oracle::chief_call(judged, "A", "pause", 0, Answered::Done),
                    result_says(judged, "A", "A_AGAIN"),
                    one_window(judged, "A"),
                ]
            },
            &[],
            &[],
        ),
        // Field report 8: told to stop, then resumed; the task shows working while its window
        // works on the words.
        scenario(
            "field8-tell-resume",
            vec![
                run!("A", "task add --tier standard :: SLEEP 30 Reply with exactly: A_DONE", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["working"])
                }),
                // The window answers the tell and goes on with its work, its record growing, as the
                // field's did (`REPLY … THEN PACED`): the resume comes while it works.
                run!("tell A", "tell {A} :: Stop now. REPLY TOLD THEN PACED 6 2 Reply with exactly: TELL_REPORT", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["paused"])
                        && bindings.nth("A", 0).is_some_and(|number| {
                            snapshot.chief_inbox.iter().any(|message| {
                                message["kind"] == "answer"
                                    && message["taskNumber"] == number
                                    && (message["state"] == "delivered" || message["state"] == "read")
                            })
                        })
                }),
                run!("resume A", "task resume {A} :: Go on. Reply with exactly: A_AGAIN", |snapshot, bindings| {
                    answered(snapshot, bindings, "A", 0)
                }),
            ],
            ONE,
            |judged| {
                vec![
                    oracle::chief_call(judged, "A", "resume", 0, Answered::Done),
                    working_within(judged, "A", "resume", 5_000),
                    result_says(judged, "A", "A_AGAIN"),
                    one_window(judged, "A"),
                ]
            },
            &[],
            &[],
        ),
        // Cancel of an open, a working, a paused and a done task.
        scenario(
            "cancel-each-state",
            vec![
                run!(
                    "W, P and D",
                    "task add --tier standard :: SLEEP 40 Reply with exactly: W_DONE\ntask add --test --tier standard :: SLEEP 40 Reply with exactly: P_DONE\ntask add --review --tier standard :: Reply with exactly: D_DONE",
                    |snapshot, bindings| {
                        is(snapshot, bindings, "W", 0, &["working"])
                            && is(snapshot, bindings, "P", 0, &["working"])
                            && answered(snapshot, bindings, "D", 0)
                    }
                ),
                run!("O needs W, and pause P", "task add --advice --tier standard --needs {W} :: Reply with exactly: O_DONE\ntask pause {P}", |snapshot, bindings| {
                    is(snapshot, bindings, "O", 0, &["open"]) && is(snapshot, bindings, "P", 0, &["paused"])
                }),
                run!(
                    "cancel each",
                    "task cancel {O}\ntask cancel {W}\ntask cancel {P}\ntask cancel {D}",
                    |snapshot, bindings| called(snapshot, bindings, "D", "cancel", 1)
                ),
            ],
            &[
                Bind { name: "W", is: Is::Pool("worker"), requester: "chief" },
                Bind { name: "P", is: Is::Pool("tester"), requester: "chief" },
                Bind { name: "D", is: Is::Pool("reviewer"), requester: "chief" },
                Bind { name: "O", is: Is::Pool("advisor"), requester: "chief" },
            ],
            |judged| {
                let mut checks = Vec::new();
                for name in ["O", "W", "P", "D"] {
                    checks.push(oracle::chief_call(judged, name, "cancel", 0, Answered::Done));
                    checks.push(oracle::state(judged, name, 0, &["cancelled"]));
                }
                checks.push(oracle::result_kept(judged, "D"));
                checks.push(oracle::never_briefed(judged, "O", 0));
                checks
            },
            &[],
            &[],
        ).ending(&["O", "W", "P", "D"]),
        // Edit of an open task: the brief its window is given is the edited one.
        scenario(
            "edit-open",
            vec![
                // W works long enough for a loaded Windows machine's window to be seen at it
                // (T-1195: zeewin2's half of the matrix missed a 10 s turn three times).
                run!("W", "task add --tier standard :: SLEEP 30 Reply with exactly: W_DONE", |snapshot, bindings| {
                    is(snapshot, bindings, "W", 0, &["working", "done"])
                }),
                run!("O needs W", "task add --advice --tier standard --needs {W} :: Reply with exactly: OLD_WORDS", |snapshot, bindings| {
                    is(snapshot, bindings, "O", 0, &["open"])
                }),
                run!("edit O", "task edit {O} :: Reply with exactly: NEW_WORDS", |snapshot, bindings| {
                    called(snapshot, bindings, "O", "edit", 1) && answered(snapshot, bindings, "W", 0)
                }),
                run!("accept W", "task accept {W}", |snapshot, bindings| answered(snapshot, bindings, "O", 0)),
            ],
            &[
                Bind { name: "W", is: Is::Pool("worker"), requester: "chief" },
                Bind { name: "O", is: Is::Pool("advisor"), requester: "chief" },
            ],
            |judged| {
                let edited = judged.nth("O", 0).and_then(|number| judged.snapshot.task(number));
                vec![
                    oracle::chief_call(judged, "O", "edit", 0, Answered::Done),
                    result_says(judged, "O", "NEW_WORDS"),
                    Check::new(
                        "O",
                        "its brief is the edited one",
                        edited.is_some_and(|task| {
                            task["body"].as_str().is_some_and(|body| body.contains("NEW_WORDS") && !body.contains("OLD_WORDS"))
                        }),
                        format!("{:?}", edited.map(|task| task["body"].clone())),
                    ),
                ]
            },
            &[],
            &[],
        ),
        // Finish of a working, a waiting (a question asked, unanswered) and a paused task: each
        // done with what its window said last.
        scenario(
            "finish-each-state",
            vec![
                run!(
                    "W, Q and P",
                    "task add --tier standard :: CLAUDE-JOB 600 W_ANSWERED :: The job ended\ntask add --test --tier standard :: CF ask :: Which format?\ntask add --review --tier standard :: CLAUDE-JOB 600 P_ANSWERED :: The job ended",
                    |snapshot, bindings| {
                        is(snapshot, bindings, "W", 0, &["working"])
                            && is(snapshot, bindings, "Q", 0, &["waiting"])
                            && is(snapshot, bindings, "P", 0, &["working"])
                    }
                ),
                run!("pause P", "task pause {P}", |snapshot, bindings| is(snapshot, bindings, "P", 0, &["paused"])),
                run!(
                    "finish each",
                    "task finish {W}\ntask finish {Q}\ntask finish {P}",
                    |snapshot, bindings| called(snapshot, bindings, "P", "finish", 1)
                ),
            ],
            &[
                Bind { name: "W", is: Is::Pool("worker"), requester: "chief" },
                Bind { name: "Q", is: Is::Pool("tester"), requester: "chief" },
                Bind { name: "P", is: Is::Pool("reviewer"), requester: "chief" },
            ],
            |judged| {
                let mut checks = Vec::new();
                for name in ["W", "Q", "P"] {
                    checks.push(oracle::chief_call(judged, name, "finish", 0, Answered::Done));
                    checks.push(oracle::state(judged, name, 0, &["done"]));
                }
                checks
            },
            &[],
            &[],
        ),
    ]
}
