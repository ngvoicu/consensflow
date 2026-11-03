//! The folder a chief works in: the scenario's fixture in a fresh workspace, which
//! Claude trusts, with nothing it remembered from the last run. Its `memory/` in
//! `<Claude's config folder>/projects/<the folder's name>` is read by every later
//! session in that folder, so a run's chief would learn from the last run's: it is
//! cleared first. The config folder is the run's own (a throwaway, shared by the runs of
//! one `--repeat`: [`super::login`]), never the user's `~/.claude`, where nothing is
//! cleared or written.
//! A scenario with a change of the owner's not committed has the workspace made a
//! git repository of the fixture, and the change made after the commit.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

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

/// The folder where Claude Code, whose config folder is `config`, keeps what it remembers of
/// `workspace`.
fn claude_memory(config: &Path, workspace: &Path) -> PathBuf {
    config
        .join("projects")
        .join(project_name(&workspace.to_string_lossy()))
        .join("memory")
}

/// A name for the workspace of one play, which no other play of any run has.
fn unique_name(scenario: &Scenario) -> String {
    name_at(jiff::Timestamp::now(), scenario)
}

/// The name of a play made at `at`: the instant, the process, and a count of the plays of this
/// process. Two plays of one process in one instant are told apart by the count, and two processes
/// whose counts began alike (each at none) by their numbers.
fn name_at(at: jiff::Timestamp, scenario: &Scenario) -> String {
    static PLAYS: AtomicU32 = AtomicU32::new(0);
    format!(
        "{}-{}-{}-{}",
        crate::clock::stamp(at),
        std::process::id(),
        PLAYS.fetch_add(1, Ordering::Relaxed),
        scenario.id
    )
}

/// The workspace of a play, made fresh from the fixture of `scenario` in a folder of its own
/// ([`Machine::workspaces`]), with the owner's uncommitted change if it has one. Two plays, in
/// one run or in two that go at once, never share a folder: what one writes (a stray file of
/// another scenario's, a page half made) is not in the other's.
pub fn fresh(machine: &Machine, scenario: &Scenario) -> Result<PathBuf> {
    let workspace = machine.workspaces().join(unique_name(scenario));
    renew(
        machine.claude_config()?.as_deref(),
        &workspace,
        &fixture(scenario.fixture),
    )?;
    if let Some(change) = &scenario.uncommitted {
        let git = Run::new(live::command("git")?).inheriting_env();
        uncommitted(&git, &workspace, change)?;
    }
    Ok(workspace)
}

/// The workspace of a play, taken away once the play is measured. Only a folder made by
/// [`fresh`] goes (one that is directly below a folder called `workspaces`); a path that is
/// none is left alone. A folder that is not there is already gone.
pub fn retire(workspace: &Path) {
    let made = workspace
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|parent| parent == "workspaces");
    if made {
        let _ = fs::remove_dir_all(workspace);
    }
}

