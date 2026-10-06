//! What opening the app prepares, as `tests/install.test.mjs` holds Node's:
//! the five sentences of that file, in order. The first three are the
//! harnesses' discovery (`src/harnesses.js`), which `detect` is the port of
//! and holds under sentences of its own (`src/detect/tests.rs`); they are here
//! too, as the file has them. The last two are the app's own preparation
//! (`prepareApp`, `src/install.js`), which `prepare::prepare_app` is.
//!
//! Node's `addAgent` wrote the agent the test then found untouched; this
//! crate has no roster, and the file is written by hand and found as it was.
//! The launcher it checked for was in `CONSENSFLOW_BIN_DIR`, which is the
//! home's `bin` in the test's environment and which nothing reads, and is
//! looked for in the home's `bin` here.

// A test's own folders and files: a failure in them is the test's.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Component, Path, PathBuf};

use cf_base::env::Env;
use cf_harness::detect::{detect_harnesses, harness_path};
use cf_harness::prepare::prepare_app;
use cf_harness::{opencode, pi};
use cf_proto::agents::Harness;
use common::{launcher_name, Home};

#[test]
fn finds_only_the_harnesses_whose_cli_resolves() {
    let home = Home::new();
    home.stub_cli("claude");
    home.stub_cli("codex");

    let harnesses = detect_harnesses(&home.env());

    let mut ids: Vec<&str> = harnesses.iter().map(|each| each.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["claude", "codex"]);
}

#[test]
fn detection_returns_executable_identities_without_unused_global_skill_destinations() {
    let home = Home::new();
    for name in ["claude", "codex", "opencode", "pi", "devin"] {
        home.stub_cli(name);
    }

    let harnesses = detect_harnesses(&home.env());

    assert_eq!(harnesses.len(), 5);
    for harness in &harnesses {
        let json = serde_json::to_value(harness).unwrap();
        let mut keys: Vec<&String> = json.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["command", "id"]);
    }
}

/// `target` as a path from `from`, which Node's test made by changing its
/// working folder to the one that held `bin`: here the working folder stays
/// and the path climbs out of it, which no test running beside this one can
/// see.
fn relative_to(target: &Path, from: &Path) -> PathBuf {
    let target: Vec<Component> = target.components().collect();
    let from: Vec<Component> = from.components().collect();
    let shared = target.iter().zip(&from).take_while(|(a, b)| a == b).count();
    let mut path = PathBuf::new();
    for _ in &from[shared..] {
        path.push("..");
    }
    for part in &target[shared..] {
        path.push(part);
    }
    path
}

#[test]
fn resolves_a_relative_path_entry_before_handing_it_to_the_pane_host() {
    // `pane.open` refuses a relative argv[0] outright
    // (`validate_open_request` in `crates/cf-panes/src/pane_handlers.rs`), and a
    // PATH carrying a relative entry is ordinary: `PATH=.:...` or a `bin` a
    // launcher exported from wherever it happened to be. Joining that with the
    // command name produces a relative candidate, and the pane never opens.
    let home = Home::near();
    let shim = home.stub_cli("claude");
    let relative = relative_to(&home.path_dir(), &std::env::current_dir().unwrap());
    assert!(relative.is_relative(), "{}", relative.display());
    let env = Env::from_vars([
        ("PATH", relative.to_string_lossy().into_owned()),
        ("HOME", home.root().to_string_lossy().into_owned()),
    ]);

    let found = harness_path(Harness::Claude, &env);

    let found = found.expect("it is on PATH, relatively");
    assert!(found.is_absolute(), "relative argv[0]: {}", found.display());
    assert_eq!(
        fs::canonicalize(&found).unwrap(),
        fs::canonicalize(shim).unwrap()
    );
}

