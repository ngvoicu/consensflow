//! A scenario: the fixture a fresh project is made with, what the scripted human asks the chief
//! step by step, when each step is over (from the board, never from what the chief says), and
//! the bounds it runs in. Its asks are said twice, once to a real chief in a person's words and
//! once to the stand-in chief in its own (`DISPATCH`, `CF`, `WRITE`): the steps, the milestones
//! and the oracle are the same for both.
//!
//! `smoke` (LMX-04, row 1): the human has the chief get advice, have a worker make a small
//! change, have it reviewed and tested, and accept it.

use std::time::Duration;

use crate::{Error, Result};

use super::observe::{Bindings, Snapshot};

/// The fixture's code, before the change: what the checker refuses.
pub const GREET: &str =
    "// The greeting the checker reads.\nmodule.exports = (name) => `Hi ${name}`;\n";

/// The fixture's checker: `OK` and 0 when `greet("Ada")` is exactly `Hello, Ada!`, and a
/// line that says what it was and 1 otherwise. The oracle runs its own copy of it, and checks
/// the project's is as it was.
pub const CHECK: &str = r#"const greet = require("./greet.cjs");
const said = greet("Ada");
if (said === "Hello, Ada!") {
  console.log("OK");
} else {
  console.log(`FAIL: greet("Ada") is ${JSON.stringify(said)}, not "Hello, Ada!"`);
  process.exitCode = 1;
}
"#;

/// The stand-in worker's change: the line it writes over the fixture's code.
const STAND_IN_CHANGE: &str = "module.exports = (name) => `Hello, ${name}!`;";

/// A step of a scenario.
#[derive(Debug, Clone, Copy)]
pub struct Step {
    pub name: &'static str,
    /// What the human types to a real chief, after the step's marker.
    pub asks: &'static str,
    /// What the human types to the stand-in chief, after the marker: its own words for the
    /// same ask, with the tasks bound so far.
    pub stand_in: fn(&Bindings) -> String,
    /// Whether the step is over, as the board says.
    pub reached: fn(&Snapshot, &Bindings) -> bool,
}

/// A scenario.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub name: &'static str,
    /// The files of the project, made before anything opens.
    pub fixture: Vec<(&'static str, &'static str)>,
    pub steps: Vec<Step>,
    /// The scenario's absolute deadline, from the first ask: activity never moves it.
    pub deadline: Duration,
    /// The most turns all the windows together may begin.
    pub turns: u32,
}

impl Scenario {
    /// The scenario called `name`.
    pub fn named(name: &str) -> Result<Self> {
        match name {
            "smoke" => Ok(smoke()),
            other => Err(Error::Daemon(format!(
                "the matrix has the scenario smoke alone so far: no {other}"
            ))),
        }
    }

    /// The marker of step `index` (from 0) in run `run`: the human's own words at the start of
    /// an ask, which the chief's record is read for, and nothing a model writes.
    pub fn marker(&self, run: &str, index: usize) -> String {
        format!("[matrix {run} ask {}/{}]", index + 1, self.steps.len())
    }
}

/// Whether the task bound to `name` is done or accepted, and its result was delivered to
/// the chief.
fn answered(snapshot: &Snapshot, bindings: &Bindings, name: &str) -> bool {
    bindings.of_name(name).is_some_and(|number| {
        snapshot
            .task(number)
            .is_some_and(|task| task["state"] == "done" || task["state"] == "accepted")
            && snapshot.delivered_result(number).is_some()
    })
}

/// Whether the task bound to `name` is accepted.
pub fn accepted(snapshot: &Snapshot, bindings: &Bindings, name: &str) -> bool {
    bindings
        .of_name(name)
        .and_then(|number| snapshot.task(number))
        .is_some_and(|task| task["state"] == "accepted")
}

/// The words the stand-in chief accepts the task bound to `name` with.
fn accept_line(bindings: &Bindings, name: &str) -> String {
    bindings.of_name(name).map_or_else(
        || format!("no {name} task to accept"),
        |number| format!("CF task accept T-{number}"),
    )
}

