//! The patterns of Node's quota code, each compiled once, the first time it is
//! used, as JavaScript reads it (`shared::pattern`).

use std::sync::LazyLock;

use regex::Regex;

use crate::shared::pattern::compile;

/// `/resets?\s+in\s+((?:\d+\s*[a-z]+[\s,]*(?:and\s+)?)+)/i`: a span of time,
/// "Resets in 3hr 4min", "reset in 1 week and 2 days". Group 1 is the span.
pub(super) static SPAN: LazyLock<Regex> = LazyLock::new(|| {
    compile(r"(?i-u:resets?)\s+(?i-u:in)\s+((?:[0-9]+\s*[a-zA-Z]+[\s,]*(?:(?i-u:and)\s+)?)+)")
});

/// `/(\d+)\s*([a-z]+)/gi`: one number and its unit within a span. Groups
/// 1 and 2.
pub(super) static UNIT: LazyLock<Regex> = LazyLock::new(|| compile(r"([0-9]+)\s*([a-zA-Z]+)"));

/// A time of day, "resets 7:30pm (Europe/Bucharest)", or on a date, "resets
/// Sep 29 at 11am (Europe/Bucharest)". Groups 1 to 6: the month's name, its
/// day, the hour, the minute, `am` or `pm`, and the zone.
///
/// `/resets?\s+(?:([a-z]{3})[a-z]*\s+(\d{1,2})\s+at\s+)?(\d{1,2})(?::(\d{2}))?\s*([ap]m)\b(?:\s*\(([^)]+)\))?/i`
pub(super) static AT: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"(?i-u:resets?)\s+(?:([a-zA-Z]{3})[a-zA-Z]*\s+([0-9]{1,2})\s+(?i-u:at)\s+)?([0-9]{1,2})(?::([0-9]{2}))?\s*((?i-u:[ap]m))(?-u:\b)(?:\s*\(([^)]+)\))?",
    )
});

/// `/^(?:[^(:\n]*\()?(\d{3})\b/`: a status the text opens on, "429: …" or
/// "OpenAI API error (429): …". Group 1 is the status.
pub(super) static REFUSED: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^(?:[^(:\n]*\()?([0-9]{3})(?-u:\b)"));

/// `/out of extra usage/i`: Anthropic's refusal of a subscription whose extra
/// usage is spent, which it sends as a 400 (Pi's sessions, 2026-10).
pub(super) static SPENT: LazyLock<Regex> = LazyLock::new(|| compile(r"(?i-u:out of extra usage)"));

/// An account's billing refused (nothing resets it): Muse's `billing_error`, OpenRouter's
/// "requires more credits", Anthropic's "credit balance is too low", OpenAI's
/// `insufficient_quota`.
pub(super) static BILLING: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"(?i-u:billing_error|requires more credits|credit balance is too low|insufficient_quota)",
    )
});

/// An account's login refused (nothing resets it): Anthropic's `authentication_error`, a key
/// that is not valid, a token whose refresh a 401 refused (Pi, 2026-10).
pub(super) static CREDENTIALS: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"(?i-u:authentication_error|invalid x-api-key|invalid[_ ]api[_ ]key|refresh failed \(401\))",
    )
});

/// A provider's words for a request it refused for a moment (T-1112): Anthropic's
/// `overloaded_error` ("Overloaded"), a model "at capacity" (Codex's "Selected model is at
/// capacity"), servers "currently busy" or in "high demand", a service "temporarily
/// unavailable", a server error by its name: the load and server words of Pi's own retry
/// classifier (pi-ai's `utils/retry.js`, 2026-11), without its network and limit words. A
/// quota ("rate limit") and an account's refusal are read before these.
pub(super) static BUSY: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"(?i-u:overloaded|at capacity|server(?:s)? (?:is |are )?(?:currently )?busy|server_busy|high demand|temporarily unavailable|service unavailable|service_unavailable|internal server error|internal_server_error|bad gateway|gateway time-?out)",
    )
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_builds() {
        for pattern in [&SPAN, &UNIT, &AT, &REFUSED] {
            assert!(pattern.captures_len() > 1, "{}", pattern.as_str());
        }
        assert!(SPENT.is_match("You're OUT of extra usage."));
        assert!(BILLING.is_match("... (billing_error)"));
        assert!(CREDENTIALS.is_match("Invalid API key"));
        assert!(BUSY.is_match("Selected model is at capacity. Please try a different model."));
    }
}
