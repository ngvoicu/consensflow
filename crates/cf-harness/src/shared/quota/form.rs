//! The closed form of an account hold (T-2353): which of billing or login a refusal was, the
//! status and the provider error type it carried, each as a literal of [`cf_proto::hold`]'s
//! lists and never a piece of the harness's text. The readers' `because` is only looked at
//! here, never kept: what is written is the list's own literal for what matched.

use std::sync::LazyLock;

use cf_proto::hold::{HoldForm, BILLING_CODES, LOGIN_CODES};
use regex::Regex;

use super::patterns::REFUSED;
use crate::muse::record::LOGIN_REFUSED as MUSE_LOGIN_REFUSED;
use crate::muse::record::NO_ALLOWANCE as MUSE_NO_ALLOWANCE;
use crate::shared::pattern::compile;

/// The provider error types the readers already match on (`patterns`: `BILLING`, `CREDENTIALS`),
/// each with the literal of the list that stands for it.
#[allow(clippy::expect_used)]
static CODE_WORDS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        (
            "authentication_error",
            r"(?i-u:(?-u:\b)authentication_error(?-u:\b))",
        ),
        ("invalid_grant", r"(?i-u:(?-u:\b)invalid_grant(?-u:\b))"),
        (
            "invalid_api_key",
            r"(?i-u:(?-u:\b)invalid[_ ]api[_ ]key(?-u:\b))",
        ),
        (
            "invalid_x_api_key",
            r"(?i-u:(?-u:\b)invalid x-api-key(?-u:\b))",
        ),
        ("refresh_failed", r"(?i-u:refresh failed \(401\))"),
        (
            "session_expired",
            r"(?i-u:session expired \(status 40[13]\))",
        ),
        ("billing_error", r"(?i-u:(?-u:\b)billing_error(?-u:\b))"),
        (
            "insufficient_quota",
            r"(?i-u:(?-u:\b)insufficient_quota(?-u:\b))",
        ),
        (
            "credit_balance_too_low",
            r"(?i-u:(?-u:\b)credit balance is too low(?-u:\b))",
        ),
        (
            "requires_more_credits",
            r"(?i-u:(?-u:\b)requires more credits(?-u:\b))",
        ),
    ]
    .into_iter()
    .map(|(literal, pattern)| (literal, compile(pattern)))
    .collect()
});

/// Which of `billing` or `login` a reader's reason leads with, and the words after the lead:
/// ConsensFlow's own fixed lead, never the harness's text. None for a reason that is no account's
/// refusal.
fn refusal_lead(because: &str) -> Option<(&'static str, &str)> {
    if let Some(rest) = because.strip_prefix("its billing was refused") {
        Some(("billing", rest))
    } else if let Some(rest) = because.strip_prefix("its login was refused") {
        Some(("login", rest))
    } else if because == MUSE_NO_ALLOWANCE {
        Some(("billing", ""))
    } else if because == MUSE_LOGIN_REFUSED {
        Some(("login", ""))
    } else {
        None
    }
}

/// Whether an account hold's reason is a refused login (and not billing): read from the reason's
/// own lead, as [`hold_form`] reads its kind, never from the words of the harness. The engine
/// tries such a refusal again before it holds the member.
pub fn login_refused(because: &str) -> bool {
    refusal_lead(because).is_some_and(|(kind, _)| kind == "login")
}

/// The form of an account hold whose reader's reason is `because`: its kind from the reason's
/// own lead (`its billing was refused`, `its login was refused`, and Muse's two fixed
/// sentences), the status from the status the words open on, the code from the first of the
/// kind's list the words name. None for a reason that is no account's refusal.
pub fn hold_form(because: &str) -> Option<HoldForm> {
    let (kind, words) = refusal_lead(because)?;
    let words = words.strip_prefix(": ").unwrap_or(words);
    let status = REFUSED
        .captures(words)
        .and_then(|found| found[1].parse::<u64>().ok());
    let list: &[&str] = if kind == "login" {
        &LOGIN_CODES
    } else {
        &BILLING_CODES
    };
    let code = CODE_WORDS
        .iter()
        .find(|(literal, pattern)| list.contains(literal) && pattern.is_match(words))
        .map(|(literal, _)| *literal);
    HoldForm::checked(kind, status, code)
}

#[cfg(test)]
mod tests;
