//! What the scenarios after the smoke share: the human's fact sheet and nudge, their bounds, the
//! board's reads a step's milestone is made of, and [`fill`], which writes a `cf` command line of
//! a scenario's with the numbers the board gave its tasks. A scenario's command line names a
//! bound task as `{A}` (its first) or `{A#1}` (its second), and its last word, spaces and all,
//! after ` :: ` (as the stand-in chief's `CF` line takes it); a real chief is given the same line
//! with that word quoted.

use std::time::Duration;

use crate::live::report::Check;
use crate::matrix::machine::Fact;
use crate::matrix::observe::{self, Bind, Bindings, Snapshot};
use crate::matrix::oracle::Judged;

use super::{Known, Refused, Scenario, Step};

/// What the human answers a chief's questions with in these scenarios: do what was asked.
pub(super) const FACTS: &[Fact] = &[
    Fact {
        asked_by: &["tier", "which member", "who should"],
        answer: "Every member of our staff is at tier standard.",
    },
    Fact {
        asked_by: &[
            "accept", "proceed", "go ahead", "shall i", "should i", "approve", "confirm", "cancel",
            "reopen", "pause", "resume", "finish", "edit", "tell",
        ],
        answer: "Yes: run exactly the command I gave, and tell me what it answered.",
    },
];

pub(super) const NUDGE: &str =
    "Please run the command I gave last: the board does not show it yet.";

/// A real chief's deadline for one of these scenarios (the stand-ins' is the runner's own).
pub(super) const DEADLINE: Duration = Duration::from_secs(20 * 60);

/// The task bound to `name`, the `nth`, as a stand-in's or a person's words name it.
pub(super) fn t(bindings: &Bindings, name: &str, nth: usize) -> String {
    bindings.nth(name, nth).map_or_else(
        || format!("(no {name} task yet)"),
        |number| format!("T-{number}"),
    )
}

