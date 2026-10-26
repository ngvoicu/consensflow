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

/// The files under `folder` that hold `text`, by their names.
fn holding(folder: &std::path::Path, text: &str) -> Vec<String> {
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
            } else if std::fs::read(&path).is_ok_and(|bytes| {
                bytes
                    .windows(text.len())
                    .any(|window| window == text.as_bytes())
            }) {
                found.push(path.display().to_string());
            }
        }
    }
    found
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
    // The daemon's home holds no value it was given: its ledger, its trace, its log.
    for value in [VALUE, KEY_VALUE] {
        let found = holding(&rig.home(), value);
        assert!(
            found.is_empty(),
            "a value given the daemon is written in {found:?}"
        );
    }
    rig.close()?;
    Ok(())
}
