//! The homes a killed run left. A rig's home goes with the rig when it ends; a run that
//! was killed, or one asked to keep its homes (`CONSENSFLOW_LIVE_KEEP`), leaves them,
//! and what a real harness's windows wrote in one is there for good. A rig that starts
//! sweeps the ones in the folder it makes its own in: those named like a rig's, that are
//! older than a day, and that no process works in. What is in them is not read, only
//! taken away.
//!
//! A home is older than a day by the last change of its own folder: its first level is
//! made when the rig starts. A process works in it where its working folder is in it or
//! its command line names it. Where the processes cannot be listed nothing is swept, and
//! on Windows the processes are not listed here, so nothing is.

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::time::{Duration, SystemTime};

/// How old a home is before it is swept.
#[cfg(unix)]
const OLDER: Duration = Duration::from_secs(24 * 60 * 60);

/// How a rig's home is named, by the start of its name.
#[cfg(unix)]
const PREFIX: &str = "consensflow-integration-";

/// Takes away the stale homes in `folder`.
// The time of day is read in one place of the product ([`cf_base::time`]), which this crate
// does not depend on: the age of a folder is read against the clock its files are stamped by.
#[allow(clippy::disallowed_methods)]
pub(super) fn homes(folder: &Path) {
    #[cfg(unix)]
    let _ = sweep(folder, SystemTime::now(), &works::in_any);
    #[cfg(not(unix))]
    let _ = folder;
}

/// Which of the homes (in the order given) a process works in; none at all where the
/// processes cannot be listed.
#[cfg(unix)]
type WorkedIn<'a> = &'a dyn Fn(&[PathBuf]) -> Option<Vec<bool>>;

/// Takes away the homes in `folder` that are older than a day at `now` and that
/// `worked_in` says no process works in; says which.
#[cfg(unix)]
fn sweep(folder: &Path, now: SystemTime, worked_in: WorkedIn<'_>) -> Vec<PathBuf> {
    use std::fs;

    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let stale: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with(PREFIX) && !name.ends_with("-left.txt")
        })
        .filter_map(|entry| {
            // The entry itself, not what a link leads to: a link is none of the rig's.
            let found = fs::symlink_metadata(entry.path()).ok()?;
            let age = now.duration_since(found.modified().ok()?).ok()?;
            (found.is_dir() && age > OLDER).then(|| entry.path())
        })
        .collect();
    if stale.is_empty() {
        return Vec::new();
    }
    let Some(busy) = worked_in(&stale) else {
        return Vec::new();
    };
    stale
        .into_iter()
        .zip(busy)
        .filter(|(_, busy)| !busy)
        .filter(|(home, _)| fs::remove_dir_all(home).is_ok())
        .map(|(home, _)| home)
        .collect()
}

/// Which processes work in a folder, as the machine says it.
#[cfg(unix)]
mod works {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use crate::process::Run;

    /// How long the machine has to list its processes.
    const LISTING: Duration = Duration::from_secs(30);

    /// What `program` wrote of itself, whatever its code (`lsof` says 1 for the
    /// processes it may not look at); none where it did not start or wrote nothing.
    fn listed(program: &[&str], args: &[&str]) -> Option<String> {
        program.iter().find_map(|program| {
            let ran = Run::new(program)
                .args(args)
                .inheriting_env()
                .limit(LISTING)
                .run()
                .ok()?;
            (!ran.timed_out && !ran.stdout.trim().is_empty()).then_some(ran.stdout)
        })
    }

    /// Which of `homes` a process works in: its working folder is in it, or its command
    /// line names it, by the path it has or the one it resolves to.
    pub(super) fn in_any(homes: &[PathBuf]) -> Option<Vec<bool>> {
        let folders = listed(&["lsof", "/usr/sbin/lsof"], &["-d", "cwd", "-Fn"])?;
        let commands = listed(&["ps", "/bin/ps"], &["-ax", "-ww", "-o", "command="])?;
        let working: Vec<&str> = folders
            .lines()
            .filter_map(|line| line.strip_prefix('n'))
            .collect();
        Some(
            homes
                .iter()
                .map(|home| {
                    let names = [
                        home.clone(),
                        std::fs::canonicalize(home).unwrap_or_default(),
                    ];
                    names
                        .iter()
                        .filter(|name| !name.as_os_str().is_empty())
                        .any(|name| {
                            working.iter().any(|cwd| Path::new(cwd).starts_with(name))
                                || commands.contains(name.to_string_lossy().as_ref())
                        })
                })
                .collect(),
        )
    }
}

