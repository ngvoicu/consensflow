//! What a rig is given to run on: the environment of the daemon, the pane host
//! and so every window; the stand-in `claude` the windows find on their `PATH`;
//! and the roster a fresh rig starts with.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::Config;
use crate::process::own_var;
use crate::{files, stand_in, Error, Result};

/// The value of `name` in `env`, the last one given.
pub(super) fn var<'e>(env: &'e [(String, String)], name: &str) -> &'e str {
    env.iter()
        .rev()
        .find(|(given, _)| given == name)
        .map_or("", |(_, value)| value.as_str())
}

/// What the daemon, the pane host and so every window is given, and nothing
/// else of the machine's: a home of its own, a `PATH` that finds the stand-in
/// `claude` first, and the stand-in's settings. `config`'s variables come last.
pub(super) fn environment(root: &Path, fake_bin: &Path, config: &Config) -> Vec<(String, String)> {
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let home = root.join("home");
    let mut env = vec![
        ("HOME".to_owned(), text(&home)),
        (
            "CONSENSFLOW_HOME".to_owned(),
            text(&root.join("consensflow")),
        ),
        (
            "CONSENSFLOW_BIN_DIR".to_owned(),
            text(&root.join("consensflow").join("bin")),
        ),
        ("CLAUDE_CONFIG_DIR".to_owned(), text(&home.join(".claude"))),
        ("CODEX_HOME".to_owned(), text(&home.join(".codex"))),
        ("XDG_CONFIG_HOME".to_owned(), text(&home.join(".config"))),
    ];
    if cfg!(windows) {
        let system = own_var("SystemRoot").unwrap_or_else(|| "C:\\Windows".to_owned());
        env.push((
            "PATH".to_owned(),
            format!("{};{system}\\System32", fake_bin.display()),
        ));
        // What Windows itself needs to start a process, and the home it reads there.
        for name in ["SystemRoot", "ComSpec", "PATHEXT"] {
            if let Some(value) = own_var(name) {
                env.push((name.to_owned(), value));
            }
        }
        let temp = text(&std::env::temp_dir());
        env.push(("TEMP".to_owned(), temp.clone()));
        env.push(("TMP".to_owned(), temp));
        env.push(("USERPROFILE".to_owned(), text(&home)));
    } else {
        env.push((
            "PATH".to_owned(),
            format!("{}:/usr/local/bin:/usr/bin:/bin", fake_bin.display()),
        ));
    }
    env.push(("CF_TEST_HARNESS".to_owned(), text(&config.stand_in)));
    env.push(("TERM".to_owned(), "xterm-256color".to_owned()));
    env.extend(config.vars.iter().cloned());
    env
}

/// The stand-in `claude` the windows find on their `PATH`, and the folder
/// the stand-in agent writes its conversations in. On Windows it has the shape
/// of an npm shim ([`stand_in::window_shim`]), which is how a window opens on
/// it: it names the stand-in `agent` as both program and script, and the
/// stand-in knows its own file when it is given it first. The folder it is in.
pub(super) fn write_fake_install(root: &Path, agent: &Path) -> Result<PathBuf> {
    let fake_bin = root.join("fake-bin");
    let projects = root
        .join("home")
        .join(".claude")
        .join("projects")
        .join("integration");
    files::make_dir(&projects)?;
    let claude = fake_bin.join(stand_in::program_name("claude"));
    if cfg!(windows) {
        files::write_executable(&claude, stand_in::window_shim(agent))?;
    } else {
        files::write_executable(&claude, "#!/bin/sh\nexec \"$CF_TEST_HARNESS\" \"$@\"\n")?;
    }
    Ok(fake_bin)
}

/// The roster a fresh rig starts with: a chief and a worker, on the stand-in.
pub(super) fn write_roster(root: &Path) -> Result<()> {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "claude-code", "model": "fake" },
        ],
    });
    let text = serde_json::to_string_pretty(&roster).map_err(|source| Error::Json {
        text: roster.to_string(),
        source,
    })?;
    files::write(
        &root.join("consensflow").join("agents.json"),
        format!("{text}\n"),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn config() -> Config {
        Config::new("/the/stand-in").var("CF_TEST_QUOTA_OUT", "worker")
    }

    #[test]
    fn the_environment_is_a_home_of_its_own_a_path_that_finds_the_stand_in_first_and_the_cases_own()
    {
        let root = Path::new("root");
        let at = |parts: &[&str]| {
            parts
                .iter()
                .fold(root.to_path_buf(), |path, part| path.join(part))
                .to_string_lossy()
                .into_owned()
        };
        let env = environment(root, &root.join("fake-bin"), &config());
        let get = |name: &str| var(&env, name);
        assert_eq!(get("HOME"), at(&["home"]));
        assert_eq!(get("CONSENSFLOW_HOME"), at(&["consensflow"]));
        assert_eq!(get("CONSENSFLOW_BIN_DIR"), at(&["consensflow", "bin"]));
        assert_eq!(get("CLAUDE_CONFIG_DIR"), at(&["home", ".claude"]));
        assert_eq!(get("CODEX_HOME"), at(&["home", ".codex"]));
        assert_eq!(get("XDG_CONFIG_HOME"), at(&["home", ".config"]));
        assert!(
            get("PATH").starts_with(&at(&["fake-bin"])),
            "{}",
            get("PATH")
        );
        assert_eq!(get("CF_TEST_HARNESS"), "/the/stand-in");
        assert_eq!(get("TERM"), "xterm-256color");
        assert_eq!(get("CF_TEST_QUOTA_OUT"), "worker");
        assert_eq!(get("CF_TEST_NO_SUCH"), "");
        // Nothing of the machine's own gets in.
        assert!(!env.iter().any(|(name, _)| name == "CONSENSFLOW_URL"));
    }

    #[test]
    fn a_variable_a_case_names_beats_the_one_the_rig_sets() {
        let root = Path::new("root");
        let config = config().var("TERM", "dumb");
        let env = environment(root, &root.join("fake-bin"), &config);
        assert_eq!(var(&env, "TERM"), "dumb");
    }

    #[test]
    fn the_stand_in_claude_runs_the_program_the_environment_names_and_the_roster_has_two_agents() {
        let root = tempfile::tempdir().unwrap();
        let fake_bin = write_fake_install(root.path(), Path::new("/the/stand-in")).unwrap();
        assert_eq!(fake_bin, root.path().join("fake-bin"));
        let claude = fake_bin.join(stand_in::program_name("claude"));
        let script = files::read_string(&claude).unwrap();
        if cfg!(windows) {
            assert_eq!(
                script,
                "@echo off\r\n\"/the/stand-in\" \"/the/stand-in\" %*\r\n"
            );
        } else {
            assert_eq!(script, "#!/bin/sh\nexec \"$CF_TEST_HARNESS\" \"$@\"\n");
        }
        // The folder the agent writes its conversations in is there to write in.
        assert!(root
            .path()
            .join("home/.claude/projects/integration")
            .is_dir());
        write_roster(root.path()).unwrap();
        let roster: Value = serde_json::from_str(
            &files::read_string(&root.path().join("consensflow/agents.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(roster["schemaVersion"], 1);
        assert_eq!(roster["agents"][0]["id"], "chief");
        assert_eq!(roster["agents"][1]["model"], "fake");
    }
}
