//! Cargo must still enumerate the workspace when an ignored file is the only
//! thing left in a deleted crate's directory. The repository and file are ours.
#![allow(clippy::unwrap_used)]

#[allow(clippy::disallowed_methods)]
mod common;
#[path = "../../../crates/cf-process/src/nested_cargo.rs"]
mod nested_cargo;

use std::fs;
use std::path::Path;

fn make_crate(root: &Path, relative: &Path) {
    let dir = root.join(relative);
    fs::create_dir_all(dir.join("src")).unwrap();
    let name = relative.file_name().unwrap().to_str().unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )
    .unwrap();
    fs::write(dir.join("src/lib.rs"), "").unwrap();
}

#[test]
fn cargo_enumerates_members_without_taking_an_ignored_file_in_a_deleted_crate() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    assert!(common::command("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .unwrap()
        .success());

    // Use the real workspace membership and exclusions, with small independent
    // crates in place of the project's packages, so metadata builds nothing.
    let manifest = fs::read_to_string(source.join("Cargo.toml")).unwrap();
    let workspace = manifest.split("[workspace.package]").next().unwrap();
    fs::write(root.join("Cargo.toml"), workspace).unwrap();
    let mut expected = 1;
    make_crate(root, Path::new("app/src-tauri"));
    for group in ["crates", "tools"] {
        for entry in fs::read_dir(source.join(group)).unwrap() {
            let entry = entry.unwrap();
            if entry.path().join("Cargo.toml").is_file() {
                make_crate(root, &Path::new(group).join(entry.file_name()));
                expected += 1;
            }
        }
    }
    fs::write(root.join(".gitignore"), ".DS_Store\n").unwrap();
    let leftover = root.join("crates/cf-evals/.DS_Store");
    fs::create_dir_all(leftover.parent().unwrap()).unwrap();
    fs::write(&leftover, b"owner's ignored file").unwrap();
    assert!(common::command("git")
        .args(["check-ignore", "-q", "crates/cf-evals/.DS_Store"])
        .current_dir(root)
        .status()
        .unwrap()
        .success());

    let metadata = nested_cargo::cargo()
        .args(["metadata", "--offline", "--no-deps", "--format-version=1"])
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let data: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    assert_eq!(
        data["workspace_members"].as_array().unwrap().len(),
        expected
    );
    assert_eq!(fs::read(leftover).unwrap(), b"owner's ignored file");
}
