//! The login a live run's Claude Code windows start on, and what keeps the run
//! out of the person's own `~/.claude`.
//!
//! A Claude Code that starts in the person's config folder writes there: its
//! settings, the prompts it is typed (`history.jsonl`), the project it was started
//! in (`~/.claude.json`), its conversations. A live run on the person's own
//! config left such things behind, and rewrote `settings.json` (T-249, 2026-10-22).
//! So a run's Claude windows run on a config folder of the run's own
//! (`CLAUDE_CONFIG_DIR`), which holds a copy of the machine's login and is removed
//! when the run is over: [`Login`] makes it, and the rig asks it what the run came
//! to ([`ClaudeConfig`]).
//!
//! **Where the login is.** On a Mac Claude Code keeps it in the login Keychain, as
//! the item `Claude Code-credentials` of the account that is the user's name: the
//! Keychain also holds one for the account `unknown`, from a run that had no `USER`,
//! which holds no refresh token and is long expired, and a lookup that names no
//! account returns that one. On Linux and on Windows it is
//! `~/.claude/.credentials.json` (`%USERPROFILE%\.claude\.credentials.json`).
//! Either is a JSON text that holds the login (`claudeAiOauth`) and the OAuth
//! tokens of the person's MCP servers (`mcpOAuth`): only the login is copied, into
//! the folder's `.credentials.json` (mode 600 where the system has a mode; the user's
//! own temporary folder on Windows), where a Claude Code with `CLAUDE_CONFIG_DIR` set
//! reads it. Nothing is ever written to the Keychain, and nothing is ever printed of
//! the login.
//!
//! **A fresh config folder** opens Claude Code on its first-run dialogs; the folder
//! is given the one file that answers them (`.claude.json`, with the onboarding
//! done). The question about trusting a folder is the run's to answer, as it is
//! for any folder Claude Code has not seen ([`super::window::trust_claude`]).
//!
//! **The refresh hazard.** A refresh token is used once: if the copy refreshes the
//! access token, the person's Keychain item holds a refresh token that no longer
//! works, and they are logged out the next time theirs needs it. Claude Code
//! refreshes a token when five minutes of it are left (`now + 300000 >= expiresAt`,
//! 2.1.309), so a run that is not longer than the token has left never does. A run
//! starts only on a login whose access token lasts at least as long as the run is
//! to take, with a margin: [`LEFT`] for the live suites (all shorter), or the
//! length a run states for itself ([`Login::of_machine_lasting`]: an eval states its
//! own, hours long). It says it if the copy changed all the same: the folder's
//! `.credentials.json` is compared with what was copied, and the Keychain with the
//! items it had. Nothing refreshes it by hand, here or anywhere.
//!
//! **What the run is held to** ([`ClaudeConfig::finish`], when its rig ends): the
//! person's `~/.claude/settings.json` is as it was; `~/.claude.json` has no project
//! of the run's folders that it did not have; `~/.claude/history.jsonl` has no
//! prompt typed in one; `~/.claude/projects` has no conversation of one. (Not the
//! whole of `~/.claude.json`: the person's own Claude Code sessions write the rest
//! of it all day.) A run that fails any of them fails, with the Claude Code of the
//! person's windows named as the other possible writer. The run's folders are the
//! live suites' (`~/.consensflow-candidate/live/…`), the rigs' homes, and the evals'
//! workspace (`~/.consensflow-candidate/evals/…`).
//!
//! **What a kept home keeps.** The folder goes with the run, because it holds a login.
//! A rig whose home is kept (`--keep`, or a case that reads the home after it ended)
//! asks the folder to copy what Claude Code kept there into the home first
//! ([`ClaudeConfig::keep_records`]): its conversations (`projects/`) and its sessions
//! (`sessions/`, but for the `.key` files of its peers' sockets), never the login.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::process::{is_alive, own_var, Run};
use crate::rig::ClaudeConfig;
use crate::{files, Error, Result};

/// The Keychain service Claude Code keeps its login under, and the start of the
/// names it keeps another config folder's under (`Claude Code-credentials-<8 hex>`).
const SERVICE: &str = "Claude Code-credentials";

