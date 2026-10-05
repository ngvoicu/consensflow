//! Which Devin records complete worker replies (`supportedDevinVersion`,
//! `src/devin-install.js`), judged by what it says to `--version`.

use std::sync::LazyLock;

use cf_base::js;
use regex::Regex;

use crate::shared::pattern::compile;

/// The oldest Devin that records complete worker replies.
const MINIMUM: [u32; 3] = [3000, 10, 21];

/// A version in what Devin says: `/\b(\d+)\.(\d+)\.(\d+)\b/`, the first.
static VERSION: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?-u:\b)([0-9]+)\.([0-9]+)\.([0-9]+)(?-u:\b)"));

/// Whether Devin of `output`, what it says to `--version`, records complete
/// worker replies: its first three dotted numbers are the minimum's or
/// higher, compared a number at a time.
pub(crate) fn supported(output: &str) -> bool {
    let Some(found) = VERSION.captures(output) else {
        return false;
    };
    for (at, minimum) in MINIMUM.into_iter().enumerate() {
        let (part, minimum) = (js::number(&found[at + 1]), f64::from(minimum));
        if part != minimum {
            return part > minimum;
        }
    }
    true
}

/// The oldest Devin that records complete worker replies, as its version is
/// written (`DEVIN_MINIMUM_VERSION`).
pub(crate) fn minimum() -> String {
    MINIMUM.map(|part| part.to_string()).join(".")
}

/// What a launch is refused with when Devin is older than that.
pub(super) fn required() -> String {
    format!(
        "Devin {} or newer is required for complete worker replies. Update Devin before opening this pane.",
        minimum()
    )
}

#[cfg(test)]
mod tests;
