//! What the real harnesses leave of a run where they keep their own records:
//! a Claude Code window writes its conversations in a folder of `~/.claude`,
//! keeps what was typed in a project in `~/.claude/history.jsonl` and the trust
//! the run asked of it in `~/.claude.json`, and a Pi window writes its sessions
//! in `~/.pi/agent/sessions`. The project's folders are in a home that goes with
//! the run; these would stay, and be read by nobody. Claude Code is asked to
//! forget the folder (`claude purge`, which is how it deletes a project's
//! state), and Pi's folder of it is taken away. The others (OpenCode's and
//! Devin's databases, Codex's session files) are not taken apart: they belong
//! to a database or to a day's folder that is the person's.

use std::path::{Path, PathBuf};

use cf_e2e::process::{own_var, Run};
use cf_e2e::rig::project_name as claude_name;

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

/// The places `home`'s harnesses keep their sessions of the folders known by
/// `paths`, which are taken away: Claude Code's, which `claude purge` does not
/// always find by a path the folder no longer has, and Pi's.
pub fn records(home: &Path, paths: &[String]) -> Vec<PathBuf> {
    let mut records = Vec::new();
    for path in paths {
        records.push(
            home.join(".claude")
                .join("projects")
                .join(claude_name(path)),
        );
        records.push(
            home.join(".pi")
                .join("agent")
                .join("sessions")
                .join(pi_name(path)),
        );
    }
    records
}

/// Removes the `records` that are there, and says which.
pub fn remove(records: &[PathBuf]) -> Vec<PathBuf> {
    records
        .iter()
        .filter(|record| record.is_dir() && std::fs::remove_dir_all(record).is_ok())
        .cloned()
        .collect()
}

/// Has the Claude Code at `claude` forget the projects at `paths` in the home
/// `home`: `claude purge --yes` for each, which deletes the project's entry in
/// `~/.claude.json` (the trust the run asked of it) and the prompts typed in it
/// in `~/.claude/history.jsonl`. The paths it had something to forget of.
pub fn forget_in_claude(claude: &Path, home: &Path, paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| {
            Run::new(claude)
                .args(["purge", "--yes", path.as_str()])
                .var("HOME", home)
                .var("PATH", own_var("PATH").unwrap_or_default())
                .run()
                .is_ok_and(|ran| ran.code == Some(0))
        })
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
    fn what_a_run_left_in_the_harness_records_of_its_own_folder_goes_and_nothing_else() {
        let home = tempfile::tempdir().unwrap();
        let ours = records(
            home.path(),
            &["/var/folders/x/consensflow-integration-X/workspace/p1".to_owned()],
        );
        let (claude, pi) = (&ours[0], &ours[1]);
        let other = home
            .path()
            .join(".claude")
            .join("projects")
            .join("-people-work");
        for dir in [claude, pi, &other] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("one.jsonl"), "{}").unwrap();
        }
        let removed = remove(&ours);
        assert!(
            removed.contains(claude) && removed.contains(pi),
            "{removed:?}"
        );
        assert!(!claude.exists() && !pi.exists());
        assert!(
            other.join("one.jsonl").is_file(),
            "another folder's records stay"
        );
    }

    #[cfg(unix)]
    #[test]
    fn claude_is_asked_to_purge_each_path_in_the_home_it_is_given_and_says_which_it_knew() {
        let folder = tempfile::tempdir().unwrap();
        let asked = folder.path().join("asked");
        // A stand-in Claude that knows the project of the path `/known` only.
        let program = folder.path().join("claude");
        cf_e2e::files::write_executable(
            &program,
            format!(
                "#!/bin/sh\necho \"$HOME $*\" >> '{}'\n[ \"$3\" = /known ]\n",
                asked.display()
            ),
        )
        .unwrap();
        let home = folder.path().join("home");
        let purged = forget_in_claude(
            &program,
            &home,
            &["/known".to_owned(), "/unknown".to_owned()],
        );
        assert_eq!(purged, ["/known"]);
        let said = std::fs::read_to_string(&asked).unwrap();
        let named = home.to_string_lossy();
        assert_eq!(
            said,
            format!("{named} purge --yes /known\n{named} purge --yes /unknown\n")
        );
        // A Claude that is not there knows nothing.
        let none = folder.path().join("none");
        assert!(forget_in_claude(&none, &home, &["/known".to_owned()]).is_empty());
    }
}
