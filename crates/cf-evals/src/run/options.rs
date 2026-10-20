//! What a run is asked to do: the scenario, who the chief and the staff are, which
//! arm it is, and how long it may take.
//!
//! ```text
//! cargo xtask eval --scenario six-decisions [--chief claude] [--staff claude,codex]
//!                  [--model …] [--claude-staff-model …] [--repeat 1] [--timeout-min 40]
//!                  [--effort high] [--staff-effort medium]
//! ```
//!
//! `--model` is the chief's: Opus for Claude Code and the cheap model for OpenCode
//! unless given; Codex, Pi and Devin run their own default and ignore it. `--effort`
//! is the chief's reasoning level (Claude, Codex, Pi; OpenCode's window and Devin
//! have no switch for it), `--staff-effort` every member's.

use std::path::PathBuf;
use std::time::Duration;

use cf_e2e::checkout;

use crate::cli::{self, Flag};
use crate::plan::{harness, HARNESSES};
use crate::scenarios::{self, Scenario};
use crate::{Error, Result};

/// Which chief is measured: ConsensFlow's, as it is told by its card; ConsensFlow
/// with a one-line card that names no board; or the harness alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    /// ConsensFlow's chief card.
    Card,
    /// ConsensFlow with a one-line card that names no board.
    NoCard,
    /// The harness alone, no ConsensFlow.
    Bare,
}

impl Arm {
    /// What a command line and a report call it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Card => "card",
            Self::NoCard => "nocard",
            Self::Bare => "bare",
        }
    }

    fn of(word: &str) -> Result<Self> {
        [Self::Card, Self::NoCard, Self::Bare]
            .into_iter()
            .find(|arm| arm.name() == word)
            .ok_or_else(|| Error::Usage(format!("no such arm: {word}")))
    }
}

/// The flags of `run`.
const FLAGS: [Flag; 14] = [
    Flag::or("scenario", "six-decisions"),
    Flag::or("chief", "claude"),
    Flag::value("staff"),
    Flag::value("model"),
    Flag::switch("gate"),
    Flag::or("claude-staff-model", HARNESSES[0].model),
    // A Pi member's model, as Pi names it (provider/model): the machine's signed-in provider.
    Flag::or("pi-staff-model", HARNESSES[2].model),
    Flag::or("repeat", "1"),
    Flag::or("timeout-min", "40"),
    Flag::or("effort", "high"),
    Flag::or("staff-effort", "medium"),
    Flag::or("arm", "card"),
    // A scenario's `Switch` step moves the chief to this harness, on its staff
    // agent's model (so the harness must be in `--staff`).
    Flag::value("switch-to"),
    // Where the report of each run is kept: `evals/reports`, which `eval summary` reads.
    Flag::value("reports"),
];

/// Who the windows of a run are, as the machine, the rig and the wrappers need it:
/// the chief's harness and the staff's, the chief's reasoning level, and whether
/// the chief is given the one-line card. An eval's run says it ([`Options::crew`]),
/// and so does a long run (`crate::long_run`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crew {
    pub chief: String,
    pub staff: Vec<String>,
    pub effort: String,
    pub nocard: bool,
}

impl Crew {
    /// Whether a harness has a window in the run, as the chief or on the staff.
    pub fn in_run(&self, name: &str) -> bool {
        self.chief == name || self.staff.iter().any(|staff| staff == name)
    }
}

/// What a run is asked to do.
#[derive(Debug)]
pub struct Options {
    pub scenario: Scenario,
    pub chief: String,
    /// The harnesses of the staff: the chief's alone, unless `--staff` names others.
    pub staff: Vec<String>,
    /// The chief's model, where `--model` names one.
    pub model: Option<String>,
    /// The project is opened with the owner's approval required.
    pub gate: bool,
    pub claude_staff_model: String,
    pub pi_staff_model: String,
    pub repeat: u32,
    pub timeout: Duration,
    pub effort: String,
    pub staff_effort: String,
    pub arm: Arm,
    pub switch_to: Option<String>,
    /// The folder the report of each run is kept in.
    pub reports: PathBuf,
}

impl Options {
    /// What the words of a `run` ask for, or what is wrong with them.
    pub fn of(words: &[String]) -> Result<Self> {
        let flags = cli::parse(&FLAGS, words)?;
        let text = |name: &str| flags.get(name).unwrap_or_default().to_owned();
        let arm = Arm::of(&text("arm"))?;
        let id = text("scenario");
        let scenario = scenarios::by_id(&id).ok_or_else(|| {
            Error::Usage(format!(
                "no such scenario: {id} ({})",
                scenarios::IDS.join(", ")
            ))
        })?;
        let chief = text("chief");
        let staff: Vec<String> = flags
            .get("staff")
            .unwrap_or(&chief)
            .split(',')
            .map(|name| name.trim().to_owned())
            .collect();
        let switch_to = flags.get("switch-to").map(str::to_owned);
        if arm == Arm::Bare && scenario.switches_the_chief() {
            return Err(Error::Usage(format!(
                "{} switches the chief from its card, which a bare chief has none of",
                scenario.id
            )));
        }
        if scenario.switches_the_chief()
            && !switch_to
                .as_ref()
                .is_some_and(|target| staff.contains(target) && *target != chief)
        {
            return Err(Error::Usage(format!(
                "{} switches the chief: --switch-to names a --staff harness other than the chief's",
                scenario.id
            )));
        }
        harness(&chief)?;
        for name in &staff {
            harness(name)?;
        }
        let number = |name: &str| -> Result<f64> {
            text(name)
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite() && *number >= 0.0)
                .ok_or_else(|| Error::Usage(format!("--{name} takes a number")))
        };
        Ok(Self {
            scenario,
            chief,
            staff,
            model: flags.get("model").map(str::to_owned),
            gate: flags.has("gate"),
            claude_staff_model: text("claude-staff-model"),
            pi_staff_model: text("pi-staff-model"),
            repeat: u32::try_from(number("repeat")? as u64)
                .map_err(|_| Error::Usage("--repeat takes a smaller number".to_owned()))?,
            timeout: Duration::from_secs_f64(number("timeout-min")? * 60.0),
            effort: text("effort"),
            staff_effort: text("staff-effort"),
            arm,
            switch_to,
            reports: flags
                .get("reports")
                .map_or_else(|| checkout::path("evals/reports"), PathBuf::from),
        })
    }

    /// The chief's effort as it reached the chief: none where its harness has no
    /// switch for it.
    pub fn chief_effort(&self) -> Option<&str> {
        ["claude", "codex", "pi"]
            .contains(&self.chief.as_str())
            .then_some(self.effort.as_str())
    }

    /// Whether a harness has a window in this run, as the chief or on the staff.
    pub fn in_run(&self, name: &str) -> bool {
        self.crew().in_run(name)
    }

    /// Who the windows of this run are, as the machine and the wrappers need it.
    pub fn crew(&self) -> Crew {
        Crew {
            chief: self.chief.clone(),
            staff: self.staff.clone(),
            effort: self.effort.clone(),
            nocard: self.arm == Arm::NoCard,
        }
    }

    /// The models the staff's harnesses run on where `--…-staff-model` names one.
    pub fn staff_models(&self) -> [(&str, &str); 2] {
        [
            ("claude", self.claude_staff_model.as_str()),
            ("pi", self.pi_staff_model.as_str()),
        ]
    }
}

#[cfg(test)]
mod tests;
