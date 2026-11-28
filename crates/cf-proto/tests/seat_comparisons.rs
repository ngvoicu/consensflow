//! The source test of the seat: no place of the product compares a participant's role with
//! the chief's (or a lead's) word. A place asks `Seat`'s questions (`cf_proto::seat`): is it
//! the chief, does it coordinate, is it a member, does it give tasks. A new comparison fails
//! this test, so the next person asks a question instead of adding a literal.
//!
//! What it reads: the product's Rust sources, with test modules, test support, the
//! end-to-end crate, frozen migrations and `Seat` itself left out, and for each file the count of its
//! comparisons, in Rust (`role == "chief"`, `matches!(role, "chief" | "human")`) and in SQL
//! (`role = 'chief'`, `role NOT IN ('human', 'chief')`) and in the page's `app/ui/core`
//! (`role === 'chief'`; `seat.js` holds its questions). A file whose count differs from its
//! entry below fails: a new comparison, or one converted whose entry is still here.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// File (from the repository's root), the comparisons it holds, and why it still has them.
/// "step 5" and "step 10" are the steps of the leads build that convert them (the leads
/// contracts' section 6); "own" is the chief's seat itself, asked in SQL.
const ALLOWED: &[(&str, usize, &str)] = &[
    // 905: the held answer's recipient that left; 907 and 1169: the chief that is current.
    ("crates/cf-ledger/src/holding.rs", 3, "step 10: the heir"),
    // The reminder's wait: the coordinator that acted, not any coordinator.
    (
        "crates/cf-ledger/src/waiting_questions.rs",
        1,
        "step 10: the heir",
    ),
    // The stranded task's decider, and the decider told of a resume that failed.
    (
        "crates/cf-ledger/src/tasks/finishing.rs",
        1,
        "step 5: the decider",
    ),
    (
        "crates/cf-engine/src/resume_wait.rs",
        1,
        "step 5: the decider",
    ),
    ("crates/cf-engine/src/stalls.rs", 1, "step 5: the decider"),
    // `add_lead` writes the role it makes and counts the leads present against the limit.
    (
        "crates/cf-ledger/src/leads.rs",
        2,
        "own: the lead's own module makes and counts the role",
    ),
    // The meeting and the wake-up calls are the chief's alone.
    (
        "crates/cf-ledger/src/meeting.rs",
        1,
        "own: the meeting is the chief's",
    ),
    (
        "crates/cf-ledger/src/wake_ups/waiting.rs",
        1,
        "own: the wake-up is the chief's",
    ),
    (
        "crates/cf-ledger/src/wake_ups/due.rs",
        1,
        "own: the wake-up is the chief's",
    ),
    // The helpers of migrations 0031 and 0033, which read the table as it was.
    (
        "crates/cf-ledger/src/handles.rs",
        2,
        "frozen: a migration's helper",
    ),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The product's sources: not tests, test support, the end-to-end crate or migrations.
fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            let skipped = matches!(
                name.as_str(),
                "tests" | "testing" | "migrations" | "target" | "cf-e2e" | "node_modules"
            );
            if !skipped {
                sources(&path, found);
            }
        } else if name.ends_with(".rs")
            && name != "tests.rs"
            && !name.ends_with("_tests.rs")
            && !path.ends_with("cf-proto/src/seat.rs")
        {
            found.push(path);
        }
    }
}

/// The file's text without its test-only items: a `#[cfg(test)]` at the margin and the item
/// after it, to its closing brace at the margin or its semicolon.
fn without_tests(text: &str) -> String {
    let mut kept = String::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim_end() != "#[cfg(test)]" {
            kept.push_str(line);
            kept.push('\n');
            continue;
        }
        // The item it marks: one line ending in a semicolon, or up to its closing brace.
        let mut item = lines.next().unwrap_or_default();
        while item.starts_with("#[") {
            item = lines.next().unwrap_or_default();
        }
        if !item.ends_with(';') {
            for rest in lines.by_ref() {
                if rest == "}" {
                    break;
                }
            }
        }
    }
    kept
}

/// The page's comparisons: `role === 'chief'`, `['chief', 'human'].includes(...)`, and a
/// task's assignee compared with the chief's handle.
fn page_comparisons(found: &mut BTreeMap<String, usize>) {
    let page = Regex::new(concat!(
        r"\brole\s*[!=]==\s*'(?:chief|lead)'",
        r"|\['chief',\s*'human'\]|\['human',\s*'chief'\]",
        r"|\bassignee\s*[!=]==\s*'chief'",
    ))
    .unwrap();
    let root = root();
    let Ok(entries) = fs::read_dir(root.join("app/ui/core")) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".js") || name == "seat.js" {
            continue;
        }
        let count = page
            .find_iter(&fs::read_to_string(entry.path()).unwrap())
            .count();
        if count > 0 {
            found.insert(format!("app/ui/core/{name}"), count);
        }
    }
}

fn comparisons() -> BTreeMap<String, usize> {
    let rust = Regex::new(concat!(
        r#"(?s)\brole(?:\(\))?(?:\.as_str\(\))?\s*[!=]=\s*"(?:chief|lead)""#,
        r#"|"chief"\s*\|\s*"human"|"human"\s*\|\s*"chief""#,
        r#"|\bhandle\s*[!=]=\s*"chief""#,
    ))
    .unwrap();
    let sql = Regex::new(concat!(
        r"(?i)\brole\s*(?:=|<>|!=)\s*'(?:chief|lead)'",
        r"|\brole\s+(?:NOT\s+)?IN\s*\([^)]*'(?:chief|lead)'",
    ))
    .unwrap();
    let mut files = Vec::new();
    let root = root();
    for dir in ["crates", "app/src-tauri/src", "tools"] {
        sources(&root.join(dir), &mut files);
    }
    let mut found = BTreeMap::new();
    for file in files {
        let text = without_tests(&fs::read_to_string(&file).unwrap());
        let count = rust.find_iter(&text).count() + sql.find_iter(&text).count();
        if count > 0 {
            let path = file.canonicalize().unwrap();
            let relative = path
                .strip_prefix(root.canonicalize().unwrap())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            found.insert(relative, count);
        }
    }
    page_comparisons(&mut found);
    found
}

#[test]
fn no_place_compares_a_role_with_the_chiefs_but_the_ones_named() {
    let found = comparisons();
    let mut told = Vec::new();
    for (file, count) in &found {
        let allowed = ALLOWED
            .iter()
            .find(|(name, _, _)| name == file)
            .map_or(0, |(_, count, _)| *count);
        if *count != allowed {
            told.push(format!("{file}: {count} comparisons, {allowed} allowed"));
        }
    }
    for (file, count, _) in ALLOWED {
        if !found.contains_key(*file) {
            told.push(format!(
                "{file}: none left, {count} allowed: take its entry out"
            ));
        }
    }
    assert!(
        told.is_empty(),
        "ask a question of `cf_proto::seat::Seat` (is_chief, coordinates, is_member, \
         gives_tasks) instead of comparing a role with a word:\n{}",
        told.join("\n")
    );
}
