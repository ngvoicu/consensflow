//! The evals leave the config folder of their Claude Code to the kit.
//!
//! An eval starts the real Claude Code for hours, and a Claude Code that starts in the owner's
//! `~/.claude` rewrites its `settings.json`, keeps every prompt it is typed in `history.jsonl`
//! and the workspace in `~/.claude.json` (T-249, 2026-10-22). So its windows run on a throwaway
//! folder with a copy of the Keychain's login (`cf_e2e::live::login`), which the rig makes and
//! hands to the daemon and every window as `CLAUDE_CONFIG_DIR`: a variable the evals set, or
//! clear (which leaves Claude Code on the owner's config, and drops the rig's), would run a
//! window past it.

// A failure in the scaffolding is the test's.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use cf_e2e::checkout;
use regex::Regex;

/// The Rust files of the crate: its sources and its tests.
fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// What a line of code that sets or clears the variable looks like: a call that is given its
/// name, as the rig's `Config` (`var`, `unset`), a process (`env`, `env_remove`), the run's own
/// (`set_var`, `remove_var`) or the window's environment (`drop_env`, `dropEnv`) takes one. (Not a
/// test that says the rig has none: the name in a `contains`.)
const CHOOSING: &str = concat!(
    r#"(?:\.|\b)(?:var|unset|env|env_remove|set_var|remove_var|drop_env|dropEnv)\s*\(\s*"#,
    r#"(?:&?\[\s*)?"CLAUDE_CONFIG"#,
    "_DIR\"",
);

#[test]
fn no_eval_sets_or_clears_the_config_folder_of_its_claude_code() {
    let choosing = Regex::new(CHOOSING).unwrap();
    let crate_root = checkout::path("crates/cf-evals");
    let mut files = Vec::new();
    sources(&crate_root.join("src"), &mut files);
    sources(&crate_root.join("tests"), &mut files);
    assert!(
        files.len() > 50,
        "no sources found in {}",
        crate_root.display()
    );
    for file in files {
        // This file names the variable in the pattern it looks for.
        if file.ends_with("tests/claude_config.rs") {
            continue;
        }
        let text = fs::read_to_string(&file).unwrap();
        for (at, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            assert!(
                !choosing.is_match(code),
                "{}:{}: an eval leaves the config folder of its Claude Code to the kit's throwaway \
                 login (`Installed::with_claude_config`): {code}",
                file.display(),
                at + 1
            );
        }
    }
}

#[test]
fn the_rule_finds_the_calls_that_choose_the_folder_and_not_the_words_that_name_it() {
    let choosing = Regex::new(CHOOSING).unwrap();
    let name = concat!("CLAUDE_CONFIG", "_DIR");
    for chosen in [
        format!(r#"            .unset("{name}")"#),
        format!(r#"config.var("{name}", folder)"#),
        format!(r#"Command::new("claude").env("{name}", x)"#),
        format!(r#"command.env_remove( "{name}" );"#),
        format!(r#"std::env::set_var("{name}", x);"#),
        format!(r#"let dropped = drop_env(["{name}"]);"#),
    ] {
        assert!(choosing.is_match(&chosen), "{chosen}");
    }
    for named in [
        format!(r#"assert!(!said.contains("{name}"), "{{said}}");"#),
        format!(r#"let note = "{name} is the kit's";"#),
        r#"            .var("HOME", &home)"#.to_owned(),
        r#"config.unset("OTHER_VARIABLE")"#.to_owned(),
    ] {
        assert!(!choosing.is_match(&named), "{named}");
    }
}