/// `workspace` made a git repository of what it holds, committed, and then `change`
/// made in its file and not committed: the owner's work in progress. `git` is the
/// program with the environment it runs in.
///
/// Git keeps the files' line ends as they are, on Windows too, so that a file a worker
/// restores from the commit is the fixture's to the byte. Nothing of the person's git
/// has a part in the setup (a hook folder, an excludes file, signing, line-end rules):
/// neither their global nor their system configuration, nor what their environment
/// passes in `GIT_CONFIG_PARAMETERS` or in `GIT_CONFIG_COUNT` and its
/// `GIT_CONFIG_KEY_n` and `GIT_CONFIG_VALUE_n` (Sol, 2026-10-15), nor a `GIT_CONFIG`
/// that would take the setup's own `git config` elsewhere, nor a template folder
/// (`--template=`, over `GIT_TEMPLATE_DIR` too); the hooks are an empty folder and
/// nothing is signed. Only the setup runs so: the workers run the person's git as it is.
fn uncommitted(git: &Run, workspace: &Path, change: &Uncommitted) -> Result {
    let repository = workspace.join(".git");
    let hooks = repository.join("hooks-off");
    let setup = |words: &[&str]| -> Result {
        let ran = git
            .clone()
            .var("GIT_CONFIG_GLOBAL", repository.join("no-global-config"))
            .var("GIT_CONFIG_NOSYSTEM", "1")
            .var("GIT_CONFIG_PARAMETERS", "")
            .var("GIT_CONFIG_COUNT", "0")
            .var("GIT_CONFIG", repository.join("config"))
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

/// `workspace` made again from the folder `from`, and what Claude Code (whose config folder
/// is `config`, where the run has a Claude Code) remembered about it forgotten.
fn renew(config: Option<&Path>, workspace: &Path, from: &Path) -> Result {
    // A folder that is not there is already gone.
    let _ = fs::remove_dir_all(workspace);
    files::make_dir(workspace)?;
    if let Some(config) = config {
        let _ = fs::remove_dir_all(claude_memory(config, workspace));
    }
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
        renew(Some(&home), &workspace, &fixture).unwrap();
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
        renew(Some(&home), &fresh, &fixture).unwrap();
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
        let other = home.join("projects/-Users-me-work/memory/MEMORY.md");
        write(&home, "projects/-Users-me-work/memory/MEMORY.md", "theirs");
        renew(Some(&home), &workspace, &fixture).unwrap();
        assert!(!memory.exists(), "the memory of the workspace is cleared");
        assert!(conversation.exists(), "its conversations are the person's");
        assert!(other.exists(), "so is what it remembered of another folder");
    }

    /// A config folder that is the folder at `path`.
    #[derive(Debug)]
    struct At(PathBuf);

    impl cf_e2e::rig::ClaudeConfig for At {
        fn folder(&self) -> cf_e2e::Result<PathBuf> {
            Ok(self.0.clone())
        }

        fn described(&self) -> String {
            "the test".to_owned()
        }

        fn finish(&self) -> cf_e2e::Result {
            Ok(())
        }
    }

    /// A machine of a user whose home is `home`, with a Claude Code whose throwaway is `throwaway`.
    fn machine_with_a_throwaway(home: &Path, throwaway: &Path) -> Machine {
        Machine {
            home: home.to_path_buf(),
            installs: vec![cf_e2e::live::Installed::at(
                live::Harness::Claude,
                "/bin/claude",
                home,
            )],
            node: None,
            login: Some(std::sync::Arc::new(super::super::login::Throwaway::over(
                At(throwaway.to_path_buf()),
            ))),
        }
    }

    #[test]
    fn every_play_has_a_folder_of_its_own_below_the_workspaces_and_what_one_holds_is_not_in_another(
    ) {
        let root = tempfile::tempdir().unwrap();
        let (home, throwaway) = (root.path().join("home"), root.path().join("throwaway"));
        let machine = machine_with_a_throwaway(&home, &throwaway);
        let scenario = crate::scenarios::by_id("six-decisions").unwrap();
        let (one, two) = (
            fresh(&machine, &scenario).unwrap(),
            fresh(&machine, &scenario).unwrap(),
        );
        assert_ne!(one, two, "two plays of one scenario, in one process");
        for workspace in [&one, &two] {
            assert_eq!(workspace.parent(), Some(machine.workspaces().as_path()));
            assert!(
                workspace
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .ends_with("-six-decisions"),
                "{}",
                workspace.display()
            );
            // The fixture, made fresh.
            assert!(workspace.join("site/evaluare.html").is_file());
        }
        // A stray file in one (another scenario's page, a page half made) never reaches the other.
        write(&one, "site/notes.md", "albastru");
        assert!(!two.join("site/notes.md").exists());
        // Neither is the fixed folder of the evals before, which a play begun beside another shared.
        assert!(!machine
            .workspaces()
            .parent()
            .unwrap()
            .join("workspace")
            .exists());
    }

    #[test]
    fn two_plays_of_one_process_made_in_the_same_instant_are_told_apart_by_their_count() {
        let scenario = crate::scenarios::by_id("six-decisions").unwrap();
        let at = jiff::Timestamp::UNIX_EPOCH;
        assert_ne!(name_at(at, &scenario), name_at(at, &scenario));
    }

    #[test]
    fn a_name_says_the_process_it_was_made_in_so_two_runs_at_once_never_share_one() {
        let scenario = crate::scenarios::by_id("six-decisions").unwrap();
        let name = name_at(jiff::Timestamp::UNIX_EPOCH, &scenario);
        assert!(
            name.contains(&format!("-{}-", std::process::id())),
            "{name}"
        );
    }

    #[test]
    fn a_fresh_workspace_leaves_the_users_claude_alone() {
        let root = tempfile::tempdir().unwrap();
        let (home, throwaway) = (root.path().join("home"), root.path().join("throwaway"));
        let machine = machine_with_a_throwaway(&home, &throwaway);
        let scenario = crate::scenarios::by_id("six-decisions").unwrap();
        // What the user's own Claude remembers of a folder of the same kind, and the throwaway's.
        let theirs = claude_memory(&home.join(".claude"), &machine.workspaces().join("x"));
        write(&theirs, "MEMORY.md", "the user's");
        let workspace = fresh(&machine, &scenario).unwrap();
        assert!(
            theirs.join("MEMORY.md").is_file(),
            "the user's is not touched"
        );
        assert!(!claude_memory(&throwaway, &workspace).exists());
    }

    #[test]
    fn a_play_retires_the_folder_it_made_and_no_other() {
        let root = tempfile::tempdir().unwrap();
        let (home, throwaway) = (root.path().join("home"), root.path().join("throwaway"));
        let machine = machine_with_a_throwaway(&home, &throwaway);
        let scenario = crate::scenarios::by_id("six-decisions").unwrap();
        let made = fresh(&machine, &scenario).unwrap();
        let other = fresh(&machine, &scenario).unwrap();
        retire(&made);
        assert!(!made.exists());
        assert!(other.join("site/evaluare.html").is_file(), "another play's");
        // A folder that was not made by `fresh` is left where it is, whatever it holds.
        let elsewhere = root.path().join("work");
        write(&elsewhere, "a.txt", "a");
        retire(&elsewhere);
        assert!(elsewhere.join("a.txt").is_file());
        // And one that is gone is gone.
        retire(&made);
    }

    #[test]
    fn a_run_with_no_claude_code_clears_nothing_of_any_config_folder() {
        let root = tempfile::tempdir().unwrap();
        let (config, fixture, workspace) = (
            root.path().join("config"),
            root.path().join("fixture"),
            root.path().join("workspace"),
        );
        write(&fixture, "a.txt", "a");
        let memory = claude_memory(&config, &workspace);
        write(&memory, "MEMORY.md", "# what it learned");
        renew(None, &workspace, &fixture).unwrap();
        assert!(memory.join("MEMORY.md").is_file());
        assert!(workspace.join("a.txt").is_file());
    }

    #[test]
    fn claude_names_the_folder_of_a_workspace_by_its_path_with_a_dash_for_each_mark() {
        assert_eq!(
            claude_memory(
                Path::new("/tmp/consensflow-live-claude-1-a"),
                Path::new("/Users/x/.consensflow-candidate/evals/workspace")
            ),
            Path::new(
                "/tmp/consensflow-live-claude-1-a/projects/-Users-x--consensflow-candidate-evals-workspace/memory"
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
        renew(Some(&home), &workspace, &fixture).unwrap();
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
        renew(Some(&home), &workspace, &fixture).unwrap();
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
        // And what their environment passes on top (Sol, 2026-10-15): the same excludes
        // file in both of the environment's channels, `GIT_CONFIG_PARAMETERS` (which
        // `git -c` writes for the programs it starts) and `GIT_CONFIG_COUNT` with its
        // keys and values; and a `GIT_CONFIG` that would take a `git config` elsewhere.
        let hostile = plain_git()
            .var("GIT_CONFIG_GLOBAL", theirs.join("global"))
            .var("GIT_CONFIG_SYSTEM", theirs.join("system"))
            .var("GIT_TEMPLATE_DIR", theirs.join("templates"))
            .var(
                "GIT_CONFIG_PARAMETERS",
                format!(
                    "'core.hooksPath'='{}' 'commit.gpgsign'='true' 'gpg.program'='{}' \
                     'core.excludesFile'='{}'",
                    path("hooks"),
                    path("no-such-gpg"),
                    path("excludes"),
                ),
            )
            .var("GIT_CONFIG_COUNT", "1")
            .var("GIT_CONFIG_KEY_0", "core.excludesFile")
            .var("GIT_CONFIG_VALUE_0", path("excludes"))
            .var("GIT_CONFIG", theirs.join("elsewhere"));
        uncommitted(&hostile, &workspace, &CHANGE).unwrap();
        // The repository's own configuration keeps the line ends as they are.
        assert_eq!(
            git(
                &workspace,
                &["config", "--file", ".git/config", "core.autocrlf"]
            )
            .trim(),
            "false"
        );
        assert!(!theirs.join("elsewhere").exists());
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
        renew(Some(&home), &workspace, &fixture).unwrap();
        let failed = uncommitted(&plain_git(), &workspace, &CHANGE)
            .unwrap_err()
            .to_string();
        assert!(failed.contains("0 și 100."), "{failed}");
    }

    #[test]
    fn a_fixture_that_is_not_there_is_an_error_that_names_it() {
        let root = tempfile::tempdir().unwrap();
        let failed = renew(
            Some(root.path()),
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
