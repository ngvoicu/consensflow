//! What Muse says of its allowance ahead of a refusal (MUS-14), as `muse usage --json`
//! answers it (`{schemaVersion, allowance, reason, hint}`, seen on a paid account,
//! 2026-10-26) and as the wire's `usage/read` does (`{usage?}`, the schema's:
//! `SubscriptionUsage`, omitted while the host has observed nothing, which a fresh
//! one-shot of `muse serve` always has: `{}`, 2026-10-27). Both name a `window` (the
//! current block, five hours' worth) and a `weekly` one, each with `usedPercent` and
//! `resetsAtMs`. Read as Codex's windows are: the fullest decides, from 95 percent used
//! the quota is low, and a window used up is exhausted until its reset.
//!
//! An account with no plan answers `allowance: null, reason: "no_allowance"` (the account of
//! 2026-10-26, T-705): it is unavailable before any turn is refused (MUS-23), with no reset.
//!
//! The wire's refusal names its limit too: `error.quota` (`{limitKind, resetsAtMs}`, the
//! schema's `QuotaDetail`), which [`wire_quota`] reads. A reset the wire names beats any
//! scraped text.

use std::path::PathBuf;
use std::time::Duration;

use cf_base::env::Env;
use serde_json::Value;

use crate::seams::processes::{Limits, Processes, Program};
use crate::seams::Time;
use crate::shared::quota::{date, Level, Quota};

/// What the human is told of an account that has no allowance (MUS-23).
pub(crate) const NO_ALLOWANCE: &str = "Muse refused: the account has no allowance; set up billing at accountscenter.meta.com/muse_code";

/// From this much of a window used, the quota is low (Codex's).
const LOW_PERCENT: f64 = 95.0;

/// What `muse usage --json` is given to answer in, and how much it may say.
const LIMITS: Limits = Limits {
    timeout: Duration::from_secs(30),
    max_buffer: 64 * 1024,
};

/// What an answer says of the account, at `now_ms`: none where it says nothing known (the
/// wire's `{}`, a reason other than no allowance), the account unavailable where it has no
/// allowance, else the count of its fullest window. Fails where a reset no date can hold.
pub fn quota_of(answer: &Value, now_ms: i64) -> Result<Option<Quota>, String> {
    let usage = answer.get("allowance").or_else(|| answer.get("usage"));
    let Some(usage) = usage.filter(|usage| !usage.is_null()) else {
        let none = answer.get("reason").and_then(Value::as_str) == Some("no_allowance");
        #[allow(clippy::cast_precision_loss)] // A time is within 2^53 milliseconds.
        return none
            .then(|| {
                Ok(Quota::Unavailable {
                    at: Some(date(now_ms as f64)?),
                    because: NO_ALLOWANCE.to_owned(),
                })
            })
            .transpose();
    };
    let windows: Vec<&Value> = ["window", "weekly"]
        .iter()
        .filter_map(|name| usage.get(name))
        .filter(|window| window.get("usedPercent").is_some_and(Value::is_number))
        .collect();
    let used = |window: &Value| window["usedPercent"].as_f64().unwrap_or(0.0);
    // The fullest, the current window on a tie.
    let Some(fullest) = windows.iter().copied().reduce(|fullest, next| {
        if used(next) > used(fullest) {
            next
        } else {
            fullest
        }
    }) else {
        return Ok(None);
    };
    let level = match used(fullest) {
        full if full >= 100.0 => Level::Exhausted,
        close if close >= LOW_PERCENT => Level::Low,
        _ => Level::Ok,
    };
    // Used up, it is back when the last window that is used up resets.
    let resets = |window: &Value| window.get("resetsAtMs").and_then(Value::as_f64);
    let reset = if level == Level::Exhausted {
        windows
            .iter()
            .copied()
            .filter(|window| used(window) >= 100.0)
            .filter_map(resets)
            .reduce(f64::max)
    } else {
        resets(fullest)
    };
    Ok(Some(Quota::Usage {
        level,
        used_percent: match &fullest["usedPercent"] {
            Value::Number(number) => Some(number.clone()),
            _ => None,
        },
        resets_at: reset.map(date).transpose()?,
    }))
}

/// The quota a refusal of the wire says (`error.quota`, `{limitKind, resetsAtMs}`), refused
/// at `at` (an ISO time, where the record names one): exhausted until the reset it names.
/// None where it names no reset.
pub fn wire_quota(detail: &Value, at: Option<&str>) -> Result<Option<Quota>, String> {
    let Some(resets) = detail.get("resetsAtMs").and_then(Value::as_f64) else {
        return Ok(None);
    };
    Ok(Some(Quota::Exhausted {
        at: at.map(str::to_owned),
        resets_at: Some(date(resets)?),
    }))
}

/// `muse usage --json` at `executable`, with no update check and the login of `env`.
pub fn program(executable: PathBuf, env: &Env) -> Program {
    Program {
        executable,
        args: ["usage", "--json"].map(str::to_owned).to_vec(),
        cwd: None,
        env: env.clone().with_var("MUSE_NO_AUTO_UPDATE", "1"),
    }
}

/// What `muse usage --json` (`program`) says of the account: one run, no model turn, no
/// model's allowance spent. Fails where it does not answer or says no JSON.
pub async fn read(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
) -> Result<Option<Quota>, String> {
    let said = processes
        .run(program, LIMITS)
        .await
        .map_err(|failed| failed.message)?;
    let answer: Value = serde_json::from_str(said.trim())
        .map_err(|failed| format!("muse usage did not answer JSON: {failed}"))?;
    quota_of(&answer, time.wall_ms())
}

#[cfg(test)]
mod tests;
