//! Where a program is on the `PATH` of this run, as a shell finds it: the first
//! folder that holds a file of its name (on Windows, of its name with one of
//! the extensions of `PATHEXT`), but for the folders that hold no harness of
//! the machine's own.

use std::io;
use std::path::{Path, PathBuf};

use crate::process::{own_var, Run};
use crate::{Error, Result};

/// Where the program `name` is, on the `PATH` of this run, as a shell finds it.
/// A machine that has none is an error that says so.
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

/// Where the program `name` is, as [`command`] finds it, if it runs: asked `probe`, it
/// ends 0. A file a shell finds is not always a program: the Microsoft Store's
/// app-execution alias of `python3`, on the `PATH` of a Windows 11 user who has not
/// turned it off, ends 9009 with "Python was not found" whatever it is asked (zeewin,
/// 2026-10-16), and a Python the Store installed is an alias of the same kind in the
/// same folder, so only running it tells the two apart. The `probe` is never empty:
/// the alias asked nothing opens the Store.
pub fn working_command(name: &str, probe: &[&str]) -> Result<PathBuf> {
    runs(command(name)?, name, probe)
}

/// `found`, the program `name`, if it ends 0 when it is asked `probe`; an error that
/// says how it ended otherwise.
pub(super) fn runs(found: PathBuf, name: &str, probe: &[&str]) -> Result<PathBuf> {
    let ran = Run::new(&found)
        .args(probe)
        .inheriting_env()
        .closed_input()
        .run()?;
    if ran.code == Some(0) {
        return Ok(found);
    }
    Err(Error::Program {
        action: "run",
        program: name.to_owned(),
        source: io::Error::other(format!(
            "{} {} is no program that runs: {ran}",
            found.display(),
            probe.join(" ")
        )),
    })
}

/// [`command`] on the `path` and the extensions it is given.
pub(super) fn find(command: &str, path: &str, extensions: Option<&str>) -> Option<PathBuf> {
    let suffixes: Vec<&str> = if cfg!(windows) {
        extensions
            .unwrap_or(".COM;.EXE;.BAT;.CMD")
            .split(';')
            .filter(|extension| !extension.is_empty())
            .collect()
    } else {
        vec![""]
    };
    std::env::split_paths(path)
        .filter(|folder| !holds_no_harness(folder))
        .find_map(|folder| {
            suffixes
                .iter()
                .map(|suffix| folder.join(format!("{command}{suffix}")))
                .find(|candidate| candidate.is_file())
        })
}

/// Whether a folder of the `PATH` holds a stand-in for a harness and not the
/// harness. A terminal app's shims come first on the `PATH` inside its panes
/// (cmux has its own `claude`) and start nothing outside it: an eval window on
/// one printed nothing for five minutes (2026-10-01). And the evals' own wrappers
/// (`evals/bin`) are first on the `PATH` of a run's children, and a wrapper
/// that found itself would start itself.
fn holds_no_harness(folder: &Path) -> bool {
    let names: Vec<String> = folder
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    names
        .iter()
        .any(|name| name == "cmux-cli-shims" || name == "cmux.app")
        || names.ends_with(&["evals".to_owned(), "bin".to_owned()])
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
    fn a_terminal_apps_shims_and_the_evals_wrappers_are_no_harness_of_the_machine() {
        let root = tempfile::tempdir().unwrap();
        let name = file_of("claude");
        let (cmux, app, wrappers, real) = (
            root.path().join("cmux-cli-shims"),
            root.path().join("cmux.app").join("Contents").join("bin"),
            root.path().join("evals").join("bin"),
            root.path().join("real"),
        );
        for folder in [&cmux, &app, &wrappers, &real] {
            fs::create_dir_all(folder).unwrap();
            fs::write(folder.join(&name), "").unwrap();
        }
        // Each is first on the path, and none of them is the command.
        let path = path_of(&[&cmux, &app, &wrappers, &real]);
        assert_eq!(find("claude", &path, Some(".cmd")), Some(real.join(&name)));
        // Without a real one there is none: a shim is not taken for a harness.
        let only = path_of(&[&cmux, &app, &wrappers]);
        assert_eq!(find("claude", &only, Some(".cmd")), None);
        // A folder of another name that ends in `bin` is as good as any.
        assert!(!holds_no_harness(&root.path().join("tools").join("bin")));
        assert!(holds_no_harness(&root.path().join("EVALS").join("Bin")));
    }

    /// A program of the name `command` in `folder` that says `said` and ends `code`: a
    /// script on Unix, a batch file on Windows (where a status is not cut to a byte).
    fn program(folder: &Path, command: &str, said: &str, code: u32) -> PathBuf {
        let path = folder.join(file_of(command));
        let text = if cfg!(windows) {
            format!("@echo {said}\r\n@exit /b {code}\r\n")
        } else {
            format!("#!/bin/sh\necho '{said}'\nexit {code}\n")
        };
        crate::files::write_executable(&path, text).unwrap();
        path
    }

    #[test]
    fn a_file_of_the_name_that_does_not_run_as_it_is_asked_is_no_working_program() {
        // The Store's alias of `python3`, as zeewin has it first on its PATH: a shell finds
        // it, and whatever it is asked it ends 9009 and says there is no Python.
        let (alias, real) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let stub = program(
            alias.path(),
            "python3",
            "Python was not found; run without arguments to install from the Microsoft Store",
            9009,
        );
        let path = path_of(&[alias.path(), real.path()]);
        let found = find("python3", &path, Some(".cmd")).unwrap();
        assert_eq!(found, stub);
        let refused = runs(found, "python3", &["--version"]).unwrap_err();
        let said = refused.to_string();
        assert!(
            matches!(&refused, Error::Program { action: "run", program, .. } if program == "python3"),
            "{said}"
        );
        assert!(said.contains("is no program that runs"), "{said}");
        assert!(said.contains("Python was not found"), "{said}");
        // One that runs is taken, as it was found.
        let python = program(real.path(), "python3", "Python 3.12.1", 0);
        assert_eq!(
            runs(python.clone(), "python3", &["--version"]).unwrap(),
            python
        );
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
}
