//! What the opt-in live tests share. A live test runs a real harness on the
//! machine's own login and spends real quota, so it is ignored unless asked for
//! (`cargo xtask live <name>`), and it needs the machine to be set up for it:
//! the harness installed and logged in. The rest is as the rig's cases: the
//! `cf` and the pane host built from this checkout, in a home of the case's own
//! that is never `~/.consensflow`.
//!
//! The harness is the machine's: [`codex`] finds its command and the home its
//! login is read from, and [`Codex::rig`] is a rig whose windows find it.
//! Nothing is copied: a window reads the login where it is, as it does for the
//! person it belongs to (and writes its own sessions there, as it does for them).

use std::io;
use std::path::{Path, PathBuf};

use crate::process::own_var;
use crate::rig::Config;
use crate::{Error, Result};

/// The variable that leaves a live run's home and project where they are, for a
/// person to look at.
pub const KEEP: &str = "CONSENSFLOW_LIVE_KEEP";

/// Whether the run is to leave what it made: [`KEEP`] is set to something that
/// is not empty or `0`.
pub fn keep_requested() -> bool {
    kept(own_var(KEEP).as_deref())
}

fn kept(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

/// Where the program `name` is, on the `PATH` of this run, as a shell finds it:
/// the first folder that holds a file of its name (on Windows, of its name with
/// one of the extensions of `PATHEXT`). A machine that has none is an error that
/// says so.
pub fn command(name: &str) -> Result<PathBuf> {
    let path = own_var("PATH").unwrap_or_default();
    find(name, &path, own_var("PATHEXT").as_deref()).ok_or_else(|| Error::Program {
        action: "find",
        program: name.to_owned(),
        source: io::Error::new(
            io::ErrorKind::NotFound,
            "it is in no folder of the PATH of this run: install it, or put its folder on the PATH",
        ),
    })
}

/// [`command`] on the `path` and the extensions it is given.
fn find(command: &str, path: &str, extensions: Option<&str>) -> Option<PathBuf> {
    let suffixes: Vec<&str> = if cfg!(windows) {
        extensions
            .unwrap_or(".COM;.EXE;.BAT;.CMD")
            .split(';')
            .filter(|extension| !extension.is_empty())
            .collect()
    } else {
        vec![""]
    };
    std::env::split_paths(path).find_map(|folder| {
        suffixes
            .iter()
            .map(|suffix| folder.join(format!("{command}{suffix}")))
            .find(|candidate| candidate.is_file())
    })
}

/// The machine's own Codex: its command, the home it keeps its login in, and
/// the folder of Node, which a Codex installed by npm needs on the `PATH` to
/// start (on Windows its command is a shim that starts Node).
#[derive(Debug, Clone)]
pub struct Codex {
    command: PathBuf,
    home: PathBuf,
    node: Option<PathBuf>,
}

impl Codex {
    /// A Codex whose command is `command` and whose login is read from `home`.
    pub fn at(command: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            home: home.into(),
            node: None,
        }
    }

    /// This Codex, whose windows also find Node in the folder `node`.
    #[must_use]
    pub fn with_node(mut self, node: impl Into<PathBuf>) -> Self {
        self.node = Some(node.into());
        self
    }

    /// Where its command is.
    pub fn command(&self) -> &Path {
        &self.command
    }

    /// The home its login is read from (`CODEX_HOME`).
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// A rig whose windows run the stand-in `stand_in` for Claude and find this
    /// Codex, logged in as the machine is: its command's folder, and Node's
    /// where it is another, are on their `PATH`, and its home is theirs.
    pub fn rig(&self, stand_in: impl Into<PathBuf>) -> Config {
        let folder = self.command.parent().unwrap_or_else(|| Path::new(""));
        let config = Config::new(stand_in)
            .also_on_path(folder)
            .var("CODEX_HOME", self.home.to_string_lossy());
        match &self.node {
            Some(node) if node != folder => config.also_on_path(node),
            _ => config,
        }
    }
}

/// The machine's Codex, found as a person's shell finds it. A machine without
/// one, or without the home it logs in at, is an error that says what to do.
pub fn codex() -> Result<Codex> {
    let command = command("codex")?;
    let home = home_of(
        own_var("CODEX_HOME"),
        own_var("HOME").or_else(|| own_var("USERPROFILE")),
    )
    .ok_or_else(|| Error::Program {
        action: "find the home of",
        program: "codex".to_owned(),
        source: io::Error::new(
            io::ErrorKind::NotFound,
            "this run has neither CODEX_HOME nor a home to look for `.codex` in",
        ),
    })?;
    if !home.is_dir() {
        return Err(Error::File {
            action: "find",
            path: home,
            source: io::Error::new(
                io::ErrorKind::NotFound,
                "Codex keeps its login there: log in with `codex login`, or name its home in CODEX_HOME",
            ),
        });
    }
    let codex = Codex::at(command, home);
    // A machine with no Node has a Codex that needs none (a native one).
    Ok(match command_folder("node") {
        Some(node) => codex.with_node(node),
        None => codex,
    })
}

/// The folder the program `name` is in, if it is on the `PATH`.
fn command_folder(name: &str) -> Option<PathBuf> {
    command(name).ok()?.parent().map(Path::to_path_buf)
}

