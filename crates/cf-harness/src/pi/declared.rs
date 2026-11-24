//! Pi releases tested and the release its CLI names.

use crate::shared::pattern::compile;
use regex::Regex;
use std::sync::LazyLock;

/// Last live-tested release, as the published Supported versions page records (T-1874).
pub(crate) const NEWEST_TESTED: &str = "1.5.2";
/// Oldest tested, informational rather than an enforced minimum (T-751).
pub(crate) const OLDEST_TESTED: &str = "0.87.0";

static LINE: LazyLock<Regex> = LazyLock::new(|| compile(r"^([0-9]+\.[0-9]+\.[0-9]+)$"));

/// The one whole native version line in the CLI's answer, if there is exactly one.
pub(crate) fn release(said: &str) -> Option<&str> {
    let mut found = said
        .lines()
        .filter_map(|line| LINE.captures(line)?.get(1).map(|m| m.as_str()));
    let only = found.next()?;
    found.next().is_none().then_some(only)
}
