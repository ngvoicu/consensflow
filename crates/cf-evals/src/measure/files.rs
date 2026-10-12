//! Which files of the fixture a run changed or added: the workspace the chief
//! worked in against the fixture it was copied from, any harness.

use std::fs;
use std::path::Path;

use crate::{Error, Result};

/// Files a harness keeps in the project for itself, which no agent wrote: Claude
/// Code's lock while a wakeup it scheduled waits (a Claude chief on Windows
/// scheduled one to check on its worker, 2026-10-04).
const HARNESS_OWN: [&str; 1] = [".claude/scheduled_tasks.lock"];

/// Files of the fixture that differ in the workspace, and files the run added,
/// relative to the folders, with `/` between the folders of a path on every
/// platform (a report names a file one way). A `.git` and a `node_modules` are
/// not looked into.
pub fn changed(fixture: &Path, workspace: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    list(workspace, workspace, &mut files)?;
    let mut changed = Vec::new();
    for name in files {
        if !same(&fixture.join(&name), &workspace.join(&name))? {
            changed.push(name);
        }
    }
    changed.sort();
    Ok(changed)
}

/// Every file below `dir`, as a path relative to `root`.
fn list(root: &Path, dir: &Path, found: &mut Vec<String>) -> Result {
    for entry in fs::read_dir(dir).map_err(Error::file("list", dir))? {
        let entry = entry.map_err(Error::file("list", dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == "node_modules" {
            continue;
        }
        let path = entry.path();
        // What a link points to is what it is, as `stat` has it.
        if fs::metadata(&path)
            .map_err(Error::file("look at", &path))?
            .is_dir()
        {
            list(root, &path, found)?;
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let file: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        let file = file.join("/");
        if !HARNESS_OWN.contains(&file.as_str()) {
            found.push(file);
        }
    }
    Ok(())
}

/// Whether the file `before` is there and holds what the file `after` does.
fn same(before: &Path, after: &Path) -> Result<bool> {
    if !before.is_file() {
        return Ok(false);
    }
    let (before, after) = (
        fs::read(before).map_err(Error::file("read", before))?,
        fs::read(after).map_err(Error::file("read", after))?,
    );
    Ok(before == after)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, file: &str, text: &str) {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn the_fixture_files_that_differ_in_the_workspace_and_the_files_the_run_added_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        for (file, text) in [
            ("fixture/site/index.html", "home"),
            ("fixture/site/evaluare.html", "scor 0 și 100"),
            ("fixture/docs/guide.md", "guide"),
            ("workspace/site/index.html", "home"),
            ("workspace/site/evaluare.html", "scor 0 și 10"),
            ("workspace/docs/guide.md", "guide"),
            ("workspace/site/legislatie.html", "new page"),
            ("workspace/.claude/scheduled_tasks.lock", "{\"pid\":1}"),
        ] {
            write(dir.path(), file, text);
        }
        assert_eq!(
            changed(&dir.path().join("fixture"), &dir.path().join("workspace")).unwrap(),
            ["site/evaluare.html", "site/legislatie.html"]
        );
    }

    #[test]
    fn a_git_folder_and_a_node_modules_are_not_looked_into_and_a_deleted_file_is_not_a_change() {
        let dir = tempfile::tempdir().unwrap();
        for (file, text) in [
            ("fixture/a.txt", "a"),
            ("fixture/b.txt", "b"),
            ("workspace/a.txt", "a"),
            ("workspace/.git/HEAD", "ref"),
            ("workspace/node_modules/x/index.js", "x"),
            ("workspace/site/node_modules/y.js", "y"),
        ] {
            write(dir.path(), file, text);
        }
        // b.txt was deleted: the workspace does not have it, which is no file of it.
        assert!(
            changed(&dir.path().join("fixture"), &dir.path().join("workspace"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_workspace_that_is_not_there_is_named_in_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let failed = changed(dir.path(), &dir.path().join("missing")).unwrap_err();
        assert!(failed.to_string().contains("missing"), "{failed}");
    }
}
