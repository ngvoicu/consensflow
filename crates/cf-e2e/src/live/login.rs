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
//! works, and they are logged out the next time theirs needs it. So the folder's
//! `.credentials.json` holds the access token and **no refresh token** ([`RENEWING`]): a
//! Claude Code that cannot renew cannot use up the person's (measured on zeewin2, 2.1.316: it
//! runs on such a copy, does not look at the copy's expiry, and where the server refuses the
//! token it fails loudly with a 401, the person's login untouched). A run starts only on a login
//! whose access token lasts at least as long as the run is to take, with a margin: the length the
//! case plans for itself and the margin ([`Login::of_machine_lasting`]: a live case states its own,
//! minutes long, and an eval its own, hours long). It says it if the copy changed all the same:
//! the folder's `.credentials.json` is compared with what was copied, and the Keychain with the
//! items it had. Nothing refreshes it by hand, here or anywhere. (On a test machine, the
//! one exception: a login that has run short is renewed by Claude Code itself, run once as
//! the owner, before the copy is made; see [`super::renew`], T-1067.)
//!
//! **What the run is held to** ([`ClaudeConfig::finish`], when its rig ends): the
//! person's login (the Keychain item's `claudeAiOauth`, never its other tokens) is as it
//! was; `~/.claude/settings.json` is as it was; `~/.claude.json` has no project
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

use super::keychain;
use super::machine::Harness;
use super::renew::{self, Kept, Renewal, Renewed, TestMachine};
use crate::process::{is_alive, own_var, Run};
use crate::rig::ClaudeConfig;
use crate::{files, Error, Result};

/// The Keychain service Claude Code keeps its login under, and the start of the
/// names it keeps another config folder's under (`Claude Code-credentials-<8 hex>`).
pub(super) const SERVICE: &str = "Claude Code-credentials";

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
             Claude Code renews a token itself, but only when five minutes of it are left, so using it \
             now does not help: run again once it has ended (Claude Code then renews it, as it does \
             when the owner opens it, and on a test machine the kit asks it to), or make the run \
             shorter",
            span(left)
        )));
    }
    Ok(oauth.clone())
}

/// When the login in `text`, the machine's, ends: the `expiresAt` of its `claudeAiOauth`
/// (milliseconds since the epoch); none where the text holds none.
fn expires_at(text: &str) -> Option<i64> {
    serde_json::from_str::<Value>(text)
        .ok()?
        .get("claudeAiOauth")?
        .get("expiresAt")?
        .as_i64()
}

/// Whether the login in `text` has run short of `left` from `now` and Claude Code could renew
/// it: it has a refresh token, and an expiry that is less than `left` away. A text that is no
/// login, or has no refresh token, is not for a renewal to mend.
fn renewable(text: &str, now: i64, left: Duration) -> bool {
    let has_refresh = serde_json::from_str::<Value>(text)
        .ok()
        .is_some_and(|login| {
            login["claudeAiOauth"]["refreshToken"]
                .as_str()
                .is_some_and(|token| !token.is_empty())
        });
    let remaining = |expires: i64| Duration::from_millis(u64::try_from(expires - now).unwrap_or(0));
    has_refresh && expires_at(text).is_some_and(|expires| remaining(expires) < left)
}

