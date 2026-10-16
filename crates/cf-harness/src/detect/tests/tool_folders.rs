//! The system's own folders of programs (`/opt/homebrew/bin` on a Mac): a CLI
//! found there when PATH lacks it, as a Homebrew `codex` was not on the night the
//! login shell was too slow to give the daemon its PATH (2026-10-15), and looked
//! in after everything else. The folders are a test's own, named by the
//! environment it makes (`CONSENSFLOW_TOOL_FOLDERS`): never the machine's.

use std::path::PathBuf;

use cf_base::env::TOOL_FOLDERS;

use super::*;

/// `command` as a file this system starts it by.
fn name(command: &str) -> String {
    if cfg!(windows) {
        format!("{command}.exe")
    } else {
        command.to_owned()
    }
}

/// A machine as a test lays one out: a home, a folder PATH names, and two
/// folders of the system's programs, Homebrew's and the one after it.
struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }

    /// The system's folders, in the order they are named.
    fn system(&self) -> [PathBuf; 2] {
        ["homebrew-bin", "usr-local-bin"].map(|folder| self.root.path().join(folder))
    }

    /// Its environment, which names its system's folders when `named`.
    fn env(&self, named: bool) -> Env {
        let mut vars = vec![
            (
                "HOME".to_owned(),
                self.home().into_os_string().into_string().unwrap(),
            ),
            (
                "PATH".to_owned(),
                self.bin().into_os_string().into_string().unwrap(),
            ),
        ];
        if named {
            let folders = std::env::join_paths(self.system()).unwrap();
            vars.push((TOOL_FOLDERS.to_owned(), folders.into_string().unwrap()));
        }
        Env::from_vars(vars)
    }
}

#[test]
fn a_cli_only_in_a_folder_of_the_systems_is_found_when_path_lacks_it() {
    let machine = Machine::new();
    let env = machine.env(true);
    assert_eq!(
        executable(Harness::Codex, &env),
        Err("codex is not installed on this machine".to_owned())
    );
    let codex = machine.system()[1].join(name("codex"));
    startable(&codex);
    assert_eq!(harness_path(Harness::Codex, &env), Some(codex.clone()));
    // A window opens on it, and the pickers and the Harnesses page have it installed.
    assert_eq!(
        executable(Harness::Codex, &env),
        Ok(codex.to_string_lossy().into_owned())
    );
    assert_eq!(
        detect_harnesses(&env)
            .iter()
            .map(|each| each.id)
            .collect::<Vec<_>>(),
        [Harness::Codex]
    );
    assert!(!missing_harnesses(&env).contains(&Harness::Codex));
    // An environment that names no folder of the system's looks in none.
    let unnamed = machine.env(false);
    assert_eq!(harness_path(Harness::Codex, &unnamed), None);
    assert_eq!(
        executable(Harness::Codex, &unnamed),
        Err("codex is not installed on this machine".to_owned())
    );
}

#[test]
fn they_come_after_path_and_every_place_under_the_home_each_in_the_order_named() {
    let machine = Machine::new();
    let env = machine.env(true);
    let [homebrew, after] = machine.system();
    startable(&after.join(name("codex")));
    let first = homebrew.join(name("codex"));
    startable(&first);
    assert_eq!(
        harness_path(Harness::Codex, &env),
        Some(first),
        "the first named first"
    );
    for (place, parts) in [
        ("the common one", &[".volta", "bin"][..]),
        ("its own", &[".codex", "bin"][..]),
    ] {
        let homed = parts
            .iter()
            .fold(machine.home(), |folder, part| folder.join(part))
            .join(name("codex"));
        startable(&homed);
        assert_eq!(
            harness_path(Harness::Codex, &env),
            Some(homed),
            "{place} under the home before the system's"
        );
    }
    let on_path = machine.bin().join(name("codex"));
    startable(&on_path);
    assert_eq!(harness_path(Harness::Codex, &env), Some(on_path));
}

/// The daemon runs everything with the system's folders after its PATH, and a
/// harness is still looked for on the PATH it was given first: the human's own
/// in the home comes before an older one of Homebrew's, which the PATH a window
/// runs on now holds (the reviews of 2026-10-16).
#[test]
fn on_the_path_a_daemon_runs_with_a_harness_in_the_home_still_comes_before_the_systems() {
    let machine = Machine::new();
    let [homebrew, _] = machine.system();
    startable(&homebrew.join(name("codex")));
    let own = machine
        .home()
        .join(".codex")
        .join("bin")
        .join(name("codex"));
    startable(&own);
    let running = machine.env(true).with_tool_folders_on_path();
    assert!(running
        .os("PATH")
        .is_some_and(|path| std::env::split_paths(path).any(|folder| folder == homebrew)));
    assert_eq!(harness_path(Harness::Codex, &running), Some(own));
}
