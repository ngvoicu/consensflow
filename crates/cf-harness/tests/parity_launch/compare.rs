//! Where two normalized sides differ, and how each place is said.

use std::collections::{BTreeMap, BTreeSet};

use crate::normalize::Normal;
use crate::{Change, Outcome};

/// What a plan came to, in a few words.
pub(super) fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Plan(_) => "planned".to_owned(),
        Outcome::Refused(sentence) => format!("was refused with \"{sentence}\""),
    }
}

/// How long a text may be for a difference to show it whole.
const WHOLE: usize = 200;

/// What the two sides hold where they differ, each line after `indent`: both
/// texts where both are short, none where a side holds none; else where they
/// first differ, and a stretch of each from a little before it.
fn side_by_side(node: Option<&str>, rust: Option<&str>, indent: &str) -> String {
    let long = |text: Option<&str>| text.is_some_and(|text| text.chars().count() > WHOLE);
    if !long(node) && !long(rust) {
        let said = |text: Option<&str>| {
            text.map_or_else(|| "(none)".to_owned(), |text| format!("{text:?}"))
        };
        return format!("{indent}node: {}\n{indent}rust: {}", said(node), said(rust));
    }
    let (node, rust) = (node.unwrap_or_default(), rust.unwrap_or_default());
    let at = node
        .chars()
        .zip(rust.chars())
        .take_while(|(left, right)| left == right)
        .count();
    let stretch = |text: &str| -> String {
        let stretch: String = text.chars().skip(at.saturating_sub(40)).take(120).collect();
        format!("…{stretch:?}")
    };
    format!(
        "{indent}first differ at character {at}:\n{indent}node: {}\n{indent}rust: {}",
        stretch(node),
        stretch(rust)
    )
}

/// Where two lists differ, each place said: what the node's and the rust's
/// hold there, none where a list ended. `show` is an item as text.
fn lists<T: PartialEq>(
    found: &mut Vec<String>,
    what: &str,
    node: &[T],
    rust: &[T],
    show: impl Fn(&T) -> String,
) {
    let mut places = (0..node.len().max(rust.len()))
        .filter(|&at| node.get(at) != rust.get(at))
        .map(|at| {
            let (node, rust) = (node.get(at).map(&show), rust.get(at).map(&show));
            format!(
                "  {what}[{at}]:\n{}",
                side_by_side(node.as_deref(), rust.as_deref(), "    ")
            )
        });
    found.extend(places.by_ref().take(8));
    let more = places.count();
    if more > 0 {
        found.push(format!("  {what}: {more} more places differ"));
    }
}

/// Every place the two sides' findings differ at, said.
pub(super) fn differences(node: &Normal, rust: &Normal) -> Vec<String> {
    let mut found = Vec::new();
    lists(
        &mut found,
        "setting",
        &node.setting,
        &rust.setting,
        String::clone,
    );
    match (&node.outcome, &rust.outcome) {
        (Outcome::Plan(node), Outcome::Plan(rust)) => {
            lists(&mut found, "argv", &node.argv, &rust.argv, String::clone);
            lists(&mut found, "env", &node.env, &rust.env, |(name, value)| {
                format!("{name}={value}")
            });
            lists(
                &mut found,
                "dropEnv",
                &node.drop_env,
                &rust.drop_env,
                String::clone,
            );
            if node.native_session != rust.native_session {
                found.push(format!(
                    "  nativeSession:\n{}",
                    side_by_side(
                        node.native_session.as_deref(),
                        rust.native_session.as_deref(),
                        "    "
                    )
                ));
            }
        }
        (Outcome::Refused(node), Outcome::Refused(rust)) => {
            if node != rust {
                found.push(format!(
                    "  refusal:\n{}",
                    side_by_side(Some(node), Some(rust), "    ")
                ));
            }
        }
        (node, rust) => found.push(format!(
            "  node {}, where rust {}",
            describe(node),
            describe(rust)
        )),
    }
    tree(&mut found, &node.changes, &rust.changes);
    if node.pi_hashes != rust.pi_hashes {
        found.push(format!(
            "  Pi's bundle is published under another name:\n    node: {:?}\n    rust: {:?}",
            node.pi_hashes, rust.pi_hashes
        ));
    }
    found
}

/// Where the trees the two sides changed differ, path by path.
fn tree(found: &mut Vec<String>, node: &[Change], rust: &[Change]) {
    let by_path = |changes: &'_ [Change]| -> BTreeMap<String, Change> {
        changes
            .iter()
            .map(|change| (change.path.clone(), change.clone()))
            .collect()
    };
    let (node, rust) = (by_path(node), by_path(rust));
    let paths: BTreeSet<&String> = node.keys().chain(rust.keys()).collect();
    for path in paths {
        match (node.get(path), rust.get(path)) {
            (Some(node), Some(rust)) if node != rust => {
                found.push(format!("  {path}:{}", how_they_differ(node, rust)));
            }
            (Some(_), None) => found.push(format!("  {path}: only node changed it")),
            (None, Some(_)) => found.push(format!("  {path}: only rust changed it")),
            _ => {}
        }
    }
}

/// What differs of a path both sides changed: its kind, its mode, its text.
fn how_they_differ(node: &Change, rust: &Change) -> String {
    let mut how = String::new();
    if node.kind != rust.kind {
        how += &format!("\n    kind: node {}, rust {}", node.kind, rust.kind);
    }
    if node.mode != rust.mode {
        how += &format!(
            "\n    mode: node {}, rust {}",
            octal(node.mode),
            octal(rust.mode)
        );
    }
    for (what, node, rust) in [
        ("text", &node.text, &rust.text),
        ("bytes", &node.bytes, &rust.bytes),
        ("target", &node.target, &rust.target),
    ] {
        if node != rust {
            how += &format!(
                "\n    {what}:\n{}",
                side_by_side(node.as_deref(), rust.as_deref(), "      ")
            );
        }
    }
    how
}

fn octal(mode: Option<u32>) -> String {
    mode.map_or_else(|| "none".to_owned(), |mode| format!("{mode:o}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_places_that_differ_are_said_eight_and_counted() {
        let node: Vec<String> = (0..12).map(|at| format!("n{at}")).collect();
        let rust: Vec<String> = (0..12).map(|at| format!("r{at}")).collect();
        let mut found = Vec::new();
        lists(&mut found, "argv", &node, &rust, String::clone);
        assert_eq!(found.len(), 9);
        assert!(found[8].contains("4 more places differ"), "{found:?}");
    }

    #[test]
    fn a_long_text_is_shown_where_it_first_differs_and_a_short_one_whole() {
        let node = format!("{}A{}", "x".repeat(500), "y".repeat(500));
        let rust = format!("{}B{}", "x".repeat(500), "y".repeat(500));
        let said = side_by_side(Some(&node), Some(&rust), "  ");
        assert!(
            said.starts_with("  first differ at character 500:\n"),
            "{said}"
        );
        assert!(
            said.contains(&format!(
                "  node: …\"{}A{}\"",
                "x".repeat(40),
                "y".repeat(79)
            )),
            "{said}"
        );
        assert!(
            said.contains(&format!("  rust: …\"{}B", "x".repeat(40))),
            "{said}"
        );
        assert_eq!(
            side_by_side(Some("a"), None, ""),
            "node: \"a\"\nrust: (none)"
        );
    }
}
