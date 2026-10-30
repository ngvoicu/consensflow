//! A harness's quota as its own record says it, in the dispatcher's terms.
//! Codex reports its usage ahead of time; the others say so only once a request
//! is refused (a 429), so for them the daemon learns at the first refusal. An
//! account that refuses for what no wait cures, its billing or its login (a 402,
//! a 401), is no quota: it is [`Quota::Unavailable`], with no reset, whichever
//! harness's record says it (`account_refusal`, MUS-23).
//!
//! A reset that names a time of day and a zone is read in jiff's copy of the
//! time zone database, bundled into the program (`tzdb-bundle-always`) and not
//! read from the system's files: the same data on every platform, and no file
//! or environment variable (`TZDIR`) behind the caller's back. jiff serves only
//! as that database: it finds a zone, tells the offset at an instant, and the
//! date an instant has in a zone. A zone is named as `Intl` names one, ICU's
//! names and offsets too (the `zone` module). What is kept from Node on
//! purpose:
//! - the data is tzdata 2026c where Node's ICU holds 2026a, so a zone whose
//!   rules changed between the two is read by the newer;
//! - a SystemV zone with daylight time reads it by one rule every year,
//!   where ICU's history differs before 1902 and in 1974 and 1975 (`zone`);
//! - an instant past the years jiff holds, 9999 either way, is none to it, so
//!   a reset at a time of day, asked of one, names no reset where Node would
//!   give one.

mod patterns;
mod reset;
#[cfg(test)]
mod tests;
mod zone;

pub(crate) use zone::time_zone;

use cf_base::js;
use cf_base::time::{iso, time_clip};
use jiff::tz::TimeZone;
use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;

use patterns::{BILLING, CREDENTIALS, REFUSED, SPENT};

/// What a record says of the account's quota.
#[derive(Debug, Clone, PartialEq)]
pub enum Quota {
    /// A request refused for quota (`exhaustedQuota`), or OpenCode waiting
    /// out a spent one: when it happened, if the record says, and when the
    /// quota comes back, if the refusal named it.
    Exhausted {
        at: Option<String>,
        resets_at: Option<String>,
    },
    /// Codex's own count (`codexQuota`): how full the fullest window is, the
    /// number as the rollout wrote it, and when that window resets.
    Usage {
        level: Level,
        used_percent: Option<serde_json::Number>,
        resets_at: Option<String>,
    },
    /// An account that refused for what no wait cures (its billing, its login):
    /// when it happened, if the record says, and why in plain words. It has no reset,
    /// only the human's fix, so nothing is retried but at their word (MUS-23).
    Unavailable { at: Option<String>, because: String },
    /// A window whose harness moved its model on its own (MUS-24): the account is well,
    /// and the window is not to be written to until the human says it may go on. Held as
    /// an unavailable account is, and released by the human's word for that member alone,
    /// never by a teammate's turn or an allowance (T-816).
    Moved { at: Option<String>, because: String },
}

/// What an account refused for that is no usage limit: nothing resets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Its billing: no plan, spent credit, a billing hold (a 402).
    Billing,
    /// Its login: an expired or revoked one, a key that is not valid (a 401).
    Credentials,
}

/// How much of a window is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    /// Past 95 percent.
    Low,
    Exhausted,
}

impl Quota {
    /// How spent the quota is (JavaScript's `quota.state`): a refusal is
    /// exhausted, a count says its own level.
    pub fn level(&self) -> Level {
        match self {
            Quota::Exhausted { .. } | Quota::Unavailable { .. } | Quota::Moved { .. } => {
                Level::Exhausted
            }
            Quota::Usage { level, .. } => *level,
        }
    }

    /// When the refusal happened, where its record said (`quota.at`); a
    /// count has none.
    pub fn at(&self) -> Option<&str> {
        match self {
            Quota::Exhausted { at, .. }
            | Quota::Unavailable { at, .. }
            | Quota::Moved { at, .. } => at.as_deref(),
            Quota::Usage { .. } => None,
        }
    }

    /// When the quota comes back, where its harness named it (`quota.resetsAt`).
    pub fn resets_at(&self) -> Option<&str> {
        match self {
            Quota::Exhausted { resets_at, .. } | Quota::Usage { resets_at, .. } => {
                resets_at.as_deref()
            }
            Quota::Unavailable { .. } | Quota::Moved { .. } => None,
        }
    }

    /// Whether the account is unavailable: refused for what only its human cures.
    pub fn unavailable(&self) -> bool {
        matches!(self, Quota::Unavailable { .. })
    }

    /// Whether the window's model was moved by its harness: held for the human, as
    /// an unavailable account is, and released by their word for that member alone.
    pub fn moved(&self) -> bool {
        matches!(self, Quota::Moved { .. })
    }

    /// Whether no time and no teammate cures it: an unavailable account, or a moved
    /// model. It names no reset.
    pub fn held_for_the_human(&self) -> bool {
        self.unavailable() || self.moved()
    }
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Low => "low",
            Level::Exhausted => "exhausted",
        }
    }
}

