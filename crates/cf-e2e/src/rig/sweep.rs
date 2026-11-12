//! The homes older runs left. A rig's home goes with the rig when it ends; a run that was
//! killed, or one asked to keep its homes (`CONSENSFLOW_LIVE_KEEP`), leaves them. A rig that
//! starts takes none of them: they may be another run's, at work or kept as its evidence
//! (T-1557, finding 3a). It counts the ones older than a day in the folder it makes its own in
//! and says how many, and `cargo xtask live sweep` takes them, through the door
//! ([`crate::door::Door::remove_stale_rig`]: a kept one, and one a process uses, never).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::door::RIG_PREFIX;

/// How old a home is before a rig's start counts it.
pub const OLDER: Duration = Duration::from_secs(24 * 60 * 60);

/// Says how many rig folders older than a day stand in `folder`, when any do.
// The time of day is read in one place of the product ([`cf_base::time`]), which this crate
// does not depend on: the age of a folder is read against the clock its files are stamped by.
#[allow(clippy::disallowed_methods, clippy::print_stderr)] // Said in the output of the run.
pub(super) fn homes(folder: &Path) -> usize {
    let count = older(folder, SystemTime::now(), OLDER).len();
    if count > 0 {
        eprintln!(
            "{count} older rig folder{} stand{} in {}; `cargo xtask live sweep` takes the ones \
             nobody kept or uses",
            if count == 1 { "" } else { "s" },
            if count == 1 { "s" } else { "" },
            folder.display()
        );
    }
    count
}

/// The rig folders in `folder` (named as a rig names its folder, a folder itself and not a link
/// or a report) last changed more than `older` before `now`. Nothing of them is read or taken.
pub fn older(folder: &Path, now: SystemTime, older: Duration) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with(RIG_PREFIX) && !name.ends_with("-left.txt")
        })
        .filter_map(|entry| {
            // The entry itself, not what a link leads to: a link is none of the rig's.
            let found = std::fs::symlink_metadata(entry.path()).ok()?;
            let age = now.duration_since(found.modified().ok()?).ok()?;
            (found.is_dir() && age > older).then(|| entry.path())
        })
        .collect();
    found.sort();
    found
}

// A test stamps its folders by the clock the count reads their ages against.
#[allow(clippy::disallowed_methods)]
#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const DAY: Duration = OLDER;

    /// Stamps the folder `folder` as last changed `ago` ago. Windows opens a folder only with
    /// the flag that asks for it (`FILE_FLAG_BACKUP_SEMANTICS`).
    fn aged(folder: &Path, ago: Duration) {
        let mut open = fs::OpenOptions::new();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            open.write(true).custom_flags(0x0200_0000);
        }
        #[cfg(not(windows))]
        open.read(true);
        open.open(folder)
            .unwrap()
            .set_modified(SystemTime::now() - ago)
            .unwrap();
    }

    /// A home called `name` in `folder`, last changed `ago` ago, with a note in it.
    fn home(folder: &Path, name: &str, ago: Duration) -> PathBuf {
        let home = folder.join(name);
        fs::create_dir_all(home.join("workspace")).unwrap();
        fs::write(home.join("workspace").join("note"), "a note").unwrap();
        aged(&home, ago);
        home
    }

    #[test]
    fn only_a_rig_folder_older_than_a_day_is_counted() {
        let folder = tempfile::tempdir().unwrap();
        let old = home(folder.path(), "consensflow-integration-old", DAY * 2);
        home(folder.path(), "consensflow-integration-young", DAY / 2);
        home(folder.path(), "another-old-folder", DAY * 2);
        fs::write(
            folder.path().join("consensflow-integration-old-left.txt"),
            "a report",
        )
        .unwrap();
        #[cfg(unix)]
        {
            let elsewhere = home(folder.path(), "elsewhere", DAY * 2);
            std::os::unix::fs::symlink(
                &elsewhere,
                folder.path().join("consensflow-integration-link"),
            )
            .unwrap();
        }

        assert_eq!(older(folder.path(), SystemTime::now(), DAY), [old]);
    }

    /// A rig's start takes nothing of another run's: an older folder a run kept on purpose
    /// (`CONSENSFLOW_LIVE_KEEP`, its evidence) and one a killed run left both stand after it, and
    /// nobody's process is asked about. The rig only counts them (T-1557, finding 3a).
    #[test]
    fn a_rig_start_leaves_the_folders_older_runs_left_and_only_counts_them() {
        let folder = tempfile::tempdir().unwrap();
        let kept = home(folder.path(), "consensflow-integration-kept", DAY * 2);
        fs::write(kept.join("consensflow-kept.txt"), "kept on purpose").unwrap();
        aged(&kept, DAY * 2);
        let left = home(folder.path(), "consensflow-integration-left", DAY * 2);

        assert_eq!(homes(folder.path()), 2);

        for there in [&kept, &left] {
            assert!(
                there.join("workspace").join("note").is_file(),
                "a rig's start took {}",
                there.display()
            );
        }
    }
}