#[test]
fn app_preparation_owns_its_launcher_and_integrations_not_role_documents_or_global_skills() {
    let home = Home::new();
    for name in ["claude", "codex", "pi", "opencode"] {
        home.stub_cli(name);
    }
    let globals: Vec<PathBuf> = [
        home.user().join(".claude"),
        home.user().join(".codex"),
        home.user().join(".config").join("opencode"),
        home.user().join(".pi").join("agent"),
    ]
    .map(|root| root.join("skills").join("consensflow").join("SKILL.md"))
    .into();
    for file in &globals {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "global canary").unwrap();
    }
    // The agent Node's `addAgent` made: a roster of the human's own.
    let roster = home.consensflow().join("agents.json");
    fs::create_dir_all(home.consensflow()).unwrap();
    fs::write(
        &roster,
        r#"{"agents":[{"name":"mine","kind":"claude-code","model":"example"}]}"#,
    )
    .unwrap();
    let cf = home.root().join("bundle").join("cf");

    let env = home.env();
    for _ in 0..2 {
        prepare_app(&env, &cf);
    }

    assert!(home
        .consensflow()
        .join("bin")
        .join(launcher_name())
        .exists());
    assert!(!home.consensflow().join("integrations").exists());
    assert!(!home.consensflow().join("skills-manifest.json").exists());
    for file in &globals {
        assert_eq!(fs::read_to_string(file).unwrap(), "global canary");
    }
    assert_eq!(
        fs::read_to_string(roster).unwrap(),
        r#"{"agents":[{"name":"mine","kind":"claude-code","model":"example"}]}"#
    );
}

#[test]
fn app_preparation_says_why_its_launcher_could_not_be_installed_and_prepares_the_integrations_all_the_same(
) {
    let home = Home::new();
    for name in ["pi", "opencode"] {
        home.stub_cli(name);
    }
    // A file where the launcher's folder goes: nothing can be written into it.
    fs::create_dir_all(home.consensflow()).unwrap();
    let bin = home.consensflow().join("bin");
    fs::write(&bin, "not a folder").unwrap();
    let cf = home.root().join("bundle").join("cf");

    let prepared = prepare_app(&home.env(), &cf);

    assert_eq!(prepared.report.len(), 1);
    let line = &prepared.report[0];
    assert!(
        line.strip_prefix("The cf launcher could not be installed: ")
            .is_some_and(|why| why
                .chars()
                .next()
                .is_some_and(|first| !first.is_whitespace())),
        "{line}"
    );
    #[cfg(unix)]
    assert_eq!(
        line,
        &format!(
            "The cf launcher could not be installed: ENOTDIR: not a directory, open '{}'",
            bin.join("consensflow").display()
        )
    );
    assert!(matches!(
        prepared.pi_extension,
        pi::Extension::InstalledUnverified { .. }
    ));
    assert!(matches!(
        prepared.opencode_extension,
        opencode::Extension::InstalledUnverified { .. }
    ));
    assert_eq!(fs::read_to_string(&bin).unwrap(), "not a folder");
}

#[test]
fn a_launcher_that_is_made_is_nothing_to_report_and_the_extensions_follow_the_harnesses() {
    let home = Home::new();
    let cf = home.root().join("bundle").join("cf");

    let none = prepare_app(&home.env(), &cf);
    assert_eq!(none.report, Vec::<String>::new());
    assert_eq!(none.pi_extension, pi::Extension::NotInstalled);
    assert_eq!(none.opencode_extension, opencode::Extension::NotInstalled);

    home.stub_cli("pi");
    let pi_only = prepare_app(&home.env(), &cf);
    assert!(matches!(
        pi_only.pi_extension,
        pi::Extension::InstalledUnverified { .. }
    ));
    assert_eq!(
        pi_only.opencode_extension,
        opencode::Extension::NotInstalled
    );
}

#[test]
fn without_a_home_the_launcher_says_so_and_the_extensions_say_so_for_themselves() {
    let prepared = prepare_app(&Env::default(), Path::new("/bundle/cf"));
    assert_eq!(
        prepared.report,
        ["The cf launcher could not be installed: missing home in env"]
    );
}