/// A span as hours and minutes: `1 h 12 min`, `45 min`.
pub(super) fn span(span: Duration) -> String {
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
pub fn keychain_services() -> Option<BTreeSet<String>> {
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

/// What the Keychain came to for a run whose Claude Code worked in `folder`, from the names it had
/// `before` the run and `now`.
#[derive(Debug, Default, PartialEq, Eq)]
struct Kept {
    /// The item Claude Code made for the run's folder (in words): the run's own, and a failure.
    own: Vec<String>,
    /// The other items gained since, which are some other process's as far as this run can tell (the
    /// Keychain is the whole machine's): in words, and no failure of this run. The guard of the
    /// launchers is what fails a rig for them ([`super::launcher::Guard`]).
    others: Vec<String>,
}

/// What the Keychain came to for the run that worked in `folder` ([`Kept`]).
///
/// Claude Code keeps what it stores securely in the Keychain, as an item named for the folder's path
/// ([`keychain::item_of`]), once it writes it: the OAuth state of an MCP server it connected to
/// is enough, and the login the folder held goes into the item with it and out of the folder
/// (T-1102). Before, every item gained was said to be a refresh of the login, and one made by
/// another run on the machine failed this one (T-1044).
fn keychain_of(folder: &Path, before: &BTreeSet<String>, now: &BTreeSet<String>) -> Kept {
    let own = keychain::item_of(folder);
    let mut kept = Kept::default();
    for name in now.difference(before) {
        if *name == own {
            kept.own.push(format!(
                "Claude Code kept the secure storage of the run's folder in the Keychain item `{own}` (named for the \
                 folder's path), not in the folder: the OAuth state of the MCP servers it connected to and, when the \
                 folder held a login, the copy of it, which it takes out of the folder's .credentials.json. The item \
                 outlives the folder (delete it with `security delete-generic-password -s \"{own}\"`; the kit never \
                 does), and a window that was started with --strict-mcp-config has no server to keep any of it for"
            ));
        } else {
            kept.others.push(keychain::gained_words(name));
        }
    }
    kept
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
pub(super) fn digest(bytes: &[u8]) -> String {
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

/// The folder a run's Claude windows run on, once it is made.
struct Made {
    /// The login as it was copied: the machine's, without what renews it ([`RENEWING`]).
    oauth: Value,
    /// The machine's login as it was read, refresh token and all: what the machine's file is
    /// compared with when the run ends.
    login: Value,
    /// The person's setup when it was made.
    owner: Seen,
    /// The Keychain items of Claude Code then; none where they cannot be listed.
    services: Option<BTreeSet<String>>,
    /// What the renewal of the login came to, where the login had run short on a test machine.
    renewed: Option<Renewed>,
    /// The folder, removed when this is dropped: with the run, however it ends.
    folder: TempDir,
}

/// What lets a login renew itself, and is never given to a run's copy: the refresh token and when it
/// ends. A copy that holds only the access token cannot renew it, so it cannot use up the refresh
/// token the machine keeps (T-1067, measured on zeewin2: Claude Code 2.1.316 runs on such a copy,
/// does not look at its expiry, and fails with a 401 where the server refuses the token).
const RENEWING: [&str; 2] = ["refreshToken", "refreshTokenExpiresAt"];

/// The part of `oauth`, the machine's login, that a run's folder is given: all but what renews it.
fn without_renewal(oauth: &Value) -> Value {
    let mut copy = oauth.clone();
    if let Some(fields) = copy.as_object_mut() {
        for field in RENEWING {
            fields.remove(field);
        }
    }
    copy
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

impl Copy {
    /// Whether the copy was refreshed or lost, in words: the hazard a run is failed for.
    fn refreshed(&self) -> Option<String> {
        match self {
            Self::Kept | Self::Rewritten(_) => None,
            Self::Refreshed(fields) if fields.is_empty() => Some(
                "the run's Claude Code wrote a different login in the folder's .credentials.json"
                    .to_owned(),
            ),
            Self::Refreshed(fields) => Some(format!(
                "the run's Claude Code wrote a different login in the folder's .credentials.json \
                 (it changed {})",
                fields.join(", ")
            )),
            Self::Lost => {
                Some("the folder's .credentials.json is gone or no longer holds a login".to_owned())
            }
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
        self.copy().refreshed()
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
    /// Claude Code run once as the owner to renew a login that has run short: only on a test
    /// machine ([`TestMachine::detect`]); none elsewhere, where such a login is refused.
    renewal: Option<Renewal>,
    made: Mutex<Option<Made>>,
}

impl Login {
    /// The login of this machine's Claude Code, for a run that takes `left` at the most, margin
    /// included: the folder is made only if the access token lasts that much more (a live case
    /// states its planned length and the margin, [`super::Log::lasting`]; an eval, hours long,
    /// its own). Claude Code refreshes a token when five minutes of it are left, so a run that is
    /// shorter than the token has left never does. None where the run has no home to look for it in.
    pub fn of_machine_lasting(left: Duration) -> Option<Self> {
        Self::of_machine_on(TestMachine::detect(), left)
    }

    /// [`Login::of_machine_lasting`] on `machine`, the test machine this run is on, if it is on
    /// one: only there is a login that has run short renewed.
    fn of_machine_on(machine: Option<TestMachine>, left: Duration) -> Option<Self> {
        let source = Source::of_machine()?;
        let home = machine_home()?;
        let mut login = Self::new(source, home.clone(), std::env::temp_dir(), left);
        login.renewal = machine.map(|machine| {
            claude_renewal(machine, home, || {
                super::machine::command(Harness::Claude.name())
            })
        });
        Some(login)
    }

    fn new(source: Source, owner_home: PathBuf, parent: PathBuf, left: Duration) -> Self {
        Self {
            source,
            owner: Owner { home: owner_home },
            parent,
            left,
            renewal: None,
            made: Mutex::new(None),
        }
    }

    /// The login of the machine in `text` as a run may start on it at `now`: renewed first, once,
    /// where it has run short on a test machine. The login and what the renewal came to; or why the
    /// run is refused, with the renewal's part in it said.
    fn usable(&self, text: String, now: i64) -> Result<(Value, Option<Renewed>)> {
        let refused = match oauth(&text, now, self.left) {
            Ok(oauth) => return Ok((oauth, None)),
            Err(refused) => refused,
        };
        let Some(renewal) = self
            .renewal
            .as_ref()
            .filter(|_| renewable(&text, now, self.left))
        else {
            return Err(refused);
        };
        let kept = Kept {
            file: self.source.described(),
            login: "Claude Code's login".to_owned(),
            read: &|| self.source.read(),
            expiry: &expires_at,
        };
        let (fresh, renewed) = renewal
            .renew(&kept, &text)
            .map_err(|why| Error::Daemon(format!("{refused}. {why}")))?;
        // What the renewal did is said as it is done, for a run that does not get so far.
        let _ = writeln!(io::stderr(), "note: {}", renewed.said(renewal.machine()));
        match oauth(&fresh, now, self.left) {
            Ok(oauth) => Ok((oauth, Some(renewed))),
            Err(still) => Err(Error::Daemon(format!(
                "{still}. {}",
                renewal.ended(&renewed, now)
            ))),
        }
    }

    /// The folder, made as the clock says `now` is (milliseconds since the epoch).
    fn folder_at(&self, now: i64) -> Result<PathBuf> {
        let mut made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(made) = made.as_ref() {
            return Ok(made.folder.path().to_path_buf());
        }
        let (oauth, renewed) = self.usable(self.source.read()?, now)?;
        sweep(&self.parent, PREFIX);
        let folder = tempfile::Builder::new()
            .prefix(&format!("{PREFIX}{}-", std::process::id()))
            .tempdir_in(&self.parent)
            .map_err(|source| Error::File {
                action: "make a folder in",
                path: self.parent.clone(),
                source,
            })?;
        private_folder(folder.path())?;
        let copy = without_renewal(&oauth);
        let credentials = json!({ "claudeAiOauth": copy }).to_string();
        write_private(&folder.path().join(".credentials.json"), &credentials)?;
        write_private(&folder.path().join(".claude.json"), CLAUDE_JSON)?;
        // The live folders trusted as the owner's own config trusts them (T-629).
        super::machine::seed_claude_trust(folder.path(), &self.owner.home)?;
        let path = folder.path().to_path_buf();
        *made = Some(Made {
            folder,
            oauth: copy,
            login: oauth,
            owner: self.owner.look(),
            services: keychain_services(),
            renewed,
        });
        Ok(path)
    }
}

/// Claude Code's renewal on `machine`, for the owner whose home is `home`, started as `program`
/// finds it: Claude Code run once on
/// the model the run uses ([`renew::claude_words`]), and the owner's setup compared before and
/// after, so that nothing but the login is changed ([`Owner::changes_since`]).
fn claude_renewal(
    machine: TestMachine,
    home: PathBuf,
    program: impl Fn() -> Result<PathBuf> + Send + Sync + 'static,
) -> Renewal {
    Renewal::new(
        machine,
        "Claude Code",
        "only when five minutes of it are left",
        move || {
            let model = super::agents::asked()?.of(Harness::Claude).name.clone();
            let program = program()?;
            let owner = Owner { home: home.clone() };
            let before = owner.look();
            let ended = renew::one_shot(&program, &renew::claude_words(&model))?;
            let changed = owner.changes_since(&before);
            if changed.is_empty() {
                return Ok(ended);
            }
            Err(Error::Daemon(format!(
                "it changed the owner's setup beside its login: {}",
                changed.join("; ")
            )))
        },
    )
}

/// Makes the folder `path` the user's alone (mode 700): a temporary folder is made with the
/// permissions the user's umask leaves, which let others in.
pub(super) fn private_folder(path: &Path) -> Result {
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
pub(super) fn write_private(path: &Path, text: &str) -> Result {
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

/// Takes away the folders of runs that were killed before they could (`prefix`, the id of
/// the process, and a random word), in `parent`: a folder whose process is gone is nobody's,
/// and holds a login. A process that is there keeps its own.
pub(super) fn sweep(parent: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let owner = name
            .strip_prefix(prefix)
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

impl Login {
    /// Whether the login this machine keeps is no longer the one that was copied (the same item
    /// holds the person's MCP servers' tokens, which their own servers refresh all day: only
    /// the login is held to it, and of the login its [`TOKENS`], as the copy is: Claude Code
    /// writes what it learns of the account beside them): the run's, or their own Claude
    /// Code's refresh in between.
    fn owner_login_changed(&self, copied: &Value) -> Option<String> {
        let now = self
            .source
            .read()
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|login| login.get("claudeAiOauth").cloned());
        let kept = now.is_some_and(|now| {
            TOKENS
                .iter()
                .all(|field| now.get(field) == copied.get(field))
        });
        (!kept).then(|| {
            format!(
                "the owner's Claude login in {} is not as it was",
                self.source.described()
            )
        })
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
        if let Some((renewed, renewal)) = made.renewed.as_ref().zip(self.renewal.as_ref()) {
            let _ = writeln!(io::stderr(), "{}", renewed.said(renewal.machine()));
        }
        let mut touched = self.owner.changes_since(&made.owner);
        touched.extend(self.owner_login_changed(&made.login));
        if let Some(note) = made.rewritten() {
            let _ = writeln!(io::stderr(), "note: {note}");
        }
        let mut refreshed: Vec<String> = made.refreshed().into_iter().collect();
        let mut kept = Vec::new();
        if let (Some(before), Some(now)) = (&made.services, keychain_services()) {
            let found = keychain_of(made.folder.path(), before, &now);
            for other in &found.others {
                let _ = writeln!(io::stderr(), "note: {other}");
            }
            // A login that went into the Keychain is a login gone from the folder: said once, there.
            if !found.own.is_empty() && made.copy() == Copy::Lost {
                refreshed.clear();
            }
            kept = found.own;
        }
        if touched.is_empty() && refreshed.is_empty() && kept.is_empty() {
            // What was checked, with a hash that says nothing of what the files hold.
            let hash = |text: &str| digest(text.as_bytes())[..12].to_owned();
            let _ = writeln!(
                io::stderr(),
                "the owner's Claude setup is as it was: login in {} {}, ~/.claude/settings.json {}; the copy of the login in the run's folder is as it was copied",
                self.source.described(),
                hash(&made.login.to_string()),
                made.owner.settings.as_deref().map_or("none".to_owned(), |hash| hash[..12].to_owned())
            );
            return Ok(());
        }
        let said = said(&touched, &refreshed, &kept);
        // The error is the case's to fail with; the banner is for whoever reads the output.
        let _ = writeln!(io::stderr(), "{said}");
        Err(Error::Daemon(said))
    }
}

/// What `finish` says when the run touched the person's setup (`touched`) or its copy of the
/// login was refreshed (`refreshed`): loud, and with what to do. Never what the login holds.
fn said(touched: &[String], refreshed: &[String], kept: &[String]) -> String {
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
            "THE RUN'S CLAUDE CODE REFRESHED ITS COPY OF THE LOGIN. The copy holds no refresh token, so it \
             cannot have used up the one this machine keeps (that file is checked above), but it was given a \
             login and wrote another:\n",
        );
        for change in refreshed {
            said.push_str(&format!("  - {change}\n"));
        }
    }
    if !kept.is_empty() {
        said.push_str(
            "THE RUN'S CLAUDE CODE LEFT ITS SECURE STORAGE IN THE OWNER'S KEYCHAIN (T-1102), which a live run is \
             never to do:\n",
        );
        for item in kept {
            said.push_str(&format!("  - {item}\n"));
        }
    }
    said.push_str(&bar);
    said
}

#[cfg(test)]
mod portable;
#[cfg(test)]
mod tests;
