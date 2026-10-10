//! The demo project the chief's staff works in, and what the run finds in its
//! folder: the files the designer wrote, which its role text limits to the one
//! the task named.

use std::io;
use std::path::Path;

use cf_e2e::process::Run;
use cf_e2e::{checkout, files, live, Error, Result};

use crate::{report, Log, IMAGE};

/// The demo project: a folder with a git repository in it and a README.
pub fn project(folder: &Path) -> Result {
    files::make_dir(folder)?;
    let ran = Run::new(live::command("git")?)
        .args(["init", "--quiet"])
        .cwd(folder)
        .inheriting_env()
        .run()?;
    if ran.code != Some(0) {
        return Err(Error::Program {
            action: "run",
            program: "git init".to_owned(),
            source: io::Error::other(ran.to_string()),
        });
    }
    files::write(
        &folder.join("README.md"),
        "# Honey shop\n\nA demo project: the image designer draws its pictures here.\n",
    )
}

/// The files of the project's folder but for git's own, each from the folder
/// with `/` between folders.
fn files_of(folder: &Path) -> Result<Vec<String>> {
    Ok(checkout::files_below(folder, &[".git"])?
        .iter()
        .filter_map(|file| file.strip_prefix(folder).ok())
        .map(|file| {
            let parts: Vec<_> = file
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect();
            parts.join("/")
        })
        .collect())
}

/// What the project's folder holds, for a message that says where the image is
/// not.
pub fn holds(folder: &Path) -> String {
    match files_of(folder) {
        Ok(files) => format!("the folder holds: {}", files.join(", ")),
        Err(failed) => format!("the folder could not be listed: {failed}"),
    }
}

/// The files its role text does not let the designer write: all of `files`
/// but the README the project began with and the image the task named.
fn unexpected(files: Vec<String>) -> Vec<String> {
    files
        .into_iter()
        .filter(|file| file != "README.md" && file != IMAGE)
        .collect()
}

/// What the run saw that is no failure of the brief but is worth a look: files
/// the designer wrote that its role text says it must not, and what the
/// daemon's log warned of (`daemon_log`).
pub fn notes(daemon_log: &str, folder: &Path, log: &Log) {
    let others = unexpected(files_of(folder).unwrap_or_default());
    if !others.is_empty() {
        log.say(format!(
            "note: the role text says to write no other file, and the folder also holds {}",
            others.join(", ")
        ));
    }
    let warned = report::warnings(daemon_log);
    if !warned.is_empty() {
        log.block("note: the daemon's log warned", &warned.join("\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_of_a_folder_are_listed_from_it_with_slashes_and_without_gits_own() {
        let folder = tempfile::tempdir().unwrap();
        for file in [
            "README.md",
            "images/honey.png",
            ".git/HEAD",
            ".git/objects/ab/cd",
        ] {
            let at = file.replace('/', std::path::MAIN_SEPARATOR_STR);
            files::write(&folder.path().join(at), "x").unwrap();
        }
        assert_eq!(
            files_of(folder.path()).unwrap(),
            ["README.md", "images/honey.png"]
        );
        assert_eq!(
            holds(folder.path()),
            "the folder holds: README.md, images/honey.png"
        );
        let missing = folder.path().join("missing");
        assert!(holds(&missing).starts_with("the folder could not be listed: "));
    }

    #[test]
    fn only_the_readme_and_the_image_are_files_the_designer_may_leave() {
        let files = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();
        assert!(unexpected(files(&["README.md", "images/honey.png"])).is_empty());
        assert!(unexpected(files(&[])).is_empty());
        assert_eq!(
            unexpected(files(&[
                "README.md",
                "images/honey.png",
                "images/draft.png",
                "tmp.py"
            ])),
            ["images/draft.png", "tmp.py"]
        );
    }
}
