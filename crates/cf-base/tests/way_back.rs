//! The Rust decider of the way back (`cf_base::way_back`) held to the table
//! the Node one (`src/use-node.js`, `tests/way-back.test.mjs`) is held to:
//! `way-back.json`, beside this file. Each case makes a state at
//! `<home>/use-node`, with the home named by `CONSENSFLOW_HOME` and again by
//! `HOME` alone, and every stray environment on top, among them the
//! `CONSENSFLOW_DAEMON` the product no longer reads: one answer for all.

// The tests make their own folders and files: a failure in them is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;

use cf_base::env::Env;
use cf_base::way_back::{choose, FILE};
use serde_json::Value;

fn table() -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/way-back.json");
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

/// Makes `state` at `folder/use-node`; false where this system cannot make it.
fn make(state: &str, folder: &Path) -> bool {
    let file = folder.join(FILE);
    fs::create_dir_all(folder).unwrap();
    match state {
        "absent" => {}
        "file" => fs::write(&file, "go back to node\n").unwrap(),
        "empty" => fs::write(&file, "").unwrap(),
        "folder" => fs::create_dir(&file).unwrap(),
        #[cfg(unix)]
        "unreadable" => {
            use std::os::unix::fs::PermissionsExt;
            fs::write(&file, "go back to node\n").unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
        }
        #[cfg(unix)]
        "link" => {
            fs::write(folder.join("target"), "go back to node\n").unwrap();
            std::os::unix::fs::symlink(folder.join("target"), &file).unwrap();
        }
        #[cfg(unix)]
        "dangling" => std::os::unix::fs::symlink(folder.join("nowhere"), &file).unwrap(),
        #[cfg(unix)]
        "unsearchable" => {
            use std::os::unix::fs::PermissionsExt;
            fs::write(&file, "go back to node\n").unwrap();
            fs::set_permissions(folder, fs::Permissions::from_mode(0o000)).unwrap();
            // A user who may search any folder (root) cannot be kept out of one.
            if fs::metadata(&file).is_ok() {
                fs::set_permissions(folder, fs::Permissions::from_mode(0o755)).unwrap();
                return false;
            }
        }
        other => panic!("the table has a state this test cannot make: {other}"),
    }
    true
}

/// Gives the folder back its modes, so that it can be removed.
#[cfg(unix)]
fn unmake(folder: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(folder, fs::Permissions::from_mode(0o755));
    let _ = fs::set_permissions(folder.join(FILE), fs::Permissions::from_mode(0o644));
}

#[cfg(not(unix))]
fn unmake(_folder: &Path) {}

fn strays(case: &Value) -> Vec<Vec<(String, String)>> {
    case.as_array()
        .unwrap()
        .iter()
        .map(|stray| {
            stray
                .as_object()
                .unwrap()
                .iter()
                .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned()))
                .collect()
        })
        .collect()
}

#[test]
fn every_case_has_one_answer_whatever_the_environment_adds() {
    let table = table();
    let strays = strays(&table["strays"]);
    let mut held = 0;
    for case in table["cases"].as_array().unwrap() {
        let (name, state) = (
            case["name"].as_str().unwrap(),
            case["state"].as_str().unwrap(),
        );
        if case["unix"].as_bool() == Some(true) && !cfg!(unix) {
            continue;
        }
        let expected = case["node"].as_bool().unwrap();
        for by_variable in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let (folder, vars) = if by_variable {
                let folder = root.path().join("consensflow");
                let vars = vec![("CONSENSFLOW_HOME".to_owned(), folder.display().to_string())];
                (folder, vars)
            } else {
                let user = root.path().join("user");
                let vars = vec![("HOME".to_owned(), user.display().to_string())];
                (user.join(".consensflow"), vars)
            };
            if !make(state, &folder) {
                continue;
            }
            for stray in &strays {
                let env = Env::from_vars(vars.iter().chain(stray).cloned());
                let choice = choose(&env);
                assert_eq!(
                    choice.node,
                    expected,
                    "{name} (home by {}, with {stray:?})",
                    if by_variable {
                        "CONSENSFLOW_HOME"
                    } else {
                        "HOME"
                    }
                );
                assert_eq!(choice.file, Some(folder.join(FILE)), "{name}");
                held += 1;
            }
            unmake(&folder);
        }
    }
    // The table was read: its cases were made and asked, not skipped one by one.
    assert!(held >= 2 * strays.len() * 4, "{held} answers were held");
}
