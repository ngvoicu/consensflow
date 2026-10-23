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

/// The machine's own folder of programs as the rig has it, the one Homebrew's
/// are in on a Mac (`/opt/homebrew/bin`), which the daemon is told is the
/// system's (`CONSENSFLOW_TOOL_FOLDERS`): a daemon of the rig finds nothing of
/// the machine's there, and a stand-in only where a case puts one
/// ([`Config::as_the_finder_starts_it`]).
pub(super) fn tool_folder(root: &Path) -> PathBuf {
    root.join("system-bin")
}

/// What the daemon, the pane host and so every window is given, and nothing
/// else of the machine's: a home of its own, a `PATH` that finds the stand-in
/// `claude` first, the rig's own folder of programs for the system's, and the
/// stand-in's settings. `config`'s variables come last, but for the ones it
/// unsets, which are not there at all.
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
        ("PATH".to_owned(), path(fake_bin, config)),
        (
            "CONSENSFLOW_TOOL_FOLDERS".to_owned(),
            text(&tool_folder(root)),
        ),
    ];
    if cfg!(windows) {
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
        // The same on Unix, where the programs of a rig would else find `/tmp`: the temp
        // folder of the test that runs the rig, a plant run's own for a plant run, which
        // then holds everything its rigs write.
        env.push(("TMPDIR".to_owned(), text(&std::env::temp_dir())));
    }
    env.push(("CF_TEST_HARNESS".to_owned(), text(&config.stand_in)));
    if let Some(codex) = &config.codex {
        // The stand-in Codex's own: what it runs, what it writes down, how its
        // windows go (each on a thread of its own, the TUI waiting to be ended).
        env.extend([
            ("CF_TEST_CODEX".to_owned(), text(codex)),
            (
                "CF_TEST_CODEX_LOG".to_owned(),
                text(&root.join("codex.jsonl")),
            ),
            ("CF_TEST_CODEX_TUI".to_owned(), "wait".to_owned()),
            ("CF_TEST_CODEX_THREADS".to_owned(), "unique".to_owned()),
        ]);
    }
    if let Some(devin) = &config.devin {
        env.push(("CF_TEST_DEVIN".to_owned(), text(devin)));
    }
    if let Some(opencode) = &config.opencode {
        // The stand-in's own: what it runs, and where it writes down what it is asked.
        env.extend([
            ("CF_TEST_OPENCODE".to_owned(), text(opencode)),
            (
                "CF_TEST_OPENCODE_LOG".to_owned(),
                text(&root.join("opencode.jsonl")),
            ),
        ]);
    }
    env.push(("TERM".to_owned(), "xterm-256color".to_owned()));
    env.extend(config.vars.iter().cloned());
    env.retain(|(name, _)| !config.unset.contains(name));
    env
}

/// The `PATH` of the rig: the folder of the stand-in `claude` first, then the
/// folders a case wants found before the rest ([`Config::first_on_path`]) and the
/// ones it added ([`Config::also_on_path`]), then the system's own and no folder a
/// package manager installs into (`/usr/local/bin` is Homebrew's on an Intel Mac,
/// whose harnesses a daemon of the rig found there); as the Finder starts it, no
/// stand-in's folder, and the system's as the Finder gives it.
fn path(fake_bin: &Path, config: &Config) -> String {
    let system: Vec<String> = if cfg!(windows) {
        let root = own_var("SystemRoot").unwrap_or_else(|| "C:\\Windows".to_owned());
        vec![format!("{root}\\System32")]
    } else if config.finder {
        ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
            .map(str::to_owned)
            .into()
    } else {
        ["/usr/bin", "/bin"].map(str::to_owned).into()
    };
    (!config.finder)
        .then_some(fake_bin)
        .into_iter()
        .chain(config.first.iter().map(PathBuf::as_path))
        .chain(config.path.iter().map(PathBuf::as_path))
        .map(|dir| dir.to_string_lossy().into_owned())
        .chain(system)
        .collect::<Vec<_>>()
        .join(if cfg!(windows) { ";" } else { ":" })
}

