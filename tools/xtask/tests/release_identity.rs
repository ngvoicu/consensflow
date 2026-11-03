//! The release's bundle identifier, `dev.ngvoicu.consensflow`, is built by GitHub's
//! release workflows and by nothing else (T-903).
//!
//! macOS keeps what a person allows an app (Screen & System Audio Recording,
//! say) for the app's bundle identifier, and for an app signed ad hoc by nothing
//! but that one binary's hash. A build made on a developer's Mac that carried the
//! release's identifier and was allowed anything left a grant the installed,
//! Developer ID signed app could never satisfy: System Settings showed
//! ConsensFlow on, and ConsensFlow read the permission as missing. So:
//!
//! - the configuration every build starts from names another identifier;
//! - the release's own is a file by itself, which only the two workflows that
//!   build the release name;
//! - the step that prepares every `tauri build` and `tauri dev` refuses the
//!   release's identifier anywhere but on GitHub's runner.
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{checkout, err, xtask};
use serde_json::Value;

/// The release's bundle identifier, as the app's own source has it.
fn release_identifier() -> String {
    let source = fs::read_to_string(checkout().join("app/src-tauri/src/lib.rs")).unwrap();
    let line = source
        .lines()
        .find(|line| line.starts_with("pub const PRODUCTION_IDENTIFIER"))
        .expect("lib.rs names the production identifier");
    line.split('"').nth(1).unwrap().to_owned()
}

fn config(file: &str) -> Value {
    let path = checkout().join("app/src-tauri").join(file);
    serde_json::from_str(&fs::read_to_string(&path).unwrap())
        .unwrap_or_else(|cause| panic!("{}: {cause}", path.display()))
}

/// The configurations of the app, by file name.
fn configurations() -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(checkout().join("app/src-tauri"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("tauri") && name.ends_with(".conf.json"))
        .collect();
    found.sort();
    found
}

const RELEASE_CONFIGURATION: &str = "tauri.release.conf.json";

#[test]
fn the_configuration_every_build_starts_from_is_not_the_releases() {
    let release = release_identifier();
    let default = config("tauri.conf.json");
    let identifier = default["identifier"].as_str().unwrap();
    assert_ne!(identifier, release, "tauri.conf.json carries the release");
    assert!(
        identifier.starts_with(&format!("{release}.")),
        "{identifier} is no identifier of ConsensFlow's own"
    );
}

#[test]
fn the_release_configuration_gives_the_identifier_and_nothing_else() {
    let release = config(RELEASE_CONFIGURATION);
    let keys: Vec<&String> = release
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| *key != "$schema")
        .collect();
    assert_eq!(keys, ["identifier"], "it is merged over the default");
    assert_eq!(release["identifier"], release_identifier());
}

#[test]
fn no_other_configuration_names_the_release_identifier() {
    let release = release_identifier();
    let others: Vec<String> = configurations()
        .into_iter()
        .filter(|name| name != RELEASE_CONFIGURATION)
        .collect();
    assert!(others.len() >= 3, "{others:?}");
    for name in others {
        let identifier = config(&name)
            .get("identifier")
            .and_then(Value::as_str)
            .map(str::to_owned);
        assert_ne!(identifier.as_deref(), Some(release.as_str()), "{name}");
    }
}

/// The text files of the checkout that could name a configuration or build with
/// it: sources, scripts, workflows and docs, not what is built or fetched.
fn sources() -> Vec<PathBuf> {
    fn walk(folder: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(folder).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                let skipped = [
                    ".git",
                    "node_modules",
                    "target",
                    ".cache",
                    "gen",
                    "vendor",
                    "test-results",
                    ".consensflow",
                ];
                if !skipped.contains(&name.as_str()) {
                    walk(&entry.path(), found);
                }
            } else if [
                "rs", "toml", "yml", "yaml", "json", "mjs", "js", "sh", "md", "ps1",
            ]
            .iter()
            .any(|extension| name.ends_with(&format!(".{extension}")))
            {
                found.push(entry.path());
            }
        }
    }
    let mut found = Vec::new();
    walk(&checkout(), &mut found);
    found
}

/// The files that may name the release's configuration: the two workflows that
/// build the release, the step that holds the rule, the signing that refuses a
/// bundle built without it, and the tests and plants that hold them
/// (`scripts.rs` holds that no configuration merged over the default says `build`).
const MAY_NAME_IT: [&str; 8] = [
    ".github/workflows/release.yml",
    ".github/workflows/windows-build.yml",
    "tests/plants/release/identity.toml",
    "tools/cf-release/src/sign_mac/signing.rs",
    "tools/cf-release/src/sign_mac/tests/identity.rs",
    "tools/xtask/src/identity.rs",
    "tools/xtask/tests/release_identity.rs",
    "tools/xtask/tests/scripts.rs",
];

