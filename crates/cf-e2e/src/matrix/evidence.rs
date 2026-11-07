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

    /// Writes the checks, as JSON and as the table a person reads: `known` are the checks that
    /// fail on today's main by design (each until the work its detail names lands), said apart
    /// and never as passes.
    pub fn checks(&self, checks: &[Check], known: &[Check]) -> Result {
        let row = |check: &Check, known: bool| json!({ "who": check.who, "what": check.what, "ok": check.ok, "known": known, "detail": check.detail });
        let rows: Vec<Value> = checks
            .iter()
            .map(|check| row(check, false))
            .chain(known.iter().map(|check| row(check, true)))
            .collect();
        self.json("checks.json", &Value::Array(rows))?;
        let failed = checks.iter().filter(|check| !check.ok).count();
        let mut text = format!(
            "{}: {} of {} checks held{}\n\n{}\n",
            if failed == 0 && !checks.is_empty() {
                "GREEN"
            } else {
                "RED"
            },
            checks.len() - failed,
            checks.len(),
            if known.is_empty() {
                String::new()
            } else {
                format!(", and {} known red(s) wait for their fix", known.len())
            },
            crate::live::report::table(checks)
        );
        if !known.is_empty() {
            text.push_str(&format!(
                "\nKNOWN RED, by design on today's main, each until the work named lands (never a pass):\n{}\n",
                crate::live::report::table(known)
            ));
        }
        self.text("verdict.txt", &text)
    }
}

/// The fixture's files, each with the first 16 digits of its SHA-256: what a replay makes again.
pub fn fixture_hashes(fixture: &[(&str, &str)]) -> Value {
    Value::Array(
        fixture
            .iter()
            .map(|(name, text)| json!({ "name": name, "sha256": short_hash(text.as_bytes()) }))
            .collect(),
    )
}

