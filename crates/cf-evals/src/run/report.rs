//! What a run keeps and says of itself: the report, which is evidence (a run is
//! kept, and a later run beside it is the comparison: `evals/reports/`, which
//! `cargo xtask eval summary` reads), its name, and the verdict on the console. The
//! keys of a report are the ones the first evals wrote, in their order, and its
//! verdict is said as it always was (`PASS` and `FAIL`, the daemon it ran on, the
//! path of the report): what the Windows matrix reads back.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde_json::{json, Value};

use super::floor::Floor;
use super::options::{Arm, Options};
use crate::bare::report_value;
use crate::clock;
use crate::measure::{Check, Metrics};
use crate::plan::{ChiefSetup, Staff};
use crate::scenarios::Scenario;
use crate::{Error, Result};

/// What the owner did, counted: the nudges, the answers and the follow-ups.
#[derive(Debug, Clone, Copy, Default)]
pub struct Said {
    /// Nudges of a chief that stopped without asking.
    pub nudges: i64,
    /// Answers to what the chief asked in its terminal.
    pub replies: i64,
    /// Answers to its own question dialog.
    pub dialogs: i64,
    /// Messages of a conversation.
    pub follow_ups: i64,
}

/// An approval the owner gave at the gate that the daemon refused.
#[derive(Debug, Clone)]
pub struct Refused {
    pub message: Value,
    pub error: Value,
}

/// A run of the card or the nocard arm, to be kept.
pub struct Card<'a> {
    pub options: &'a Options,
    pub scenario: &'a Scenario,
    pub chief: &'a ChiefSetup,
    pub staff: &'a Staff,
    pub index: u32,
    pub seconds: u64,
    pub metrics: Metrics,
    pub checks: Vec<Check>,
    /// The board's plumbing, and where the gate was held to: the checks of the board.
    pub mechanics: Vec<Check>,
    /// The last lines of the chief's window.
    pub screen: Vec<String>,
    pub said: Said,
    pub refused: Vec<Refused>,
    pub approvals: i64,
    pub log: Value,
    pub path: PathBuf,
}

/// A run of the bare arm, to be kept.
pub struct Bare<'a> {
    pub options: &'a Options,
    pub scenario: &'a Scenario,
    pub chief: &'a ChiefSetup,
    /// The conversation the chief's harness named, where it names one.
    pub session: Option<String>,
    pub index: u32,
    pub seconds: u64,
    pub metrics: Metrics,
    pub checks: Vec<Check>,
    pub screen: Vec<String>,
    pub said: Said,
    pub log: Value,
    pub path: PathBuf,
}

/// The checks as a report keeps them.
fn checks_value(checks: &[Check]) -> Value {
    Value::Array(checks.iter().map(Check::to_value).collect())
}

/// What the file of a report is called: when it was made, the scenario, the chief and
/// its arm, the staff and which run of them it was.
pub fn file_name(at: Timestamp, options: &Options, index: u32) -> String {
    let arm = match options.arm {
        Arm::Card => String::new(),
        other => format!("-{}", other.name()),
    };
    let staff = if options.arm == Arm::Bare {
        String::new()
    } else {
        format!("-staff-{}", options.staff.join("+"))
    };
    format!(
        "{}-{}-chief-{}{arm}{staff}-{index}.json",
        clock::stamp(at),
        options.scenario.id,
        options.chief
    )
}

/// The effort of the chief for a line: its own, or that it has none.
fn effort_of(options: &Options) -> &str {
    options.chief_effort().unwrap_or("its default")
}

/// Whether every check of the lists held.
fn held(lists: &[&[Check]]) -> bool {
    lists
        .iter()
        .all(|checks| checks.iter().all(|check| check.ok))
}

