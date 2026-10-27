//! A script that says how a command ended reads `$?` of that command and of no other. In
//! `echo "$(date +%T) ended with $?"` the status is the `date`'s: the command substitution before it
//! has run, and has reset `$?` to its own (the status of a second run of `windows-linger.sh` was
//! always 0 for it, and so was that of `windows-silent.sh`'s first version, T-804). So every shell
//! script of the repository keeps the status in a variable first (`status=$?`), and a line that
//! reads `$?` after a command substitution of the same command fails here.
#![allow(clippy::disallowed_methods, clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::Path;

use common::checkout;

/// The folders no script of the repository is in: what is fetched and what is built.
const NOT_THE_REPOSITORYS: [&str; 5] = [".git", "node_modules", "target", ".cache", "resources"];

/// Every shell script under `folder` (a link is not followed).
fn scripts(folder: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(folder).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() && !NOT_THE_REPOSITORYS.contains(&name.as_str()) {
            scripts(&entry.path(), found);
        } else if kind.is_file() && name.ends_with(".sh") {
            found.push(entry.path());
        }
    }
}

/// Whether `line` reads `$?` where a command substitution of the same command (`$(…)` or a
/// backquote, before it in the line and since the last `;`, `&&`, `||` or `|`) has run first.
fn reads_the_status_after_a_substitution(line: &str) -> bool {
    let line = line.trim();
    if line.starts_with('#') {
        return false;
    }
    let mut from = 0;
    while let Some(at) = line[from..].find("$?").map(|found| found + from) {
        let before = &line[..at];
        let start = [";", "&&", "||", "|"]
            .iter()
            .filter_map(|cut| before.rfind(cut).map(|found| found + cut.len()))
            .max()
            .unwrap_or(0);
        let command = &before[start..];
        if command.contains("$(") || command.contains('`') {
            return true;
        }
        from = at + 2;
    }
    false
}

#[test]
fn no_script_of_the_repository_reads_a_status_after_a_command_substitution() {
    let mut found = Vec::new();
    scripts(&checkout(), &mut found);
    assert!(
        found.len() >= 3,
        "the scripts of the repository were not found: {found:?}"
    );
    let mut wrong = Vec::new();
    for path in found {
        let text = fs::read_to_string(&path).unwrap();
        for (at, line) in text.lines().enumerate() {
            if reads_the_status_after_a_substitution(line) {
                wrong.push(format!("{}:{}: {}", path.display(), at + 1, line.trim()));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "`$?` here is the status of the substitution before it: keep the status in a variable first (`status=$?`):\n{}",
        wrong.join("\n")
    );
}

#[test]
fn the_rule_knows_a_status_read_after_a_substitution_from_one_that_is_not() {
    for line in [
        r#"echo "$(date +%T) first run ended with $?" >> "$log""#,
        r#"echo "$(date +%T) second run ended with $?: $(grep -m1 busy "$f")""#,
        "echo `date` $?",
        r#"echo "$(date) ended with $?""#,
    ] {
        assert!(reads_the_status_after_a_substitution(line), "{line}");
    }
    for line in [
        r#"status=$?"#,
        r#"x=$(foo); echo "ended with $?""#,
        r#"foo || echo "$?""#,
        r#"if ! out=$(foo) && [ -n "$out" ]; then echo $?; fi"#,
        r#"echo "$(date +%T) the second run ended with $status""#,
        r#"# echo "$(date) $?""#,
    ] {
        assert!(!reads_the_status_after_a_substitution(line), "{line}");
    }
}
