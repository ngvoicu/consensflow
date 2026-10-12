//! The call sites that hand over to `cargo xtask`: the npm scripts of the root's
//! `package.json` and of the app's, and the build hooks of Tauri's configuration.
//! The npm names stay for the habit, for the workflows and for whoever's scripts
//! run them, and each hands over to the one command. Here are held that each
//! hands over to the command it is named for, that every one of them names a
//! command xtask has, that every `live:*` script names a live test of
//! `crates/cf-e2e` that is there, that no script runs a file that is not there,
//! and that Tauri prepares the app through the scripts that stage it.
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use common::{checkout, err, xtask};
use serde_json::{json, Value};

/// The JSON file `relative` (written from the checkout's root with `/`).
fn json_of(relative: &str) -> Value {
    let path = relative
        .split('/')
        .fold(checkout(), |path, part| path.join(part));
    let text = fs::read_to_string(&path).unwrap();
    serde_json::from_str(&text).unwrap_or_else(|cause| panic!("{}: {cause}", path.display()))
}

/// The scripts of the `package.json` at `relative`, by name.
fn scripts(relative: &str) -> BTreeMap<String, String> {
    json_of(relative)["scripts"]
        .as_object()
        .unwrap_or_else(|| panic!("{relative} has no scripts"))
        .iter()
        .map(|(name, command)| (name.clone(), command.as_str().unwrap().to_owned()))
        .collect()
}

/// The two `package.json` files, and the folder each one's scripts run in.
const PACKAGES: [(&str, &str); 2] = [("package.json", ""), ("app/package.json", "app")];

/// The scripts of the root `package.json` that hand over to a command, with the
/// command each hands over to: the drivers and the build steps, whose names the
/// workflows, the docs and the habit call them by.
const ROOT: [(&str, &str); 27] = [
    ("build:cf", "cargo xtask build-cf"),
    ("test:app", "cargo xtask app test"),
    ("clippy:app", "cargo xtask app clippy"),
    ("clippy:windows", "cargo xtask clippy-windows"),
    ("test:daemons", "cargo xtask test daemons"),
    ("test:integration", "cargo xtask test integration"),
    ("test:clis", "cargo xtask test clis"),
    ("test:agents", "cargo xtask test agents"),
    ("load", "cargo xtask test load"),
    ("live:designer", "cargo xtask live designer"),
    ("live:approval", "cargo xtask live approval"),
    ("live:hook-status", "cargo xtask live door claude --hold 90"),
    ("bench:core", "cargo xtask live bench"),
    ("bench:records-memory", "cargo xtask bench records-memory"),
    ("eval", "cargo xtask eval"),
    ("eval:summary", "cargo xtask eval summary"),
    ("eval:windows", "cargo xtask windows-matrix"),
    ("watch", "cargo xtask watch"),
    ("windows", "cargo xtask windows"),
    ("departures", "cargo xtask departures"),
    ("smoke", "cargo xtask smoke"),
    ("smoke:updater", "cargo xtask smoke-updater"),
    ("candidate", "cargo xtask candidate"),
    ("plants:daemon", "cargo xtask plants daemon"),
    ("plants:cli", "cargo xtask plants cli"),
    ("plants:release", "cargo xtask plants release"),
    ("check:all", "cargo xtask check"),
];

#[test]
fn the_root_package_runs_each_driver_and_build_step_through_cargo_xtask() {
    let scripts = scripts("package.json");
    let held: BTreeMap<&str, Option<&str>> = ROOT
        .iter()
        .map(|(name, _)| (*name, scripts.get(*name).map(String::as_str)))
        .collect();
    let wanted: BTreeMap<&str, Option<&str>> = ROOT
        .iter()
        .map(|(name, command)| (*name, Some(*command)))
        .collect();
    assert_eq!(held, wanted);
}

#[test]
fn the_npm_check_is_the_lint_and_the_tests_which_cargo_xtask_check_runs_as_two_steps() {
    assert_eq!(
        scripts("package.json")["check"],
        "npm run lint && npm test",
        "cargo xtask check runs `npm run lint` and `npm test`, which is what this is"
    );
}

#[test]
fn the_app_package_stages_and_packs_through_cargo_xtask_and_leaves_building_to_tauri() {
    let scripts = scripts("app/package.json");
    let held: BTreeMap<&str, &str> = ["prepare-sidecar", "prepare:app", "portable", "build", "dev"]
        .into_iter()
        .map(|name| (name, scripts[name].as_str()))
        .collect();
    assert_eq!(
        held,
        BTreeMap::from([
            ("prepare-sidecar", "cargo xtask stage"),
            (
                "prepare:app",
                "npm run bundle:ui && npm run prepare-sidecar"
            ),
            ("portable", "cargo xtask portable pack"),
            ("build", "tauri build"),
            ("dev", "tauri dev"),
        ])
    );
}

#[test]
fn tauri_prepares_the_app_before_a_build_and_before_a_dev_run_which_waits_for_it() {
    let build = &json_of("app/src-tauri/tauri.conf.json")["build"];
    assert_eq!(build["beforeBuildCommand"], "npm run prepare:app");
    assert_eq!(
        build["beforeDevCommand"],
        json!({ "script": "npm run prepare:app", "wait": true })
    );
}

