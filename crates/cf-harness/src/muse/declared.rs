//! Muse Code releases tested and the release its CLI or pin names.

use crate::shared::pattern::compile;
use regex::Regex;
use std::sync::LazyLock;

/// Recorded on both hosts at 3cd07bf78 (research/harness-facts-r1-muse.md).
pub(crate) const NEWEST_TESTED: &str = "1.4.23-R8977.1";

static LINE: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"^Muse Code ([0-9]+\.[0-9]+\.[0-9]+) \(([0-9]+\.[0-9]+\.[0-9]+-R[0-9]+(?:\.[0-9]+)?)\)$",
    )
});
static PIN: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^[0-9]+\.[0-9]+\.[0-9]+-R[0-9]+(?:\.[0-9]+)?$"));

/// The one whole native version line, with matching inner and outer numbers.
pub(crate) fn release(said: &str) -> Option<&str> {
    let mut found = said.lines().filter_map(|line| {
        let caps = LINE.captures(line)?;
        let outer = caps.get(1)?.as_str();
        let inner = caps.get(2)?.as_str();
        (inner.split_once("-R")?.0 == outer && super::version::Version::of(line).is_some())
            .then_some(inner)
    });
    let only = found.next()?;
    found.next().is_none().then_some(only)
}

/// A pin is a different source from CLI output: it names one build by itself.
pub(crate) fn pin_release(pin: &str) -> Option<&str> {
    (PIN.is_match(pin) && super::version::Version::of(pin).is_some()).then_some(pin)
}
