//! What a rig that ran the stand-in agents in the machine's own Claude folder
//! takes away when it ends. The folder is the person's: their conversations are
//! in it, and the stand-ins wrote theirs beside them (`projects/integration`,
//! and a status file in `sessions` for each window), where the daemon looks for a
//! Claude window's records. A rig that ends removes those it started, one by one,
//! and nothing else of the folder; and, where it is asked to, what a real
//! Claude Code kept of the folders of the run (`projects/<name of the folder>`,
//! with the `memory/` the next session in that folder reads).

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::Started;

/// The name Claude Code gives a folder under its `projects`: the path with
/// every character that is not a letter or a digit made a dash.
pub fn project_name(path: &str) -> String {
    path.chars()
        .map(|at| if at.is_ascii_alphanumeric() { at } else { '-' })
        .collect()
}

/// What a rig is to take away, and where from.
#[derive(Debug)]
pub(super) struct Forget {
    /// The Claude config folder the stand-ins write in.
    config: PathBuf,
    /// The folders of the run, whose Claude memory is taken away with the run.
    folders: Vec<PathBuf>,
}

impl Forget {
    /// The stand-ins write in the Claude config folder `config`.
    pub fn new(config: PathBuf) -> Self {
        Self {
            config,
            folders: Vec::new(),
        }
    }

    /// The folder `folder` is one of the run's: what Claude Code keeps of it is
    /// taken away with the rig. A folder that is no deeper than the root of a
    /// disk and one folder is refused: what a run forgets is what it made.
    pub fn folder(&mut self, folder: &Path) {
        let names = folder
            .components()
            .filter(|part| matches!(part, Component::Normal(_)))
            .count();
        if folder.is_absolute() && names >= 2 && !self.folders.iter().any(|known| known == folder) {
            self.folders.push(folder.to_path_buf());
        }
    }

    /// Takes away what the stand-ins of `started` wrote and, unless `leave_memory`
    /// (the folders of the run are left for a person to look at, and so is what
    /// Claude Code kept of them), what Claude Code kept of the run's folders.
    pub fn clear(&self, started: &[Started], leave_memory: bool) {
        stand_ins(&self.config, started);
        if !leave_memory {
            for folder in &self.folders {
                memory_of(&self.config, folder);
            }
        }
    }
}

/// Removes the records the stand-ins of `started` wrote under `config`: each
/// conversation they recorded, and the status file of each window if it names
/// that conversation (a window that was killed could not take it away itself, and
/// the process id may be another program's by now); then the folder of the
/// conversations, if it is empty. It is not removed with anything else of the
/// person's in it.
fn stand_ins(config: &Path, started: &[Started]) {
    let records = config.join("projects").join("integration");
    for window in started {
        let _ = fs::remove_file(records.join(format!("{}.jsonl", window.session)));
        let status = config.join("sessions").join(format!("{}.json", window.pid));
        if names_session(&status, &window.session) {
            let _ = fs::remove_file(&status);
        }
    }
    let _ = fs::remove_dir(&records);
}

/// Whether the status file `file` says it is of the conversation `session`.
fn names_session(file: &Path, session: &str) -> bool {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|status| status["sessionId"] == session)
}

