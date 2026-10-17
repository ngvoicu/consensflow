//! What is said of each plant, and of the run: one line for a plant, what
//! followed from how it ended, and the count at the end. All of it is text made
//! from what the runs wrote, so that it is held to as text.
//!
//! ```text
//! CAUGHT           parser: a flag given a value is taken (4 s) by every_list_of_words_is_read_as_node_reads_it
//! MISSED           name (12 s)
//!     running 3 tests
//!     test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
//! ```

/// How a plant ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The test it was meant for failed.
    Caught,
    /// Every run passed: nothing notices the bug.
    Missed,
    /// A run failed naming tests, none of them the one it was meant for: they are said,
    /// and it counts as not caught.
    MissedByMeant,
    /// The planted source does not build, which is no catch.
    DoesNotCompile,
    /// The build before a run that builds the native `cf` failed, and cargo did not say
    /// the source does not build (a lock or a program it could not have): no catch.
    NotBuilt,
    /// A run did not end in its time.
    Hung,
    /// The text it replaces is not as it says, or what it takes as given is not there.
    DoesNotApply,
    /// A run that would decide it fails without the plant too, twice, or cannot be
    /// started: what it says tells nothing of the plant.
    NotRun,
    /// A run that would decide it failed with the plant once and not the second time,
    /// or passed without it once and failed once: neither caught nor missed.
    Unstable,
    /// A run that could not load a module of its own: no test ran to see the plant.
    Broken,
}

impl Verdict {
    /// What is said first of a plant that ended so.
    pub fn label(self) -> &'static str {
        match self {
            Self::Caught => "CAUGHT",
            Self::Missed => "MISSED",
            Self::MissedByMeant => "MISSED-BY-MEANT",
            Self::DoesNotCompile => "DOES NOT COMPILE",
            Self::NotBuilt => "NOT BUILT",
            Self::Hung => "HUNG",
            Self::DoesNotApply => "DOES NOT APPLY",
            Self::NotRun => "NOT RUN",
            Self::Unstable => "UNSTABLE",
            Self::Broken => "BROKEN",
        }
    }
}

/// The line of a plant: its verdict (padded to the longest), its name, the
/// seconds it took, and what caught it.
pub fn line(verdict: Verdict, name: &str, seconds: u64, caught_by: &str) -> String {
    format!("{:<16} {name} ({seconds} s){caught_by}", verdict.label())
}

/// Whether the test named `name` is the one `meant`: its whole name, or the end of
/// it from a boundary of its path, `::` of a Rust test's (`m::tests::the_test`) or
/// ` › ` of the groups of a Playwright test or a Node one (`a group › the test`). `the_test` is not
/// `not_the_test` nor `the_test_too`.
pub fn is_meant(name: &str, meant: &str) -> bool {
    !meant.is_empty()
        && (name == meant
            || ["::", " › "].iter().any(|boundary| {
                name.strip_suffix(meant)
                    .is_some_and(|path| path.ends_with(boundary))
            }))
}

/// What caught a plant: the first test that failed, how many others did, and, when
/// none of them is the test it was written for (`is_meant`), that, where it names one.
pub fn caught_by(caught: &[String], meant: &str) -> String {
    let Some(first) = caught.first() else {
        return String::new();
    };
    let more = if caught.len() > 1 {
        format!(" (+{})", caught.len() - 1)
    } else {
        String::new()
    };
    let other = if meant.is_empty() || caught.iter().any(|name| is_meant(name, meant)) {
        String::new()
    } else {
        format!(", not by {meant}")
    };
    format!(" by {first}{more}{other}")
}

/// What the last run of a plant that was missed ran, to tell a test that passed
/// from none that ran: the lines in which `cargo test`, `node --test` and
/// Playwright count them.
pub fn what_ran(output: &str) -> Vec<&str> {
    output.lines().filter(|line| counts_tests(line)).collect()
}

fn counts_tests(line: &str) -> bool {
    if line.starts_with("test result:") {
        return true;
    }
    if let Some(rest) = line.strip_prefix("ℹ ") {
        return ["tests", "pass", "fail"]
            .iter()
            .any(|word| rest.starts_with(word));
    }
    // Playwright's summary: `  13 passed (3.1s)`, `  1 failed`.
    let summary = line.trim_start();
    let digits = summary.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && line.starts_with("  ") {
        let rest = &summary[digits..];
        return [" passed", " failed", " skipped", " flaky"]
            .iter()
            .any(|word| rest.starts_with(word));
    }
    // `running 3 tests` (cargo), `Running 13 tests using 1 worker` (Playwright).
    let Some(rest) = line
        .strip_prefix("running ")
        .or_else(|| line.strip_prefix("Running "))
    else {
        return false;
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && rest[digits..].starts_with(" test")
}

/// What the first test to fail said, which its name does not: the line of the
/// first error that `node --test` ends its report with, cut short (the text after
/// `failing tests:`, a line `✖ <test>` and, under it, the error); else the first
/// line of the first failure Playwright tells (`  1) <test>`, and under it the error).
pub fn first_failure(output: &str) -> Option<String> {
    node_failure(output).or_else(|| playwright_failure(output))
}

/// The first error of `node --test`'s report.
fn node_failure(output: &str) -> Option<String> {
    let report = output.split("failing tests:").nth(1)?;
    // The text after the colon is the rest of that line, which names no test.
    let mut lines = report.split('\n').skip(1).peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("✖ ") {
            continue;
        }
        let Some(said) = lines.peek().and_then(|next| next.strip_prefix("  ")) else {
            continue;
        };
        // `\w*Error`: the word the line starts with has `Error` in it.
        let word: String = said
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if word.contains("Error") {
            return Some(said.chars().take(220).collect());
        }
    }
    None
}

/// The first line under Playwright's first failure, `  1) <test>`: its error.
fn playwright_failure(output: &str) -> Option<String> {
    let mut lines = output.lines();
    lines.find(|line| line.starts_with("  1) "))?;
    let said = lines.map(str::trim).find(|line| !line.is_empty())?;
    said.contains("Error")
        .then(|| said.chars().take(220).collect())
}

/// The last `count` characters of `output`.
pub fn tail(output: &str, count: usize) -> &str {
    let skip = output.chars().count().saturating_sub(count);
    output
        .char_indices()
        .nth(skip)
        .map_or(output, |(at, _)| &output[at..])
}

/// How many plants were caught of those chosen and run here, how many were unstable,
/// and how many were for other systems and skipped, where any were.
pub fn totals(caught: usize, run: usize, unstable: usize, skipped: usize) -> String {
    let mut said = format!("{caught} of {run} plants caught");
    if unstable > 0 {
        said.push_str(&format!(", {unstable} unstable"));
    }
    if skipped > 0 {
        said.push_str(&format!(", {skipped} skipped (platform)"));
    }
    said
}

/// The line of a plant for systems other than `here`, which is not run: neither caught
/// nor missed.
pub fn skipped(name: &str, platforms: &[String], here: &str) -> String {
    format!(
        "{:<16} {name}: for {}, and this is {here}",
        "SKIPPED (platform)",
        platforms.join(", ")
    )
}

#[cfg(test)]
mod tests;
