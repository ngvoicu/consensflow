//! The terminal command as Node says it: the goldens that `npm run
//! goldens:launcher` records from the real `src/terminal.js`
//! (`tests/goldens/launcher/goldens.mjs` says what each holds), replayed.
//!
//! - `installs-cmd.json` and `installs-sh.json`: each case of
//!   `installTerminalCommand`, played in the form of cmd.exe and of `sh`. What
//!   Node left in the folder, what it answered and what it threw are what this
//!   leaves, answers and throws, with one difference, the one the new shape
//!   is: Node's launcher ran a runtime and its `cf.mjs`, and this one runs the
//!   native `cf`. The difference is kept, and said once ([`kept`]): Node's
//!   text with the two lines that name what it runs replaced by ours is the
//!   text this writes, byte for byte, the home's pin and every line ending
//!   and mark included.
//! - `readings.json`: what Node read in each text a command may hold, and
//!   what the folder above a path is.

// The goldens' own reading and a test's own folders: a failure is the test's.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_launcher::{install, runtime, Places, Shape};
use serde_json::Value;

/// A golden file, by its name under `tests/goldens`.
fn golden(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}; run `npm run goldens:launcher`",
            path.display()
        )
    });
    serde_json::from_str(&text).unwrap()
}

/// Node's text for a launcher, which names a runtime and a `cf.mjs`, as this
/// build writes it, which names a `cf`: the first line of the comment and the
/// line that runs, and nothing else.
fn kept(node: &str) -> String {
    const NODE_SH: [&str; 2] = [
        "# Installed by ConsensFlow. Runs the app's own runtime and its own copy of\n# the CLI, so the terminal and the window never drift apart.\n",
        "exec \"<runtime>\" \"<cli>\" \"$@\"\n",
    ];
    const NEW_SH: [&str; 2] = [
        "# Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\n# window never drift apart.\n",
        "exec \"<cf>\" \"$@\"\n",
    ];
    const NODE_CMD: [&str; 2] = [
        "REM Installed by ConsensFlow. Runs the app's own runtime and its own copy of\r\nREM the CLI, so the terminal and the window never drift apart.\r\n",
        "\"<runtime>\" \"<cli>\" %*\r\n",
    ];
    const NEW_CMD: [&str; 2] = [
        "REM Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\r\nREM window never drift apart.\r\n",
        "\"<cf>\" %*\r\n",
    ];
    let mut text = node.to_owned();
    for (old, new) in NODE_SH
        .iter()
        .zip(NEW_SH)
        .chain(NODE_CMD.iter().zip(NEW_CMD))
    {
        text = text.replace(old, new);
    }
    text
}

/// What a message says with the root of the home as `$ROOT` and every
/// separator `/`.
fn said(message: &str, root: &Path) -> String {
    message
        .replace(&*root.to_string_lossy(), "$ROOT")
        .replace('\\', "/")
}

