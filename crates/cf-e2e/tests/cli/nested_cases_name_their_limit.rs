//! A test that runs a case of a test binary again, as a program of its own, holds it to what
//! that case runs (T-848). `Run::new` gives a program a verb of `cf`'s 30 s
//! (`cf_e2e::process::LIMIT`), and the rig's report case, which starts a rig, was ended at it on
//! main's CI after 3.0.6-alpha.3's integration B. So no source of cf-e2e starts a test binary
//! with `Run::new`: a nested case goes through `cf_e2e::process::case_of`, which takes the limit
//! its caller says, or the rig's own `cf_e2e::rig::nested::case_of`.

use std::fs;

use cf_e2e::checkout;

use crate::Outcome;

/// The folders whose sources are read: the crate's own, and its tests.
const LOOKED_IN: [&str; 2] = ["crates/cf-e2e/src", "crates/cf-e2e/tests"];

/// This file, whose test of the scan writes what it finds.
const THIS: &str = "crates/cf-e2e/tests/cli/nested_cases_name_their_limit.rs";

/// What the walk passes over.
const UNSOURCED: [&str; 1] = ["target"];

/// The lines of `source` (from 1) where a `Run::new(` starts the program a test binary is,
/// `current_exe()`, within its parentheses.
fn nested_without_a_limit(source: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (at, _) in source.match_indices("Run::new(") {
        let after = &source[at + "Run::new(".len()..];
        let mut depth = 1;
        let mut end = after.len();
        for (index, c) in after.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = index;
                        break;
                    }
                }
                _ => {}
            }
        }
        if after[..end].contains("current_exe()") {
            found.push(source[..at].lines().count().max(1));
        }
    }
    found
}

#[test]
fn a_run_of_the_test_binary_is_found_wherever_it_is_written() {
    let source = "let a = Run::new(cmd);\n\
                  let b = Run::new(&std::env::current_exe()?)\n    .args([\"--exact\"]);\n\
                  let c = Run::new(\n    std::env::current_exe().unwrap(),\n);\n\
                  let d = case_of(std::env::current_exe()?, \"x\", LIMIT);\n";
    assert_eq!(nested_without_a_limit(source), [2, 4]);
}

#[test]
fn no_test_of_cf_e2e_runs_a_case_again_with_a_verb_of_cfs_limit() -> Outcome {
    let mut found = Vec::new();
    for folder in LOOKED_IN {
        for file in checkout::files_below(&checkout::path(folder), &UNSOURCED)? {
            let is_rust = file.extension().is_some_and(|extension| extension == "rs");
            if !is_rust || checkout::relative(&file) == THIS || !checkout::is_plain_file(&file) {
                continue;
            }
            let source = String::from_utf8_lossy(&fs::read(&file)?).into_owned();
            for line in nested_without_a_limit(&source) {
                found.push(format!("{}:{line}", checkout::relative(&file)));
            }
        }
    }
    assert!(
        found.is_empty(),
        "these run a case of a test binary again held to a verb of cf's 30 s; give it the \
         limit of what it runs with cf_e2e::process::case_of (or the rig's \
         cf_e2e::rig::nested::case_of):\n{}",
        found.join("\n")
    );
    Ok(())
}
