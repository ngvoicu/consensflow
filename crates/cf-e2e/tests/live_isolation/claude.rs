//! What keeps a live run's Claude Code and Codex windows off the owner's own
//! config: a throwaway config folder holding a copy of the login, and the owner's
//! Claude files compared before and after. Shared by the live suites that open
//! Claude Code on a folder of their own (`live escape-unsent`, `live held-typed`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::report::Check;
use cf_e2e::{Error, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

/// The least time the Claude login copied must have before it ends: the run, and
/// the margin the owner's own Claude needs to have refreshed it first.
const LOGIN_LEFT: Duration = Duration::from_secs(2 * 60 * 60);

/// A throwaway config folder holding a copy of a login, readable by its owner
/// alone: removed with the value, however the run ends. `file` is the login's name
/// in it.
pub struct Throwaway {
    folder: TempDir,
    file: &'static str,
    copy: Vec<u8>,
}

impl Throwaway {
    /// A folder (mode 700) holding `login` as `file` (mode 600).
    pub fn holding(file: &'static str, login: Vec<u8>) -> Result<Self> {
        let failed = |what: &str, cause: std::io::Error| Error::Daemon(format!("{what}: {cause}"));
        let folder = TempDir::new().map_err(|cause| failed("a throwaway folder", cause))?;
        owners_alone(folder.path(), 0o700).map_err(|cause| failed("a throwaway folder", cause))?;
        let path = folder.path().join(file);
        fs::write(&path, &login).map_err(|cause| failed("the login's copy", cause))?;
        owners_alone(&path, 0o600).map_err(|cause| failed("the login's copy", cause))?;
        Ok(Self {
            folder,
            file,
            copy: login,
        })
    }

    /// The folder, `name` written in it as `text` too.
    pub fn with(self, name: &str, text: &str) -> Result<Self> {
        fs::write(self.folder.path().join(name), text)
            .map_err(|cause| Error::Daemon(format!("{name} in the throwaway folder: {cause}")))?;
        Ok(self)
    }

    pub fn path(&self) -> &Path {
        self.folder.path()
    }

    /// Whether the login is as it was copied: a harness that refreshed it has left
    /// the owner's own copy behind.
    pub fn check(&self, who: &str) -> Check {
        let now = fs::read(self.folder.path().join(self.file)).unwrap_or_default();
        Check::new(
            who,
            "the login copied is as it was",
            now == self.copy,
            if now == self.copy {
                "unchanged".to_owned()
            } else {
                "REFRESHED during the run: the owner's own login may now be stale".to_owned()
            },
        )
    }
}

/// `path` given the mode `mode`, its owner's alone, on Unix; on Windows a folder in
/// the user's temporary folder is the user's already.
#[cfg(unix)]
fn owners_alone(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn owners_alone(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// The owner's Claude login as a config folder's `.credentials.json` takes it: its
/// `claudeAiOauth` alone, none of its servers' logins, read where the machine keeps it
/// (the Keychain item of the user's account on a Mac, `~/.claude/.credentials.json`
/// elsewhere). Refused where its token ends within [`LOGIN_LEFT`]: the owner's own
/// Claude refreshes it first.
pub fn claude_login() -> Result<Vec<u8>> {
    cf_e2e::live::login::machine_login(LOGIN_LEFT)
}

/// What the owner's Claude files say: `settings.json` by its hash, and how much of
/// `history.jsonl`, `projects/` and `.claude.json`'s projects names the run's folder.
/// `.claude.json` changes by itself, so it is not hashed.
pub fn owners_claude(home: &Path, folder: &str) -> Vec<(String, String)> {
    let hash = |file: PathBuf| {
        fs::read(&file).map_or_else(
            |_| "none".to_owned(),
            |bytes| format!("{:x}", Sha256::digest(bytes)),
        )
    };
    let history = fs::read_to_string(home.join(".claude/history.jsonl")).map_or(0, |text| {
        text.lines().filter(|line| line.contains(folder)).count()
    });
    let projects = fs::read_dir(home.join(".claude/projects")).map_or(0, |entries| {
        entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(folder))
            .count()
    });
    let record: Value = fs::read_to_string(home.join(".claude.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let entries = record["projects"].as_object().map_or(0, |projects| {
        projects.keys().filter(|kept| kept.contains(folder)).count()
    });
    vec![
        (
            "~/.claude/settings.json".to_owned(),
            hash(home.join(".claude/settings.json")),
        ),
        (
            "~/.claude/history.jsonl lines of the run".to_owned(),
            history.to_string(),
        ),
        (
            "~/.claude/projects of the run".to_owned(),
            projects.to_string(),
        ),
        (
            "~/.claude.json projects of the run".to_owned(),
            entries.to_string(),
        ),
    ]
}

/// Whether the owner's Claude files are as `before`.
pub fn owners_unchanged(home: &Path, folder: &str, before: &[(String, String)]) -> Check {
    let after = owners_claude(home, folder);
    let changed: Vec<&str> = before
        .iter()
        .zip(&after)
        .filter(|(was, is)| was != is)
        .map(|(was, _)| was.0.as_str())
        .collect();
    Check::new(
        "claude",
        "the owner's Claude files",
        changed.is_empty(),
        if changed.is_empty() {
            "unchanged".to_owned()
        } else {
            format!("changed: {}", changed.join(", "))
        },
    )
}
