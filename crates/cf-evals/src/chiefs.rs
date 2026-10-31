//! `chiefs`: one scenario played once by each harness that can be a chief, on its cheap model,
//! and a line of what each came to (T-972; the issue page `chief-does-the-work-itself`). The
//! `conversation` scenario, its default, is the one that found a Claude chief making every
//! change itself (btb, 2026-09-25 to 28): it was run by hand, one chief a run; this runs it for
//! every chief harness, one after another, so that a card or a harness that starts doing the
//! work again is seen for each.
//!
//! ```text
//! cargo xtask eval chiefs                                  conversation, every chief harness
//! cargo xtask eval chiefs --chiefs claude,codex
//! cargo xtask eval chiefs --scenario six-decisions --effort medium
//! ```
//!
//! Each chief is a `run` of its own (`crate::run`): the chief's harness is its staff too, the
//! chief runs on the harness's cheap model of the evals ([`HARNESSES`]), and its report is kept
//! in the reports folder as any run's is (`eval summary` reads it). A run that fails to start is
//! that chief's line, and the next chief runs. The status is 0 when every chief's run held.
//!
//! OpenCode, which leaves ConsensFlow, runs only where `--chiefs` names it.

use std::io::Write;
use std::path::{Path, PathBuf};

use cf_e2e::checkout;
use serde_json::Value;

use crate::cli::{self, Flag};
use crate::plan::{harness, HARNESSES};
use crate::{run, Error, Result};

/// The flags of `chiefs`.
const FLAGS: [Flag; 5] = [
    Flag::or("scenario", "conversation"),
    Flag::value("chiefs"),
    Flag::or("effort", "medium"),
    Flag::or("timeout-min", "40"),
    Flag::value("reports"),
];

/// The harness that runs only when named.
const LEAVING: &str = "opencode";

/// The chiefs `named` (`claude,codex`), or every harness of the evals but the one leaving.
pub fn chiefs_of(named: Option<&str>) -> Result<Vec<&'static str>> {
    match named {
        None => Ok(HARNESSES
            .iter()
            .map(|harness| harness.name)
            .filter(|name| *name != LEAVING)
            .collect()),
        Some(list) => list
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| harness(name).map(|harness| harness.name))
            .collect::<Result<Vec<_>>>()
            .and_then(|chiefs| {
                if chiefs.is_empty() {
                    Err(Error::Usage("--chiefs names no harness".to_owned()))
                } else {
                    Ok(chiefs)
                }
            }),
    }
}

/// The words of the `run` of `chief`: the scenario, the chief its own staff on its cheap
/// model, the effort, the time it has and where its report is kept.
pub fn run_words(
    chief: &str,
    scenario: &str,
    effort: &str,
    timeout: &str,
    reports: &Path,
) -> Vec<String> {
    let model = harness(chief)
        .map(|harness| harness.model)
        .unwrap_or_default();
    [
        "--scenario",
        scenario,
        "--chief",
        chief,
        "--staff",
        chief,
        "--model",
        model,
        "--effort",
        effort,
        "--timeout-min",
        timeout,
    ]
    .iter()
    .map(|word| (*word).to_owned())
    .chain([
        "--reports".to_owned(),
        reports.to_string_lossy().into_owned(),
    ])
    .collect()
}

/// The report the `run` of `scenario` by `chief` kept in `reports` since `since` (a stamp of
/// [`crate::clock::stamp`], which sorts as the time does): the newest, by its name.
pub fn report_of(reports: &Path, scenario: &str, chief: &str, since: &str) -> Option<PathBuf> {
    let marked = format!("-{scenario}-chief-{chief}-staff-");
    let mut found: Vec<PathBuf> = std::fs::read_dir(reports)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .is_some_and(|name| {
                    name.contains(&marked) && name.ends_with(".json") && name.as_str() >= since
                })
        })
        .collect();
    found.sort();
    found.pop()
}

/// A chief's line: what its report says, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub chief: String,
    pub model: String,
    pub held: bool,
    pub said: String,
}

/// The line of `chief` from its kept report `report`: whether every check held, how many
/// tasks went to the board, and the checks that did not hold.
pub fn line_of(chief: &str, report: &Value) -> Line {
    let failed: Vec<String> = ["checks", "mechanics"]
        .iter()
        .flat_map(|list| report[*list].as_array().cloned().unwrap_or_default())
        .filter(|check| check["ok"] != true)
        .map(|check| check["name"].as_str().unwrap_or_default().to_owned())
        .collect();
    let tasks = report["metrics"]["tasks"].as_array().map_or(0, Vec::len);
    let mut said = format!(
        "{tasks} task{} on the board",
        if tasks == 1 { "" } else { "s" }
    );
    if !failed.is_empty() {
        said.push_str(&format!("; not held: {}", failed.join("; ")));
    }
    Line {
        chief: chief.to_owned(),
        model: report["model"].as_str().unwrap_or_default().to_owned(),
        held: failed.is_empty(),
        said,
    }
}

/// The table of the lines, a chief to a row.
pub fn table(lines: &[Line]) -> String {
    let mut rows = vec![
        "| chief | model | held | what it did |".to_owned(),
        "|---|---|---|---|".to_owned(),
    ];
    rows.extend(lines.iter().map(|line| {
        format!(
            "| {} | {} | {} | {} |",
            line.chief,
            line.model,
            if line.held { "yes" } else { "NO" },
            line.said
        )
    }));
    rows.join("\n")
}

