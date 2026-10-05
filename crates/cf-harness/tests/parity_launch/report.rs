//! What became of each harness's cases, told as a table with how long each side
//! took.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::verdict::Verdict;
use crate::{Case, Planned};

/// What became of one harness's cases.
#[derive(Default)]
pub(super) struct Tally {
    pub(super) kind: String,
    pub(super) cases: usize,
    pub(super) equal: usize,
    pub(super) differ: usize,
    /// Why a case was skipped, by case; why the harness was left out.
    pub(super) skipped: Vec<(String, String)>,
    pub(super) left_out: Option<String>,
    pub(super) node: Duration,
    pub(super) rust: Duration,
    /// How many entries the CLIs made in their folders, by side.
    pub(super) owned: BTreeMap<String, (i64, i64)>,
}

impl Tally {
    /// Counts a case with how it ended: what differed is added to
    /// `differences`; what each side took, and what the CLIs made, are added
    /// up. How the case is called in the line that tells of it.
    pub(super) fn count(
        &mut self,
        case: &Case,
        verdict: Verdict,
        planned: &Planned,
        differences: &mut Vec<String>,
    ) -> &'static str {
        self.cases += 1;
        let said = match verdict {
            Verdict::Equal => {
                self.equal += 1;
                "equal"
            }
            Verdict::Skipped(reason) => {
                self.skipped.push((case.name.clone(), reason));
                "skipped"
            }
            Verdict::Differs(found) => {
                self.differ += 1;
                differences.push(format!(
                    "{} {}:\n{}",
                    case.kind,
                    case.name,
                    found.join("\n")
                ));
                "DIFFERS"
            }
        };
        if said != "skipped" {
            self.node += Duration::from_secs_f64(case.node.ms / 1000.0);
            self.rust += planned.took;
        }
        for (folder, made) in &planned.owned {
            self.owned.entry(folder.clone()).or_default().1 += made;
        }
        for (folder, made) in &case.node.owned {
            self.owned.entry(folder.clone()).or_default().0 += made;
        }
        said
    }
}

pub(super) fn millis(time: Duration) -> String {
    format!("{:.1} ms", time.as_secs_f64() * 1000.0)
}

/// What became of each harness's cases, and how long each side took.
pub(super) fn report(tallies: &[Tally]) -> String {
    let mut lines = vec![format!(
        "{:<12} {:>5} {:>5} {:>6}   {:<44} {:>13} {:>13}",
        "harness", "cases", "equal", "differ", "skipped (why)", "node's time", "rust's time"
    )];
    for tally in tallies {
        let skipped = match (&tally.left_out, tally.skipped.first()) {
            (Some(reason), _) => format!("left out ({reason})"),
            (None, Some((_, reason))) => {
                format!("{} ({})", tally.skipped.len(), clipped(reason, 36))
            }
            (None, None) => "-".to_owned(),
        };
        lines.push(format!(
            "{:<12} {:>5} {:>5} {:>6}   {:<44} {:>13} {:>13}",
            tally.kind,
            tally.cases,
            tally.equal,
            tally.differ,
            clipped(&skipped, 44),
            millis(tally.node),
            millis(tally.rust)
        ));
    }
    for tally in tallies {
        for (case, reason) in &tally.skipped {
            lines.push(format!("skipped: {} {case}: {reason}", tally.kind));
        }
        for (folder, (node, rust)) in &tally.owned {
            lines.push(format!(
                "written by the CLIs, not compared: {} {folder}: {node} entries (node), {rust} (rust)",
                tally.kind
            ));
        }
    }
    lines.push(
        "Node plans first: by the time Rust plans, the files of the CLIs are in the system's cache."
            .to_owned(),
    );
    lines.join("\n")
}

/// `text` as long as `width` at most.
fn clipped(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let kept: String = text.chars().take(width - 1).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_has_a_row_for_each_harness_and_says_what_was_skipped_and_left_out() {
        let tallies = [
            Tally {
                kind: "codex".to_owned(),
                cases: 3,
                equal: 2,
                skipped: vec![("member-fresh".to_owned(), "could not list".to_owned())],
                node: Duration::from_millis(30),
                rust: Duration::from_millis(20),
                owned: BTreeMap::from([("codex".to_owned(), (4, 5))]),
                ..Tally::default()
            },
            Tally {
                kind: "devin".to_owned(),
                left_out: Some("devin is not installed on this machine".to_owned()),
                ..Tally::default()
            },
        ];
        let table = report(&tallies);
        assert!(table.contains("1 (could not list)"), "{table}");
        assert!(
            table.contains("left out (devin is not installed on this ma…"),
            "{table}"
        );
        assert!(
            table.contains("skipped: codex member-fresh: could not list"),
            "{table}"
        );
        assert!(
            table.contains("codex codex: 4 entries (node), 5 (rust)"),
            "{table}"
        );
        assert!(
            table.contains("30.0 ms") && table.contains("20.0 ms"),
            "{table}"
        );
    }
}
