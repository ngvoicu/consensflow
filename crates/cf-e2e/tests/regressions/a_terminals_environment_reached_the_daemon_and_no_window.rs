//! What the app took of the human's terminal reached the daemon and no window (T-540,
//! Calliope's review T-590, P1).
//!
//! The app asks the human's interactive login shell for its environment and gives it to
//! the daemon it starts (`app/src-tauri/src/daemon_command.rs`), naming what it took in
//! `CONSENSFLOW_TERMINAL_ENV`. But each window's pane is started by the pane host, from
//! the app's own environment (the Finder's), overlaid with what the daemon's `pane.open`
//! says: so a member had the rc's PATH and nothing else of it, no `LANG`, none of the
//! `.zshrc`'s exports. The rig plays the app: the daemon alone is given the variable and
//! its name, the pane host and its windows have neither.
//!
//! What a member should have: the variable, in its `pane.open` and, where the system
//! shows a process's environment to its owner, in the window's own process. A billing
//! key an rc exports reaches the `pane.open` too, and the window's own process never:
//! the harness's `dropEnv` takes it out last. No value is written in the daemon's home
//! (its ledger, trace and log).

use cf_e2e::rig::{Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::tier;
use crate::{config, secs, Outcome};

/// The variable the app names what it took in (`cf_base::env::TERMINAL_ENV`).
const TERMINAL_ENV: &str = "CONSENSFLOW_TERMINAL_ENV";

/// The variable only the human's terminal sets, and what it says there.
const NAME: &str = "PARITY_FROM_THE_RC";
const VALUE: &str = "set-by-the-rc";

/// A key an rc exports, which a Claude window drops (`cf-harness`' window args), and a
/// value no key has.
const KEY: &str = "ANTHROPIC_API_KEY";
const KEY_VALUE: &str = "parity-not-a-key";

/// The files under `folder`, and those of them that hold one of `values`, by their
/// names.
fn scanned(folder: &std::path::Path, values: &[&str]) -> (Vec<std::path::PathBuf>, Vec<String>) {
    let mut read = Vec::new();
    let mut found = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if values.iter().any(|value| {
                bytes
                    .windows(value.len())
                    .any(|window| window == value.as_bytes())
            }) {
                found.push(path.display().to_string());
            }
            read.push(path);
        }
    }
    (read, found)
}

/// Holds the daemon's `home` to holding none of `values`: its ledger and its log among
/// the files read, so that a home that is empty, or could not be read, is no pass.
fn holds_none(home: &std::path::Path, values: &[&str], when: &str) {
    let (read, found) = scanned(home, values);
    for kept in ["consensflow.db", "daemon.log"] {
        assert!(
            read.contains(&home.join(kept)),
            "{when}: the scan read no {kept} in {}",
            home.display()
        );
    }
    assert!(
        found.is_empty(),
        "{when}: a value given the daemon is written in {found:?}"
    );
}

/// The names a `pane.open` gives its window, never their values.
fn names(frame: &Value) -> Vec<String> {
    match &frame["env"] {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(pairs) => pairs
            .iter()
            .filter_map(|pair| pair[0].as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the environment of a `pane.open` (a map, or a list of pairs) gives `name`
/// the value `value`.
fn opens_with(frame: &Value, name: &str, value: &str) -> bool {
    match &frame["env"] {
        Value::Object(map) => map.get(name).is_some_and(|held| held == value),
        Value::Array(pairs) => pairs.iter().any(|pair| pair[0] == name && pair[1] == value),
        _ => false,
    }
}

#[test]
fn a_member_window_is_given_what_the_app_took_of_the_humans_terminal() -> Outcome {
    let rig = Rig::start(
        config()
            .daemon_var(NAME, VALUE)
            .daemon_var(KEY, KEY_VALUE)
            .daemon_var(TERMINAL_ENV, json!([NAME, KEY]).to_string()),
    )?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let handle = added["member"]["handle"]
        .as_str()
        .ok_or_else(|| Error::Daemon(format!("the member has no handle: {added}")))?
        .to_owned();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} Read the parser", tier(&added)),
    )?;
    let worker = format!("p{}-{handle}", project.id());
    let frame = rig.wait_for("the worker's window to be opened", secs(60), || {
        Ok(rig.open_frames().into_iter().any(|frame| {
            frame["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&worker))
        }))
    });
    frame?;
    let opened = rig
        .open_frames()
        .into_iter()
        .find(|frame| {
            frame["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&worker))
        })
        .ok_or_else(|| Error::Daemon("the worker's window is gone".to_owned()))?;
    assert!(
        opens_with(&opened, NAME, VALUE),
        "the worker's pane.open has no {NAME}: it names {:?}",
        names(&opened)
    );
    // The key is given with the rest, and taken out last.
    assert!(
        opens_with(&opened, KEY, KEY_VALUE),
        "the worker's pane.open has no {KEY}"
    );
    assert!(
        opened["dropEnv"]
            .as_array()
            .is_some_and(|dropped| dropped.iter().any(|name| name == KEY)),
        "the worker's pane.open drops no {KEY}: {}",
        opened["dropEnv"]
    );
    // And its own process has it, where the system shows a process's environment.
    #[cfg(target_os = "macos")]
    {
        rig.wait_for("the worker's process to start", secs(30), || {
            Ok(rig.started().len() >= 2)
        })?;
        let pid = rig
            .started()
            .last()
            .map(|started| started.pid)
            .unwrap_or_default();
        let shown = cf_e2e::process::Run::new("ps")
            .args(["eww", "-o", "command=", "-p", &pid.to_string()])
            .inheriting_env()
            .run()?
            .stdout;
        assert!(
            shown.contains(&format!("{NAME}={VALUE}")),
            "the worker's process (pid {pid}) has no {NAME}"
        );
        assert!(
            !shown.contains(&format!("{KEY}=")),
            "the worker's process (pid {pid}) has {KEY}"
        );
    }
    // The chief's window is a window too.
    let chief = rig
        .open_frames()
        .into_iter()
        .find(|frame| frame["id"] == format!("p{}-chief", project.id()))
        .ok_or_else(|| Error::Daemon("the chief's window was never opened".to_owned()))?;
    assert!(
        opens_with(&chief, NAME, VALUE),
        "the chief's pane.open has no {NAME}: it names {:?}",
        names(&chief)
    );
    // The daemon's home holds no value it was given, while it runs and once it has
    // ended (what it writes as it ends): its ledger, its trace, its log.
    let home = rig.home();
    holds_none(&home, &[VALUE, KEY_VALUE], "while the daemon runs");
    let root = rig.close_keeping_the_root()?;
    holds_none(&home, &[VALUE, KEY_VALUE], "once the daemon ended");
    std::fs::remove_dir_all(&root).map_err(|source| Error::File {
        action: "remove",
        path: root.clone(),
        source,
    })?;
    Ok(())
}
