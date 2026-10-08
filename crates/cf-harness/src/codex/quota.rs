//! Codex's quota as its rollout reports it, ahead of time: the `rate_limits` of
//! a `token_count` event, where the fullest window decides.

use serde_json::Value;

use crate::shared::quota::{date, Level, Quota};

/// From this much of a window used, the quota is low.
const LOW_PERCENT: f64 = 95.0;

/// The quota `limits` says: the fuller of its two windows (the primary on a
/// tie) gives how much is used and when it resets; a limit reached is
/// exhausted whatever the windows say. A reset no date can hold fails, as
/// `toISOString` throws on it.
pub(crate) fn codex_quota(limits: &Value) -> Result<Quota, String> {
    let fullest = [limits.get("primary"), limits.get("secondary")]
        .into_iter()
        .flatten()
        .filter_map(|window| Some((window, used(window)?)))
        .reduce(|fullest, next| if next.1 > fullest.1 { next } else { fullest });
    let reached = !matches!(
        limits.get("rate_limit_reached_type"),
        None | Some(Value::Null)
    );
    let used_percent = fullest.and_then(|(window, _)| match window.get("used_percent") {
        Some(Value::Number(number)) => Some(number.clone()),
        _ => None,
    });
    let level = match fullest {
        _ if reached => Level::Exhausted,
        Some((_, used)) if used >= LOW_PERCENT => Level::Low,
        _ => Level::Ok,
    };
    let resets_at = match fullest.and_then(|(window, _)| window.get("resets_at")) {
        Some(Value::Number(seconds)) => Some(date(seconds.as_f64().unwrap_or(f64::NAN) * 1000.0)?),
        _ => None,
    };
    Ok(Quota::Usage {
        level,
        used_percent,
        resets_at,
    })
}

/// How much of `window` is used, when it says so with a number.
fn used(window: &Value) -> Option<f64> {
    window.get("used_percent")?.as_f64()
}

#[cfg(test)]
mod tests;
