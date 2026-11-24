//! Codex releases tested and the release its CLI names.

use crate::shared::pattern::compile;
use regex::Regex;
use std::sync::LazyLock;

/// T-1937's 0.171.0 live checks: 8/9 on each host; the remaining assertion was the kit's.
pub(crate) const NEWEST_TESTED: &str = "0.171.0";

static LINE: LazyLock<Regex> = LazyLock::new(|| compile(r"^codex-cli ([0-9]+\.[0-9]+\.[0-9]+)$"));

/// The one whole native version line in the CLI's answer, if there is exactly one.
pub(crate) fn release(said: &str) -> Option<&str> {
    let mut found = said
        .lines()
        .filter_map(|line| LINE.captures(line)?.get(1).map(|m| m.as_str()));
    let only = found.next()?;
    found.next().is_none().then_some(only)
}
