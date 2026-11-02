//! What a run leaves to be read and run again: a folder of its own,
//! `evals/reports/live-matrix/<run-id>/` (ignored by git, and kept by the Windows runner),
//! written before the rig's homes are dropped. It holds the seats and the models, the
//! scenario and its bounds, the human's lines with their times, each task's binding, each
//! window's last lines and turns, and every check. And the owner's files the run must never
//! touch, looked at before and after: the link Claude Code runs by, and Muse's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::live::report::Check;
use crate::{files, Error, Result};

/// The folder of the reports, from the checkout's root.
pub const REPORTS: &str = "evals/reports/live-matrix";

/// What the owner's files were at a moment: each path with what it was (a link's target, a
/// file's hash, or that it was not there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    links: Vec<PathBuf>,
    files: Vec<PathBuf>,
    seen: BTreeMap<PathBuf, String>,
}

impl Owner {
    /// The links `links` and the files `files`, as they are now.
    pub fn look(links: &[PathBuf], files: &[PathBuf]) -> Self {
        let mut seen = BTreeMap::new();
        for link in links {
            let target = std::fs::read_link(link).map_or_else(
                |_| "no link".to_owned(),
                |to| format!("-> {}", to.display()),
            );
            seen.insert(link.clone(), target);
        }
        for file in files {
            let hash = std::fs::read(file).map_or_else(
                |_| "absent".to_owned(),
                |bytes| {
                    let digest = Sha256::digest(bytes);
                    digest
                        .iter()
                        .take(8)
                        .map(|byte| format!("{byte:02x}"))
                        .collect()
                },
            );
            seen.insert(file.clone(), hash);
        }
        Self {
            links: links.to_vec(),
            files: files.to_vec(),
            seen,
        }
    }

    /// The owner's files of this machine that a matrix run must never touch, as they are now:
    /// the link `~/.local/bin/claude` (which a Claude window's updater re-points: T-1082), the
    /// owner's Claude settings, and Muse's login, settings, trust and session index.
    pub fn of_machine() -> Result<Self> {
        let home = crate::live::machine::home()?;
        let links = [home.join(".local/bin/claude")];
        let files = [
            home.join(".claude/settings.json"),
            home.join(".config/muse/auth.json"),
            home.join(".config/muse/settings.json"),
            home.join(".config/muse/trust.json"),
            home.join(".local/share/muse/session-index.db"),
        ];
        Ok(Self::look(&links, &files))
    }

    /// The same paths, looked at again now.
    pub fn again(&self) -> Self {
        Self::look(&self.links, &self.files)
    }

    /// The check that nothing of them changed between `self` and `now`.
    pub fn unchanged(&self, now: &Self) -> Check {
        let changed: Vec<String> = self
            .seen
            .iter()
            .filter(|(path, was)| now.seen.get(*path) != Some(*was))
            .map(|(path, was)| {
                format!(
                    "{}: {was} became {}",
                    path.display(),
                    now.seen.get(path).map_or("unread", String::as_str)
                )
            })
            .collect();
        Check::new(
            "owner",
            "no file of the owner's changed",
            changed.is_empty(),
            if changed.is_empty() {
                self.seen
                    .iter()
                    .map(|(path, was)| format!("{} {was}", path.display()))
                    .collect::<Vec<_>>()
                    .join("; ")
            } else {
                changed.join("; ")
            },
        )
    }

    /// As the evidence has it.
    pub fn manifest(&self) -> Value {
        json!(self
            .seen
            .iter()
            .map(|(path, was)| (path.display().to_string(), was.clone()))
            .collect::<BTreeMap<_, _>>())
    }
}

/// The folder of a run's evidence.
#[derive(Debug, Clone)]
pub struct Evidence {
    pub folder: PathBuf,
}

impl Evidence {
    /// The evidence of the run `run` under `reports`.
    pub fn under(reports: &Path, run: &str) -> Self {
        Self {
            folder: reports.join(run),
        }
    }

    /// Writes `value` as `name` (pretty JSON), the folder made first.
    pub fn json(&self, name: &str, value: &Value) -> Result {
        let text = serde_json::to_string_pretty(value)
            .map_err(|failed| Error::Daemon(format!("{name}: {failed}")))?;
        files::write(&self.folder.join(name), format!("{text}\n"))
    }

