//! LMX-13's deliveries and pastes (LMX-16's class, and LMX-15's image paths): the issues a real
//! board found of a message typed into a window, each as the field's shape on the stand-ins
//! (the brain's `issues/field-found.md` names each line's scenario). The fixes are on today's
//! main, so each is a green case; what it holds is that a message goes into its window once,
//! whole, and when it may.
//!
//! - `field-draft-kept`: the human types in the chief's window while a result waits; the result
//!   is not typed over the draft, and goes in after the human's own words (T-716, m-4697).
//! - `field-retyped-under-load`: the pane host answers nothing for minutes at a result's paste,
//!   as a Mac at a load of 100 did; the result is typed once (3.0.5, m-3093).
//! - `field-late-record`: the chief's record shows nothing of a result for 100 s, at rest; it is
//!   not pasted again (T-760, m-3093 and m-4273).
//! - `field-recap-not-typed`: a terminal's reports on the human's path, which nobody typed, hold
//!   no result (T-839).
//! - `field-long-paste`: a long result, which Claude Code is given as a paste of its own, goes in
//!   once (T-760, T-763).
//! - `field-image-result`: a result that begins with the path of a picture, which Claude Code
//!   makes an attachment of, goes in once (T-215, m-2619).
//! - `field-question-not-retyped`: a question that came while the chief was in a turn, and that
//!   it answered there, is never typed into its window (3.0.5, m-2927).

use std::time::Duration;

use serde_json::Value;

use crate::live::members::Members;
use crate::live::report::Check;
use crate::matrix::observe::{self, Bind, Is, Snapshot};
use crate::matrix::oracle::{self, Judged};
use crate::matrix::seating::Role;
use crate::Result;

use super::kit::{answered, is, run, scenario, since};
use super::{Scenario, Step};

/// The chief's messages of `kind` about the task bound to `name`, oldest first.
fn of_task<'a>(judged: &'a Judged, name: &str, kind: &str) -> Vec<&'a Value> {
    let number = judged.nth(name, 0);
    judged
        .snapshot
        .chief_inbox
        .iter()
        .filter(|message| {
            message["kind"] == kind && number.is_some_and(|n| message["taskNumber"] == n)
        })
        .collect()
}

/// The items of the chief's own record.
fn chief_record(judged: &Judged) -> Vec<crate::live::ask::Item> {
    judged
        .records
        .items(judged.seating.seat(Role::Chief).kind(), "chief")
        .unwrap_or_default()
}

/// Where in the chief's record the message `id` went in: each user item that holds its header.
fn typed_at(items: &[crate::live::ask::Item], id: &Value) -> Vec<usize> {
    let header = format!("[ConsensFlow m-{id} ");
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.role == "user" && item.text.contains(&header))
        .map(|(at, _)| at)
        .collect()
}

/// How many of the daemon's events of `kind` are about the message `id`.
fn events_of(snapshot: &Snapshot, kind: &str, id: &Value) -> usize {
    snapshot
        .events
        .iter()
        .filter(|event| event["kind"] == kind && event["data"]["message"] == *id)
        .count()
}

/// The checks that the one message of `kind` about the task bound to `name` was delivered, went
/// into the chief's window once, and was neither confirmed twice nor tried again.
fn into_the_chief_once(judged: &Judged, name: &str, kind: &str) -> Vec<Check> {
    let messages = of_task(judged, name, kind);
    let [message] = messages[..] else {
        return vec![Check::failed(
            name,
            &format!("its one {kind} reached the chief once"),
            format!("{} {kind}(s)", messages.len()),
        )];
    };
    let id = &message["id"];
    let typed = typed_at(&chief_record(judged), id).len();
    let (confirmed, retried) = (
        events_of(judged.snapshot, "delivery.confirmed", id),
        events_of(judged.snapshot, "delivery.retried", id),
    );
    vec![
        Check::new(
            name,
            &format!("its {kind} was delivered"),
            !["queued", "delivering", "gated", "failed"]
                .contains(&message["state"].as_str().unwrap_or_default()),
            format!("m-{id} is {}", message["state"]),
        ),
        Check::new(
            name,
            &format!("its {kind} went into the chief's window once"),
            typed == 1,
            format!("m-{id} is in the chief's record {typed} time(s)"),
        ),
        Check::new(
            name,
            &format!("its {kind} was typed once, never tried again"),
            confirmed == 1 && retried == 0,
            format!("m-{id}: confirmed {confirmed} time(s), tried again {retried} time(s)"),
        ),
    ]
}

/// Whether the chief's window is at rest: the field's human typed into a chief at rest, its turn
/// over, while the board's messages waited.
fn chief_idle(snapshot: &Snapshot) -> bool {
    snapshot
        .lanes
        .iter()
        .any(|lane| lane["participant"]["handle"] == "chief" && lane["activity"]["state"] == "idle")
}