/// LMX-04's smoke: advice, a change by the worker, a review and a test of it, and all four
/// accepted, in a project whose `greet.cjs` the checker refuses until the change.
pub fn smoke() -> Scenario {
    Scenario {
        name: "smoke",
        fixture: vec![("greet.cjs", GREET), ("check.cjs", CHECK)],
        steps: vec![
            Step {
                name: "advice",
                asks: "This project has greet.cjs and check.cjs: `node check.cjs` prints OK once greet(\"Ada\") returns exactly \"Hello, Ada!\", and it does not yet. Before anything changes, get advice: put one advice task on the board for our advisor, with `cf task add --advice --tier standard \"...\"`, asking how greet.cjs should change so that the check passes. Every member of our staff is at tier standard. Change no file yourself, ask me nothing, and tell me in one line when the advice is back.",
                stand_in: |_| "DISPATCH --advice --tier standard Reply with exactly: ADVICE_OK".to_owned(),
                reached: |snapshot, bindings| answered(snapshot, bindings, "advice"),
            },
            Step {
                name: "change",
                asks: "Now have our worker make that change: put one task on the board with `cf task add --tier standard \"...\"` to change greet.cjs so that `node check.cjs` prints OK, leaving check.cjs as it is. Do not make the change yourself, and tell me in one line when the worker's result is back.",
                stand_in: |_| format!("DISPATCH --tier standard WRITE greet.cjs :: {STAND_IN_CHANGE}"),
                reached: |snapshot, bindings| answered(snapshot, bindings, "change"),
            },
            Step {
                name: "review and test",
                asks: "Have the change checked: put one review task on the board for our reviewer with `cf task add --review --tier standard \"...\"` to read greet.cjs, and one test task for our tester with `cf task add --test --tier standard \"...\"` to run `node check.cjs` and say what it prints. Tell me in one line when both results are back.",
                stand_in: |_| {
                    "DISPATCH --review --tier standard Reply with exactly: REVIEW_OK\nDISPATCH --test --tier standard Reply with exactly: TEST_OK".to_owned()
                },
                reached: |snapshot, bindings| {
                    answered(snapshot, bindings, "review") && answered(snapshot, bindings, "test")
                },
            },
            Step {
                name: "accept",
                asks: "If the review and the test found nothing wrong, accept the four tasks: the advice, the change, the review and the test, each with `cf task accept`. Tell me in one line when they are accepted.",
                stand_in: |bindings| {
                    ["advice", "change", "review", "test"]
                        .map(|name| accept_line(bindings, name))
                        .join("\n")
                },
                reached: |snapshot, bindings| {
                    ["advice", "change", "review", "test"]
                        .iter()
                        .all(|name| accepted(snapshot, bindings, name))
                },
            },
        ],
        deadline: Duration::from_secs(15 * 60),
        turns: 20,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    fn bound(pairs: &[(&'static str, i64)]) -> Bindings {
        Bindings {
            found: pairs
                .iter()
                .map(|(name, number)| (*name, vec![*number]))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn smoke_is_the_only_scenario_and_runs_in_fifteen_minutes_and_twenty_turns() {
        let smoke = Scenario::named("smoke").unwrap();
        let steps: Vec<&str> = smoke.steps.iter().map(|step| step.name).collect();
        assert_eq!(steps, ["advice", "change", "review and test", "accept"]);
        assert_eq!(smoke.deadline, Duration::from_secs(900));
        assert_eq!(smoke.turns, 20);
        assert!(Scenario::named("core")
            .unwrap_err()
            .to_string()
            .contains("smoke alone so far"));
        assert_eq!(smoke.marker("r1", 1), "[matrix r1 ask 2/4]");
    }

    #[test]
    fn every_ask_to_a_real_chief_is_one_line_and_names_the_tier_it_is_for() {
        for step in smoke().steps {
            assert!(!step.asks.contains('\n'), "{}", step.name);
            assert!(
                step.asks.contains("--tier standard") || step.name == "accept",
                "{}",
                step.name
            );
        }
    }

    #[test]
    fn the_stand_in_chief_is_told_the_same_in_its_own_words_with_the_tasks_bound_so_far() {
        let smoke = smoke();
        let bindings = bound(&[("advice", 1), ("change", 2), ("review", 3), ("test", 4)]);
        let said: Vec<String> = smoke
            .steps
            .iter()
            .map(|step| (step.stand_in)(&bindings))
            .collect();
        assert_eq!(
            said[0],
            "DISPATCH --advice --tier standard Reply with exactly: ADVICE_OK"
        );
        assert!(said[1].starts_with("DISPATCH --tier standard WRITE greet.cjs :: "));
        assert_eq!(said[2].lines().count(), 2);
        assert_eq!(
            said[3],
            "CF task accept T-1\nCF task accept T-2\nCF task accept T-3\nCF task accept T-4"
        );
    }

    #[test]
    fn a_step_is_over_when_the_board_says_so_and_not_before() {
        let smoke = smoke();
        let bindings = bound(&[("advice", 1)]);
        let mut snapshot = Snapshot {
            tasks: vec![json!({ "number": 1, "state": "working" })],
            ..Snapshot::default()
        };
        assert!(!(smoke.steps[0].reached)(&snapshot, &bindings));
        snapshot.tasks[0]["state"] = json!("done");
        // Done, but its result is not in the chief's window yet.
        assert!(!(smoke.steps[0].reached)(&snapshot, &bindings));
        snapshot.chief_inbox =
            vec![json!({ "kind": "result", "taskNumber": 1, "state": "delivered" })];
        assert!((smoke.steps[0].reached)(&snapshot, &bindings));
        // Nothing is bound to the change yet.
        assert!(!(smoke.steps[1].reached)(&snapshot, &bindings));
        assert!(!(smoke.steps[3].reached)(&snapshot, &bindings));
    }

    #[test]
    fn the_stand_ins_change_is_what_the_checker_takes() {
        assert!(STAND_IN_CHANGE.contains("`Hello, ${name}!`"));
        assert!(GREET.contains("`Hi ${name}`"));
        assert!(CHECK.contains("said === \"Hello, Ada!\""));
    }
}
