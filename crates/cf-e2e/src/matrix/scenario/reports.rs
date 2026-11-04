//! LMX-12's rest: the board-CLI reports of Gabriel's other board (T-1118) that are not field
//! report 2 (in `order`), and the nudges half of field report 5 (T-1113).
//!
//! - A stale transcript: a window opened again on its conversation writes its record anew,
//!   shorter than the copy ConsensFlow kept, and the copy gave the new items the places of old
//!   ones, so `cf task get --transcript --last` ended on old items.
//! - Retitle: a task's title follows its scope, `cf task edit --title`, in any state.
//! - Nudges: the wake-up calls on, a minute apart, the chief waiting on the human (its note to
//!   the human unread) and answering each call by writing to the human again; a call is
//!   skipped, and no second call comes before three are. On the fix's branch the interval also
//!   doubles after such an answer, so five minutes see one call and a skip. Minutes of wake-up
//!   intervals: a case run on request only, not in CI's step (`field5_nudges` in
//!   tests/live_matrix.rs).

use serde_json::Value;

use crate::live::report::Check;
use crate::matrix::observe::{self, Bind, Is, Snapshot};
use crate::matrix::oracle::{self, Judged};

use super::kit::{answered, is, run, scenario, since};
use super::{Scenario, Step};

/// The results of the task `number` that reached the chief's inbox.
fn results(snapshot: &Snapshot, number: Option<i64>) -> usize {
    number.map_or(0, |number| {
        snapshot
            .chief_inbox
            .iter()
            .filter(|message| message["kind"] == "result" && message["taskNumber"] == number)
            .count()
    })
}

/// How long ago, in seconds, the chief's first call of `path` was answered; none while it made
/// none.
fn since_chief_called(snapshot: &Snapshot, path: &str) -> Option<i64> {
    let at = observe::calls(&snapshot.events)
        .into_iter()
        .find(|call| call.participant == "chief" && call.path == path && call.error.is_none())?
        .at_ms?;
    Some((jiff::Timestamp::now().as_millisecond() - at) / 1000)
}

/// The text of a transcript's item.
fn text(item: &Value) -> &str {
    item["text"].as_str().unwrap_or_default()
}

/// The words of A's first brief and of the brief it was sent back with, each a word its answer
/// repeats.
const FIRST: &str = "PARSER_DONE";
const AGAIN: &str = "EMPTY_INPUT_HANDLED";