#[test]
fn only_the_release_workflows_name_the_release_configuration() {
    let root = checkout();
    let mut named = Vec::new();
    for file in sources() {
        let text = fs::read_to_string(&file).unwrap_or_default();
        if text.contains(RELEASE_CONFIGURATION) {
            let relative = file.strip_prefix(&root).unwrap();
            named.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    named.sort();
    let mut allowed: Vec<String> = MAY_NAME_IT.iter().map(|name| (*name).to_owned()).collect();
    allowed.sort();
    assert_eq!(named, allowed, "who names {RELEASE_CONFIGURATION}");
}

#[test]
fn every_workflow_that_builds_the_app_builds_it_with_the_release_configuration() {
    let root = checkout().join(".github/workflows");
    let mut building = Vec::new();
    for entry in fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            let builds = line.contains("run build") || line.contains("tauri build");
            if !builds || line.trim_start().starts_with('#') {
                continue;
            }
            assert!(
                line.contains(&format!("--config src-tauri/{RELEASE_CONFIGURATION}")),
                "{} builds the app without the release configuration: {line}",
                path.display()
            );
            building.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    building.sort();
    assert_eq!(building, ["release.yml", "windows-build.yml"]);
}

/// The step run with `vars` set, and the release's identifier given by the
/// variable Tauri hands a build's hooks: `GITHUB_ACTIONS` and
/// `RUNNER_ENVIRONMENT` are always given, so that a test run on GitHub's own
/// runner does not read the runner's.
fn guard(config: Option<&str>, github: &str, runner: &str) -> std::process::Output {
    let nowhere = tempfile::tempdir().unwrap();
    let mut vars = vec![("GITHUB_ACTIONS", github), ("RUNNER_ENVIRONMENT", runner)];
    vars.push(("TAURI_CONFIG", config.unwrap_or("")));
    xtask(&checkout(), nowhere.path(), &vars, &["build-identity"])
}

fn release_config() -> String {
    format!(r#"{{"identifier":"{}"}}"#, release_identifier())
}

#[test]
fn a_build_that_would_carry_the_release_identifier_is_refused_off_github() {
    for (github, runner) in [
        ("false", ""),
        ("", ""),
        ("true", "self-hosted"),
        ("true", ""),
        ("false", "github-hosted"),
    ] {
        let ran = guard(Some(&release_config()), github, runner);
        let said = err(&ran);
        assert_eq!(ran.status.code(), Some(1), "{github}/{runner}: {said}");
        assert!(
            said.starts_with("build-identity: "),
            "{github}/{runner}: {said}"
        );
        assert!(said.contains(&release_identifier()), "{said}");
        assert!(said.contains("GitHub"), "{said}");
        assert!(said.contains("cargo xtask candidate"), "{said}");
    }
}

#[test]
fn the_release_identifier_is_built_on_githubs_runner() {
    let ran = guard(Some(&release_config()), "true", "github-hosted");
    assert_eq!(ran.status.code(), Some(0), "{}", err(&ran));
}

#[test]
fn another_identifier_is_built_anywhere() {
    let candidate = r#"{"identifier":"dev.ngvoicu.consensflow.candidate"}"#;
    for (github, runner) in [("false", ""), ("true", "github-hosted")] {
        let ran = guard(Some(candidate), github, runner);
        assert_eq!(ran.status.code(), Some(0), "{github}: {}", err(&ran));
    }
}

#[test]
fn a_build_with_no_override_takes_the_default_configurations_identifier() {
    let ran = guard(None, "false", "");
    assert_eq!(ran.status.code(), Some(0), "{}", err(&ran));
}

#[test]
fn the_release_identifier_among_other_overrides_is_still_refused() {
    let among = format!(
        r#"{{"bundle":{{"resources":null}},"identifier":"{}","version":"9.9.9"}}"#,
        release_identifier()
    );
    let ran = guard(Some(&among), "false", "");
    assert_eq!(ran.status.code(), Some(1), "{}", err(&ran));
}

#[test]
fn an_override_that_cannot_be_read_is_refused() {
    for unreadable in [
        "{not json",
        "[1]",
        r#"{"identifier":null}"#,
        r#"{"identifier":7}"#,
    ] {
        let ran = guard(Some(unreadable), "false", "");
        let said = err(&ran);
        assert_eq!(ran.status.code(), Some(1), "{unreadable}: {said}");
        assert!(said.contains("TAURI_CONFIG"), "{unreadable}: {said}");
    }
}
