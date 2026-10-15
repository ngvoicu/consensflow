//! The folder a chief works in: the scenario's fixture in a fresh workspace, which
//! Claude trusts, with nothing it remembered from the last run. Its `memory/` in
//! `~/.claude/projects/<the folder's name>` is read by every later session in that
//! folder, so a run's chief would learn from the last run's: it is cleared first.
//! A scenario with a change of the owner's not committed has the workspace made a
//! git repository of the fixture, and the change made after the commit.

use std::fs;
use std::path::{Path, PathBuf};

use cf_e2e::live;
use cf_e2e::process::Run;
use cf_e2e::rig::project_name;
use cf_e2e::{checkout, files};

use super::machine::Machine;
use crate::scenarios::{Scenario, Uncommitted};
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

/// The workspace of a run, made fresh from the fixture of `scenario`, with the
/// owner's uncommitted change if it has one.
pub fn fresh(machine: &Machine, scenario: &Scenario) -> Result<PathBuf> {
    let workspace = machine.workspace();
    renew(&machine.home, &workspace, &fixture(scenario.fixture))?;
    if let Some(change) = &scenario.uncommitted {
        let git = Run::new(live::command("git")?).inheriting_env();
        uncommitted(&git, &workspace, change)?;
    }
    Ok(workspace)
}