/// How long the login's access token must last for a live suite to start on a copy
/// of it. Claude Code refreshes a token when five minutes of it are left, so a run
/// shorter than this never refreshes it; the longest live suite (the approval run)
/// takes well under it, with the margin the person's own use of Claude Code needs to
/// refresh it first. A run that is longer, or that knows its length, asks for what it
/// needs ([`Login::of_machine_lasting`]): an eval is hours long.
pub const LEFT: Duration = Duration::from_secs(2 * 60 * 60);

/// The user's own home on this machine: `HOME`, else `USERPROFILE` (Windows' own), as the rest
/// of the live kit finds it ([`super::machine::home`]); none where the run has neither.
fn machine_home() -> Option<PathBuf> {
    super::machine::home().ok()
}

/// What the folder of a run starts with in `.claude.json`: the first-run dialogs
/// answered, and the notice of the default permission mode seen, which would
/// otherwise be drawn above the prompt that the windows' readers read.
const CLAUDE_JSON: &str = r#"{"hasCompletedOnboarding":true,"hasSeenAutoDefaultNotice":true}"#;

/// What the name of a run's config folder begins with, in the temporary folder: the id of
/// the process that made it follows, so that one a killed run left behind is known to be
/// nobody's ([`sweep`]).
const PREFIX: &str = "consensflow-live-claude-";

/// Where the machine keeps the login of Claude Code.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// The login Keychain, as the item of the user's own account.
    Keychain,
    /// A file, where Claude Code on Linux and on Windows keeps it.
    File(PathBuf),
}

impl Source {
    /// Where this machine keeps it.
    fn of_machine() -> Option<Self> {
        Self::on(cfg!(target_os = "macos"), machine_home())
    }

    /// Where a machine keeps it: the Keychain on a Mac (`macos`), else the file below the
    /// user's `home` (none where the run has no home to look in).
    fn on(macos: bool, home: Option<PathBuf>) -> Option<Self> {
        if macos {
            return Some(Self::Keychain);
        }
        Some(Self::File(home?.join(".claude").join(".credentials.json")))
    }

    /// What it holds, as its text; never said anywhere.
    fn read(&self) -> Result<String> {
        match self {
            Self::File(file) => files::read_string(file).map_err(|failed| {
                Error::Daemon(format!(
                    "Claude Code's login is not in {} ({failed}): log in by running `claude` once",
                    file.display()
                ))
            }),
            Self::Keychain => {
                let user = own_var("USER")
                    .or_else(|| own_var("LOGNAME"))
                    .filter(|user| !user.is_empty())
                    .ok_or_else(|| {
                        Error::Daemon(
                            "this run has no USER, which names the account of Claude Code's login in the Keychain"
                                .to_owned(),
                        )
                    })?;
                // The login Keychain asks the person once to let it be read; a run
                // waits for the answer (the call is limited, below).
                let ran = Run::new("/usr/bin/security")
                    .args(keychain_args(&user))
                    .inheriting_env()
                    .limit(Duration::from_secs(120))
                    .run()?;
                if ran.code != Some(0) {
                    // What it printed is the login, or nothing: only what it said is shown.
                    return Err(Error::Daemon(format!(
                        "Claude Code's login is not in the Keychain as the item `{SERVICE}` of the account {user} \
                         (security exited {}: {}): log in by running `claude` once",
                        ran.code.map_or_else(|| "by a signal".to_owned(), |code| code.to_string()),
                        ran.stderr.trim()
                    )));
                }
                Ok(ran.stdout.trim_end_matches('\n').to_owned())
            }
        }
    }

    /// What it is called in what a run says.
    fn described(&self) -> String {
        match self {
            Self::Keychain => format!("the Keychain item {SERVICE}"),
            Self::File(file) => file.display().to_string(),
        }
    }
}

/// The words `security` is given to print the password of the item of the user's own
/// account: with `-a`, because the Keychain has another item of the service (the account
/// `unknown`, expired and with no refresh token) and a lookup that names no account
/// returns that one.
fn keychain_args(user: &str) -> Vec<String> {
    ["find-generic-password", "-s", SERVICE, "-a", user, "-w"]
        .map(str::to_owned)
        .into()
}