/// Tauri merges the candidate's and Windows' configurations over the one (JSON
/// merge patch): a `build` of theirs would replace the hook, and a candidate or a
/// Windows bundle would go unstaged.
#[test]
fn the_candidate_and_windows_configurations_are_merged_over_it_without_a_word_of_build() {
    for file in ["tauri.candidate.conf.json", "tauri.windows.conf.json"] {
        let config = json_of(&format!("app/src-tauri/{file}"));
        assert!(config.get("build").is_none(), "{file} says `build`");
    }
}

/// A typo in a script of the package.json files is found here, not by whoever runs
/// it first: each command they hand over to is one xtask has (asked for its help,
/// which starts nothing).
#[test]
fn every_npm_script_that_hands_over_to_xtask_names_a_command_it_has() {
    let nowhere = tempfile::tempdir().unwrap();
    let mut handing_over = 0;
    for (package, _) in PACKAGES {
        for (name, command) in scripts(package) {
            let Some(words) = command.strip_prefix("cargo xtask ") else {
                continue;
            };
            handing_over += 1;
            let mut args: Vec<&str> = words.split(' ').collect();
            args.push("--help");
            let ran = xtask(&checkout(), nowhere.path(), &[], &args);
            assert!(
                ran.status.success(),
                "npm run {name} is `cargo xtask {words}`, which is no command:\n{}",
                err(&ran)
            );
        }
    }
    assert!(handing_over >= ROOT.len(), "{handing_over} scripts found");
}

/// `cargo xtask live <name>` runs the test `live_<name>` of `crates/cf-e2e`
/// (a dash in the name is an underscore in the file's). A name with no test is
/// asked for help alike, so the help above does not find it.
#[test]
fn every_live_script_names_a_live_test_of_cf_e2e_that_is_there() {
    let folder = checkout().join("crates").join("cf-e2e").join("tests");
    let mut found = 0;
    for (name, command) in scripts("package.json") {
        let Some(words) = command.strip_prefix("cargo xtask live ") else {
            continue;
        };
        found += 1;
        let test = words.split(' ').next().unwrap_or_default();
        let file = folder.join(format!("live_{}.rs", test.replace('-', "_")));
        assert!(
            file.is_file(),
            "npm run {name} is `{command}`, and {} is not there",
            file.display()
        );
    }
    assert!(found >= 10, "{found} live scripts found");
}

/// The files a script names as what it runs: the words that end as a script of Node
/// does. A name with a `*` in its last part is a pattern for the files of a folder.
fn scripts_named(command: &str) -> Vec<&str> {
    command
        .split_whitespace()
        .filter(|word| {
            [".mjs", ".js", ".cjs"]
                .iter()
                .any(|end| word.ends_with(end))
        })
        .collect()
}

/// Whether `word` names a file of `folder`: the file itself, or one that a
/// pattern in its last part (`*.test.mjs`) matches.
fn names_a_file(folder: &Path, word: &str) -> bool {
    let (dir, file) = word.rsplit_once('/').unwrap_or((".", word));
    let dir: PathBuf = dir
        .split('/')
        .fold(folder.to_path_buf(), |path, part| path.join(part));
    let Some((start, end)) = file.split_once('*') else {
        return dir.join(file).is_file();
    };
    fs::read_dir(&dir).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.len() >= start.len() + end.len() && name.starts_with(start) && name.ends_with(end)
        })
    })
}

/// A script of a package run a file that was deleted: it is found here, and not
/// when someone runs it.
#[test]
fn no_npm_script_runs_a_file_that_is_not_there() {
    let mut named = 0;
    for (package, folder) in PACKAGES {
        let folder = checkout().join(folder);
        for (name, command) in scripts(package) {
            for word in scripts_named(&command) {
                named += 1;
                assert!(
                    names_a_file(&folder, word),
                    "npm run {name} ({package}) runs {word}, which is not there"
                );
            }
        }
    }
    assert!(named >= 4, "{named} script files found");
}

#[test]
fn a_script_names_the_files_it_runs_and_a_pattern_names_the_files_it_matches() {
    assert_eq!(
        scripts_named("node --test tests/*.test.mjs"),
        ["tests/*.test.mjs"]
    );
    assert_eq!(
        scripts_named(
            "npm run bundle:ui && playwright test tests/a.spec.mjs tests/b.spec.mjs --config x.json"
        ),
        ["tests/a.spec.mjs", "tests/b.spec.mjs"]
    );
    assert!(scripts_named("cargo xtask live paste").is_empty());

    let folder = tempfile::tempdir().unwrap();
    fs::create_dir(folder.path().join("tests")).unwrap();
    fs::write(folder.path().join("tests").join("a.test.mjs"), "").unwrap();
    fs::write(folder.path().join("tests").join("helpers.mjs"), "").unwrap();
    assert!(names_a_file(folder.path(), "tests/a.test.mjs"));
    assert!(names_a_file(folder.path(), "tests/*.test.mjs"));
    assert!(!names_a_file(folder.path(), "tests/gone.mjs"));
    assert!(!names_a_file(folder.path(), "tests/*.spec.mjs"));
    assert!(!names_a_file(folder.path(), "elsewhere/*.test.mjs"));
    // A pattern's two ends do not overlap: the file is not made of the one name twice.
    assert!(!names_a_file(folder.path(), "tests/helpers*helpers.mjs"));
}
