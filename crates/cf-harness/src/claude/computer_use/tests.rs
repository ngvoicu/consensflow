//! What Claude Code's configuration says of computer use for a project's folder:
//! under the entry Claude Code keeps for it, read and never written.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use serde_json::json;

use super::*;
use crate::shared::directory::real_path;

/// A folder made in `home`, as the system names it.
fn made(home: &Path, name: &str) -> String {
    let folder = home.join(name);
    fs::create_dir_all(&folder).unwrap();
    real_path(&folder.to_string_lossy()).unwrap()
}

/// An environment whose Claude Code keeps its configuration in `config`.
fn claude_in(config: &str) -> Env {
    Env::from_vars([("CLAUDE_CONFIG_DIR", config), ("HOME", "/no/home")])
}

/// Claude Code's configuration in `config`, with computer use on for `folders`
/// and the server listed in another project's disabled ones besides.
fn configured(config: &str, folders: &[&str]) {
    let mut projects = serde_json::Map::new();
    for folder in folders {
        projects.insert(
            (*folder).to_owned(),
            json!({ "hasTrustDialogAccepted": true, "enabledMcpServers": ["other", "computer-use"] }),
        );
    }
    projects.insert(
        "/elsewhere".to_owned(),
        json!({ "enabledMcpServers": [], "disabledMcpServers": ["computer-use"] }),
    );
    fs::write(
        Path::new(config).join(".claude.json"),
        json!({ "hasCompletedOnboarding": true, "projects": projects }).to_string(),
    )
    .unwrap();
}

#[test]
fn a_folder_of_a_repository_and_a_linked_worktree_take_the_main_checkouts_entry() {
    let home = tempfile::tempdir().unwrap();
    let config = made(home.path(), "config");
    let main = made(home.path(), "main");
    fs::create_dir(Path::new(&main).join(".git")).unwrap();
    let worktree_git = made(home.path(), "main/.git/worktrees/tree");
    fs::write(Path::new(&worktree_git).join("commondir"), "../..\n").unwrap();
    let tree = made(home.path(), "trees/tree");
    fs::write(
        Path::new(&tree).join(".git"),
        format!("gitdir: {worktree_git}\n"),
    )
    .unwrap();
    let below_tree = made(home.path(), "trees/tree/app/src");
    let below_main = made(home.path(), "main/crates/x");
    configured(&config, &[&main]);
    let env = claude_in(&config);
    for folder in [&main, &below_main, &tree, &below_tree] {
        assert_eq!(
            computer_use(&env, folder),
            ComputerUse {
                folder: main.clone(),
                on: true
            },
            "{folder}"
        );
    }
}

#[test]
fn a_nested_repository_and_a_folder_out_of_any_have_their_own_entry() {
    let home = tempfile::tempdir().unwrap();
    let config = made(home.path(), "config");
    let main = made(home.path(), "main");
    fs::create_dir(Path::new(&main).join(".git")).unwrap();
    let nested = made(home.path(), "main/vendor/nested");
    fs::create_dir(Path::new(&nested).join(".git")).unwrap();
    let plain = made(home.path(), "plain");
    configured(&config, &[&main]);
    let env = claude_in(&config);
    assert_eq!(
        computer_use(&env, &nested),
        ComputerUse {
            folder: nested.clone(),
            on: false
        }
    );
    assert_eq!(
        computer_use(&env, &plain),
        ComputerUse {
            folder: plain.clone(),
            on: false
        }
    );
    configured(&config, &[&nested, &plain]);
    assert!(computer_use(&env, &nested).on);
    assert!(computer_use(&env, &plain).on);
    assert!(
        !computer_use(&env, &main).on,
        "the outer repository's own entry"
    );
}

#[test]
fn the_configuration_is_the_homes_unless_claude_is_given_a_folder_of_its_own() {
    let home = tempfile::tempdir().unwrap();
    let user = made(home.path(), "user");
    let other = made(home.path(), "other-config");
    let project = made(home.path(), "project");
    configured(&user, &[&project]);
    let in_home = Env::from_vars([("HOME", user.as_str())]);
    assert!(computer_use(&in_home, &project).on);
    let elsewhere = Env::from_vars([
        ("HOME", user.as_str()),
        ("CLAUDE_CONFIG_DIR", other.as_str()),
    ]);
    assert!(
        !computer_use(&elsewhere, &project).on,
        "a CLAUDE_CONFIG_DIR with no configuration says nothing"
    );
}

#[test]
fn a_configuration_that_is_not_there_or_no_json_or_lists_no_server_is_off() {
    let home = tempfile::tempdir().unwrap();
    let config = made(home.path(), "config");
    let project = made(home.path(), "project");
    let env = claude_in(&config);
    assert!(!computer_use(&env, &project).on, "no configuration");
    let file = Path::new(&config).join(".claude.json");
    for text in [
        "not json".to_owned(),
        json!({ "projects": { project.clone(): { "enabledMcpServers": "computer-use" } } })
            .to_string(),
        json!({ "projects": { project.clone(): { "disabledMcpServers": ["computer-use"] } } })
            .to_string(),
        json!({ "projects": { project.clone(): { "enabledMcpServers": ["computer-use-2"] } } })
            .to_string(),
        json!({ "projects": [] }).to_string(),
    ] {
        fs::write(&file, &text).unwrap();
        assert!(!computer_use(&env, &project).on, "{text}");
    }
}

#[test]
fn reading_it_writes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let config = made(home.path(), "config");
    let project = made(home.path(), "project");
    configured(&config, &[]);
    let file = Path::new(&config).join(".claude.json");
    let before = (
        fs::read(&file).unwrap(),
        fs::metadata(&file).unwrap().modified().unwrap(),
    );
    let listed = |folder: &str| {
        let mut names: Vec<_> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    };
    let files = listed(&config);
    let env = claude_in(&config);
    for _ in 0..3 {
        assert!(!computer_use(&env, &project).on);
    }
    let after = (
        fs::read(&file).unwrap(),
        fs::metadata(&file).unwrap().modified().unwrap(),
    );
    assert_eq!(after, before);
    assert_eq!(listed(&config), files, "no file was made beside it");
    assert!(
        listed(&project).is_empty(),
        "nothing was made in the project"
    );
    let _: SystemTime = before.1;
}
