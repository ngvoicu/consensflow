//! The folder a chief works in: the scenario's fixture in a fresh workspace, which
//! Claude trusts, with nothing it remembered from the last run. Its `memory/` in
//! `~/.claude/projects/<the folder's name>` is read by every later session in that
//! folder, so a run's chief would learn from the last run's: it is cleared first.

use std::fs;
use std::path::{Path, PathBuf};

use cf_e2e::rig::project_name;
use cf_e2e::{checkout, files};

use super::machine::Machine;
use crate::{Error, Result};

/// The fixture `name`: `evals/fixtures/<name>/`.
pub fn fixture(name: &str) -> PathBuf {
    checkout::path("evals/fixtures").join(name)
}

/// The folder where Claude Code keeps what it remembers of `workspace`.
fn claude_memory(home: &Path, workspace: &Path) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join(project_name(&workspace.to_string_lossy()))
        .join("memory")
}

/// The workspace of a run, made fresh from the fixture `name`.
pub fn fresh(machine: &Machine, name: &str) -> Result<PathBuf> {
    let workspace = machine.workspace();
    renew(&machine.home, &workspace, &fixture(name))?;
    Ok(workspace)
}

/// `workspace` made again from the folder `from`, and what Claude Code (whose home
/// is `home`) remembered about it forgotten.
fn renew(home: &Path, workspace: &Path, from: &Path) -> Result {
    // A folder that is not there is already gone.
    let _ = fs::remove_dir_all(workspace);
    files::make_dir(workspace)?;
    let _ = fs::remove_dir_all(claude_memory(home, workspace));
    copy_tree(from, workspace)
}

/// The files and folders of `from`, copied into `to`, which is there.
fn copy_tree(from: &Path, to: &Path) -> Result {
    for entry in fs::read_dir(from).map_err(Error::file("list", from))? {
        let entry = entry.map_err(Error::file("list", from))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            files::make_dir(&target)?;
            copy_tree(&source, &target)?;
        } else {
            files::copy(&source, &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, file: &str, text: &str) {
        files::write(&root.join(file), text).unwrap();
    }

    #[test]
    fn the_workspace_is_the_fixture_and_nothing_else_of_what_was_there() {
        let root = tempfile::tempdir().unwrap();
        let (home, fixture, workspace) = (
            root.path().join("home"),
            root.path().join("fixture"),
            root.path().join("workspace"),
        );
        write(&fixture, "site/index.html", "home");
        write(&fixture, "docs/guide.md", "guide");
        write(&workspace, "site/notes.md", "left by the last run");
        write(&workspace, "site/index.html", "changed");
        renew(&home, &workspace, &fixture).unwrap();
        assert_eq!(
            files::read_string(&workspace.join("site/index.html")).unwrap(),
            "home"
        );
        assert_eq!(
            files::read_string(&workspace.join("docs/guide.md")).unwrap(),
            "guide"
        );
        assert!(!workspace.join("site/notes.md").exists());
        // Made where there was nothing too.
        let fresh = root.path().join("another");
        renew(&home, &fresh, &fixture).unwrap();
        assert!(fresh.join("docs/guide.md").is_file());
    }

    #[test]
    fn what_claude_remembered_of_the_workspace_is_cleared_and_what_it_remembered_of_others_is_not()
    {
        let root = tempfile::tempdir().unwrap();
        let (home, fixture, workspace) = (
            root.path().join("home"),
            root.path().join("fixture"),
            root.path().join("workspace"),
        );
        write(&fixture, "a.txt", "a");
        let memory = claude_memory(&home, &workspace);
        write(&memory, "MEMORY.md", "# what it learned");
        let conversation = memory.parent().unwrap().join("s-1.jsonl");
        write(memory.parent().unwrap(), "s-1.jsonl", "{}\n");
        let other = home.join(".claude/projects/-Users-me-work/memory/MEMORY.md");
        write(
            &home,
            ".claude/projects/-Users-me-work/memory/MEMORY.md",
            "theirs",
        );
        renew(&home, &workspace, &fixture).unwrap();
        assert!(!memory.exists(), "the memory of the workspace is cleared");
        assert!(conversation.exists(), "its conversations are the person's");
        assert!(other.exists(), "so is what it remembered of another folder");
    }

    #[test]
    fn claude_names_the_folder_of_a_workspace_by_its_path_with_a_dash_for_each_mark() {
        assert_eq!(
            claude_memory(
                Path::new("/Users/x"),
                Path::new("/Users/x/.consensflow-candidate/evals/workspace")
            ),
            Path::new(
                "/Users/x/.claude/projects/-Users-x--consensflow-candidate-evals-workspace/memory"
            )
        );
    }

    #[test]
    fn a_fixture_that_is_not_there_is_an_error_that_names_it() {
        let root = tempfile::tempdir().unwrap();
        let failed = renew(
            root.path(),
            &root.path().join("workspace"),
            &root.path().join("none"),
        )
        .unwrap_err();
        assert!(failed.to_string().contains("none"), "{failed}");
    }

    #[test]
    fn every_scenarios_fixture_is_in_the_repository() {
        for id in crate::scenarios::IDS {
            let scenario = crate::scenarios::by_id(id).unwrap();
            assert!(
                fixture(scenario.fixture).is_dir(),
                "{id}: {}",
                scenario.fixture
            );
        }
        assert!(checkout::path("evals/fixtures/no-card.md").is_file());
    }
}