// A test stamps its folders by the clock the sweep reads their ages against.
#[allow(clippy::disallowed_methods)]
#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::fs::{self, File};

    use super::*;

    const DAY: Duration = OLDER;

    /// A home called `name` in `folder`, last changed `ago` ago, with a note in it.
    fn home(folder: &Path, name: &str, ago: Duration) -> PathBuf {
        let home = folder.join(name);
        fs::create_dir_all(home.join("workspace")).unwrap();
        fs::write(home.join("workspace").join("note"), "a note").unwrap();
        File::open(&home)
            .unwrap()
            .set_modified(SystemTime::now() - ago)
            .unwrap();
        home
    }

    #[cfg(unix)]
    #[test]
    fn only_a_home_older_than_a_day_that_no_process_works_in_is_swept() {
        let folder = tempfile::tempdir().unwrap();
        let old = home(folder.path(), "consensflow-integration-old", DAY * 2);
        let busy = home(folder.path(), "consensflow-integration-busy", DAY * 2);
        let young = home(folder.path(), "consensflow-integration-young", DAY / 2);
        let other = home(folder.path(), "another-old-folder", DAY * 2);
        let report = folder.path().join("consensflow-integration-old-left.txt");
        fs::write(&report, "a report").unwrap();
        let elsewhere = home(folder.path(), "elsewhere", DAY * 2);
        let link = folder.path().join("consensflow-integration-link");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();

        let swept = sweep(folder.path(), SystemTime::now(), &|homes| {
            Some(
                homes
                    .iter()
                    .map(|home| home.ends_with("consensflow-integration-busy"))
                    .collect(),
            )
        });

        assert_eq!(swept, std::slice::from_ref(&old));
        assert!(!old.exists());
        for kept in [&busy, &young, &other, &elsewhere] {
            assert!(kept.join("workspace").join("note").is_file(), "{kept:?}");
        }
        assert!(report.is_file());
        assert!(link.is_symlink());
    }

    #[test]
    fn nothing_is_swept_where_the_processes_cannot_be_listed() {
        let folder = tempfile::tempdir().unwrap();
        let old = home(folder.path(), "consensflow-integration-old", DAY * 2);

        let swept = sweep(folder.path(), SystemTime::now(), &|_| None);

        assert!(swept.is_empty());
        assert!(old.join("workspace").join("note").is_file());
    }

    #[test]
    fn the_processes_are_not_listed_when_no_home_is_old_enough_to_ask_about() {
        let folder = tempfile::tempdir().unwrap();
        home(folder.path(), "consensflow-integration-young", DAY / 2);

        let swept = sweep(folder.path(), SystemTime::now(), &|_| {
            panic!("the processes were listed for no home that is old")
        });

        assert!(swept.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_process_works_in_a_home_by_its_working_folder_or_by_naming_it() {
        use crate::process::Run;

        let folder = tempfile::tempdir().unwrap();
        let idle = home(folder.path(), "consensflow-integration-idle", DAY * 2);
        let working = home(folder.path(), "consensflow-integration-working", DAY * 2);
        let named = home(folder.path(), "consensflow-integration-named", DAY * 2);
        let mut sleeper = Run::new("sleep")
            .arg("120")
            .cwd(working.join("workspace"))
            .spawn()
            .unwrap();
        let mut namer = Run::new("sh")
            .args(["-c", "read line", "naming"])
            .arg(&named)
            .spawn()
            .unwrap();

        let found = works::in_any(&[idle, working, named]);
        sleeper.kill();
        namer.kill();

        assert_eq!(found, Some(vec![false, true, true]));
    }
}
