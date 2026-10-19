//! What a long run is asked to do.
//!
//! ```text
//! cargo xtask eval long [--chief claude] [--staff claude,codex,pi,opencode,devin]
//!                       [--models claude=a+b,...] [--model M] [--effort high]
//!                       [--staff-effort medium] [--hours 2] [--every-min 8]
//!                       [--look-sec 10] [--idle-min 20] [--dialog-min 5]
//!                       [--quiet-min 10] [--wake-min 20] [--switch-to HARNESS]
//!                       [--quiet] [--pause] [--kill] [--switch] [--compact]
//!                       [--restart] [--devin-cut] [--suspend]
//!                       [--ask real|stand-in] [--reports DIR]
//!                       [--settle-sec 60] [--drop-reading-sec N]
//! ```
//!
//! `--drop-reading-sec` is the dry run's: the watch takes every reading of the record of
//! the first Claude Code result's worker as failed for that long after the result, a gap
//! the readings after it fill, so that the run shows a gap filled.
//!
//! The chief and the staff are flags, as in the evals; the staff is every harness
//! unless it names fewer. Each disruption is a flag: none named plays them all,
//! some named play those alone, and `--quiet` plays none.

use std::path::PathBuf;
use std::time::Duration;

use cf_e2e::checkout;

use super::ask::Ask;
use super::disruptions::Disruption;
use super::invariants::Settings;
use crate::cli::{self, Flag};
use crate::plan::harness;
use crate::{Error, Result};

/// The flags of `long`.
const FLAGS: [Flag; 26] = [
    Flag::or("chief", "claude"),
    Flag::or("staff", "claude,codex,pi,opencode,devin"),
    Flag::value("models"),
    Flag::value("model"),
    Flag::or("effort", "high"),
    Flag::or("staff-effort", "medium"),
    Flag::or("hours", "2"),
    // Seven disruptions eight minutes apart fit in the first hour of the work, which a
    // planning chief begins some minutes in.
    Flag::or("every-min", "8"),
    Flag::or("look-sec", "10"),
    Flag::or("idle-min", "20"),
    Flag::or("dialog-min", "5"),
    Flag::or("quiet-min", "10"),
    Flag::or("wake-min", "20"),
    Flag::value("switch-to"),
    Flag::switch("quiet"),
    Flag::switch("pause"),
    Flag::switch("kill"),
    Flag::switch("switch"),
    Flag::switch("compact"),
    Flag::switch("restart"),
    Flag::switch("devin-cut"),
    Flag::switch("suspend"),
    Flag::or("ask", "real"),
    // Where the report of the run is kept: beside the evals', in a folder of its own,
    // so that `eval summary` reads only the evals'.
    Flag::value("reports"),
    // How long a result is let be before it is judged against the record, in seconds.
    Flag::or("settle-sec", "60"),
    // The dry run's: how long after the first result of a Claude Code worker every reading
    // of that worker's own record is taken as failed, a gap the readings after fill.
    Flag::value("drop-reading-sec"),
];

/// What a long run is asked to do.
#[derive(Debug, Clone)]
pub struct Options {
    pub chief: String,
    /// The harnesses asked for on the staff, in the order named.
    pub staff: Vec<String>,
    /// `--models` as it was written.
    pub models: Option<String>,
    /// The chief's model, where named.
    pub model: Option<String>,
    pub effort: String,
    pub staff_effort: String,
    /// How long the run may go: it ends sooner when the project is done.
    pub budget: Duration,
    /// How long after the one before each disruption is due.
    pub every: Duration,
    /// How often the watch looks.
    pub look: Duration,
    /// How long nothing moves before the run is over.
    pub quiet: Duration,
    /// How often the project's wake-up calls nudge the chief, in minutes.
    pub wake_minutes: u64,
    pub settings: Settings,
    /// The disruptions to play, in no order (the schedule has its own).
    pub disruptions: Vec<Disruption>,
    pub switch_to: Option<String>,
    pub ask: Ask,
    pub reports: PathBuf,
    /// How long after the first result of a Claude Code worker every reading of its
    /// record is taken as failed (the dry run's: a gap the readings after it fill).
    pub drop_reading: Option<Duration>,
}