/// The checks of the stale transcript: the last two items are the second brief and its answer,
/// and the whole is in the order the window wrote it.
fn transcript_checks(judged: &Judged) -> Vec<Check> {
    let Some(number) = judged.nth("A", 0) else {
        return vec![Check::failed("A", "it was bound", "no A task was bound")];
    };
    let read = |last| judged.records.transcript(number, last).unwrap_or_default();
    let tail = read(2);
    let whole = read(1_000);
    let shown = |items: &[Value]| {
        items
            .iter()
            .map(|item| {
                format!(
                    "{}: {}",
                    item["role"].as_str().unwrap_or("?"),
                    crate::live::cut(text(item).lines().next().unwrap_or_default(), 60)
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let at = |words: &str, role: &str| {
        whole
            .iter()
            .position(|item| item["role"] == role && text(item).contains(words))
    };
    let tail_is_the_second_turn = tail.len() == 2
        && tail[0]["role"] == "user"
        && text(&tail[0]).contains(AGAIN)
        && tail[1]["role"] == "assistant"
        && text(&tail[1]).contains(AGAIN);
    let in_order = matches!(
        (at(FIRST, "user"), at(FIRST, "assistant"), at(AGAIN, "user"), at(AGAIN, "assistant")),
        (Some(a), Some(b), Some(c), Some(d)) if a < b && b < c && c < d
    );
    vec![
        oracle::state(judged, "A", 0, &["done"]),
        Check::new(
            "A",
            "the last two items of its transcript are the brief it was sent back with and its answer",
            tail_is_the_second_turn,
            shown(&tail),
        ),
        Check::new(
            "A",
            "its whole transcript is in the order its window wrote it",
            in_order,
            shown(&whole),
        ),
    ]
}

/// The title A is given once its scope grew.
const TITLE: &str = "Parser and printer, both";

/// The words of the human's wake-up calls: the stand-in chief answers each by writing to the
/// human again, seconds later, as the field's chief did (each of its nudges was answered so),
/// which is something that happened since the call before it.
const NUDGED: &str = "Do you still have work to do?\nSLEEP 5 CF note --human :: Still waiting on your word: should I accept T-1 as it is, or send it back?";

/// The scenarios of LMX-12's rest; `field5-nudges` is run on request only.
pub fn reports() -> Vec<Scenario> {
    vec![
        // T-1118 item 2: A done, sent back; its window comes up again on its conversation and
        // writes its record anew.
        scenario(
            "field-stale-transcript",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: Reply with exactly: PARSER_DONE",
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0)
                ),
                run!(
                    "A sent back",
                    "task reopen {A} :: Reply with exactly: EMPTY_INPUT_HANDLED",
                    |snapshot, bindings| {
                        results(snapshot, bindings.nth("A", 0)) >= 2
                            && is(snapshot, bindings, "A", 0, &["done"])
                            && crate::matrix::run::settled(snapshot)
                    }
                ),
            ],
            &[Bind { name: "A", is: Is::Pool("worker"), requester: "chief" }],
            transcript_checks,
            &[],
            &[],
        )
        .with_vars(&[("CF_TEST_RESUMED_ANEW", "matrix-worker")], &[]),
        // T-1118 item 3: A done; its scope grew, and the chief gives it a title that says so.
        scenario(
            "field-retitle",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: Reply with exactly: PARSER_DONE",
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0)
                ),
                run!(
                    "A retitled",
                    "task edit {A} --title :: Parser and printer, both",
                    |snapshot, bindings| {
                        let titled = bindings
                            .nth("A", 0)
                            .and_then(|number| snapshot.task(number))
                            .is_some_and(|task| task["title"] == TITLE);
                        (titled || since(snapshot, bindings, "A", "done").is_some_and(|seconds| seconds >= 20))
                            && crate::matrix::run::settled(snapshot)
                    }
                ),
            ],
            &[Bind { name: "A", is: Is::Pool("worker"), requester: "chief" }],
            |judged| {
                let task = judged.nth("A", 0).and_then(|number| judged.snapshot.task(number));
                let title = task.map(|task| task["title"].clone()).unwrap_or_default();
                let retitled: Vec<&Value> = judged
                    .snapshot
                    .events
                    .iter()
                    .filter(|event| event["kind"] == "task.retitled")
                    .collect();
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new("A", "its title is the one the chief gave it", title == TITLE, format!("{title}")),
                    Check::new(
                        "A",
                        "its history keeps the title it had",
                        retitled.iter().any(|event| {
                            event["data"]["to"] == TITLE
                                && event["data"]["from"].as_str().is_some_and(|from| from.contains(FIRST))
                        }),
                        format!("{retitled:?}"),
                    ),
                ]
            },
            &[],
            &[],
        ),
        // Field report 5's nudges (T-1113): A done and undecided; the chief puts the decision to
        // the human in a note, which nobody reads; the human turns the wake-up calls on, a minute
        // apart, and leaves them for five; the chief answers each call by writing to the human.
        scenario(
            "field5-nudges",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: Reply with exactly: A_DONE",
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0)
                ),
                run!(
                    "the chief asks the human",
                    "note --human :: Should I accept T-1 as it is, or send it back? I wait for your word.",
                    |snapshot, _| since_chief_called(snapshot, "/api/notes").is_some()
                        && crate::matrix::run::settled(snapshot)
                ),
                Step {
                    human: Some(|members| {
                        members
                            .page(
                                "wakeup.set",
                                serde_json::json!({ "on": true, "every": 1, "unit": "minutes", "text": NUDGED }),
                            )
                            .map(|_| ())
                    }),
                    name: "the wake-up calls on, a minute apart, for five",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, _| {
                        since_chief_called(snapshot, "/api/notes").is_some_and(|seconds| seconds >= 320)
                            && crate::matrix::run::settled(snapshot)
                    },
                },
            ],
            &[Bind { name: "A", is: Is::Pool("worker"), requester: "chief" }],
            |judged| {
                // Where in the daemon's log each call and each skip is, in order.
                let at = |kind: &str| -> Vec<usize> {
                    judged
                        .snapshot
                        .events
                        .iter()
                        .enumerate()
                        .filter(|(_, event)| event["kind"] == kind)
                        .map(|(at, _)| at)
                        .collect()
                };
                let calls = at("project.wake_up");
                // The skips for the chief's unread notes, not those for work on its way.
                let skipped: Vec<usize> = at("project.wake_up_skipped")
                    .into_iter()
                    .filter(|at| !judged.snapshot.events[*at]["data"]["waitingOnTheHuman"].is_null())
                    .collect();
                let waited = match (calls.get(1), skipped.get(2)) {
                    (None, _) => true,
                    (Some(second), Some(third)) => third < second,
                    (Some(_), None) => false,
                };
                vec![
                    Check::new("chief", "a wake-up call reached it once the calls were on", !calls.is_empty(), format!("{} call(s)", calls.len())),
                    Check::new(
                        "chief",
                        "no second call came while its note to the human was unread, before three were skipped",
                        waited,
                        format!("{} call(s), {} skipped, at {calls:?} and {skipped:?} of the log", calls.len(), skipped.len()),
                    ),
                    Check::new(
                        "chief",
                        "a call was skipped for its unread note, and the skip logged",
                        !skipped.is_empty(),
                        format!("{} skipped", skipped.len()),
                    ),
                ]
            },
            &[],
            &[],
        )
        .waiting(std::time::Duration::from_secs(360)),
    ]
}