/// The stand-in `claude` the windows find on their `PATH`, and the folder the
/// stand-in agent writes its conversations in. On Windows it has the shape of an
/// npm shim ([`stand_in::window_shim`]), which is how a window opens on it: it
/// names the stand-in as both program and script, and the stand-in knows its own
/// file when it is given it first. The stand-in `devin` beside it, where the rig
/// has one, the stand-in `opencode` (of a 1.x, or of an OpenCode 2) likewise, and
/// the stand-in `codex`. The folder they are in, which the `PATH` names.
/// As the Finder starts it, they are where each harness installs itself instead
/// (see [`Config::as_the_finder_starts_it`]), and that folder holds none.
pub(super) fn write_fake_install(root: &Path, config: &Config) -> Result<PathBuf> {
    let fake_bin = root.join("fake-bin");
    let home = root.join("home");
    let projects = home.join(".claude").join("projects").join("integration");
    files::make_dir(&projects)?;
    let (own, system) = if config.finder {
        // There, as Homebrew's is on a Mac, for what a case puts in it.
        files::make_dir(&tool_folder(root))?;
        (home.join(".local").join("bin"), tool_folder(root))
    } else {
        (fake_bin.clone(), fake_bin.clone())
    };
    write_shim(&own, "claude", "CF_TEST_HARNESS", &config.stand_in)?;
    if let Some(devin) = &config.devin {
        write_shim(&own, "devin", "CF_TEST_DEVIN", devin)?;
    }
    if let Some(opencode) = &config.opencode {
        write_shim(&own, "opencode", "CF_TEST_OPENCODE", opencode)?;
    }
    if let Some(codex) = &config.codex {
        write_shim(&system, "codex", "CF_TEST_CODEX", codex)?;
    }
    Ok(fake_bin)
}

