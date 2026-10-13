//! A project's wake-up calls: the human's setting that has the chief nudged on
//! a schedule, and the nudge. A wake-up is a note from the human to the chief,
//! queued as any note is and delivered by the usual rules: once the chief is at
//! rest, and never over words the human has typed in its window and not sent.
//! It asks for no answer, so the chief's reply stays in its terminal and
//! nothing of it reaches the board or the human's inbox.
//!
//! The setting is columns of the project's row (on or off, how often, and the
//! words, or none for the default's); only the human changes it, and each
//! change is logged (`project.wake_up_setting`). Each wake-up queued is logged
//! too (`project.wake_up`), and that log is all `due` reads: when the last one
//! went, whether it is still on its way, and whether anything moved since.

mod due;

use cf_proto::wake_ups::WakeUpView;
use rusqlite::{OptionalExtension, Row};
use serde_json::{json, Value};

use crate::model::{printed, require_text, LedgerError, MAX_BODY};
use crate::store::Store;

pub(crate) use due::{queue_if_due, withdraw_queued};

/// The words of a wake-up the human has not written their own for.
pub const DEFAULT_WAKE_UP_TEXT: &str =
    "Do you still have work to do? If tasks are left on the board or in your plan, carry on.";

/// The least a wake-up may wait for the next, in milliseconds: a minute.
pub const WAKE_UP_FLOOR_MS: i64 = 60_000;

/// What an interval is counted in, largest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeUpUnit {
    Days,
    Hours,
    Minutes,
}

impl WakeUpUnit {
    const ALL: [Self; 3] = [Self::Days, Self::Hours, Self::Minutes];

    /// The word the page says it by.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Days => "days",
            Self::Hours => "hours",
            Self::Minutes => "minutes",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|unit| unit.name() == name)
    }

    /// How long one of it is, in milliseconds. A day is twenty-four hours.
    const fn ms(self) -> i64 {
        match self {
            Self::Days => 86_400_000,
            Self::Hours => 3_600_000,
            Self::Minutes => 60_000,
        }
    }
}

/// A project's wake-up calls: on or off, how often in milliseconds, and the
/// words, none for the default's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeUpSetting {
    pub on: bool,
    pub every_ms: i64,
    pub text: Option<String>,
}

impl WakeUpSetting {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            on: row.get::<_, i64>("wake_up_on")? == 1,
            every_ms: row.get("wake_up_every_ms")?,
            text: row.get("wake_up_text")?,
        })
    }

    /// What a wake-up says.
    pub fn words(&self) -> &str {
        self.text.as_deref().unwrap_or(DEFAULT_WAKE_UP_TEXT)
    }

    /// The setting as the page reads it: the interval in the largest unit that
    /// says it in a whole number.
    pub fn view(&self) -> WakeUpView {
        let unit = WakeUpUnit::ALL
            .into_iter()
            .find(|unit| self.every_ms % unit.ms() == 0)
            .unwrap_or(WakeUpUnit::Minutes);
        WakeUpView {
            on: self.on,
            every_ms: self.every_ms,
            every: self.every_ms / unit.ms(),
            unit: unit.name(),
            text: self.words().to_owned(),
            default_text: self.text.is_none(),
        }
    }

    /// The setting as the log says it.
    fn logged(&self) -> Value {
        json!({ "on": self.on, "everyMs": self.every_ms, "text": self.text })
    }
}

/// What the human asks of a project's wake-up calls: on or off, how many
/// `unit`s apart, and the words, none for the default's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWakeUp {
    pub on: bool,
    pub every: i64,
    pub unit: WakeUpUnit,
    pub text: Option<String>,
}