impl Options {
    /// What the words of a `long` ask for, or what is wrong with them.
    pub fn of(words: &[String]) -> Result<Self> {
        let flags = cli::parse(&FLAGS, words)?;
        let text = |name: &str| flags.get(name).unwrap_or_default().to_owned();
        let number = |name: &str| -> Result<f64> {
            text(name)
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite() && *number > 0.0)
                .ok_or_else(|| Error::Usage(format!("--{name} takes a number above 0")))
        };
        let minutes =
            |name: &str| -> Result<Duration> { Ok(Duration::from_secs_f64(number(name)? * 60.0)) };
        let chief = text("chief");
        harness(&chief).map_err(|refused| Error::Usage(refused.to_string()))?;
        let staff: Vec<String> = text("staff")
            .split(',')
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .collect();
        for name in &staff {
            harness(name).map_err(|refused| Error::Usage(refused.to_string()))?;
        }
        if staff.is_empty() {
            return Err(Error::Usage(
                "--staff names at least one harness".to_owned(),
            ));
        }
        let switch_to = flags.get("switch-to").map(str::to_owned);
        if let Some(target) = &switch_to {
            if !staff.contains(target) {
                return Err(Error::Usage(format!(
                    "--switch-to names a harness of the staff, not {target}"
                )));
            }
        }
        let named: Vec<Disruption> = Disruption::ALL
            .into_iter()
            .filter(|disruption| flags.has(disruption.flag()))
            .collect();
        let disruptions = if flags.has("quiet") {
            if !named.is_empty() {
                return Err(Error::Usage(
                    "--quiet plays no disruption, and names none".to_owned(),
                ));
            }
            Vec::new()
        } else if named.is_empty() {
            Disruption::ALL.to_vec()
        } else {
            named
        };
        let millis = |duration: Duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
        Ok(Self {
            chief,
            staff,
            models: flags.get("models").map(str::to_owned),
            model: flags.get("model").map(str::to_owned),
            effort: text("effort"),
            staff_effort: text("staff-effort"),
            budget: Duration::from_secs_f64(number("hours")? * 3600.0),
            every: minutes("every-min")?,
            look: Duration::from_secs_f64(number("look-sec")?),
            quiet: minutes("quiet-min")?,
            wake_minutes: number("wake-min")?.ceil() as u64,
            settings: {
                let settle = millis(Duration::from_secs_f64(number("settle-sec")?));
                Settings {
                    idle: millis(minutes("idle-min")?),
                    dialog: millis(minutes("dialog-min")?),
                    settle,
                    young: settle,
                }
            },
            disruptions,
            switch_to,
            ask: Ask::of(&text("ask"))?,
            reports: flags
                .get("reports")
                .map_or_else(|| checkout::path("evals/reports/long"), PathBuf::from),
            drop_reading: match flags.get("drop-reading-sec") {
                Some(_) => Some(Duration::from_secs_f64(number("drop-reading-sec")?)),
                None => None,
            },
        })
    }

    /// The harness the chief is switched to: the one named, else the first of the
    /// staff that is not the chief's, else the chief's own.
    pub fn switch_target(&self, staff: &[String]) -> Option<String> {
        self.switch_to
            .clone()
            .filter(|named| staff.contains(named))
            .or_else(|| staff.iter().find(|name| **name != self.chief).cloned())
            .or_else(|| staff.first().cloned())
    }

    /// The chief's effort as it reached the chief: none where its harness has no
    /// switch for it.
    pub fn chief_effort(&self) -> Option<&str> {
        ["claude", "codex", "pi"]
            .contains(&self.chief.as_str())
            .then_some(self.effort.as_str())
    }
}

#[cfg(test)]
mod tests;