/// The command `name` in `dir`, which starts `program`: a script that runs the
/// program the variable `var` names, or on Windows the shim of `program`.
fn write_shim(dir: &Path, name: &str, var: &str, program: &Path) -> Result {
    let shim = dir.join(stand_in::program_name(name));
    if cfg!(windows) {
        files::write_executable(&shim, stand_in::window_shim(program))
    } else {
        files::write_executable(&shim, format!("#!/bin/sh\nexec \"${var}\" \"$@\"\n"))
    }
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
    fn the_programs_of_a_rig_are_told_the_temp_folder_of_the_test_that_runs_it() {
        // Whatever they write there (a socket's folder, a harness's scratch files) is in the
        // folder a plant run gave its test, with the homes and the reports, and goes with it.
        let root = Path::new("root");
        let env = environment(root, &root.join("fake-bin"), &config());
        let temp = std::env::temp_dir().to_string_lossy().into_owned();
        let names: &[&str] = if cfg!(windows) {
            &["TEMP", "TMP"]
        } else {
            &["TMPDIR"]
        };
        for name in names {
            assert_eq!(var(&env, name), temp, "{name}");
        }
        // A case that wants another says so, and it comes last.
        let own = environment(
            root,
            &root.join("fake-bin"),
            &config().var(names[0], "/mine"),
        );
        assert_eq!(var(&own, names[0]), "/mine");
    }

    #[test]
    fn a_folder_a_case_adds_to_the_path_comes_after_the_stand_ins_and_before_the_systems() {
        let root = Path::new("root");
        let fake_bin = root.join("fake-bin");
        let delimiter = if cfg!(windows) { ';' } else { ':' };
        let added = config()
            .also_on_path("/real/codex")
            .also_on_path("/real/node");
        let env = environment(root, &fake_bin, &added);
        let folders: Vec<&str> = var(&env, "PATH").split(delimiter).collect();
        assert_eq!(folders[0], fake_bin.to_string_lossy());
        assert_eq!(folders[1..3], ["/real/codex", "/real/node"]);
        // What follows is what a rig that adds none ends with.
        let plain = environment(root, &fake_bin, &config());
        let plain: Vec<&str> = var(&plain, "PATH").split(delimiter).collect();
        assert_eq!(plain.len(), folders.len() - 2);
        assert_eq!(folders[3..], plain[1..]);
    }

    #[test]
    fn a_folder_a_case_wants_found_first_comes_right_after_the_stand_in_claudes() {
        let root = Path::new("root");
        let fake_bin = root.join("fake-bin");
        let delimiter = if cfg!(windows) { ';' } else { ':' };
        // Added after the others, it is still ahead of them: the machine's own program
        // of its name is in one of the others' folders.
        let added = config()
            .also_on_path("/real/devin")
            .first_on_path("/stand-in/cf");
        let env = environment(root, &fake_bin, &added);
        let folders: Vec<&str> = var(&env, "PATH").split(delimiter).collect();
        assert_eq!(folders[0], fake_bin.to_string_lossy());
        assert_eq!(folders[1..3], ["/stand-in/cf", "/real/devin"]);
    }

    #[test]
    fn a_variable_a_case_names_beats_the_one_the_rig_sets() {
        let root = Path::new("root");
        let config = config().var("TERM", "dumb");
        let env = environment(root, &root.join("fake-bin"), &config);
        assert_eq!(var(&env, "TERM"), "dumb");
    }

    #[test]
    fn a_variable_a_case_unsets_is_not_in_the_environment_whoever_set_it() {
        let root = Path::new("root");
        let plain = environment(root, &root.join("fake-bin"), &config());
        assert!(plain.iter().any(|(name, _)| name == "CLAUDE_CONFIG_DIR"));
        // The rig's own, and one the case set: both go, and nothing else with them.
        let config = config()
            .var("CF_TEST_ONE", "1")
            .unset("CLAUDE_CONFIG_DIR")
            .unset("CF_TEST_ONE");
        let env = environment(root, &root.join("fake-bin"), &config);
        assert!(!env
            .iter()
            .any(|(name, _)| name == "CLAUDE_CONFIG_DIR" || name == "CF_TEST_ONE"));
        assert_eq!(env.len(), plain.len() - 1);
        assert_eq!(var(&env, "CLAUDE_CONFIG_DIR"), "");
    }

    #[test]
    fn a_rig_given_a_devin_has_its_stand_in_found_as_devin_beside_claudes_and_named_in_the_environment(
    ) {
        let root = tempfile::tempdir().unwrap();
        let devin = Path::new("/the/stand-in-devin");
        let given = Config::new("/the/stand-in").devin(devin);
        let fake_bin = write_fake_install(root.path(), &given).unwrap();
        let script = files::read_string(&fake_bin.join(stand_in::program_name("devin"))).unwrap();
        if cfg!(windows) {
            assert_eq!(
                script,
                "@echo off\r\n\"/the/stand-in-devin\" \"/the/stand-in-devin\" %*\r\n"
            );
        } else {
            assert_eq!(script, "#!/bin/sh\nexec \"$CF_TEST_DEVIN\" \"$@\"\n");
        }
        // Claude's is still there, and the environment names the one and the other.
        assert!(fake_bin.join(stand_in::program_name("claude")).is_file());
        let env = environment(root.path(), &fake_bin, &config().devin(devin));
        assert_eq!(var(&env, "CF_TEST_DEVIN"), "/the/stand-in-devin");
        assert_eq!(var(&env, "CF_TEST_HARNESS"), "/the/stand-in");
        let plain = environment(root.path(), &fake_bin, &config());
        assert!(!plain.iter().any(|(name, _)| name == "CF_TEST_DEVIN"));
    }

    #[test]
    fn a_rig_given_an_opencode_2_has_its_stand_in_found_as_opencode_and_named_in_the_environment() {
        let root = tempfile::tempdir().unwrap();
        let opencode = Path::new("/the/stand-in-opencode-2");
        let given = Config::new("/the/stand-in").opencode_2(opencode);
        let fake_bin = write_fake_install(root.path(), &given).unwrap();
        let script =
            files::read_string(&fake_bin.join(stand_in::program_name("opencode"))).unwrap();
        if cfg!(windows) {
            assert_eq!(
                script,
                "@echo off\r\n\"/the/stand-in-opencode-2\" \"/the/stand-in-opencode-2\" %*\r\n"
            );
        } else {
            assert_eq!(script, "#!/bin/sh\nexec \"$CF_TEST_OPENCODE\" \"$@\"\n");
        }
        let env = environment(root.path(), &fake_bin, &given);
        assert_eq!(var(&env, "CF_TEST_OPENCODE"), "/the/stand-in-opencode-2");
        assert_eq!(
            var(&env, "CF_TEST_OPENCODE_LOG"),
            root.path().join("opencode.jsonl").to_string_lossy()
        );
        // And none of it where the rig is given none.
        let bare = tempfile::tempdir().unwrap();
        let fake_bin = write_fake_install(bare.path(), &config()).unwrap();
        assert!(!fake_bin.join(stand_in::program_name("opencode")).exists());
        let plain = environment(bare.path(), &fake_bin, &config());
        assert!(!plain
            .iter()
            .any(|(name, _)| name.starts_with("CF_TEST_OPENCODE")));
    }

    #[test]
    fn the_stand_in_claude_runs_the_program_the_environment_names_and_the_roster_has_two_agents() {
        let root = tempfile::tempdir().unwrap();
        let fake_bin = write_fake_install(root.path(), &Config::new("/the/stand-in")).unwrap();
        assert_eq!(fake_bin, root.path().join("fake-bin"));
        assert!(
            !fake_bin.join("codex").exists(),
            "a rig that is given no stand-in Codex has none"
        );
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
        // No Devin unless the rig is given one.
        assert!(!fake_bin.join(stand_in::program_name("devin")).exists());
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

    #[test]
    fn a_rig_given_a_stand_in_codex_has_it_on_its_path_and_its_windows_go_as_the_daemons_do() {
        let config = config().codex("/the/fake-codex");
        let root = Path::new("root");
        let env = environment(root, &root.join("fake-bin"), &config);
        assert_eq!(var(&env, "CF_TEST_CODEX"), "/the/fake-codex");
        assert_eq!(
            var(&env, "CF_TEST_CODEX_LOG"),
            root.join("codex.jsonl").to_string_lossy()
        );
        assert_eq!(var(&env, "CF_TEST_CODEX_TUI"), "wait");
        assert_eq!(var(&env, "CF_TEST_CODEX_THREADS"), "unique");
        // A case's own word is still the last.
        let env = environment(
            root,
            &root.join("fake-bin"),
            &config.var("CF_TEST_CODEX_TUI", "x"),
        );
        assert_eq!(var(&env, "CF_TEST_CODEX_TUI"), "x");
        // And none of it where the rig has no stand-in Codex.
        let plain = environment(root, &root.join("fake-bin"), &self::config());
        assert!(!plain
            .iter()
            .any(|(name, _)| name.starts_with("CF_TEST_CODEX")));
    }

    #[test]
    fn the_stand_in_codex_runs_the_program_the_environment_names() {
        let root = tempfile::tempdir().unwrap();
        let fake_bin = write_fake_install(root.path(), &config().codex("/the/fake-codex")).unwrap();
        let script = files::read_string(&fake_bin.join(stand_in::program_name("codex"))).unwrap();
        if cfg!(windows) {
            assert_eq!(
                script,
                "@echo off\r\n\"/the/fake-codex\" \"/the/fake-codex\" %*\r\n"
            );
        } else {
            assert_eq!(script, "#!/bin/sh\nexec \"$CF_TEST_CODEX\" \"$@\"\n");
        }
    }

    /// The stand-ins' folder, then the system's own, and no folder a package
    /// manager installs into: `/usr/local/bin` is Homebrew's on an Intel Mac,
    /// where a daemon of the rig found the machine's harnesses (the reviews of
    /// 2026-10-16).
    #[cfg(unix)]
    #[test]
    fn the_path_is_the_stand_ins_folder_then_the_systems_own_and_no_package_managers() {
        let root = Path::new("root");
        let env = environment(root, &root.join("fake-bin"), &config());
        assert_eq!(
            var(&env, "PATH"),
            format!("{}:/usr/bin:/bin", root.join("fake-bin").display())
        );
    }

    #[test]
    fn every_rig_names_a_folder_of_its_own_for_the_systems_programs() {
        let root = Path::new("root");
        for config in [config(), config().as_the_finder_starts_it()] {
            let env = environment(root, &root.join("fake-bin"), &config);
            assert_eq!(
                var(&env, "CONSENSFLOW_TOOL_FOLDERS"),
                root.join("system-bin").to_string_lossy()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn as_the_finder_starts_it_the_path_is_the_systems_and_each_stand_in_is_where_it_installs_itself(
    ) {
        let root = tempfile::tempdir().unwrap();
        let given = config()
            .codex("/the/fake-codex")
            .devin("/the/fake-devin")
            .as_the_finder_starts_it();
        let fake_bin = write_fake_install(root.path(), &given).unwrap();
        let env = environment(root.path(), &fake_bin, &given);
        assert_eq!(var(&env, "PATH"), "/usr/bin:/bin:/usr/sbin:/sbin");
        let own = root.path().join("home").join(".local").join("bin");
        assert!(own.join("claude").is_file() && own.join("devin").is_file());
        // The system's folder is there with no Codex in it too, as Homebrew's is.
        let bare = tempfile::tempdir().unwrap();
        write_fake_install(bare.path(), &config().as_the_finder_starts_it()).unwrap();
        assert!(bare.path().join("system-bin").is_dir());
        let system = root.path().join("system-bin");
        assert_eq!(
            files::read_string(&system.join("codex")).unwrap(),
            "#!/bin/sh\nexec \"$CF_TEST_CODEX\" \"$@\"\n"
        );
        for name in ["claude", "devin", "codex"] {
            assert!(!fake_bin.join(name).exists(), "{name} is on no PATH");
        }
        assert!(!own.join("codex").exists() && !system.join("claude").exists());
        // A folder a case adds is still on it, before the system's.
        let env = environment(root.path(), &fake_bin, &given.also_on_path("/real/node"));
        assert_eq!(
            var(&env, "PATH"),
            "/real/node:/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }
}