/// Removes what Claude Code kept of the folder `folder` under `config`: the
/// folder it names by the path as it is and, where that is another, by the path
/// it resolves to (`/var` is `/private/var` on macOS).
fn memory_of(config: &Path, folder: &Path) {
    let mut names = vec![project_name(&folder.to_string_lossy())];
    if let Ok(resolved) = fs::canonicalize(folder) {
        let resolved = project_name(&resolved.to_string_lossy());
        if !names.contains(&resolved) {
            names.push(resolved);
        }
    }
    for name in names {
        let _ = fs::remove_dir_all(config.join("projects").join(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(config: &Path, relative: &str, text: &str) -> PathBuf {
        let file = config.join(relative);
        crate::files::write(&file, text).unwrap();
        file
    }

    fn started(pid: u32, session: &str) -> Started {
        Started {
            pid,
            session: session.to_owned(),
        }
    }

    #[test]
    fn claude_names_a_folder_by_its_path_with_dashes_for_what_is_no_letter_or_digit() {
        assert_eq!(
            project_name(
                "/private/var/folders/ab/x_y/T/consensflow-integration-Q1/workspace/p1-claude"
            ),
            "-private-var-folders-ab-x-y-T-consensflow-integration-Q1-workspace-p1-claude"
        );
        assert_eq!(project_name("/a.b"), "-a-b");
        assert_eq!(
            project_name(r"C:\Users\me\.consensflow-candidate"),
            "C--Users-me--consensflow-candidate"
        );
        assert_eq!(project_name(""), "");
    }

    #[test]
    fn what_the_stand_ins_wrote_goes_one_record_at_a_time_and_nothing_of_the_persons_with_it() {
        let config = tempfile::tempdir().unwrap();
        let (a, b) = (
            at(config.path(), "projects/integration/s-1.jsonl", "{}\n"),
            at(config.path(), "projects/integration/s-2.jsonl", "{}\n"),
        );
        let persons = at(config.path(), "projects/integration/theirs.jsonl", "{}\n");
        let other = at(config.path(), "projects/-Users-me-work/c.jsonl", "{}\n");
        let status_1 = at(
            config.path(),
            "sessions/101.json",
            r#"{"pid":101,"sessionId":"s-1","status":"busy"}"#,
        );
        // A status file whose process id is another conversation's now is not the stand-in's.
        let status_2 = at(
            config.path(),
            "sessions/102.json",
            r#"{"pid":102,"sessionId":"another","status":"busy"}"#,
        );
        let unknown = at(config.path(), "sessions/103.json", "not JSON");
        stand_ins(config.path(), &[started(101, "s-1"), started(102, "s-2")]);
        assert!(!a.exists() && !b.exists() && !status_1.exists());
        for kept in [&persons, &other, &status_2, &unknown] {
            assert!(kept.exists(), "{} is not the stand-ins'", kept.display());
        }
        // Their folder stays while anything else is in it.
        assert!(config.path().join("projects/integration").is_dir());
    }

    #[test]
    fn the_stand_ins_folder_goes_when_it_held_only_what_they_wrote() {
        let config = tempfile::tempdir().unwrap();
        at(config.path(), "projects/integration/s-1.jsonl", "{}\n");
        let neighbour = at(config.path(), "projects/-Users-me-work/c.jsonl", "{}\n");
        stand_ins(config.path(), &[started(7, "s-1")]);
        assert!(!config.path().join("projects/integration").exists());
        assert!(neighbour.exists());
        // A rig whose stand-ins wrote nothing takes nothing away, and asks nothing of a folder that is not there.
        stand_ins(config.path(), &[]);
        stand_ins(&config.path().join("not-there"), &[started(1, "x")]);
        assert!(neighbour.exists());
    }

    #[test]
    fn what_claude_kept_of_a_folder_of_the_run_goes_by_both_names_the_folder_has() {
        let config = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("consensflow-integration-X").join("live");
        fs::create_dir_all(&folder).unwrap();
        let by_path = project_name(&folder.to_string_lossy());
        let by_resolved = project_name(&fs::canonicalize(&folder).unwrap().to_string_lossy());
        let memory = at(
            config.path(),
            &format!("projects/{by_path}/memory/MEMORY.md"),
            "# what it learned",
        );
        let resolved = at(
            config.path(),
            &format!("projects/{by_resolved}/s.jsonl"),
            "{}\n",
        );
        let another = at(config.path(), "projects/-Users-me-work/c.jsonl", "{}\n");
        let mut forget = Forget::new(config.path().to_path_buf());
        forget.folder(&folder);
        // Left for a person to look at, the run's folders keep what Claude kept of them.
        forget.clear(&[], true);
        assert!(memory.exists() && resolved.exists());
        forget.clear(&[], false);
        assert!(!memory.exists() && !resolved.exists());
        assert!(another.exists(), "another folder's memory is the person's");
    }

    #[test]
    fn a_folder_that_is_a_root_or_not_absolute_is_not_a_folder_of_the_run() {
        let root = if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        };
        let mut forget = Forget::new(PathBuf::from("config"));
        for refused in [
            root.clone(),
            root.join("tmp"),
            PathBuf::from("relative").join("folder"),
            PathBuf::new(),
        ] {
            forget.folder(&refused);
        }
        assert!(forget.folders.is_empty());
        let live = root.join("tmp").join("live");
        forget.folder(&live);
        forget.folder(&live);
        assert_eq!(forget.folders, [live]);
    }
}