/// The part of `text`, the login as the machine keeps it, that a run's folder is given:
/// `claudeAiOauth` and nothing else of it (the OAuth tokens of the person's MCP servers
/// are in the same text). An error says why the machine's login cannot be used, or that
/// the access token lasts less than `left` from `now` (milliseconds since the epoch);
/// never what the text held.
fn oauth(text: &str, now: i64, left: Duration) -> Result<Value> {
    let not_usable =
        |why: &str| Error::Daemon(format!("Claude Code's login cannot be used: {why}"));
    let login: Value = serde_json::from_str(text).map_err(|_| not_usable("it is not JSON"))?;
    let oauth = login
        .get("claudeAiOauth")
        .filter(|oauth| oauth.is_object())
        .ok_or_else(|| {
            not_usable(
                "it holds no `claudeAiOauth`; Claude Code is not logged in with a Claude account",
            )
        })?;
    for field in ["accessToken", "refreshToken"] {
        if !oauth[field].as_str().is_some_and(|token| !token.is_empty()) {
            return Err(not_usable(&format!("its `claudeAiOauth` has no `{field}`")));
        }
    }
    let expires = oauth["expiresAt"]
        .as_i64()
        .ok_or_else(|| not_usable("its `claudeAiOauth` says no `expiresAt`"))?;
    let remaining = Duration::from_millis(u64::try_from(expires - now).unwrap_or(0));
    if remaining < left {
        let state = if expires <= now {
            "has expired".to_owned()
        } else {
            format!("expires in {}", span(remaining))
        };
        return Err(Error::Daemon(format!(
            "Claude Code's login {state}, and this run needs it to last at least {} more: a run's \
             Claude Code refreshes a token that is about to end, and the refresh would leave the login \
             that this machine keeps unusable (the owner is logged out the next time theirs needs it). \
             Claude Code refreshes a token only when five minutes of it are left, so using it now does \
             not help: run again after the owner's own Claude Code has refreshed it, or make the run \
             shorter",
            span(left)
        )));
    }
    Ok(oauth.clone())
}

/// A span as hours and minutes: `1 h 12 min`, `45 min`.
fn span(span: Duration) -> String {
    let minutes = span.as_secs() / 60;
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

/// The names of the Keychain items of the service `Claude Code-credentials` (and of the
/// ones Claude Code names after a config folder) in what `security dump-keychain` prints,
/// which holds the attributes of the items and none of their passwords.
fn services(dump: &str) -> BTreeSet<String> {
    dump.lines()
        .filter_map(|line| line.trim().strip_prefix("\"svce\"<blob>=\""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|name| name.starts_with(SERVICE))
        .map(str::to_owned)
        .collect()
}

/// The names of those items now; none where the machine has no Keychain (or it cannot be
/// listed, which a run then cannot check).
fn keychain_services() -> Option<BTreeSet<String>> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let ran = Run::new("/usr/bin/security")
        .arg("dump-keychain")
        .inheriting_env()
        .limit(Duration::from_secs(60))
        .run()
        .ok()
        .filter(|ran| ran.code == Some(0))?;
    Some(services(&ran.stdout))
}

/// What the Keychain has now (`now`) of Claude Code's items that it did not have before the run
/// (`before`), in words: Claude Code writes an item for the config folder it refreshes a
/// login in.
fn gained(before: &BTreeSet<String>, now: &BTreeSet<String>) -> Vec<String> {
    now.difference(before)
        .map(|name| {
            format!(
                "the Keychain gained the item `{name}`, which Claude Code writes when it refreshes a login \
                 (delete it with `security delete-generic-password -s \"{name}\"`)"
            )
        })
        .collect()
}

/// Whether `path` is a folder a run worked in: a live folder of the candidate
/// (`~/.consensflow-candidate/live/…`), the evals' (`~/.consensflow-candidate/evals/…`: the
/// workspace of every eval), or in the home of a rig.
fn is_run_folder(path: &str) -> bool {
    // Windows writes a folder with its own separator in some places and with ours in others.
    let path = path.replace('\\', "/");
    path.contains("/.consensflow-candidate/live/")
        || path.contains("/.consensflow-candidate/evals/")
        || path.contains("consensflow-integration-")
}

/// Whether `name` is the name Claude Code gives (in `~/.claude/projects`) to a folder of a run.
fn is_run_name(name: &str) -> bool {
    name.contains("-consensflow-candidate-live-")
        || name.contains("-consensflow-candidate-evals-")
        || name.contains("consensflow-integration-")
}

/// The hash of `bytes`, as hexadecimal.
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// What the person's own Claude setup is at a moment, as far as a run could have changed it
/// (the rest of `~/.claude.json` is written all day by the person's own sessions).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    /// The hash of `~/.claude/settings.json`; none where there is none.
    settings: Option<String>,
    /// The size of `~/.claude/history.jsonl`: what is after it later was typed since.
    history: u64,
    /// The hash of each project of `~/.claude.json` that is a folder of a run.
    projects: BTreeMap<String, String>,
    /// The folders of `~/.claude/projects` that are a folder of a run's.
    conversations: BTreeSet<String>,
}