/// The home Codex reads its login from: `CODEX_HOME` when it is set, otherwise
/// `.codex` in the user's home, made whole against the folder the run is in.
fn home_of(codex_home: Option<String>, user_home: Option<String>) -> Option<PathBuf> {
    let named = |value: Option<String>| value.filter(|value| !value.is_empty());
    let home = match named(codex_home) {
        Some(home) => PathBuf::from(home),
        None => Path::new(&named(user_home)?).join(".codex"),
    };
    Some(std::path::absolute(&home).unwrap_or(home))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// The name a command's file has on this platform.
    fn file_of(command: &str) -> String {
        if cfg!(windows) {
            format!("{command}.cmd")
        } else {
            command.to_owned()
        }
    }

    fn path_of(folders: &[&Path]) -> String {
        std::env::join_paths(folders)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn a_command_is_the_first_file_of_its_name_on_the_path() {
        let (first, second, empty) = (
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let name = file_of("codex");
        fs::write(second.path().join(&name), "second").unwrap();
        let path = path_of(&[empty.path(), first.path(), second.path()]);
        // (On Windows the extension is the one `PATHEXT` names, as it names it.)
        assert_eq!(
            find("codex", &path, Some(".cmd")),
            Some(second.path().join(&name))
        );
        // Another folder has one before it: that one is the command.
        fs::write(first.path().join(&name), "first").unwrap();
        assert_eq!(
            find("codex", &path, Some(".cmd")),
            Some(first.path().join(&name))
        );
    }

    #[test]
    fn a_folder_of_the_name_is_no_command_and_a_command_that_is_not_there_is_none() {
        let folder = tempfile::tempdir().unwrap();
        fs::create_dir(folder.path().join(file_of("codex"))).unwrap();
        let path = path_of(&[folder.path(), Path::new("no-such-folder")]);
        assert_eq!(find("codex", &path, None), None);
        assert_eq!(find("claude", &path, None), None);
        assert_eq!(find("codex", "", None), None);
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_a_command_is_the_file_with_an_extension_of_pathext_in_its_order() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("codex.cmd"), "").unwrap();
        fs::write(folder.path().join("codex.exe"), "").unwrap();
        fs::write(folder.path().join("codex"), "").unwrap();
        let path = path_of(&[folder.path()]);
        assert_eq!(
            find("codex", &path, Some(".CMD;.EXE")),
            Some(folder.path().join("codex.CMD"))
        );
        assert_eq!(
            find("codex", &path, Some(".EXE;.CMD")),
            Some(folder.path().join("codex.EXE"))
        );
        assert_eq!(find("codex", &path, Some(".BAT")), None, "no bare name");
    }

    #[test]
    fn codex_logs_in_at_its_home_when_named_and_in_the_dot_codex_of_the_users_otherwise() {
        let home = |codex: Option<&str>, user: Option<&str>| {
            home_of(codex.map(str::to_owned), user.map(str::to_owned))
        };
        let given = std::env::temp_dir().join("elsewhere");
        let user = std::env::temp_dir().join("user");
        let (given_text, user_text) = (given.to_string_lossy(), user.to_string_lossy());
        assert_eq!(home(Some(&given_text), Some(&user_text)), Some(given));
        assert_eq!(home(None, Some(&user_text)), Some(user.join(".codex")));
        // A variable that is set to nothing is not set.
        assert_eq!(home(Some(""), Some(&user_text)), Some(user.join(".codex")));
        assert_eq!(home(None, None), None);
        assert_eq!(home(None, Some("")), None);
    }

    #[test]
    fn a_home_named_by_a_relative_path_is_made_whole_against_the_folder_of_the_run() {
        let home = home_of(Some("login".to_owned()), None).unwrap();
        assert!(home.is_absolute(), "{}", home.display());
        assert!(home.ends_with("login"));
    }

    #[test]
    fn a_live_run_keeps_what_it_made_when_the_variable_says_anything_but_nothing_or_zero() {
        assert!(!kept(None));
        assert!(!kept(Some("")));
        assert!(!kept(Some("0")));
        assert!(kept(Some("1")));
        assert!(kept(Some("yes")));
    }

    /// A run of the workspace's tests that found a live test not ignored would
    /// spend the machine's quota, on every developer's machine that has Codex,
    /// and fail on every one that has not.
    #[test]
    fn every_live_test_is_ignored_unless_asked_for() {
        let folder = crate::checkout::path("crates/cf-e2e/tests");
        let mut live_files = 0;
        for entry in fs::read_dir(&folder).unwrap() {
            let file = entry.unwrap().path();
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            if !name.starts_with("live_") || !name.ends_with(".rs") {
                continue;
            }
            live_files += 1;
            let source = fs::read_to_string(&file).unwrap();
            // What a live file tests of its own helpers, below its `#[cfg(test)]`,
            // runs nothing live.
            let live = source.split("#[cfg(test)]").next().unwrap();
            let (tests, ignored) = (
                live.matches("#[test]").count(),
                live.matches("#[ignore").count(),
            );
            assert!(tests > 0, "{name} holds no live test");
            assert_eq!(
                tests, ignored,
                "{name}: a live test is ignored unless asked for"
            );
        }
        assert!(live_files > 0, "no live test file in {}", folder.display());
    }

    #[test]
    fn a_program_that_is_on_no_folder_of_the_path_is_an_error_that_names_it() {
        let failed = command("no-such-program-for-the-live-tests").unwrap_err();
        assert!(
            matches!(&failed, Error::Program { action: "find", program, .. }
                if program == "no-such-program-for-the-live-tests"),
            "{failed}"
        );
        assert!(
            failed.to_string().starts_with(
                "could not find `no-such-program-for-the-live-tests`: it is in no folder"
            ),
            "{failed}"
        );
    }

    #[test]
    fn a_codex_is_its_command_and_the_home_it_logs_in_at() {
        let command = Path::new("bin").join(file_of("codex"));
        let codex = Codex::at(&command, "login");
        assert_eq!(codex.command(), command);
        assert_eq!(codex.home(), Path::new("login"));
    }
}
