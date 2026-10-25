//! What the real harnesses leave of a run where they keep their own records: a Pi
//! window writes its sessions in `~/.pi/agent/sessions`. The project's folders are in
//! a home that goes with the run; these would stay, and be read by nobody, so Pi's
//! folder of it is taken away. The others (OpenCode's and Devin's databases, Codex's
//! session files) are not taken apart: they belong to a database or to a day's folder
//! that is the person's. Claude Code's are not here: its windows run on a config
//! folder of the run's own, which goes with the run ([`super::login`]). The approval
//! suite and the specs suite both clean up so.

use std::path::{Path, PathBuf};

/// The name Pi gives a folder under its `sessions`: the path without its first
/// slash and with the others made dashes, between two dashes at each end.
fn pi_name(path: &str) -> String {
    format!("--{}--", path.trim_start_matches('/').replace('/', "-"))
}

/// The paths a harness may know the folder `folder` by: its own, and the one
/// it resolves to, which a harness may use instead (`/var` is `/private/var`
/// on macOS). Asked while the folder is there. A folder that is not one a rig
/// made (it is not in a home of a rig's) has none: what the harnesses kept of
/// it is not this run's to take away.
pub fn paths(folder: &Path) -> Vec<String> {
    if !folder
        .to_string_lossy()
        .contains("consensflow-integration-")
    {
        return Vec::new();
    }
    let mut paths = vec![folder.to_string_lossy().into_owned()];
    if let Ok(resolved) = std::fs::canonicalize(folder) {
        let resolved = resolved.to_string_lossy().into_owned();
        if !paths.contains(&resolved) {
            paths.push(resolved);
        }
    }
    paths
}

/// The places `home`'s Pi keeps its sessions of the folders known by `paths`, which are
/// taken away.
pub fn records(home: &Path, paths: &[String]) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|path| {
            home.join(".pi")
                .join("agent")
                .join("sessions")
                .join(pi_name(path))
        })
        .collect()
}

/// Removes the `records` that are there, and says which.
pub fn remove(records: &[PathBuf]) -> Vec<PathBuf> {
    records
        .iter()
        .filter(|record| record.is_dir() && std::fs::remove_dir_all(record).is_ok())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_names_a_folder_by_its_path_between_double_dashes() {
        assert_eq!(pi_name("/var/folders/ab/p1"), "--var-folders-ab-p1--");
    }

    #[test]
    fn a_folder_is_known_by_its_own_path_and_by_the_one_it_resolves_to() {
        let root = tempfile::tempdir().unwrap();
        let folder = root
            .path()
            .join("consensflow-integration-X")
            .join("p1-claude");
        std::fs::create_dir_all(&folder).unwrap();
        let known = paths(&folder);
        assert_eq!(known[0], folder.to_string_lossy());
        let resolved = std::fs::canonicalize(&folder).unwrap();
        assert!(known.contains(&resolved.to_string_lossy().into_owned()));
        assert!(known.len() <= 2, "{known:?}");
    }

    #[test]
    fn a_folder_that_no_rig_made_has_none_of_its_paths_named() {
        assert_eq!(
            paths(Path::new("/people/work/project")),
            Vec::<String>::new()
        );
    }

    #[test]
    fn what_a_run_left_in_pis_records_of_its_own_folder_goes_and_nothing_else() {
        let home = tempfile::tempdir().unwrap();
        let ours = records(
            home.path(),
            &["/var/folders/x/consensflow-integration-X/workspace/p1".to_owned()],
        );
        let pi = &ours[0];
        let other = home
            .path()
            .join(".pi")
            .join("agent")
            .join("sessions")
            .join("--people-work--");
        for dir in [pi, &other] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("one.jsonl"), "{}").unwrap();
        }
        let removed = remove(&ours);
        assert_eq!(&removed, &ours);
        assert!(!pi.exists());
        assert!(
            other.join("one.jsonl").is_file(),
            "another folder's sessions stay"
        );
    }
}