impl NewWakeUp {
    /// The request as the page gives it: `on`, `every`, `unit`, and `text`
    /// where the human wrote their own.
    pub fn from_json(value: &Value) -> Result<Self, LedgerError> {
        let Some(Value::Bool(on)) = value.get("on") else {
            return Err(LedgerError::refused(
                "invalid-wake-up",
                "wake-up calls are on (true) or off (false)",
            ));
        };
        let every = value.get("every").and_then(Value::as_f64).filter(|every| {
            every.fract() == 0.0 && (1.0..=9_007_199_254_740_991.0).contains(every)
        });
        let unit = value
            .get("unit")
            .and_then(Value::as_str)
            .and_then(WakeUpUnit::from_name);
        let text = match value.get("text") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.clone()),
            Some(other) => {
                return Err(LedgerError::refused(
                    "invalid-text",
                    format!(
                        "the wake-up text must be text, not {}",
                        printed(Some(other))
                    ),
                ))
            }
        };
        let (Some(every), Some(unit)) = (every, unit) else {
            return Err(LedgerError::refused(
                "invalid-interval",
                format!(
                    "a wake-up comes every whole number of minutes, hours or days, not {} {}",
                    printed(value.get("every")),
                    printed(value.get("unit"))
                ),
            ));
        };
        Ok(Self {
            on: *on,
            every: every as i64,
            unit,
            text,
        })
    }

    /// The setting this asks for, or why it cannot be: an interval under a
    /// minute (or too long to count), and words that say nothing or are too long.
    pub fn setting(&self) -> Result<WakeUpSetting, LedgerError> {
        let apart = format!("{} {}", self.every, self.unit.name());
        let every_ms = self.every.checked_mul(self.unit.ms()).ok_or_else(|| {
            LedgerError::refused(
                "invalid-interval",
                format!("{apart} is longer than a wake-up can wait"),
            )
        })?;
        if every_ms < WAKE_UP_FLOOR_MS {
            return Err(LedgerError::refused(
                "invalid-interval",
                format!("a wake-up comes once a minute at the most, not every {apart}"),
            ));
        }
        if let Some(text) = &self.text {
            require_text(text, "the wake-up text", MAX_BODY)?;
        }
        Ok(WakeUpSetting {
            on: self.on,
            every_ms,
            // The default's words are kept as none: they follow the default.
            text: self
                .text
                .clone()
                .filter(|text| text != DEFAULT_WAKE_UP_TEXT),
        })
    }
}

/// A project's setting, or a refusal naming the id.
pub(crate) fn setting(store: &Store, project_id: i64) -> Result<WakeUpSetting, LedgerError> {
    store
        .db
        .query_row(
            "SELECT wake_up_on, wake_up_every_ms, wake_up_text FROM project WHERE id = ?",
            [project_id],
            WakeUpSetting::read,
        )
        .optional()?
        .ok_or_else(|| {
            LedgerError::refused_with("unknown-project", format!("no project {project_id}"), 404)
        })
}

/// The human sets a project's wake-up calls. A setting that is the one it
/// already was changes nothing and logs nothing, so saving the dialog again
/// does not restart the clock. A setting that turns the calls off withdraws a
/// wake-up still waiting for its turn.
pub(crate) fn set_wake_up(
    store: &mut Store,
    project_id: i64,
    request: &NewWakeUp,
) -> Result<WakeUpSetting, LedgerError> {
    let to = request.setting()?;
    store.write(|store| {
        let from = setting(store, project_id)?;
        if from == to {
            return Ok(to);
        }
        let at = store.at();
        store.db.execute(
            "UPDATE project SET wake_up_on = ?, wake_up_every_ms = ?, wake_up_text = ?, updated_at = ?
             WHERE id = ?",
            rusqlite::params![i64::from(to.on), to.every_ms, to.text, at, project_id],
        )?;
        store.log(
            project_id,
            "project.wake_up_setting",
            json!({ "from": from.logged(), "to": to.logged() }),
        )?;
        if from.on && !to.on {
            withdraw_queued(store, project_id, "wake-up calls were turned off")?;
        }
        Ok(to)
    })
}

#[cfg(test)]
mod tests;
