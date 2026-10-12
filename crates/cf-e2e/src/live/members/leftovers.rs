//! What the harnesses of a run keep of the project's folder where the rig does not
//! look: Pi's sessions of it, in the home it was given. They are taken away when the
//! project ends, however it ends, and left where a person is to look at the run
//! ([`keep_requested`]).

use std::path::{Path, PathBuf};

use super::super::keep_requested;
use super::super::machine::Harness;

/// What the harnesses of a run kept of the project's folder.
pub struct Leftovers {
    pi_sessions: Vec<PathBuf>,
}

impl Leftovers {
    /// What `harnesses` keep of `folder` in `home` (the home the rig gives its
    /// windows, where it gives one): Pi's sessions of it, where Pi is one of them.
    pub fn of(harnesses: &[Harness], home: Option<&str>, folder: &Path) -> Self {
        let pi_sessions = match home {
            Some(home) if harnesses.contains(&Harness::Pi) => {
                pi_sessions_of(Path::new(home), folder)
            }
            _ => Vec::new(),
        };
        Self { pi_sessions }
    }
}

impl Drop for Leftovers {
    fn drop(&mut self) {
        if !keep_requested() {
            for sessions in &self.pi_sessions {
                let _ = std::fs::remove_dir_all(sessions);
            }
        }
    }
}

/// The places Pi keeps its sessions of the folder `folder` under the home `home`:
/// the path without its first slash and with the others made dashes, between two
/// dashes at each end, for the folder as it is and for the path it resolves to
/// (`/var` is `/private/var` on macOS). None where Pi names them otherwise (Windows).
fn pi_sessions_of(home: &Path, folder: &Path) -> Vec<PathBuf> {
    if cfg!(windows) {
        return Vec::new();
    }
    let mut paths = vec![folder.to_path_buf()];
    if let Ok(resolved) = std::fs::canonicalize(folder) {
        if resolved != folder {
            paths.push(resolved);
        }
    }
    paths
        .iter()
        .map(|path| {
            let name = path
                .to_string_lossy()
                .trim_start_matches('/')
                .replace('/', "-");
            home.join(".pi")
                .join("agent")
                .join("sessions")
                .join(format!("--{name}--"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn pi_keeps_its_sessions_of_a_folder_in_a_folder_named_by_its_path_between_double_dashes() {
        let home = Path::new("/home/me");
        let found = pi_sessions_of(
            home,
            Path::new("/home/me/.consensflow-candidate/live/bench"),
        );
        assert_eq!(
            found,
            [Path::new(
                "/home/me/.pi/agent/sessions/--home-me-.consensflow-candidate-live-bench--"
            )]
        );
        // A folder that is there is known by the path it resolves to as well, which is another
        // on a machine whose temporary folder is a link (`/var` is `/private/var` on macOS).
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("project");
        std::fs::create_dir(&folder).unwrap();
        let found = pi_sessions_of(home, &folder);
        let resolved = std::fs::canonicalize(&folder).unwrap();
        assert_eq!(found.len(), if resolved == folder { 1 } else { 2 });
        assert!(found[0].to_string_lossy().ends_with(&format!(
            "--{}--",
            folder
                .to_string_lossy()
                .trim_start_matches('/')
                .replace('/', "-")
        )));
    }

    #[cfg(unix)]
    #[test]
    fn only_a_run_that_has_pi_in_a_home_has_its_sessions_to_take_away() {
        let folder = Path::new("/home/me/project");
        let with = |harnesses: &[Harness], home| Leftovers::of(harnesses, home, folder);
        assert_eq!(
            with(&[Harness::Claude, Harness::Pi], Some("/home/me")).pi_sessions,
            [Path::new("/home/me/.pi/agent/sessions/--home-me-project--")]
        );
        assert!(with(&[Harness::Claude, Harness::Codex], Some("/home/me"))
            .pi_sessions
            .is_empty());
        assert!(with(&[Harness::Pi], None).pi_sessions.is_empty());
    }

    #[test]
    fn what_pi_kept_of_the_folder_goes_with_the_project_and_nothing_else_of_its_folder_does() {
        let sessions = tempfile::tempdir().unwrap();
        let ours = sessions.path().join("--home-me-project--");
        let others = sessions.path().join("--home-me-other--");
        for folder in [&ours, &others] {
            std::fs::create_dir(folder).unwrap();
            std::fs::write(folder.join("s.jsonl"), "{}").unwrap();
        }
        drop(Leftovers {
            pi_sessions: vec![ours.clone(), sessions.path().join("--not-there--")],
        });
        assert!(!ours.exists());
        assert!(
            others.join("s.jsonl").is_file(),
            "another folder's sessions are Pi's"
        );
    }
}
