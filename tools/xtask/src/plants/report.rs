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
    /// A test failed.
    Caught,
    /// Every run passed: nothing notices the bug.
    Missed,
    /// The planted source does not build, which is no catch.
    DoesNotCompile,
    /// A run did not end in its time.
    Hung,
    /// The text it replaces is not as it says, or what it takes as given is not there.
    DoesNotApply,
}

impl Verdict {
    /// What is said first of a plant that ended so.
    pub fn label(self) -> &'static str {
        match self {
            Self::Caught => "CAUGHT",
            Self::Missed => "MISSED",
            Self::DoesNotCompile => "DOES NOT COMPILE",
            Self::Hung => "HUNG",
            Self::DoesNotApply => "DOES NOT APPLY",
        }
    }
}

/// The line of a plant: its verdict (padded to the longest), its name, the
/// seconds it took, and what caught it.
pub fn line(verdict: Verdict, name: &str, seconds: u64, caught_by: &str) -> String {
    format!("{:<16} {name} ({seconds} s){caught_by}", verdict.label())
}

/// What caught a plant: the first test that failed, how many others did, and, when
/// none of them is the test it was written for, that. A test is the one meant where
/// its name holds `meant`: a Rust test's name is a path that ends with the function,
/// and what a plant says it is meant for may be the start of that name, or the words
/// a Node test is named by.
pub fn caught_by(caught: &[String], meant: &str) -> String {
    let Some(first) = caught.first() else {
        return String::new();
    };
    let more = if caught.len() > 1 {
        format!(" (+{})", caught.len() - 1)
    } else {
        String::new()
    };
    let other = if caught.iter().any(|name| name.contains(meant)) {
        String::new()
    } else {
        format!(", not by {meant}")
    };
    format!(" by {first}{more}{other}")
}

/// What the last run of a plant that was missed ran, to tell a test that passed
/// from none that ran: the lines in which `cargo test` and `node --test` count them.
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
    let Some(rest) = line.strip_prefix("running ") else {
        return false;
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && rest[digits..].starts_with(" test")
}

/// What the first test to fail said, which its name does not: the line of the
/// first error that `node --test` ends its report with, cut short. That report is
/// the text after `failing tests:`, a line `✖ <test>` and, under it, the error.
pub fn first_failure(output: &str) -> Option<String> {
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

/// The last `count` characters of `output`.
pub fn tail(output: &str, count: usize) -> &str {
    let skip = output.chars().count().saturating_sub(count);
    output
        .char_indices()
        .nth(skip)
        .map_or(output, |(at, _)| &output[at..])
}

/// How many plants were caught of those chosen.
pub fn totals(caught: usize, chosen: usize) -> String {
    format!("{caught} of {chosen} plants caught")
}

#[cfg(test)]
mod tests;