fn short_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A consistent copy of the ledger `db` in `to`. The daemon holds its ledger in SQLite's
/// exclusive locking mode, which no reader can join, so the file and its WAL are cloned while
/// they hold still (as the lead's `backup-live-ledger.sh` does, five tries), the clone opened on
/// its own, and SQLite's `VACUUM INTO` writes the copy, whose integrity is checked.
pub fn ledger_snapshot(db: &Path, to: &Path) -> Result {
    let failed = |failed: rusqlite::Error| {
        Error::Daemon(format!(
            "the ledger {} could not be copied: {failed}",
            db.display()
        ))
    };
    let wal = PathBuf::from(format!("{}-wal", db.display()));
    let stamp = || {
        [db, wal.as_path()].map(|file| {
            std::fs::metadata(file)
                .ok()
                .map(|meta| (meta.len(), meta.modified().ok()))
        })
    };
    let clones =
        tempfile::tempdir().map_err(Error::file("make a folder in", &std::env::temp_dir()))?;
    let clone = clones.path().join("clone.db");
    let mut held = false;
    for _ in 0..5 {
        let before = stamp();
        std::fs::copy(db, &clone).map_err(Error::file("copy", db))?;
        if wal.is_file() {
            std::fs::copy(&wal, clones.path().join("clone.db-wal"))
                .map_err(Error::file("copy", &wal))?;
        }
        if stamp() == before {
            held = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    if !held {
        return Err(Error::Daemon(format!(
            "the ledger {} never held still for a copy",
            db.display()
        )));
    }
    if let Some(folder) = to.parent() {
        files::make_dir(folder)?;
    }
    let source = rusqlite::Connection::open(&clone).map_err(failed)?;
    source
        .execute("VACUUM INTO ?1", [to.to_string_lossy().as_ref()])
        .map_err(failed)?;
    let check: String = rusqlite::Connection::open(to)
        .and_then(|copy| copy.query_row("PRAGMA integrity_check", [], |row| row.get(0)))
        .map_err(failed)?;
    if check != "ok" {
        return Err(Error::Daemon(format!(
            "the copy of the ledger is not whole: {check}"
        )));
    }
    Ok(())
}

/// The check that no file of `folder` holds any of `secrets` (the windows' tokens): the
/// evidence is kept and shared, and holds no login, token or owner's history.
pub fn holds_none(folder: &Path, secrets: &[String]) -> Check {
    let secrets: Vec<&String> = secrets.iter().filter(|secret| secret.len() >= 8).collect();
    let mut found = Vec::new();
    let mut read = 0;
    for file in crate::checkout::files_below(folder, &[]).unwrap_or_default() {
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        read += 1;
        let text = String::from_utf8_lossy(&bytes);
        if secrets.iter().any(|secret| text.contains(secret.as_str())) {
            found.push(file.display().to_string());
        }
    }
    Check::new(
        "evidence",
        "no file of the evidence holds a window's token",
        found.is_empty() && read > 0,
        if found.is_empty() {
            format!("{read} file(s) read for {} token(s)", secrets.len())
        } else {
            format!("a token is in {}", found.join(", "))
        },
    )
}

/// The id of a run that starts now, of the seats `label` says (`board3`, `seats`, `stand-ins`)
/// and the scenario `scenario`: its time, to the second, then what it ran, the process and the
/// run's number in it.
pub fn run_id(label: &str, scenario: &str) -> String {
    static RUNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let now = jiff::Timestamp::now()
        .strftime("%Y%m%dT%H%M%SZ")
        .to_string();
    // Two runs of one process in the same second (the stand-in cases, side by side) have
    // folders of their own.
    let nth = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{now}-{label}-{scenario}-{}-{nth}", std::process::id())
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
            .checks(
                &[
                    Check::passed("a", "held", "x"),
                    Check::failed("b", "did not", "y"),
                ],
                &[Check::failed("c", "known", "until T-9")],
            )
            .unwrap();
        let verdict = std::fs::read_to_string(reports.path().join("r1/verdict.txt")).unwrap();
        assert!(verdict.starts_with("RED: 1 of 2 checks held"), "{verdict}");
        let rows: Value = serde_json::from_str(
            &std::fs::read_to_string(reports.path().join("r1/checks.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(rows[1]["ok"], false);
        assert_eq!(
            (rows[2]["known"].clone(), rows[2]["ok"].clone()),
            (json!(true), json!(false))
        );
        assert!(
            verdict.contains("and 1 known red(s) wait for their fix"),
            "{verdict}"
        );
        assert!(
            verdict.contains("KNOWN RED, by design on today's main"),
            "{verdict}"
        );
        assert!(reports.path().join("r1/manifest.json").is_file());
    }

    #[test]
    fn the_evidence_holds_no_token_and_a_ledger_is_copied_whole_while_it_is_held() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("a.json"), "{\"said\":\"hello\"}").unwrap();
        let tokens = ["0123456789abcdef".to_owned(), "short".to_owned()];
        assert!(holds_none(folder.path(), &tokens).ok);
        std::fs::write(
            folder.path().join("b.log"),
            "env CONSENSFLOW_TOKEN=0123456789abcdef",
        )
        .unwrap();
        let found = holds_none(folder.path(), &tokens);
        assert!(
            !found.ok && found.detail.contains("b.log"),
            "{}",
            found.detail
        );
        assert!(
            !holds_none(&folder.path().join("none"), &tokens).ok,
            "a folder with nothing read proves nothing"
        );

        let db = folder.path().join("ledger.db");
        let held = rusqlite::Connection::open(&db).unwrap();
        // Held as the daemon holds its ledger: no reader can join it.
        held.execute_batch("PRAGMA journal_mode=WAL; PRAGMA locking_mode=EXCLUSIVE; CREATE TABLE task(n INTEGER); INSERT INTO task VALUES (1), (2);").unwrap();
        assert!(rusqlite::Connection::open(&db)
            .and_then(|reader| reader
                .query_row("SELECT count(*) FROM task", [], |row| row.get::<_, i64>(0)))
            .is_err());
        let copy = folder.path().join("copy/ledger.db");
        ledger_snapshot(&db, &copy).unwrap();
        let read: i64 = rusqlite::Connection::open(&copy)
            .unwrap()
            .query_row("SELECT count(*) FROM task", [], |row| row.get(0))
            .unwrap();
        assert_eq!(read, 2);
        drop(held);
        assert_eq!(
            fixture_hashes(&[("a.cjs", "x")])[0]["sha256"],
            "2d711642b726b044"
        );
    }

    #[test]
    fn a_run_id_says_when_and_what_ran() {
        let id = run_id("board0", "smoke");
        assert!(id.contains("-board0-smoke-"), "{id}");
        assert_ne!(run_id("stand-ins", "smoke"), run_id("stand-ins", "smoke"));
        assert!(
            id.starts_with("20") && id.len() > "20261102T231500Z-board0-smoke-".len(),
            "{id}"
        );
    }
}