/// One case of an install golden, played as `windows`, against the files Node
/// left.
fn replay(case: &Value, windows: bool) {
    let name = case["name"].as_str().unwrap();
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let place = case["place"].as_str().unwrap();
    let bin = match place {
        "blocked" => {
            fs::write(root.join("blocked"), "").unwrap();
            root.join("blocked").join("bin")
        }
        "file" => {
            fs::write(root.join("bin"), "not a folder").unwrap();
            root.join("bin")
        }
        _ => root.join("bin"),
    };
    let extension = if windows { ".cmd" } else { "" };
    let before = case["before"].as_array().unwrap();
    if !before.is_empty() {
        fs::create_dir_all(&bin).unwrap();
    }
    for held in before {
        let file = bin.join(format!("{}{extension}", held["name"].as_str().unwrap()));
        if held["directory"].as_bool() == Some(true) {
            fs::create_dir(&file).unwrap();
            continue;
        }
        fs::write(&file, held["text"].as_str().unwrap()).unwrap();
        #[cfg(unix)]
        if let Some(mode) = held["mode"].as_u64() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &file,
                fs::Permissions::from_mode(u32::try_from(mode).unwrap()),
            )
            .unwrap();
        }
    }

    let home = root.join("home").join(".consensflow");
    let home_text = home.to_string_lossy().into_owned();
    let path = match case["path"].as_str() {
        Some("on") => bin.to_string_lossy().into_owned(),
        Some("off") => "/nowhere".to_owned(),
        Some("among") => std::env::join_paths(["/a".as_ref(), bin.as_path(), "/b".as_ref()])
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        Some("near") => {
            let text = bin.to_string_lossy().into_owned();
            let (shorter, longer) = (&text[..text.len() - 1], format!("{text}-near"));
            std::env::join_paths([
                longer.as_ref(),
                Path::new(shorter),
                Path::new(&format!("{text}/")),
            ])
            .unwrap()
            .to_string_lossy()
            .into_owned()
        }
        _ => root.join("elsewhere").to_string_lossy().into_owned(),
    };
    let mut vars = vec![
        (
            "HOME".to_owned(),
            root.join("home").to_string_lossy().into_owned(),
        ),
        ("PATH".to_owned(), path),
    ];
    if case["pin"].as_bool() == Some(true) {
        vars.push(("CONSENSFLOW_HOME".to_owned(), home_text.clone()));
    }
    if windows {
        vars.push(("OS".to_owned(), "Windows_NT".to_owned()));
    }
    let cf = root.join("bundle").join("cf");
    let outcome = install(&Env::from_vars(vars), &cf, &Places::at(vec![bin.clone()]));

    // What it answered, or threw.
    match (&outcome, case["error"].as_str()) {
        (Err(message), Some(thrown)) => assert_eq!(said(message, root), thrown, "{name}"),
        (Ok(_), None) => {}
        (outcome, thrown) => panic!("{name}: {outcome:?} where Node threw {thrown:?}"),
    }
    let installed = outcome.ok().flatten();
    match (&installed, case["installed"].as_object()) {
        (Some(found), Some(said)) => {
            let file = found.path.file_name().unwrap().to_string_lossy();
            assert_eq!(
                file,
                format!("{}{extension}", said["name"].as_str().unwrap()),
                "{name}"
            );
            assert_eq!(Some(found.on_path), said["onPath"].as_bool(), "{name}");
        }
        (None, None) => {}
        (found, said) => panic!("{name}: {found:?} where Node answered {said:?}"),
    }

    // What it left in the folder.
    let left = fs::read_dir(&bin).ok().map(|entries| {
        let mut names: Vec<PathBuf> = entries.map(|entry| entry.unwrap().path()).collect();
        names.sort();
        names
    });
    let recorded = case["after"].as_array();
    assert_eq!(
        left.as_ref().map(Vec::len),
        recorded.map(Vec::len),
        "{name}: what is left"
    );
    for (file, node) in left
        .into_iter()
        .flatten()
        .zip(recorded.into_iter().flatten())
    {
        let held = file.file_name().unwrap().to_string_lossy().into_owned();
        let base = held.strip_suffix(extension).unwrap_or(&held);
        assert_eq!(base, node["name"].as_str().unwrap(), "{name}");
        if node["directory"].as_bool() == Some(true) {
            assert!(file.is_dir(), "{name}: {base}");
            continue;
        }
        let expected = kept(node["text"].as_str().unwrap())
            .replace("<home>", &home_text)
            .replace("<cf>", &cf.to_string_lossy());
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            expected,
            "{name}: {base}"
        );
        #[cfg(unix)]
        if let Some(executable) = node["executable"].as_bool() {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o111 != 0, executable, "{name}: {base}");
        }
    }
}

#[test]
fn every_install_of_the_form_of_cmd_leaves_what_node_left_and_says_what_it_said() {
    let goldens = golden("installs-cmd.json");
    let cases = goldens["installs"].as_array().unwrap();
    assert!(cases.len() >= 10);
    for case in cases {
        replay(case, true);
    }
}

#[cfg(not(windows))]
#[test]
fn every_install_of_the_form_of_sh_leaves_what_node_left_and_says_what_it_said() {
    // The form Windows makes none of, and the words of a POSIX system.
    let goldens = golden("installs-sh.json");
    let cases = goldens["installs"].as_array().unwrap();
    assert!(cases.len() >= 12);
    for case in cases {
        replay(case, false);
    }
}

#[test]
fn every_text_a_command_may_hold_is_read_as_node_read_it() {
    let goldens = golden("readings.json");
    let readings = goldens["readings"].as_array().unwrap();
    assert!(readings.len() >= 15);
    let places = |root: &Path| Places::at(vec![root.join("bin")]);
    for reading in readings {
        let name = reading["name"].as_str().unwrap();
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("bin").join(if cfg!(windows) {
            "consensflow.cmd"
        } else {
            "consensflow"
        });
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, reading["text"].as_str().unwrap()).unwrap();
        let env = Env::from_vars([("HOME", root.path().to_string_lossy().into_owned())]);
        let cf = root.path().join("bundle").join("cf");

        let found = runtime(&env, &cf, &places(root.path())).unwrap();

        match (reading["read"].as_object(), found) {
            (Some(node), Some(wiring)) => {
                assert_eq!(wiring.shape, Shape::Node, "{name}");
                assert_eq!(
                    Some(wiring.runtime.as_str()),
                    node["runtime"].as_str(),
                    "{name}"
                );
                assert_eq!(
                    Some(wiring.entry.as_str()),
                    node["entry"].as_str(),
                    "{name}"
                );
            }
            // What Node read nothing in is not of the old shape here: it may
            // be the new, which Node never wrote.
            (None, found) => {
                assert!(
                    found.is_none_or(|wiring| wiring.shape == Shape::Native),
                    "{name}"
                );
            }
            (Some(node), None) => panic!("{name}: nothing where Node read {node:?}"),
        }
    }
}
