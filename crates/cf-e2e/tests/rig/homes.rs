//! Where a rig's home is, and the homes a killed run left. A rig's home is in the machine's
//! temp folder, whatever the rig is built for: the real harnesses' too (`live::rig`), whose
//! windows then open in no git repository. A home under the checkout would put them in
//! ConsensFlow's own: Claude Code would read the repository's settings and instructions,
//! Codex would take its root as the project for trust, so that every live window shared
//! one. That is another test than the one the live
//! suites were made for.
//!
//! A run that was killed leaves its rigs' homes, and so does `CONSENSFLOW_LIVE_KEEP`. A
//! rig that starts takes none of them, for they may be another run's (T-1557, finding 3a):
//! `cargo xtask live sweep` lists the ones older than a day, and says of each whether a person
//! kept it and whether a process works in or names it, and takes none (T-1592).

use std::fs;
use std::path::Path;

use cf_e2e::live;
use cf_e2e::process::Run;
use cf_e2e::rig::Rig;

use crate::{config, Outcome};

/// Whether `folder` is in a git repository, as `git` says.
fn in_a_repository(folder: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let ran = Run::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .cwd(folder)
        .inheriting_env()
        .run()?;
    Ok(ran.code == Some(0))
}

#[test]
fn a_live_rigs_home_is_in_the_temp_folder_and_in_no_git_repository() -> Outcome {
    for (what, config) in [
        (
            "a rig built for the real harnesses",
            live::rig(&[], crate::FAKE_AGENT),
        ),
        ("a rig that is not", config()),
    ] {
        let rig = Rig::start(config)?;
        let workspace = rig.workspace().to_path_buf();
        let repository = in_a_repository(&workspace);
        rig.close()?;

        assert!(
            workspace.starts_with(std::env::temp_dir()),
            "the home of {what} is not in the temp folder: {}",
            workspace.display()
        );
        assert!(
            !repository?,
            "the home of {what} is in a git repository: {}",
            workspace.display()
        );
    }
    Ok(())
}

#[test]
fn a_rig_is_given_its_home_in_the_folder_its_case_names() -> Outcome {
    let folder = tempfile::tempdir()?;

    let rig = Rig::start(config().homes_in(folder.path()))?;

    let workspace = rig.workspace().to_path_buf();
    assert!(
        workspace.starts_with(folder.path()),
        "{} is not under {}",
        workspace.display(),
        folder.path().display()
    );
    rig.close()?;
    assert!(
        fs::read_dir(folder.path())?.next().is_none(),
        "the home was not taken away when the rig closed"
    );
    Ok(())
}

#[cfg(unix)]
mod sweeping {
    use std::fs::File;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use cf_e2e::process::Run;
    use cf_e2e::rig::sweep;

    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    /// `path`, last changed `ago` ago, by the clock the sweep reads its age against.
    #[allow(clippy::disallowed_methods)]
    fn aged(path: &Path, ago: Duration) -> std::io::Result<()> {
        File::open(path)?.set_modified(SystemTime::now() - ago)
    }

    /// A home called `name` in `folder` with a workspace and a note in it, last changed
    /// `ago` ago.
    fn home(folder: &Path, name: &str, ago: Duration) -> std::io::Result<std::path::PathBuf> {
        let home = folder.join(name);
        fs::create_dir_all(home.join("workspace"))?;
        fs::write(home.join("workspace").join("note"), "kept or not")?;
        aged(&home, ago)?;
        Ok(home)
    }

    #[test]
    fn a_rig_that_starts_takes_no_home_and_the_sweep_lists_the_old_ones_and_says_which_a_process_works_in_or_names(
    ) -> Outcome {
        let folder = tempfile::tempdir()?;
        let stale = home(folder.path(), "consensflow-integration-stale", DAY * 3)?;
        let worked_in = home(folder.path(), "consensflow-integration-worked", DAY * 3)?;
        let named = home(folder.path(), "consensflow-integration-named", DAY * 3)?;
        let young = home(folder.path(), "consensflow-integration-young", DAY / 2)?;
        let other = home(folder.path(), "kept-other-folder", DAY * 3)?;
        // A report is a file, not a home; a link is not followed.
        let report = folder.path().join("consensflow-integration-stale-left.txt");
        fs::write(&report, "a report")?;
        aged(&report, DAY * 3)?;
        let elsewhere = home(folder.path(), "elsewhere", DAY * 3)?;
        let link = folder.path().join("consensflow-integration-link");
        std::os::unix::fs::symlink(&elsewhere, &link)?;

        // A process working in one, and one that has the other's name in its command line.
        // Each lasts until the case ends it, after the sweep: a sleeper of a fixed time ran out
        // before a rig's turn came in a loaded suite (T-655's gate, twice). And no longer than
        // the case's process: `cargo test` ends no group of a test it gave up on, so a case
        // killed outright would leave a sleeper of its own time behind (Calliope on T-679).
        let mut sleeper = Run::new("sh")
            .args([
                "-c",
                &format!(
                    "while kill -0 {} 2>/dev/null; do /bin/sleep 1; done",
                    std::process::id()
                ),
            ])
            .cwd(worked_in.join("workspace"))
            .spawn()?;
        let mut namer = Run::new("sh")
            .args(["-c", "read line", "named-home"])
            .arg(&named)
            .spawn()?;

        let rig = Rig::start(config().homes_in(folder.path()));
        let started = [&stale, &worked_in, &named, &young, &other, &elsewhere]
            .iter()
            .all(|home| home.join("workspace").join("note").is_file());
        let rig = rig?;
        let own = rig.workspace().to_path_buf();
        rig.close()?;
        assert!(started, "a rig's start took a home that was there");
        assert!(report.is_file() && link.is_symlink());

        // The sweep: what it lists of them, and what it says of the processes that use each.
        // It takes none.
        #[allow(clippy::disallowed_methods)]
        let now = SystemTime::now();
        let listed = sweep::listing(
            folder.path(),
            &folder.path().join("no-live"),
            now,
            sweep::OLDER,
        );
        sleeper.kill();
        namer.kill();
        let paths: Vec<_> = listed.iter().map(|left| left.path.clone()).collect();
        assert_eq!(paths, vec![named.clone(), stale.clone(), worked_in.clone()]);
        // A machine where the kit cannot read which processes work in a folder says so.
        let said = |home: &std::path::PathBuf| {
            listed
                .iter()
                .find(|left| &left.path == home)
                .map(|left| left.used.clone())
        };
        if cfg!(target_os = "macos") {
            assert_eq!(
                said(&stale),
                Some(sweep::Used::No),
                "a home older than a day, worked in by none"
            );
            assert_eq!(
                said(&worked_in),
                Some(sweep::Used::Yes),
                "a home a process works in"
            );
            assert_eq!(
                said(&named),
                Some(sweep::Used::Yes),
                "a home a process names"
            );
        } else {
            assert!(matches!(said(&stale), Some(sweep::Used::Unread(_))));
        }
        for home in [&stale, &worked_in, &named, &young, &other] {
            assert!(
                home.join("workspace").join("note").is_file(),
                "the sweep took {}",
                home.display()
            );
        }
        assert!(report.is_file(), "a report");
        assert!(
            link.is_symlink() && elsewhere.join("workspace").join("note").is_file(),
            "a link"
        );
        assert!(own.starts_with(folder.path()));
        Ok(())
    }
}
