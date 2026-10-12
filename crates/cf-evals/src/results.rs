//! The results file: one row per report, newest last, the same numbers the summary
//! prints. Pure, so it is tested without a report on disk.
//!
//! The reports kept in `evals/reports` are evidence, and an old one is still read:
//! a report from before the mechanics were counted has none, one from before the
//! owner was asked in the terminal says `?` for it, and one from when the board
//! took questions counted the terminal's apart and spelled the board's
//! `questionsToHuman`. `evals/RESULTS.md` written from the kept reports is byte for
//! byte what the JavaScript wrote, so a report is read as JavaScript read it: a
//! field that is not there is `undefined`, and a number is shown as `String` shows it.

use serde_json::Value;

use crate::js;

/// A report that was kept: the name of its file, which says when it was made, and
/// what it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub name: Option<String>,
    pub value: Value,
}

/// The columns of the table, as the header says them.
pub const COLUMNS: [&str; 17] = [
    "when",
    "scenario",
    "chief (model, effort)",
    "staff",
    "judgment",
    "plumbing",
    "tasks",
    "parallel",
    "advice",
    "reviews",
    "on board",
    "notes",
    "chief edits",
    "files",
    "terminal",
    "asked",
    "min",
];

/// How much of a report's name is its row's `when`: `2026-09-26T00-13`.
const WHEN: usize = 16;

/// How JavaScript's `?? ` would read the field `key` of `value`: it is there and is
/// not `null`.
fn field<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    js::present(value.get(key))
}

/// Whether JavaScript takes the value for true in a condition.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

/// How many of the checks of `list` held, of how many: `?` for a report from before
/// they were made.
fn held(list: Option<&Value>) -> String {
    match list.and_then(Value::as_array) {
        None => "?".to_owned(),
        Some(checks) => format!(
            "{}/{}",
            checks
                .iter()
                .filter(|check| truthy(check.get("ok")))
                .count(),
            checks.len()
        ),
    }
}

/// The questions the chief put to the owner in its terminal, `+Np` for its question
/// dialog: `?` before they were counted; a report from when the board took
/// questions counted the terminal's apart.
fn asked(questions: Option<&Value>) -> String {
    let Some(questions) = js::present(questions) else {
        return "?".to_owned();
    };
    if let Some(terminal) = field(questions, "terminal") {
        return js::string(terminal.get("questions"));
    }
    let pickers = questions.get("pickers");
    format!(
        "{}{}",
        js::string(questions.get("questions")),
        if truthy(pickers) {
            format!("+{}p", js::string(pickers))
        } else {
            String::new()
        }
    )
}

/// How many items the array `key` of `value` has, as text.
fn length(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .map_or_else(|| "undefined".to_owned(), |items| items.len().to_string())
}

/// `seconds` as whole minutes, as `Math.round(seconds / 60)` has them.
fn minutes(seconds: Option<&Value>) -> String {
    match seconds.and_then(Value::as_f64) {
        Some(seconds) => format!("{}", ((seconds / 60.0) + 0.5).floor()),
        None => "NaN".to_owned(),
    }
}

/// The staff of a run: the harnesses joined with `+`, none for a bare run (which has
/// no staff), and `claude` for a report from when a run had no staff of its own.
fn staff(report: &Value) -> String {
    if report.get("arm").and_then(Value::as_str) == Some("bare") {
        return "none".to_owned();
    }
    match field(report, "staff").and_then(Value::as_array) {
        Some(harnesses) => harnesses
            .iter()
            .map(|harness| match harness {
                Value::Null => String::new(),
                other => js::string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join("+"),
        None => "claude".to_owned(),
    }
}

/// The columns of one report, as strings.
pub fn row(report: &Report) -> Vec<String> {
    let value = &report.value;
    let metrics = value.get("metrics").unwrap_or(&Value::Null);
    let arm = value.get("arm").and_then(Value::as_str);
    let chief =
        field(value, "chief").map_or_else(|| "claude".to_owned(), |chief| js::string(Some(chief)));
    let arm_tag = arm
        .filter(|arm| !arm.is_empty() && *arm != "card")
        .map(|arm| format!(" [{arm}]"))
        .unwrap_or_default();
    // A report that has `effort` at all (even `null`) says it; older ones do not.
    let effort = value.get("effort").map_or_else(String::new, |effort| {
        format!(
            ", {}",
            js::present(Some(effort))
                .map_or_else(|| "default".to_owned(), |effort| js::string(Some(effort)))
        )
    });
    let staff_effort = value.get("staffEffort");
    vec![
        js::first(report.name.as_deref().unwrap_or_default(), WHEN),
        // The one column that is the field itself and not its text: joined, a field
        // that is not there is nothing, where `String` would say `undefined`.
        field(value, "scenario").map_or_else(String::new, |scenario| js::string(Some(scenario))),
        format!(
            "{chief}{arm_tag} ({}{effort})",
            js::string(value.get("model"))
        ),
        format!(
            "{}{}",
            staff(value),
            if truthy(staff_effort) {
                format!(" ({})", js::string(staff_effort))
            } else {
                String::new()
            }
        ),
        held(value.get("checks")),
        held(value.get("mechanics")),
        length(metrics.get("tasks")),
        js::string(metrics.get("parallel")),
        js::string(metrics.get("advice")),
        js::string(metrics.get("reviews")),
        // Reports from before 2026-09-29 counted the board's questions as a list.
        match field(metrics, "questionsOnBoard") {
            Some(count) => js::string(Some(count)),
            None => length(metrics.get("questionsToHuman")),
        },
        length(metrics.get("notesToHuman")),
        field(metrics, "chiefEdits")
            .map_or_else(|| "?".to_owned(), |edits| js::string(Some(edits))),
        metrics
            .get("filesChanged")
            .and_then(Value::as_array)
            .map_or_else(|| "0".to_owned(), |files| files.len().to_string()),
        field(value, "terminalAnswers")
            .map_or_else(|| "?".to_owned(), |answers| js::string(Some(answers))),
        asked(metrics.get("ownerQuestions")),
        minutes(value.get("seconds")),
    ]
}

/// The results as a Markdown table, with what each column means above it.
pub fn results_table(reports: &[Report]) -> String {
    let mut lines = vec![
        "# Chief eval results".to_owned(),
        String::new(),
        "One row per report in `evals/reports/`, oldest first; `cargo xtask eval summary` rewrites this file.".to_owned(),
        "Judgment: how many of the scenario's expectations the chief met. Plumbing: how many of the".to_owned(),
        "board's own checks held (briefs delivered, results back to the chief, members' questions".to_owned(),
        "answered and the answers delivered, every task on the board). On board: questions put to the".to_owned(),
        "owner on the board, none since 2026-09-29, when the chief began asking in its terminal.".to_owned(),
        "Terminal: how often the owner nudged a chief that stopped without asking. Asked: the questions".to_owned(),
        "the chief ended its turns with in its terminal, +Np for its own question dialog. `?` is a".to_owned(),
        "report from before that column existed. A chief marked [nocard] had a one-line card naming no".to_owned(),
        "board; [bare] ran without ConsensFlow.".to_owned(),
        String::new(),
        format!("| {} |", COLUMNS.join(" | ")),
        format!("| {} |", COLUMNS.map(|_| "---").join(" | ")),
    ];
    lines.extend(
        reports
            .iter()
            .map(|report| format!("| {} |", row(report).join(" | "))),
    );
    format!("{}\n", lines.join("\n"))
}