/// `chiefs`: the scenario run by each chief, one after another, and the table: the status is
/// 0 when every chief's run held.
pub fn command(words: &[String], out: &mut dyn Write) -> Result<i32> {
    let flags = cli::parse(&FLAGS, words)?;
    let text = |name: &str| flags.get(name).unwrap_or_default().to_owned();
    let chiefs = chiefs_of(flags.get("chiefs"))?;
    let reports = flags
        .get("reports")
        .map_or_else(|| checkout::path("evals/reports"), PathBuf::from);
    let scenario = text("scenario");
    let mut lines = Vec::new();
    for chief in chiefs {
        writeln!(out, "== {scenario}, chief {chief}")?;
        let since = crate::clock::stamp(jiff::Timestamp::now());
        let words = run_words(
            chief,
            &scenario,
            &text("effort"),
            &text("timeout-min"),
            &reports,
        );
        let ran = run::command(&words, out);
        let report = report_of(&reports, &scenario, chief, &since)
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|kept| serde_json::from_str::<Value>(&kept).ok());
        lines.push(match (ran, report) {
            (_, Some(report)) => line_of(chief, &report),
            (Ok(status), None) => Line {
                chief: chief.to_owned(),
                model: String::new(),
                held: false,
                said: format!("the run ended with status {status} and kept no report"),
            },
            (Err(failed), None) => Line {
                chief: chief.to_owned(),
                model: String::new(),
                held: false,
                said: format!("the run failed: {failed}"),
            },
        });
    }
    writeln!(out, "\n{}", table(&lines))?;
    Ok(i32::from(!lines.iter().all(|line| line.held)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn every_eval_harness_but_opencode_is_a_chief_unless_some_are_named() {
        assert_eq!(chiefs_of(None).unwrap(), ["claude", "codex", "pi", "devin"]);
        assert_eq!(
            chiefs_of(Some("codex, opencode")).unwrap(),
            ["codex", "opencode"]
        );
        assert_eq!(
            chiefs_of(Some("kimi")).unwrap_err().to_string(),
            "no such eval harness: kimi"
        );
        assert!(chiefs_of(Some(" , ")).is_err());
    }

    #[test]
    fn a_chiefs_run_is_its_own_staff_on_its_cheap_model_with_the_reports_kept_together() {
        let words = run_words("codex", "conversation", "medium", "40", Path::new("/r"));
        assert_eq!(
            words.join(" "),
            "--scenario conversation --chief codex --staff codex --model gpt-5.6-luna \
             --effort medium --timeout-min 40 --reports /r"
        );
        let claude = run_words("claude", "conversation", "low", "30", Path::new("/r"));
        assert!(claude
            .join(" ")
            .contains("--model claude-haiku-4-5-20251001"));
        // Each is a run `run` takes.
        assert!(run::Options::of(&words).is_ok());
    }

    #[test]
    fn a_chiefs_report_is_the_newest_of_its_scenario_and_chief_since_it_began() {
        let folder = tempfile::tempdir().unwrap();
        for name in [
            "2026-10-31T10-00-00-000Z-conversation-chief-codex-staff-codex-1.json",
            "2026-10-31T12-00-00-000Z-conversation-chief-codex-staff-codex-1.json",
            "2026-10-31T12-00-00-000Z-conversation-chief-claude-staff-claude-1.json",
            "2026-10-31T12-30-00-000Z-six-decisions-chief-codex-staff-codex-1.json",
        ] {
            std::fs::write(folder.path().join(name), "{}").unwrap();
        }
        let found = report_of(folder.path(), "conversation", "codex", "2026-10-31T11");
        assert_eq!(
            found.unwrap().file_name().unwrap().to_string_lossy(),
            "2026-10-31T12-00-00-000Z-conversation-chief-codex-staff-codex-1.json"
        );
        assert!(report_of(folder.path(), "conversation", "codex", "2026-10-31T13").is_none());
        assert!(report_of(folder.path(), "conversation", "pi", "2026").is_none());
    }

    #[test]
    fn a_line_says_the_tasks_on_the_board_and_every_check_that_did_not_hold() {
        let report = json!({
            "model": "gpt-5.6-luna",
            "metrics": { "tasks": [{ "number": 1 }, { "number": 2 }] },
            "checks": [
                { "name": "at least three changes go to the board", "ok": false },
                { "name": "the work is done", "ok": true },
            ],
            "mechanics": [{ "name": "every brief delivered", "ok": true }],
        });
        let line = line_of("codex", &report);
        assert_eq!(
            line,
            Line {
                chief: "codex".to_owned(),
                model: "gpt-5.6-luna".to_owned(),
                held: false,
                said: "2 tasks on the board; not held: at least three changes go to the board"
                    .to_owned(),
            }
        );
        let held = line_of(
            "pi",
            &json!({ "model": "m", "metrics": { "tasks": [{}] }, "checks": [], "mechanics": [] }),
        );
        assert!(held.held);
        assert_eq!(held.said, "1 task on the board");
        assert_eq!(
            table(&[line, held]),
            "| chief | model | held | what it did |\n|---|---|---|---|\n\
             | codex | gpt-5.6-luna | NO | 2 tasks on the board; not held: at least three changes go to the board |\n\
             | pi | m | yes | 1 task on the board |"
        );
    }
}