/// `workspace` made a git repository of what it holds, committed, and then `change`
/// made in its file and not committed: the owner's work in progress. `git` is the
/// program with the environment it runs in.
///
/// Git keeps the files' line ends as they are, on Windows too, so that a file a worker
/// restores from the commit is the fixture's to the byte. Nothing of the person's git
/// has a part in the setup: neither their global nor their system configuration (a
/// hook folder, an excludes file, signing, line-end rules), nor a template folder
/// (`--template=`, over `GIT_TEMPLATE_DIR` too), and over what the environment may
/// still pass (`GIT_CONFIG_PARAMETERS`) the hooks are an empty folder and nothing is
/// signed. Only the setup runs so: the workers run the person's git as it is.
fn uncommitted(git: &Run, workspace: &Path, change: &Uncommitted) -> Result {
    let repository = workspace.join(".git");
    let hooks = repository.join("hooks-off");
    let setup = |words: &[&str]| -> Result {
        let ran = git
            .clone()
            .var("GIT_CONFIG_GLOBAL", repository.join("no-global-config"))
            .var("GIT_CONFIG_NOSYSTEM", "1")
            .arg("-c")
            .arg(format!("core.hooksPath={}", hooks.display()))
            .args(["-c", "commit.gpgsign=false"])
            .args(words)
            .cwd(workspace)
            .run()?;
        if ran.code == Some(0) {
            return Ok(());
        }
        Err(Error::Refused(format!(
            "git {} in {} ended with {ran}: {}",
            words.join(" "),
            workspace.display(),
            ran.stderr.trim()
        )))
    };
    setup(&["init", "-q", "--template="])?;
    files::make_dir(&hooks)?;
    setup(&["config", "core.autocrlf", "false"])?;
    setup(&["add", "-A"])?;
    setup(&[
        "-c",
        "user.name=Owner",
        "-c",
        "user.email=owner@example.com",
        "commit",
        "-q",
        "--no-verify",
        "-m",
        "The site",
    ])?;
    let file = workspace.join(change.file);
    let text = files::read_string(&file)?;
    if !text.contains(change.find) {
        return Err(Error::Refused(format!(
            "{} does not hold the owner's change to make: {}",
            file.display(),
            change.find
        )));
    }
    files::write(&file, text.replacen(change.find, change.replace, 1))?;
    Ok(())
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

    /// The machine's `git`, in the test's own environment.
    fn plain_git() -> Run {
        Run::new(live::command("git").unwrap()).inheriting_env()
    }

    /// What `git` says, run with `words` in `folder`.
    fn git(folder: &Path, words: &[&str]) -> String {
        let ran = plain_git().args(words).cwd(folder).run().unwrap();
        assert_eq!(ran.code, Some(0), "{ran}");
        ran.stdout
    }

    /// The owner's change of the tests.
    const CHANGE: Uncommitted = Uncommitted {
        file: "site/evaluare.html",
        find: "0 și 100.",
        replace: "0 și 10.",
    };

    #[test]
    fn the_owner_s_change_is_in_the_file_and_not_in_the_commit_of_the_fixture() {
        let root = tempfile::tempdir().unwrap();
        let (home, fixture, workspace) = (
            root.path().join("home"),
            root.path().join("fixture"),
            root.path().join("workspace"),
        );
        write(
            &fixture,
            "site/evaluare.html",
            "un scor între 0 și 100.\r\n",
        );
        write(&fixture, "site/index.html", "home\r\n");
        renew(&home, &workspace, &fixture).unwrap();
        uncommitted(&plain_git(), &workspace, &CHANGE).unwrap();
        assert_eq!(
            files::read_string(&workspace.join("site/evaluare.html")).unwrap(),
            "un scor între 0 și 10.\r\n"
        );
        // One commit of the whole fixture, and the owner's change the only one not in it.
        assert_eq!(
            git(&workspace, &["rev-list", "--count", "HEAD"]).trim(),
            "1"
        );
        assert_eq!(
            git(&workspace, &["status", "--porcelain"]),
            " M site/evaluare.html\n"
        );
        // A worker that restores the file from the commit gets the fixture's, to the byte.
        git(&workspace, &["checkout", "--", "site/evaluare.html"]);
        assert_eq!(
            fs::read(workspace.join("site/evaluare.html")).unwrap(),
            fs::read(fixture.join("site/evaluare.html")).unwrap()
        );
    }

    #[test]
    fn the_setup_of_the_owner_s_repository_takes_nothing_of_the_person_s_git() {
        let root = tempfile::tempdir().unwrap();
        let (home, fixture, workspace, theirs) = (
            root.path().join("home"),
            root.path().join("fixture"),
            root.path().join("workspace"),
            root.path().join("theirs"),
        );
        write(&fixture, "site/evaluare.html", "un scor între 0 și 100.\n");
        write(&fixture, "site/index.html", "home\n");
        write(&fixture, "README.md", "readme\n");
        renew(&home, &workspace, &fixture).unwrap();
        // The person's git, as hostile as it gets: hooks that refuse every commit
        // (`--no-verify` does not stop a prepare-commit-msg), in their hook folder and
        // in a template folder; an excludes file and a template's exclude that hide the
        // pages; signing with a program that is not there; CR LF on the way in.
        let refuse = "#!/bin/sh\nexit 1\n";
        for hook in ["pre-commit", "prepare-commit-msg", "commit-msg"] {
            files::write_executable(&theirs.join("hooks").join(hook), refuse).unwrap();
            files::write_executable(&theirs.join("templates/hooks").join(hook), refuse).unwrap();
        }
        write(&theirs, "templates/info/exclude", "*\n");
        write(&theirs, "excludes", "*.html\n");
        // Git reads a backslash in its configuration as an escape: a path of Windows is
        // written with slashes.
        let path = |name: &str| theirs.join(name).display().to_string().replace('\\', "/");
        let configuration = format!(
            "[core]\n\thooksPath = {}\n\texcludesFile = {}\n\tautocrlf = true\n\
             [commit]\n\tgpgsign = true\n[gpg]\n\tprogram = {}\n[init]\n\ttemplateDir = {}\n",
            path("hooks"),
            path("excludes"),
            path("no-such-gpg"),
            path("templates"),
        );
        write(&theirs, "global", &configuration);
        write(&theirs, "system", &configuration);
        let hostile = plain_git()
            .var("GIT_CONFIG_GLOBAL", theirs.join("global"))
            .var("GIT_CONFIG_SYSTEM", theirs.join("system"))
            .var("GIT_TEMPLATE_DIR", theirs.join("templates"))
            .var(
                "GIT_CONFIG_PARAMETERS",
                format!(
                    "'core.hooksPath'='{}' 'commit.gpgsign'='true' 'gpg.program'='{}'",
                    path("hooks"),
                    path("no-such-gpg")
                ),
            );
        uncommitted(&hostile, &workspace, &CHANGE).unwrap();
        // The whole fixture in one commit, with its line ends, and the owner's change
        // the only one not in it.
        assert_eq!(
            git(&workspace, &["ls-files"]),
            "README.md\nsite/evaluare.html\nsite/index.html\n"
        );
        assert_eq!(
            git(&workspace, &["rev-list", "--count", "HEAD"]).trim(),
            "1"
        );
        assert_eq!(
            git(&workspace, &["status", "--porcelain"]),
            " M site/evaluare.html\n"
        );
        assert_eq!(git(&workspace, &["show", "HEAD:site/index.html"]), "home\n");
        // No template came in with the repository, and none of its hooks.
        assert!(!workspace.join(".git/info/exclude").exists());
        assert!(!workspace.join(".git/hooks").exists());
    }

    #[test]
    fn an_owner_s_change_the_fixture_has_no_place_for_is_an_error_that_names_it() {
        let root = tempfile::tempdir().unwrap();
        let (home, fixture, workspace) = (
            root.path().join("home"),
            root.path().join("fixture"),
            root.path().join("workspace"),
        );
        write(&fixture, "site/evaluare.html", "un scor între 0 și 10.");
        renew(&home, &workspace, &fixture).unwrap();
        let failed = uncommitted(&plain_git(), &workspace, &CHANGE)
            .unwrap_err()
            .to_string();
        assert!(failed.contains("0 și 100."), "{failed}");
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

    #[test]
    fn every_owner_s_change_a_scenario_makes_has_its_place_in_the_fixture_once() {
        let mut changes = 0;
        for id in crate::scenarios::IDS {
            let scenario = crate::scenarios::by_id(id).unwrap();
            let Some(change) = scenario.uncommitted else {
                continue;
            };
            let text = files::read_string(&fixture(scenario.fixture).join(change.file)).unwrap();
            assert_eq!(text.matches(change.find).count(), 1, "{id}");
            assert!(!text.contains(change.replace), "{id}");
            changes += 1;
        }
        assert_eq!(changes, 1);
    }
}