    /// Writes `text` as `name`.
    pub fn text(&self, name: &str, text: &str) -> Result {
        files::write(&self.folder.join(name), text)
    }

    /// Writes the checks, as JSON and as the table a person reads.
    pub fn checks(&self, checks: &[Check]) -> Result {
        let rows: Vec<Value> = checks
            .iter()
            .map(|check| json!({ "who": check.who, "what": check.what, "ok": check.ok, "detail": check.detail }))
            .collect();
        self.json("checks.json", &Value::Array(rows))?;
        let failed = checks.iter().filter(|check| !check.ok).count();
        self.text(
            "verdict.txt",
            &format!(
                "{}: {} of {} checks held\n\n{}\n",
                if failed == 0 && !checks.is_empty() {
                    "GREEN"
                } else {
                    "RED"
                },
                checks.len() - failed,
                checks.len(),
                crate::live::report::table(checks)
            ),
        )
    }
}

/// The id of a run that starts now on `board` (none: the stand-ins' control) with the
/// scenario `scenario`: its time, to the second, then what it ran.
pub fn run_id(board: Option<u8>, scenario: &str) -> String {
    let now = jiff::Timestamp::now()
        .strftime("%Y%m%dT%H%M%SZ")
        .to_string();
    let board = board.map_or_else(|| "stand-ins".to_owned(), |board| format!("board{board}"));
    format!("{now}-{board}-{scenario}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owners_files_are_unchanged_until_one_is_written_or_a_link_moved() {
        let folder = tempfile::tempdir().unwrap();
        let (file, link) = (
            folder.path().join("auth.json"),
            folder.path().join("claude"),
        );
        std::fs::write(&file, "{}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("versions/2.1.316", &link).unwrap();
        let missing = folder.path().join("none.json");
        let before = Owner::look(std::slice::from_ref(&link), &[file.clone(), missing]);
        assert!(before.unchanged(&before.again()).ok);
        std::fs::write(&file, "{\"a\":1}").unwrap();
        let after = before.again();
        let check = before.unchanged(&after);
        assert!(!check.ok);
        assert!(check.detail.contains("auth.json"), "{}", check.detail);
        assert!(!check.detail.contains("none.json"), "{}", check.detail);
        #[cfg(unix)]
        {
            std::fs::write(&file, "{}").unwrap();
            std::fs::remove_file(&link).unwrap();
            std::os::unix::fs::symlink("versions/2.1.400", &link).unwrap();
            let moved = before.unchanged(&before.again());
            assert!(!moved.ok);
            assert!(
                moved
                    .detail
                    .contains("-> versions/2.1.316 became -> versions/2.1.400"),
                "{}",
                moved.detail
            );
        }
    }

    #[test]
    fn the_evidence_is_written_in_a_folder_of_the_runs_own_with_a_verdict_a_person_reads() {
        let reports = tempfile::tempdir().unwrap();
        let evidence = Evidence::under(reports.path(), "r1");
        evidence
            .json("manifest.json", &json!({ "board": 0 }))
            .unwrap();
        evidence
            .checks(&[
                Check::passed("a", "held", "x"),
                Check::failed("b", "did not", "y"),
            ])
            .unwrap();
        let verdict = std::fs::read_to_string(reports.path().join("r1/verdict.txt")).unwrap();
        assert!(verdict.starts_with("RED: 1 of 2 checks held"), "{verdict}");
        let rows: Value = serde_json::from_str(
            &std::fs::read_to_string(reports.path().join("r1/checks.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(rows[1]["ok"], false);
        assert!(reports.path().join("r1/manifest.json").is_file());
    }

    #[test]
    fn a_run_id_says_when_and_what_ran() {
        let id = run_id(Some(0), "smoke");
        assert!(id.contains("-board0-smoke-"), "{id}");
        assert!(run_id(None, "smoke").contains("-stand-ins-smoke-"));
        assert_eq!(
            id.len(),
            "20261102T231500Z".len()
                + "-board0-smoke-".len()
                + std::process::id().to_string().len()
        );
    }
}
