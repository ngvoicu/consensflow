//! A project's wake-up calls as the page reads them: whether the chief is
//! nudged on a schedule, how often, and the words the nudge says.

use serde::Serialize;

/// The setting of a project's wake-up calls, as Project settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeUpView {
    /// Whether the chief is nudged.
    pub on: bool,
    /// How often, in milliseconds: what the daemon counts by.
    pub every_ms: i64,
    /// How often as the human says it: this many `unit`s, the largest unit
    /// that says it in a whole number.
    pub every: i64,
    /// `minutes`, `hours` or `days`.
    pub unit: &'static str,
    /// The words of the nudge: the default's when the human wrote none.
    pub text: String,
    /// Whether `text` is the default.
    pub default_text: bool,
}