/// `line` with each `{X}` or `{X#n}` written as the bound task's `T-<number>`.
pub(super) fn named(bindings: &Bindings, line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        out.push_str(&rest[..open]);
        let inner = &rest[open + 1..open + close];
        // `{X+1}`: the task after X's, the next one the chief adds in the same step.
        if let Some((name, plus)) = inner.split_once('+') {
            let after = bindings.nth(name, 0).zip(plus.parse::<i64>().ok());
            out.push_str(&after.map_or_else(
                || format!("(no {name} task yet)"),
                |(number, plus)| format!("T-{}", number + plus),
            ));
            rest = &rest[open + close + 1..];
            continue;
        }
        let (name, nth) = match inner.split_once('#') {
            Some((name, nth)) => (name, nth.parse().unwrap_or(0)),
            None => (inner, 0),
        };
        out.push_str(&t(bindings, name, nth));
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// The stand-in chief's words for the command lines `lines` (one a line): `CF` and each line,
/// its tasks named.
pub(super) fn stand_in(bindings: &Bindings, lines: &str) -> String {
    lines
        .lines()
        .map(|line| format!("CF {}", named(bindings, line)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A real chief's ask for the command lines `lines`: each as a shell runs it, its last word (after
/// ` :: `) quoted, run in order, and one line back.
pub(super) fn asks(bindings: &Bindings, lines: &str) -> String {
    let commands: Vec<String> = lines
        .lines()
        .map(|line| {
            let line = named(bindings, line);
            match line.split_once(" :: ") {
                Some((words, last)) => format!("`cf {words} \"{last}\"`"),
                None => format!("`cf {line}`"),
            }
        })
        .collect();
    format!(
        "Run exactly {} in your shell, in this order, and tell me in one line what each answered.",
        commands.join(", then ")
    )
}

/// A step that runs the `cf` command lines `$lines` (each `{X}` a bound task, the last word
/// after ` :: `), over when `$reached` says it is.
macro_rules! run {
    ($step:literal, $lines:literal, $reached:expr) => {
        Step {
            human: None,
            restarts: false,
            name: $step,
            asks: |bindings| super::kit::asks(bindings, $lines),
            stand_in: |bindings| super::kit::stand_in(bindings, $lines),
            reached: $reached,
        }
    };
}
pub(super) use run;

/// The state of the task bound to `name` (the `nth`).
pub(super) fn state<'a>(
    snapshot: &'a Snapshot,
    bindings: &Bindings,
    name: &str,
    nth: usize,
) -> &'a str {
    bindings
        .nth(name, nth)
        .and_then(|number| snapshot.task(number))
        .and_then(|task| task["state"].as_str())
        .unwrap_or("none")
}

/// How long ago, in seconds, the task bound to `name` (its first) first went to `state`; none
/// while it never did.
/// How long ago, in seconds, the chief's first call of `action` on the task bound to `name` was
/// answered; none while it made none.
pub(super) fn since_call(
    snapshot: &Snapshot,
    bindings: &Bindings,
    name: &str,
    action: &str,
) -> Option<i64> {
    let path = format!("/api/tasks/{}/{action}", bindings.nth(name, 0)?);
    let at = observe::calls(&snapshot.events)
        .into_iter()
        .find(|call| call.participant == "chief" && call.path == path)?
        .at_ms?;
    Some((jiff::Timestamp::now().as_millisecond() - at) / 1000)
}

pub(super) fn since(
    snapshot: &Snapshot,
    bindings: &Bindings,
    name: &str,
    state: &str,
) -> Option<i64> {
    let at = observe::reached_at(&snapshot.events, bindings.nth(name, 0)?, state)?;
    Some((jiff::Timestamp::now().as_millisecond() - at) / 1000)
}

/// Whether the task bound to `name` (the `nth`) is in one of `states`.
pub(super) fn is(
    snapshot: &Snapshot,
    bindings: &Bindings,
    name: &str,
    nth: usize,
    states: &[&str],
) -> bool {
    states.contains(&state(snapshot, bindings, name, nth))
}

/// Whether the task bound to `name` (the `nth`) is done or accepted and its result was delivered.
pub(super) fn answered(snapshot: &Snapshot, bindings: &Bindings, name: &str, nth: usize) -> bool {
    bindings.nth(name, nth).is_some_and(|number| {
        is(snapshot, bindings, name, nth, &["done", "accepted"])
            && snapshot.delivered_result(number).is_some()
    })
}

/// How many times the chief called `action` on the task bound to `name`.
pub(super) fn calls_of(
    snapshot: &Snapshot,
    bindings: &Bindings,
    name: &str,
    action: &str,
) -> usize {
    let Some(number) = bindings.nth(name, 0) else {
        return 0;
    };
    let path = format!("/api/tasks/{number}/{action}");
    observe::calls(&snapshot.events)
        .iter()
        .filter(|call| call.participant == "chief" && call.method == "POST" && call.path == path)
        .count()
}

/// Whether the chief called `action` on the task bound to `name` at least `times` times, and
/// the board is at rest after it.
pub(super) fn called(
    snapshot: &Snapshot,
    bindings: &Bindings,
    name: &str,
    action: &str,
    times: usize,
) -> bool {
    calls_of(snapshot, bindings, name, action) >= times && crate::matrix::run::settled(snapshot)
}

/// The check that the brief of `later` (a name and its `nth`) went into its window only after
/// `earlier` (likewise) reached `state`.
pub(super) fn brief_after(
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

/// The check that the task bound to `name` needs the tasks bound to each of `needed`, as the
/// board lists its needs.
pub(super) fn needs(judged: &Judged, name: &str, needed: &[&str]) -> Check {
    let what = format!("it needs {}", needed.join(" and "));
    let Some(task) = judged
        .nth(name, 0)
        .and_then(|number| judged.snapshot.task(number))
    else {
        return Check::failed(name, &what, format!("no {name} task was bound"));
    };
    let listed: Vec<i64> = task["needs"]
        .as_array()
        .map(|needs| {
            needs
                .iter()
                .filter_map(|need| need["number"].as_i64().or_else(|| need.as_i64()))
                .collect()
        })
        .unwrap_or_default();
    let wanted: Vec<i64> = needed
        .iter()
        .filter_map(|other| judged.nth(other, 0))
        .collect();
    Check::new(
        name,
        &what,
        wanted.len() == needed.len() && wanted.iter().all(|number| listed.contains(number)),
        format!("T-{} needs {listed:?}; wanted {wanted:?}", task["number"]),
    )
}

/// The check that the result of the task bound to `name` holds `words`.
pub(super) fn result_says(judged: &Judged, name: &str, words: &str) -> Check {
    let what = format!("its result says {words}");
    let Some(number) = judged.nth(name, 0) else {
        return Check::failed(name, &what, format!("no {name} task was bound"));
    };
    let bodies: Vec<&str> = judged
        .snapshot
        .chief_inbox
        .iter()
        .filter(|message| message["kind"] == "result" && message["taskNumber"] == number)
        .filter_map(|message| message["body"].as_str())
        .collect();
    Check::new(
        name,
        &what,
        bodies.iter().any(|body| body.contains(words)),
        format!(
            "T-{number}'s results: {}",
            crate::live::cut(&bodies.join(" | "), 200)
        ),
    )
}

/// The check that the tasks bound to `a` and `b` went to one window.
pub(super) fn same_window(judged: &Judged, a: &str, b: &str) -> Check {
    let window = |name: &str| {
        judged
            .nth(name, 0)
            .and_then(|number| judged.snapshot.task(number))
            .map(|task| task["assignee"].clone())
    };
    Check::new(
        b,
        &format!("it went to the window that did {a}"),
        window(a).is_some_and(|first| !first.is_null() && Some(first) == window(b)),
        format!("{a}: {:?}, {b}: {:?}", window(a), window(b)),
    )
}

/// A scenario of these modules', with what they share.
pub(super) fn scenario(
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
        ends: &[],
        vars: &[],
        daemon_vars: &[],
        extra: &[],
        waits: std::time::Duration::ZERO,
        finder: false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn a_command_line_names_its_tasks_by_their_numbers_and_quotes_its_last_word_for_a_person() {
        let bound = Bindings {
            found: BTreeMap::from([("A", vec![3, 7]), ("B", vec![5])]),
        };
        assert_eq!(
            named(&bound, "task add --needs {A} --needs {B} :: x {A#1}"),
            "task add --needs T-3 --needs T-5 :: x T-7"
        );
        assert_eq!(
            named(&bound, "task cancel {C}"),
            "task cancel (no C task yet)"
        );
        // `{B+1}`: the task the chief adds after B's in the same step.
        assert_eq!(
            named(&bound, "task add --needs {B+1}"),
            "task add --needs T-6"
        );
        assert_eq!(
            stand_in(&bound, "task pause {A}\ntask cancel {B}"),
            "CF task pause T-3\nCF task cancel T-5"
        );
        assert_eq!(
            asks(&bound, "task resume {A} :: Go on.\ntask cancel {B}"),
            "Run exactly `cf task resume T-3 \"Go on.\"`, then `cf task cancel T-5` in your shell, in this order, and tell me in one line what each answered."
        );
    }
}
