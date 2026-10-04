//! A harness's quota as its own record says it (`hosts/lib/quota.js`), in
//! the dispatcher's terms. Codex reports its usage ahead of time; the
//! others say so only once a request is refused (a 429, or a 402 for spent
//! credit), so for them the daemon learns at the first refusal.

use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quota_is_written_as_node_wrote_it() {
        let refused = Quota::Exhausted {
            at: Some("2026-09-19T10:00:00.000Z".to_owned()),
            resets_at: None,
        };
        assert_eq!(
            serde_json::to_string(&refused).unwrap(),
            r#"{"state":"exhausted","at":"2026-09-19T10:00:00.000Z","resetsAt":null}"#
        );
        let usage = Quota::Usage {
            level: Level::Low,
            used_percent: Some(serde_json::Number::from(97)),
            resets_at: Some("2026-09-26T11:49:53.000Z".to_owned()),
        };
        assert_eq!(
            serde_json::to_string(&usage).unwrap(),
            r#"{"state":"low","usedPercent":97,"resetsAt":"2026-09-26T11:49:53.000Z"}"#
        );
    }
}