impl Card<'_> {
    /// The report, as a file keeps it.
    pub fn value(&self) -> Value {
        let options = self.options;
        let staff_models: serde_json::Map<String, Value> = self
            .staff
            .agents
            .iter()
            .map(|agent| (agent.id.clone(), json!(agent.model)))
            .collect();
        json!({
            "scenario": self.scenario.id,
            "chief": options.chief,
            "model": self.chief.model,
            "effort": options.chief_effort(),
            "staffEffort": options.staff_effort,
            "staff": options.staff,
            "switchTo": options.switch_to,
            "staffModels": staff_models,
            "run": self.index,
            "seconds": self.seconds,
            "metrics": self.metrics.to_value(),
            "checks": checks_value(&self.checks),
            "mechanics": checks_value(&self.mechanics),
            "chiefScreen": self.screen,
            "arm": options.arm.name(),
            "terminalAnswers": self.said.nudges,
            "terminalReplies": self.said.replies,
            "pickerAnswers": self.said.dialogs,
            "followUpsSent": self.said.follow_ups,
            "refusedApprovals": self.refused.iter()
                .map(|refused| json!({ "message": refused.message, "error": refused.error }))
                .collect::<Vec<_>>(),
            "gate": options.gate,
            "approvals": self.approvals,
            "log": self.log,
        })
    }

    /// Whether the run held: every expectation and every check of the board.
    pub fn held(&self) -> bool {
        held(&[&self.checks, &self.mechanics])
    }

    /// Says the verdict on the console, as it always was said.
    pub fn say(&self, floor: &mut Floor) -> Result {
        let (options, metrics) = (self.options, &self.metrics);
        let arm = match options.arm {
            Arm::Card => String::new(),
            other => format!(" [{}]", other.name()),
        };
        floor.say(format!(
            "\n{} · chief {}{arm} ({}, effort {}) · staff effort {} · staff {} · run {} · {}s",
            self.scenario.id,
            options.chief,
            self.chief.model,
            effort_of(options),
            options.staff_effort,
            options.staff.join("+"),
            self.index,
            self.seconds
        ))?;
        verdict_lines(floor, &self.checks)?;
        floor.say("  the plumbing:")?;
        verdict_lines(floor, &self.mechanics)?;
        if metrics.chief_turns == 0 {
            floor.say("  the chief's screen ended with:")?;
            let tail = self.screen.len().saturating_sub(8);
            for line in &self.screen[tail..] {
                floor.say(format!("    {line}"))?;
            }
        }
        let owner = &metrics.owner_questions;
        let members: Vec<String> = metrics
            .member_questions_by
            .iter()
            .map(|row| {
                format!(
                    "{}/{} {} (answered {}, delivered {}, longest {})",
                    row.role,
                    row.harness.as_deref().unwrap_or("null"),
                    row.asked,
                    row.answered,
                    row.delivered,
                    row.longest
                )
            })
            .collect();
        floor.say(format!(
            "  tasks {} (parallel {}, advice {}, reviews {}) · notes {} · chief edits {} in {} turns · files changed {} · answered in the terminal {} (nudges) + {} (its questions)",
            metrics.tasks.len(),
            metrics.parallel,
            metrics.advice,
            metrics.reviews,
            metrics.notes_to_human.len(),
            metrics.chief_edits.map_or_else(|| "?".to_owned(), |edits| edits.to_string()),
            metrics.chief_turns,
            metrics.files_changed.len(),
            self.said.nudges,
            self.said.replies
        ))?;
        floor.say(format!(
            "  the owner was asked in the terminal: {} questions in {} turns, {} in its question dialog · on the board {} · cf ask refused {}",
            owner.questions, owner.turns_asking, owner.pickers, metrics.questions_on_board, metrics.ask_refused
        ))?;
        floor.say(format!(
            "  members asked the chief: {}",
            if members.is_empty() {
                "nothing".to_owned()
            } else {
                members.join(" · ")
            }
        ))?;
        floor.say(format!("  report: {}", self.path.display()))
    }
}

/// A line to each check: `  PASS the page is fixed`.
fn verdict_lines(floor: &mut Floor, checks: &[Check]) -> Result {
    for check in checks {
        floor.say(format!(
            "  {} {}",
            if check.ok { "PASS" } else { "FAIL" },
            check.name
        ))?;
    }
    Ok(())
}

impl Bare<'_> {
    /// The report, as a file keeps it.
    pub fn value(&self) -> Value {
        json!({
            "scenario": self.scenario.id,
            "chief": self.options.chief,
            "model": self.chief.model,
            "effort": self.options.chief_effort(),
            "arm": "bare",
            "session": self.session,
            "run": self.index,
            "seconds": self.seconds,
            "metrics": report_value(&self.metrics),
            "checks": checks_value(&self.checks),
            "chiefScreen": self.screen,
            "terminalAnswers": self.said.nudges,
            "terminalReplies": self.said.replies,
            "followUpsSent": self.said.follow_ups,
            "log": self.log,
        })
    }

    /// Whether the run held: every expectation.
    pub fn held(&self) -> bool {
        held(&[&self.checks])
    }

    /// Says the verdict on the console, as it always was said.
    pub fn say(&self, floor: &mut Floor) -> Result {
        let (options, metrics) = (self.options, &self.metrics);
        floor.say(format!(
            "\n{} · chief {} [bare] ({}, effort {}) · run {} · {}s",
            self.scenario.id,
            options.chief,
            self.chief.model,
            effort_of(options),
            self.index,
            self.seconds
        ))?;
        verdict_lines(floor, &self.checks)?;
        floor.say(format!(
            "  turns {} · files changed {} · answered in the terminal {} (nudges) + {} (its questions)",
            metrics.chief_turns,
            metrics.files_changed.len(),
            self.said.nudges,
            self.said.replies
        ))?;
        floor.say(format!(
            "  the owner was asked in the terminal: {} questions in {} turns",
            metrics.owner_questions.questions, metrics.owner_questions.turns_asking
        ))?;
        floor.say(format!("  report: {}", self.path.display()))
    }
}

/// Writes `report` as JSON, two spaces to an indent, to a file called `name` in
/// `folder` (made if it is not there): where it is.
pub fn keep(folder: &Path, name: &str, report: &Value) -> Result<PathBuf> {
    let text = serde_json::to_string_pretty(report)
        .map_err(|cause| Error::Refused(format!("a report that is no JSON ({cause}): {report}")))?;
    let file = folder.join(name);
    cf_e2e::files::write(&file, format!("{text}\n"))?;
    Ok(file)
}

#[cfg(test)]
mod tests;