/// The person's own Claude setup, in `home`.
#[derive(Debug, Clone)]
struct Owner {
    home: PathBuf,
}

impl Owner {
    fn settings(&self) -> PathBuf {
        self.home.join(".claude").join("settings.json")
    }

    fn history(&self) -> PathBuf {
        self.home.join(".claude").join("history.jsonl")
    }

    /// What it is now.
    fn look(&self) -> Seen {
        let settings = fs::read(self.settings()).ok().map(|bytes| digest(&bytes));
        let history = fs::metadata(self.history()).map_or(0, |meta| meta.len());
        let projects = fs::read_to_string(self.home.join(".claude.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|json| json["projects"].as_object().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|(path, _)| is_run_folder(path))
            .map(|(path, entry)| (path, digest(entry.to_string().as_bytes())))
            .collect();
        let conversations = fs::read_dir(self.home.join(".claude").join("projects"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_run_name(name))
            .collect();
        Seen {
            settings,
            history,
            projects,
            conversations,
        }
    }

    /// What `history.jsonl` gained since `before`: the prompts typed since.
    fn typed_since(&self, before: &Seen) -> String {
        let read = || -> io::Result<Vec<u8>> {
            let mut file = fs::File::open(self.history())?;
            file.seek(SeekFrom::Start(before.history))?;
            let mut appended = Vec::new();
            file.read_to_end(&mut appended)?;
            Ok(appended)
        };
        read()
            .map(|appended| String::from_utf8_lossy(&appended).into_owned())
            .unwrap_or_default()
    }

    /// What the run did to the person's setup since `before`, in words: nothing, if it left it be.
    fn changes_since(&self, before: &Seen) -> Vec<String> {
        changes(before, &self.look(), &self.typed_since(before))
    }
}

/// What differs between `before` and `after` that only a run could have done, in words;
/// `typed` is the text `history.jsonl` gained in between.
fn changes(before: &Seen, after: &Seen, typed: &str) -> Vec<String> {
    let mut found = Vec::new();
    if before.settings != after.settings {
        found.push("~/.claude/settings.json is not as it was".to_owned());
    }
    for (path, hash) in &after.projects {
        match before.projects.get(path) {
            None => found.push(format!("~/.claude.json gained the project {path}")),
            Some(was) if was != hash => {
                found.push(format!("~/.claude.json changed the project {path}"));
            }
            Some(_) => {}
        }
    }
    for name in after.conversations.difference(&before.conversations) {
        found.push(format!("~/.claude/projects gained {name}"));
    }
    let prompts = typed
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|line| line["project"].as_str().is_some_and(is_run_folder))
        .count();
    if prompts > 0 {
        found.push(format!(
            "~/.claude/history.jsonl gained {prompts} prompt(s) typed in a folder of the run"
        ));
    }
    found
}

/// The machine's login as a config folder's `.credentials.json` holds it (`claudeAiOauth`
/// alone, as JSON), for a suite that makes its own config folder: read where this machine
/// keeps it, and refused as a [`Login`] refuses it, when its access token lasts less than
/// `left` more. Never said anywhere.
pub fn machine_login(left: Duration) -> Result<Vec<u8>> {
    let source = Source::of_machine().ok_or_else(|| {
        Error::Daemon("this run has no home to look for Claude Code's login in".to_owned())
    })?;
    let now = jiff::Timestamp::now().as_millisecond();
    let oauth = oauth(&source.read()?, now, left)?;
    serde_json::to_vec(&json!({ "claudeAiOauth": oauth }))
        .map_err(|failed| Error::Daemon(format!("the login's copy: {failed}")))
}

/// The folder a run's Claude windows run on, once it is made.
struct Made {
    /// The folder, removed when this is dropped: with the run, however it ends.
    folder: TempDir,
    /// The login as it was copied.
    oauth: Value,
    /// The person's setup when it was made.
    owner: Seen,
    /// The Keychain items of Claude Code then; none where they cannot be listed.
    services: Option<BTreeSet<String>>,
}

/// The fields of a login that a refresh changes: the tokens, and when the access token ends. A
/// refresh cannot leave all three as they were, so a login whose three are as they were copied
/// was not refreshed, whatever else Claude Code wrote beside them.
const TOKENS: [&str; 3] = ["accessToken", "refreshToken", "expiresAt"];

/// What a run's Claude Code left of the copy of the login in its folder, judged by the names of
/// the fields that differ (never their values).
#[derive(Debug, PartialEq, Eq)]
enum Copy {
    /// The login as it was copied.
    Kept,
    /// Claude Code wrote other fields of the login (`subscriptionType`, `rateLimitTier`: what it
    /// learns of the account) and left its tokens as they were: the machine's login is not stale.
    Rewritten(Vec<String>),
    /// A token was changed: Claude Code refreshed the copy, and the refresh token the machine keeps
    /// may be used up. The names of the fields that differ; none where the login itself is gone.
    Refreshed(Vec<String>),
    /// The folder's `.credentials.json` is gone or holds no JSON.
    Lost,
}

impl Copy {
    /// What `now`, the folder's `.credentials.json` as it is (none where it is gone or no JSON), is
    /// of the login `copied`.
    fn of(copied: &Value, now: Option<&Value>) -> Self {
        let Some(now) = now else {
            return Self::Lost;
        };
        let Some(now) = now.get("claudeAiOauth").and_then(Value::as_object) else {
            return Self::Refreshed(Vec::new());
        };
        let Some(copied) = copied.as_object() else {
            return Self::Refreshed(Vec::new());
        };
        let mut names: Vec<&String> = copied.keys().chain(now.keys()).collect();
        names.sort();
        names.dedup();
        let (tokens, others): (Vec<String>, Vec<String>) = names
            .into_iter()
            .filter(|name| copied.get(*name) != now.get(*name))
            .cloned()
            .partition(|name| TOKENS.contains(&name.as_str()));
        match (tokens.is_empty(), others.is_empty()) {
            (false, _) => Self::Refreshed(tokens.into_iter().chain(others).collect()),
            (true, false) => Self::Rewritten(others),
            (true, true) => Self::Kept,
        }
    }
}

impl Made {
    /// What the folder's `.credentials.json` is of the login that was copied.
    fn copy(&self) -> Copy {
        let now = fs::read_to_string(self.folder.path().join(".credentials.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        Copy::of(&self.oauth, now.as_ref())
    }

    /// Whether the copy was refreshed or lost, in words: the hazard `finish` fails the run for.
    fn refreshed(&self) -> Option<String> {
        match self.copy() {
            Copy::Kept | Copy::Rewritten(_) => None,
            Copy::Refreshed(fields) if fields.is_empty() => Some(
                "the run's Claude Code wrote a different login in the folder's .credentials.json"
                    .to_owned(),
            ),
            Copy::Refreshed(fields) => Some(format!(
                "the run's Claude Code wrote a different login in the folder's .credentials.json \
                 (it changed {})",
                fields.join(", ")
            )),
            Copy::Lost => {
                Some("the folder's .credentials.json is gone or no longer holds a login".to_owned())
            }
        }
    }

    /// What Claude Code wrote of the login besides its tokens, if it did, in words: no failure,
    /// but a thing for whoever reads the output to know the copy is not byte for byte.
    fn rewritten(&self) -> Option<String> {
        match self.copy() {
            Copy::Rewritten(fields) => Some(format!(
                "the run's Claude Code wrote {} of the login in the folder's .credentials.json and \
                 left its tokens as they were copied: the machine's login was not refreshed",
                fields.join(", ")
            )),
            _ => None,
        }
    }
}

/// The login a run's Claude Code windows start on: a config folder of its own, made when
/// the rig first asks for it ([`ClaudeConfig::folder`]), with a copy of the machine's
/// login, and removed when the last holder of this is dropped.
pub struct Login {
    source: Source,
    owner: Owner,
    /// Where the folder is made.
    parent: PathBuf,
    /// How long the access token must last more for the folder to be made.
    left: Duration,
    made: Mutex<Option<Made>>,
}

impl Login {
    /// The login of this machine's Claude Code, for a live suite on it ([`LEFT`]); none where
    /// the run has no home to look for it in.
    pub fn of_machine() -> Option<Self> {
        Self::of_machine_lasting(LEFT)
    }

    /// The login of this machine's Claude Code, for a run that takes `left` at the most, margin
    /// included: the folder is made only if the access token lasts that much more (an eval, hours
    /// long, states its own length; the live suites' is [`LEFT`]). None where the run has no home
    /// to look for it in.
    pub fn of_machine_lasting(left: Duration) -> Option<Self> {
        let source = Source::of_machine()?;
        let home = machine_home()?;
        let mut login = Self::new(source, home, std::env::temp_dir());
        login.left = left;
        Some(login)
    }

    fn new(source: Source, owner_home: PathBuf, parent: PathBuf) -> Self {
        Self {
            source,
            owner: Owner { home: owner_home },
            parent,
            left: LEFT,
            made: Mutex::new(None),
        }
    }

    /// The folder, made as the clock says `now` is (milliseconds since the epoch).
    fn folder_at(&self, now: i64) -> Result<PathBuf> {
        let mut made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(made) = made.as_ref() {
            return Ok(made.folder.path().to_path_buf());
        }
        let oauth = oauth(&self.source.read()?, now, self.left)?;
        sweep(&self.parent);
        let folder = tempfile::Builder::new()
            .prefix(&format!("{PREFIX}{}-", std::process::id()))
            .tempdir_in(&self.parent)
            .map_err(|source| Error::File {
                action: "make a folder in",
                path: self.parent.clone(),
                source,
            })?;
        private_folder(folder.path())?;
        let credentials = json!({ "claudeAiOauth": oauth }).to_string();
        write_private(&folder.path().join(".credentials.json"), &credentials)?;
        write_private(&folder.path().join(".claude.json"), CLAUDE_JSON)?;
        // The live folders trusted as the owner's own config trusts them (T-629).
        super::machine::seed_claude_trust(folder.path(), &self.owner.home)?;
        let path = folder.path().to_path_buf();
        *made = Some(Made {
            folder,
            oauth,
            owner: self.owner.look(),
            services: keychain_services(),
        });
        Ok(path)
    }
}

/// Makes the folder `path` the user's alone (mode 700): a temporary folder is made with the
/// permissions the user's umask leaves, which let others in.
fn private_folder(path: &Path) -> Result {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(Error::file("make private", path))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Makes `path` hold `text`, readable by the user alone (mode 600).
fn write_private(path: &Path, text: &str) -> Result {
    private(path, text).map_err(Error::file("write", path))
}

#[cfg(unix)]
fn private(path: &Path, text: &str) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())
}

#[cfg(not(unix))]
fn private(path: &Path, text: &str) -> io::Result<()> {
    fs::write(path, text)
}

/// What Claude Code keeps in its config folder that a kept home keeps of a run: the conversations
/// and the sessions. (Not the login, nor `.claude.json` and what else is in the folder.)
const RECORDS: [&str; 2] = ["projects", "sessions"];

/// Copies the folders of [`RECORDS`] that `from` has into `into`, where `into` has them already
/// as they were copied before (a run's rig that ended and was started again on its home).
fn copy_records(from: &Path, into: &Path) -> Result {
    for name in RECORDS {
        copy_tree(&from.join(name), &into.join(name))?;
    }
    Ok(())
}

/// The folder `from` copied into `to` with the files and folders in it; none of it where there is
/// none. A link is not followed, and a `.key` (the secret of a session's socket, of no use once its
/// process is gone) is not copied.
fn copy_tree(from: &Path, to: &Path) -> Result {
    let Ok(entries) = fs::read_dir(from) else {
        return Ok(());
    };
    files::make_dir(to)?;
    for entry in entries.flatten() {
        let (source, name) = (entry.path(), entry.file_name());
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            copy_tree(&source, &to.join(&name))?;
        } else if kind.is_file() && source.extension().is_none_or(|ext| ext != "key") {
            files::copy(&source, &to.join(&name))?;
        }
    }
    Ok(())
}

/// Takes away the folders of runs that were killed before they could (`PREFIX`, the id of
/// the process, and a random word), in `parent`: a folder whose process is gone is nobody's,
/// and holds a login. A process that is there keeps its own.
fn sweep(parent: &Path) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let owner = name
            .strip_prefix(PREFIX)
            .and_then(|rest| rest.split('-').next())
            .and_then(|id| id.parse::<u32>().ok());
        let is_folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if is_folder && owner.is_some_and(|id| !is_alive(id)) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

impl fmt::Debug for Login {
    /// Where it is copied from and where, never what it holds.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        formatter
            .debug_struct("Login")
            .field("source", &self.source)
            .field("folder", &made.as_ref().map(|made| made.folder.path()))
            .finish_non_exhaustive()
    }
}

impl ClaudeConfig for Login {
    fn folder(&self) -> Result<PathBuf> {
        self.folder_at(jiff::Timestamp::now().as_millisecond())
    }

