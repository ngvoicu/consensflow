//! Three field-found issues whose fixes are on branches that land after alpha.2, each as the
//! field's shape on the stand-ins, its checks known reds until its branch lands:
//!
//! - `field-hold-notes` (T-1181, `wt/hold-notes`): a worker ends its turn with a run and a watcher
//!   of it in the background; the run ends first. The chief is told once that the task still
//!   works in the background, in words that name the command, and then given its result; not
//!   told again when the count of what runs falls.
//! - `field-harness-dialog` (T-1183, T-1192, `wt/harness-dialogs`): Claude Code offers a worker's
//!   window an LSP plugin mid-turn, a dialog that waits for a key; the chief is told once, the
//!   note naming the question, and the offer is declined ("No, not now", which writes nothing).
//! - `field-role-told` (T-1182, `wt/role-change-live`): the human turns the project's specs on
//!   while the chief's window is open; the chief is told at its next rest, in the conversation
//!   it has.

use serde_json::Value;

use crate::live::report::Check;
use crate::matrix::observe::{Bind, Is};
use crate::matrix::oracle::{self, Judged};
use crate::matrix::seating::Role;

use super::kit::{answered, run, scenario, since};
use super::{Scenario, Step};

/// The notes of the daemon's own (from nobody) to the chief that hold `words`.
fn notes_with<'a>(judged: &'a Judged, words: &str) -> Vec<&'a Value> {
    judged
        .snapshot
        .chief_inbox
        .iter()
        .filter(|message| {
            message["kind"] == "note" && message.get("sender").is_none_or(Value::is_null)
        })
        .filter(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains(words))
        })
        .collect()
}

/// The user's turns of the chief's own record that hold `words`.
fn chief_given(judged: &Judged, words: &str) -> Vec<String> {
    judged
        .records
        .items(judged.seating.seat(Role::Chief).kind(), "chief")
        .unwrap_or_default()
        .into_iter()
        .filter(|item| item.role == "user" && item.text.contains(words))
        .map(|item| item.text)
        .collect()
}

/// The question Claude Code's LSP offer asks.
const OFFER_QUESTION: &str = "Would you like to install this LSP plugin?";

/// What the chief is told when the specs are turned on (T-1182).
const SPECS_ON: &str = "Specs are now on in this project: ";

const A: &[Bind] = &[Bind {
    name: "A",
    is: Is::Pool("worker"),
    requester: "chief",
}];

/// The scenarios of the three branches.
#[allow(clippy::too_many_lines)]
pub fn branches() -> Vec<Scenario> {
    vec![
        // T-1181, poker-lab's T-442 shape: a test run in the background for 16 s and Claude
        // Code's Monitor on it; the run's end wakes a turn that leaves the monitor alone, which
        // ends once the human's step lets it (the rig's `Job::let_its_monitor_end`).
        scenario(
            "field-hold-notes",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: CLAUDE-MONITOR 3 16 The suite runs in the background; Monitor wakes me on its events :: The suite passed: 412 tests, 0 failures",
                    |snapshot, bindings| since(snapshot, bindings, "A", "working").is_some_and(|seconds| seconds >= 40)
                ),
                Step {
                    human: Some(|members| {
                        members.rig().jobs("job").iter().try_for_each(crate::rig::Job::let_its_monitor_end)
                    }),
                    restarts: false,
                    name: "the monitor may end",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot),
                },
            ],
            A,
            |judged| {
                let number = judged.nth("A", 0);
                let held: Vec<&Value> = judged
                    .snapshot
                    .chief_inbox
                    .iter()
                    .filter(|message| {
                        message["kind"] == "note"
                            && number.is_some_and(|n| message["taskNumber"] == n)
                            && message["body"].as_str().is_some_and(|body| body.contains("in the background"))
                    })
                    .collect();
                let bodies: Vec<&str> = held.iter().filter_map(|note| note["body"].as_str()).collect();
                // monitor-wait-loses-the-job (poker-lab, T-442), held on today's code: the run
                // and the monitor were not lost at the turn's end, and the result is the report.
                let results: Vec<Value> = judged
                    .snapshot
                    .chief_inbox
                    .iter()
                    .filter(|message| message["kind"] == "result" && number.is_some_and(|n| message["taskNumber"] == n))
                    .map(|message| message["body"].clone())
                    .collect();
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new(
                        "A",
                        "its one result is the report the run's monitor woke",
                        results.len() == 1 && results[0] == "The suite passed: 412 tests, 0 failures",
                        format!("{results:?}"),
                    ),
                    Check::new(
                        "A",
                        "the chief was told once that it still works in the background",
                        held.len() == 1,
                        format!("{} note(s): {}", held.len(), crate::live::cut(&bodies.join(" | "), 300)),
                    ),
                    Check::new(
                        "A",
                        "the note names the command that runs and says the result comes when it ends",
                        bodies.first().is_some_and(|body| {
                            body.contains("is still working in the background (") && body.contains("its result comes when")
                        }),
                        crate::live::cut(bodies.first().copied().unwrap_or("no note"), 200),
                    ),
                ]
            },
            &[],
            &[],
        ),
        // T-1183 and T-1192: the offer stays on the worker's screen up to Claude Code's 30 s.
        scenario(
            "field-harness-dialog",
            vec![run!(
                "A",
                "task add --tier standard :: LSP-OFFER 30 Reply with exactly: DIALOG_OK",
                |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
            )],
            A,
            |judged| {
                let told = notes_with(judged, "'s window shows a dialog of its harness");
                let body = told.first().and_then(|note| note["body"].as_str()).unwrap_or("no note");
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new("A", "the chief was told once of the dialog in its window", told.len() == 1, format!("{} note(s)", told.len())),
                    Check::new("A", "the note names the dialog's question", body.contains(OFFER_QUESTION), crate::live::cut(body, 200)),
                    Check::new("A", "the note says the offer is declined, with No, not now", body.contains("No, not now"), crate::live::cut(body, 200)),
                ]
            },
            &[],
            &[],
        ),
        // T-1182: A's quick turn, then the human turns the specs on from the page.
        scenario(
            "field-role-told",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: Reply with exactly: A_DONE",
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
                ),
                Step {
                    human: Some(|members| {
                        members
                            .page("project.specs", serde_json::json!({ "on": true, "folder": "plans", "variant": "tdd" }))
                            .map(|_| ())
                    }),
                    restarts: false,
                    name: "the human turns the project's specs on",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, bindings| {
                        since(snapshot, bindings, "A", "done").is_some_and(|seconds| seconds >= 60)
                            && crate::matrix::run::settled(snapshot)
                    },
                },
            ],
            A,
            |judged| {
                let told = chief_given(judged, SPECS_ON);
                vec![
                    Check::new("chief", "it was told once, in its own window, that specs are now on", told.len() == 1, format!("{} turn(s)", told.len())),
                    Check::new(
                        "chief",
                        "it was told where the specs are",
                        told.first().is_some_and(|text| text.contains("`plans`")),
                        crate::live::cut(told.first().map_or("nothing", String::as_str), 200),
                    ),
                ]
            },
            &[],
            &[],
        ),
    ]
}
