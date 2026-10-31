//! The home the windows of `live own-cf` run on: a folder of the run's own below the live
//! folders (`~/.consensflow-candidate/live/own-cf-home`, a real path: Devin's window shows no
//! answer in a folder macOS spells through `/var`), shaped as the owner's is where it matters
//! here. A decoy `cf` (the stand-in [`calls`] program, which writes down every call it is
//! given beside itself) is in its `.local/bin` and in its package manager's folder (Homebrew's
//! on a Mac, Scoop's shims on Windows, Linuxbrew's elsewhere), and every login and
//! interactive profile a shell reads puts both first on the `PATH`, as the owner's
//! `.zprofile` and `.zshrc` put `~/.local/bin` and `/opt/homebrew/bin`. A `cf` some other tool
//! installs there (Cloud Foundry's is called `cf`; the stale launcher of 2026-10-01 was in
//! `~/.local/bin`) is then the first a shell that reads its profile finds, ahead of the window's
//! own, which the daemon put first on the window's `PATH`.
//!
//! The folder is made again by each run: the decoys and the profiles written over, the decoys'
//! logs taken away (by their own paths), so a call is one of this run's.

use std::path::{Path, PathBuf};

use cf_e2e::live::{calls, live_folder};
use cf_e2e::{files, Result};

/// The folder below the live folders the home is.
pub const FOLDER: &str = "own-cf-home";

/// A folder of the home a decoy `cf` is in, and what a report calls it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoy {
    pub label: &'static str,
    pub folder: PathBuf,
}

/// The home of a run, made.
#[derive(Debug)]
pub struct Home {
    pub root: PathBuf,
    pub decoys: Vec<Decoy>,
}

/// The folders below `root` that hold a decoy, with what a report calls each: `.local/bin`,
/// and the package manager's folder of this platform (`windows` says which).
pub fn decoys_in(root: &Path, windows: bool) -> Vec<Decoy> {
    let (label, below): (&'static str, &[&str]) = if windows {
        ("Scoop's shims (~/scoop/shims)", &["scoop", "shims"])
    } else if cfg!(target_os = "macos") {
        ("Homebrew's bin (~/homebrew/bin)", &["homebrew", "bin"])
    } else {
        ("Linuxbrew's bin (~/.linuxbrew/bin)", &[".linuxbrew", "bin"])
    };
    vec![
        Decoy {
            label: "~/.local/bin",
            folder: root.join(".local").join("bin"),
        },
        Decoy {
            label,
            folder: below
                .iter()
                .fold(root.to_path_buf(), |path, part| path.join(part)),
        },
    ]
}

/// `path` as a POSIX shell names it: the same on Unix; on Windows (`windows`) as Git Bash
/// names a drive's folder, `C:\Users\me` as `/c/Users/me`.
pub fn shell_path(path: &Path, windows: bool) -> String {
    let text = path.to_string_lossy().into_owned();
    if !windows {
        return text;
    }
    let forward = text.replace('\\', "/");
    let mut chars = forward.chars();
    match (chars.next(), chars.next()) {
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic() => {
            format!("/{}{}", drive.to_ascii_lowercase(), chars.as_str())
        }
        _ => forward,
    }
}

/// The profile files a shell reads at its start, each with its text: every one puts the
/// decoys' folders first on the `PATH`, the package manager's and then `.local/bin` in front
/// of it, as the owner's put `/opt/homebrew/bin` (`brew shellenv`) and then `~/.local/bin`.
/// The folders are written out whole, as the owner's own `.zshrc` writes one, so a shell
/// given another `HOME` (Muse's, below) reads the same.
pub fn profiles(decoys: &[Decoy], windows: bool) -> Vec<(&'static str, String)> {
    let mut lines = vec![
        "# ConsensFlow's live own-cf: the owner's shape, decoy cf folders first on PATH."
            .to_owned(),
    ];
    for decoy in decoys.iter().rev() {
        lines.push(format!(
            "export PATH=\"{}:$PATH\"",
            shell_path(&decoy.folder, windows)
        ));
    }
    let text = lines.join("\n") + "\n";
    [
        ".zshenv",
        ".zprofile",
        ".zshrc",
        ".bash_profile",
        ".bashrc",
        ".profile",
    ]
    .map(|name| (name, text.clone()))
    .to_vec()
}

/// Makes the home below the live folders: the decoys (the stand-in `program`), their logs
/// taken away, and the profiles.
pub fn make(program: &Path) -> Result<Home> {
    let root = live_folder(FOLDER)?;
    let windows = cfg!(windows);
    let decoys = decoys_in(&root, windows);
    for decoy in &decoys {
        calls::install(&decoy.folder, program)?;
        let log = decoy.folder.join(calls::LOG);
        if log.exists() {
            std::fs::remove_file(&log).map_err(|source| cf_e2e::Error::File {
                action: "remove",
                path: log.clone(),
                source,
            })?;
        }
    }
    for (name, text) in profiles(&decoys, windows) {
        files::write(&root.join(name), text)?;
    }
    Ok(Home { root, decoys })
}

/// The calls each decoy was given, by its label: those that were given none are left out.
pub fn called(home: &Home) -> Vec<(&'static str, Vec<String>)> {
    home.decoys
        .iter()
        .filter_map(|decoy| {
            let calls: Vec<String> = calls::calls(&decoy.folder)
                .into_iter()
                .map(|call| call.words)
                .collect();
            (!calls.is_empty()).then_some((decoy.label, calls))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decoy_is_in_local_bin_and_in_the_package_managers_folder_of_the_platform() {
        let root = Path::new("/h");
        let unix = decoys_in(root, false);
        assert_eq!(unix[0].folder, root.join(".local").join("bin"));
        assert_eq!(unix[0].label, "~/.local/bin");
        if cfg!(target_os = "macos") {
            assert_eq!(unix[1].folder, root.join("homebrew").join("bin"));
        }
        let windows = decoys_in(root, true);
        assert_eq!(windows[1].folder, root.join("scoop").join("shims"));
        assert_eq!(windows.len(), 2);
    }

    #[test]
    fn a_windows_path_is_written_as_git_bash_names_it_and_a_unix_one_as_it_is() {
        assert_eq!(
            shell_path(Path::new(r"C:\Users\me\.local\bin"), true),
            "/c/Users/me/.local/bin"
        );
        assert_eq!(
            shell_path(Path::new("/h/.local/bin"), false),
            "/h/.local/bin"
        );
    }

    #[test]
    fn every_profile_puts_local_bin_first_then_the_package_managers_folder_by_whole_paths() {
        let decoys = decoys_in(Path::new("/h"), false);
        let written = profiles(&decoys, false);
        let names: Vec<&str> = written.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                ".zshenv",
                ".zprofile",
                ".zshrc",
                ".bash_profile",
                ".bashrc",
                ".profile"
            ]
        );
        for (_, text) in &written {
            let exports: Vec<&str> = text
                .lines()
                .filter(|line| line.starts_with("export"))
                .collect();
            // The last export is the first on the PATH: `.local/bin`, ahead of the package
            // manager's folder.
            assert_eq!(exports.len(), 2, "{text}");
            assert!(exports[0].contains(&decoys[1].folder.to_string_lossy().into_owned()));
            assert_eq!(exports[1], "export PATH=\"/h/.local/bin:$PATH\"");
            assert!(!text.contains("$HOME"), "whole paths: {text}");
        }
    }
}
