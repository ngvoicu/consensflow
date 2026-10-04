//! A harness's quota as its own record says it (`hosts/lib/quota.js`), in
//! the dispatcher's terms. Codex reports its usage ahead of time; the
//! others say so only once a request is refused (a 429, or a 402 for spent
//! credit), so for them the daemon learns at the first refusal.
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

use cf_base::js;
use cf_base::time::{iso, time_clip};
use jiff::tz::TimeZone;
use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;

use patterns::REFUSED;

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
}

/// How much of a window is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    /// Past 95 percent.
    Low,
    Exhausted,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Low => "low",
            Level::Exhausted => "exhausted",
        }
    }
}

impl Serialize for Quota {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
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
        }
        map.end()
    }
}

/// The statuses that mean an account takes no more for now: a rate or usage
/// limit (429), or its credit spent (402, OpenRouter's "requires more
/// credits").
const QUOTA_STATUSES: [f64; 2] = [402.0, 429.0];

/// Whether a provider's status, as `Number` reads it (`js::to_number`), is a
/// refusal for quota.
pub(crate) fn quota_status(status: f64) -> bool {
    QUOTA_STATUSES.contains(&status)
}

/// Whether a provider's error text opens on a quota status: "429: …", or
/// "OpenAI API error (429): …", Pi's two shapes (seen on Pi, 2026-09).
pub(crate) fn refused_for_quota(text: &str) -> bool {
    REFUSED
        .captures(text)
        .is_some_and(|found| quota_status(js::number(&found[1])))
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