/// The chief's pane, as the board names it.
fn chief_pane(members: &Members) -> Result<Value> {
    Ok(members.lane("chief")?["pane"].clone())
}

/// What the human types in the chief's window and leaves unsent for a while.
const DRAFT: &str = "HUMAN_DRAFT: before you go on, keep the parser as it is";

/// What a terminal reports of itself when a program asks, which the page sends on the human's
/// path as it sends keys: its background colour (OSC 11, ended by ST), its foreground (OSC 10,
/// ended by BEL) and its name (DCS) (T-839).
const REPORTS: [&[u8]; 3] = [
    b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\",
    b"\x1b]10;rgb:cccc/cccc/cccc\x07",
    b"\x1bP>|xterm.js(6.0.0)\x1b\\",
];

/// How long the pane host answers nothing at a result's paste: past the paste's 30 s deadline,
/// two of the Enter's snapshots at theirs and the daemon's minute of rest after them.
const STALL: Duration = Duration::from_secs(200);

/// The binds of a scenario of one worker's task, A.
const A: &[Bind] = &[Bind {
    name: "A",
    is: Is::Pool("worker"),
    requester: "chief",
}];

/// The scenarios of the class.
#[allow(clippy::too_many_lines)]
pub fn deliveries() -> Vec<Scenario> {
    vec![
        scenario(
            "field-draft-kept",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 10 Reply with exactly: A_DONE",
                    |snapshot, bindings| is(snapshot, bindings, "A", 0, &["working"]) && chief_idle(snapshot)
                ),
                Step {
                    human: Some(|members| members.rig().types(&chief_pane(members)?, DRAFT.as_bytes())),
                    restarts: false,
                    name: "the human types in the chief's window and sends nothing",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, bindings| since(snapshot, bindings, "A", "done").is_some_and(|seconds| seconds >= 15),
                },
                Step {
                    human: Some(|members| members.rig().types(&chief_pane(members)?, b"\r")),
                    restarts: false,
                    name: "the human sends what they typed",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot),
                },
            ],
            A,
            |judged| {
                let items = chief_record(judged);
                let draft: Vec<usize> = items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item.role == "user" && item.text.contains(DRAFT))
                    .map(|(at, _)| at)
                    .collect();
                let result = of_task(judged, "A", "result").first().map(|message| message["id"].clone());
                let typed = result.as_ref().map(|id| typed_at(&items, id)).unwrap_or_default();
                let events = &judged.snapshot.events;
                let held = judged.nth("A", 0).and_then(|number| {
                    let done = observe::reached_at(events, number, "done")?;
                    let delivered = observe::delivered_at(events, result.as_ref()?.as_i64()?)?;
                    Some(delivered - done)
                });
                let mut checks = into_the_chief_once(judged, "A", "result");
                checks.extend([
                    Check::new(
                        "human",
                        "their words went into the chief's window as their own message, no result in it",
                        draft.len() == 1 && !items[draft[0]].text.contains("[ConsensFlow"),
                        format!("{} item(s) hold them", draft.len()),
                    ),
                    Check::new(
                        "A",
                        "its result waited while the human's words stood unsent, and went in after them",
                        held.is_some_and(|held| held >= 15_000)
                            && matches!((draft.first(), typed.first()), (Some(words), Some(result)) if words < result),
                        format!("delivered {held:?} ms after it was done; the human's words at {draft:?}, the result at {typed:?}"),
                    ),
                ]);
                checks
            },
            &[],
            &[],
        ),
        scenario(
            "field-retyped-under-load",
            vec![
                Step {
                    human: Some(|members| {
                        members.rig().stall_host_at_paste(" · result from @", STALL);
                        Ok(())
                    }),
                    restarts: false,
                    name: "the pane host stalls at the paste of a result",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |_, _| true,
                },
                run!(
                    "A",
                    "task add --tier standard :: Reply with exactly: A_DONE",
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
                ),
            ],
            A,
            |judged| into_the_chief_once(judged, "A", "result"),
            &[],
            &[],
        )
        .waiting(Duration::from_secs(240)),
        scenario(
            "field-late-record",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: Reply with exactly: A_DONE",
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
                ),
                // A second paste, had there been one, reaches the record a hold later.
                Step {
                    human: Some(|_| Ok(())),
                    restarts: false,
                    name: "a hold goes by, in which a second paste would show",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, bindings| {
                        since(snapshot, bindings, "A", "done").is_some_and(|seconds| seconds >= 220)
                            && crate::matrix::run::settled(snapshot)
                    },
                },
            ],
            A,
            |judged| into_the_chief_once(judged, "A", "result"),
            &[],
            &[],
        )
        .with_vars(&[("CF_TEST_RESULTS_RECORDED_LATE", "chief 100000")], &[])
        .waiting(Duration::from_secs(150)),
        scenario(
            "field-recap-not-typed",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 8 Reply with exactly: A_DONE",
                    |snapshot, bindings| is(snapshot, bindings, "A", 0, &["working"])
                ),
                Step {
                    human: Some(|members| {
                        let pane = chief_pane(members)?;
                        REPORTS.iter().try_for_each(|report| members.rig().types(&pane, report))
                    }),
                    restarts: false,
                    name: "the terminal's reports reach the chief's window on the human's path",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot),
                },
            ],
            A,
            |judged| {
                let events = &judged.snapshot.events;
                let result = of_task(judged, "A", "result").first().and_then(|message| message["id"].as_i64());
                let waited = judged.nth("A", 0).zip(result).and_then(|(number, id)| {
                    Some(observe::delivered_at(events, id)? - observe::reached_at(events, number, "done")?)
                });
                let mut checks = into_the_chief_once(judged, "A", "result");
                checks.push(Check::new(
                    "A",
                    "its result went in within a minute of the task's end, held for nothing nobody typed",
                    waited.is_some_and(|waited| waited < 60_000),
                    format!("{waited:?} ms after it was done"),
                ));
                checks
            },
            &[],
            &[],
        ),
        scenario(
            "field-long-paste",
            vec![run!(
                "A",
                "task add --tier standard :: LONG 12000",
                |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
            )],
            A,
            |judged| {
                let mut checks = into_the_chief_once(judged, "A", "result");
                let long = of_task(judged, "A", "result")
                    .first()
                    .and_then(|message| message["body"].as_str())
                    .map_or(0, |body| body.chars().count());
                checks.push(Check::new("A", "its result is the long report, whole", long >= 12_000, format!("{long} characters")));
                checks
            },
            &[],
            &[],
        ),
        scenario(
            "field-image-result",
            vec![run!(
                "A",
                "task add --tier standard :: SAVED hero.png Here is the hero image.",
                |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
            )],
            A,
            |judged| {
                let mut checks = into_the_chief_once(judged, "A", "result");
                let starts = of_task(judged, "A", "result")
                    .first()
                    .and_then(|message| message["body"].as_str())
                    .is_some_and(|body| body.lines().next().is_some_and(|line| line.ends_with("hero.png")));
                checks.push(Check::new("A", "its result begins with the path of its picture", starts, format!("{starts}")));
                // The field's shape: Claude Code made the path an attachment, its placeholder
                // ahead of the header, which no longer begins the item.
                let id = of_task(judged, "A", "result").first().map(|message| message["id"].clone());
                let items = chief_record(judged);
                let attached = id.as_ref().is_some_and(|id| {
                    typed_at(&items, id).first().is_some_and(|at| items[*at].text.starts_with("[Image #"))
                });
                checks.push(Check::new(
                    "A",
                    "the chief's window took its picture as an attachment ahead of the header",
                    attached,
                    format!("{attached}"),
                ));
                checks
            },
            &[],
            &[],
        ),
        // The field's shape: the question came while the chief was in a turn, and the chief
        // answered it there with `cf answer`, the question still waiting behind the turn.
        scenario(
            "field-question-not-retyped",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 8 CF ask :: Which format, JSON or text?",
                    |snapshot, bindings| is(snapshot, bindings, "A", 0, &["working"])
                ),
                Step {
                    human: None,
                    restarts: false,
                    name: "the chief answers it in a turn of its own",
                    asks: |_| "Work for half a minute on the plan in your head. Our worker will ask you a question meanwhile: answer it with `cf answer` and the word JSON, without waiting for it to be typed to you. Tell me in one line when its result is back.".to_owned(),
                    stand_in: |_| "SLEEP 30 CF answer m-2 :: JSON".to_owned(),
                    reached: |snapshot, bindings| answered(snapshot, bindings, "A", 0),
                },
                Step {
                    human: Some(|_| Ok(())),
                    restarts: false,
                    name: "two minutes go by",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, bindings| {
                        since(snapshot, bindings, "A", "done").is_some_and(|seconds| seconds >= 120)
                            && crate::matrix::run::settled(snapshot)
                    },
                },
            ],
            A,
            |judged| {
                let asked = of_task(judged, "A", "question");
                let typed = asked
                    .first()
                    .map_or(0, |question| typed_at(&chief_record(judged), &question["id"]).len());
                let answers = observe::calls(&judged.snapshot.events)
                    .into_iter()
                    .filter(|call| call.participant == "chief" && call.path == "/api/answers" && call.error.is_none())
                    .count();
                vec![
                    Check::new("A", "its window asked the chief one question", asked.len() == 1, format!("{} question(s)", asked.len())),
                    Check::new("chief", "the chief answered it with `cf answer` while it waited", answers == 1, format!("{answers} answer call(s)")),
                    Check::new(
                        "A",
                        "the question it answered was never typed into the chief's window",
                        typed == 0,
                        format!("in the chief's record {typed} time(s)"),
                    ),
                    oracle::state(judged, "A", 0, &["done"]),
                ]
            },
            &[],
            &[],
        )
        .waiting(Duration::from_secs(150)),
    ]
}