    fn described(&self) -> String {
        self.source.described()
    }

    fn keep_records(&self, into: &Path) -> Result {
        let made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        match made.as_ref() {
            Some(made) => copy_records(made.folder.path(), into),
            None => Ok(()),
        }
    }

    fn finish(&self) -> Result {
        let made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(made) = made.as_ref() else {
            return Ok(());
        };
        let touched = self.owner.changes_since(&made.owner);
        if let Some(note) = made.rewritten() {
            let _ = writeln!(io::stderr(), "note: {note}");
        }
        let mut refreshed: Vec<String> = made.refreshed().into_iter().collect();
        if let (Some(before), Some(now)) = (&made.services, keychain_services()) {
            refreshed.extend(gained(before, &now));
        }
        if touched.is_empty() && refreshed.is_empty() {
            return Ok(());
        }
        let said = said(&touched, &refreshed);
        // The error is the case's to fail with; the banner is for whoever reads the output.
        let _ = writeln!(io::stderr(), "{said}");
        Err(Error::Daemon(said))
    }
}

/// What `finish` says when the run touched the person's setup (`touched`) or its copy of the
/// login was refreshed (`refreshed`): loud, and with what to do. Never what the login holds.
fn said(touched: &[String], refreshed: &[String]) -> String {
    let bar = "!".repeat(78);
    let mut said = format!("\n{bar}\n");
    if !touched.is_empty() {
        said.push_str(
            "THE RUN CHANGED THE OWNER'S CLAUDE SETUP, which a live run on its own login is never to touch \
             (or the owner's own Claude Code did, while it ran):\n",
        );
        for change in touched {
            said.push_str(&format!("  - {change}\n"));
        }
    }
    if !refreshed.is_empty() {
        said.push_str(
            "THE RUN'S CLAUDE CODE REFRESHED ITS COPY OF THE LOGIN. A refresh token is used once, so the login \
             this machine keeps (the Keychain's) may now be stale, and Claude Code may ask the owner to log in \
             again:\n",
        );
        for change in refreshed {
            said.push_str(&format!("  - {change}\n"));
        }
    }
    said.push_str(&bar);
    said
}

#[cfg(test)]
mod portable;
#[cfg(test)]
mod tests;