impl Serialize for Quota {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3 + usize::from(self.held_for_the_human())))?;
        match self {
            Quota::Exhausted { at, resets_at } => {
                map.serialize_entry("state", "exhausted")?;
                map.serialize_entry("at", at)?;
                map.serialize_entry("resetsAt", resets_at)?;
            }
            Quota::Usage {
                level,
                used_percent,
                resets_at,
            } => {
                map.serialize_entry("state", level.as_str())?;
                map.serialize_entry("usedPercent", used_percent)?;
                map.serialize_entry("resetsAt", resets_at)?;
            }
            Quota::Unavailable { at, because } | Quota::Moved { at, because } => {
                map.serialize_entry("state", if self.moved() { "moved" } else { "unavailable" })?;
                map.serialize_entry("at", at)?;
                map.serialize_entry("resetsAt", &Option::<&str>::None)?;
                map.serialize_entry("because", because)?;
            }
        }
        map.end()
    }
}

/// The statuses that mean an account takes no more for now: a rate or usage
/// limit (429), which resets. Its credit spent (402, OpenRouter's "requires more
/// credits") is no limit that resets: [`account_refusal`] reads it.
const QUOTA_STATUSES: [f64; 1] = [429.0];

/// The status of a refusal for billing, and of one for login.
const BILLING_STATUS: f64 = 402.0;
const CREDENTIALS_STATUS: f64 = 401.0;

/// Whether a provider's status, as `Number` reads it (`js::to_number`), is a
/// refusal for quota.
pub(crate) fn quota_status(status: f64) -> bool {
    QUOTA_STATUSES.contains(&status)
}

/// Whether a provider's error text opens on a quota status: "429: …", or
/// "OpenAI API error (429): …", Pi's two shapes (seen on Pi, 2026-09); or says
/// Anthropic's extra usage is spent, whatever its status (a 400, 2026-10).
pub(crate) fn refused_for_quota(text: &str) -> bool {
    REFUSED
        .captures(text)
        .is_some_and(|found| quota_status(js::number(&found[1])))
        || spent_extra_usage(text)
}

/// Whether a provider's words say Anthropic's extra usage is spent, which it
/// answers with a 400 (Pi's sessions on this Mac, 2026-10): a refusal for quota
/// whatever the status, in any harness's words.
pub(crate) fn spent_extra_usage(text: &str) -> bool {
    SPENT.is_match(text)
}

/// What a provider's refusal says the account refused for that no wait cures: its
/// billing (a 402, "requires more credits", Muse's `billing_error`, Anthropic's "credit
/// balance is too low", an `insufficient_quota`), or its login (a 401, an
/// `authentication_error`, a token whose refresh was refused with a 401). `status` is
/// where the record names one; else the status the text opens on counts
/// (`refused_for_quota`'s two shapes). None for a limit that resets, backoff, and a
/// failure of the request itself. The same reading for every harness (MUS-23): where a
/// reader called these a quota it waited out an hour at a time, and where it called them
/// nothing the next task went to the same refused account.
pub(crate) fn account_refusal(text: &str, status: Option<f64>) -> Option<Refusal> {
    let status = status.or_else(|| REFUSED.captures(text).map(|found| js::number(&found[1])));
    match status {
        Some(status) if status == BILLING_STATUS => Some(Refusal::Billing),
        Some(status) if status == CREDENTIALS_STATUS => Some(Refusal::Credentials),
        _ if BILLING.is_match(text) => Some(Refusal::Billing),
        _ if CREDENTIALS.is_match(text) => Some(Refusal::Credentials),
        _ => None,
    }
}

/// An account that refused at `at_ms` (none when it is no number) for what only its
/// human cures, told as the harness said it (`said`, held to a line's length). Fails
/// where `toISOString` throws.
pub(crate) fn unavailable_quota(at_ms: f64, refusal: Refusal, said: &str) -> Result<Quota, String> {
    let at = if at_ms.is_finite() {
        Some(date(at_ms)?)
    } else {
        None
    };
    let what = match refusal {
        Refusal::Billing => "its billing was refused",
        Refusal::Credentials => "its login was refused",
    };
    let said: String = said.trim().chars().take(300).collect();
    Ok(Quota::Unavailable {
        at,
        because: format!("{what}: {said}"),
    })
}

/// A refused request: exhausted, with the reset the text names when it names
/// one, and when it happened (`at_ms`, none when it is no number), so the
/// daemon can tell an old refusal still in the record from a new one. A reset
/// at a time of day that names no zone is read in `local`, the machine's.
/// Fails where `toISOString` throws: for a time, or a span from it, past what
/// a date holds.
pub(crate) fn exhausted_quota(text: &str, at_ms: f64, local: &TimeZone) -> Result<Quota, String> {
    let at = if at_ms.is_finite() {
        Some(date(at_ms)?)
    } else {
        None
    };
    Ok(Quota::Exhausted {
        at,
        resets_at: reset::named_reset(text, at_ms, local)?,
    })
}

/// `new Date(ms).toISOString()`, or the failure that throws.
pub(crate) fn date(ms: f64) -> Result<String, String> {
    time_clip(ms)
        .map(iso)
        .ok_or_else(|| "Invalid time value".to_owned())
}
