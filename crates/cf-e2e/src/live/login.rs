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
//! `.credentials.json` holds the access token and **no refresh token** ([`without_refresh`]): a
//! Claude Code that cannot renew cannot use up the person's (measured on zeewin2, 2.1.316: it
//! runs on such a copy, does not look at the copy's expiry, and where the server refuses the
//! token it fails loudly with a 401, the person's login untouched). A run starts only on a login
//! whose access token lasts at least as long as the run is to take, with a margin: the length the
//! case plans for itself and the margin ([`Login::of_machine_lasting`]: a live case states its own,
//! minutes long). It says it if the copy changed all the same:
//! the folder's `.credentials.json` is compared with what was copied, and the Keychain with the
//! items it had. Nothing refreshes it by hand, here or anywhere. (On a test machine, the
//! one exception: a login that has run short is renewed by Claude Code itself, run once as
//! the owner, before the copy is made; see [`super::renew`], T-1067.)
//!
//! **What the run is held to** ([`ClaudeConfig::finish`], when its rig ends): the
//! person's login is as it was: a file's identity, size and modification time are compared,
//! or the Keychain item's `claudeAiOauth` fields (never its other tokens); `~/.claude/settings.json`
//! is as it was; `~/.claude.json` has no project
//! of the run's folders that it did not have; `~/.claude/history.jsonl` has no
//! prompt typed in one; `~/.claude/projects` has no conversation of one. (Not the
//! whole of `~/.claude.json`: the person's own Claude Code sessions write the rest
//! of it all day.) A run that fails any of them fails, with the Claude Code of the
//! person's windows named as the other possible writer. The run's folders are the
//! live suites' (`~/.consensflow-candidate/live/…`) and the rigs' homes. Claude parity
//! deliberately reads none of that other configuration: it checks the login and its copy,
//! but cannot report changes to the other owner files; it also never renews the login.
//!
//! **What a kept home keeps.** The folder goes with the run, because it holds a login.
//! A rig whose home is kept (`--keep`, or a case that reads the home after it ended)
//! asks the folder to copy what Claude Code kept there into the home first
//! ([`ClaudeConfig::keep_records`]): its conversations (`projects/`) and its sessions
//! (`sessions/`, but for the `.key` files of its peers' sockets), never the login.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
#[cfg(unix)]
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::keychain;
use super::machine::Harness;
use super::renew::{self, Kept, Renewal, Renewed, TestMachine};
use crate::door::source::{Account, Purpose, Source as Owned};
use crate::process::own_var;
use crate::rig::ClaudeConfig;
use crate::{Error, Result};

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
/// nobody's to take, and is counted ([`crate::rig::sweep::say_copies`]).
pub(crate) const PREFIX: &str = "consensflow-live-claude-";

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
            Self::File(file) => {
                let file = Account::this().named(file, Purpose::Seed);
                file.require_home("Claude login lookup");
                file.read_to_string().map_err(|failed| {
                    Error::Daemon(format!(
                    "Claude Code's login is not in {file} ({failed}): log in by running `claude` once"
                ))
                })
            }
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
                let ran = super::keychain::login_item(&user, Duration::from_secs(120))?;
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
pub(crate) fn keychain_args(user: &str) -> Vec<String> {
    ["find-generic-password", "-s", SERVICE, "-a", user, "-w"]
        .map(str::to_owned)
        .into()
}

/// The fields of `claudeAiOauth` the machine's login must hold to be copied.
const MACHINES: [&str; 2] = ["accessToken", "refreshToken"];

/// The part of `text`, the login as the machine keeps it, that a run's folder is given:
/// `claudeAiOauth` and nothing else of it (the OAuth tokens of the person's MCP servers
/// are in the same text). An error says why the machine's login cannot be used, or that
/// the access token lasts less than `left` from `now` (milliseconds since the epoch);
/// never what the text held.
///
/// It asks the login for the fields `required` of its `claudeAiOauth`: both tokens
/// ([`MACHINES`]) where the machine keeps it, and the access token alone in a folder the run's caller
/// copied the login into, which may hold it without its refresh token (the run's own copy holds
/// none).
fn oauth_with(text: &str, now: i64, left: Duration, required: &[&str]) -> Result<Value> {
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
    for field in required {
        if !oauth[*field]
            .as_str()
            .is_some_and(|token| !token.is_empty())
        {
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
             when the owner opens it, and an eligible test run may ask it to), or make the run \
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
    has_refresh
        && expires_at(text).is_some_and(|expires| {
            remaining(expires) < left && remaining(expires) <= renew::RENEWS_WITHIN
        })
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
    if !cfg!(target_os = "macos") && !cfg!(test) {
        return None;
    }
    let ran = super::keychain::item_names(Duration::from_secs(60))
        .ok()
        .filter(|ran| ran.code == Some(0))?;
    Some(services(&ran.stdout))
}

/// What the Keychain came to for a run whose Claude Code worked in `folder`, from the names it had
/// `before` the run and `now`.
#[derive(Debug, Default, PartialEq, Eq)]
struct InKeychain {
    /// The item Claude Code made for the run's folder (in words): the run's own, and a failure.
    own: Vec<String>,
    /// The other items gained since, which are some other process's as far as this run can tell (the
    /// Keychain is the whole machine's): in words, and no failure of this run. The guard of the
    /// launchers is what fails a rig for them ([`super::launcher::Guard`]).
    others: Vec<String>,
}

/// What the Keychain came to for the run that worked in `folder` ([`InKeychain`]).
///
/// Claude Code keeps what it stores securely in the Keychain, as an item named for the folder's path
/// ([`keychain::item_of`]), once it writes it: the OAuth state of an MCP server it connected to
/// is enough, and the login the folder held goes into the item with it and out of the folder
/// (T-1102). Before, every item gained was said to be a refresh of the login, and one made by
/// another run on the machine failed this one (T-1044).
fn keychain_of(folder: &Path, before: &BTreeSet<String>, now: &BTreeSet<String>) -> InKeychain {
    let own = keychain::item_of(folder);
    let mut kept = InKeychain::default();
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
/// (`~/.consensflow-candidate/live/…`) or in the home of a rig.
fn is_run_folder(path: &str) -> bool {
    // Windows writes a folder with its own separator in some places and with ours in others.
    let path = path.replace('\\', "/");
    path.contains("/.consensflow-candidate/live/") || path.contains("consensflow-integration-")
}

/// Whether `name` is the name Claude Code gives (in `~/.claude/projects`) to a folder of a run.
fn is_run_name(name: &str) -> bool {
    name.contains("-consensflow-candidate-live-") || name.contains("consensflow-integration-")
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
    /// The home as the owner's files are read.
    fn owned(&self) -> Owned {
        Account::this().named(&self.home, Purpose::Guard)
    }

    fn settings(&self) -> Owned {
        self.owned().join(".claude/settings.json")
    }

    fn history(&self) -> Owned {
        self.owned().join(".claude/history.jsonl")
    }

    /// What it is now.
    fn look(&self) -> Seen {
        let settings = self.settings().read().ok().map(|bytes| digest(&bytes));
        let history = self.history().meta().map_or(0, |meta| meta.len());
        let projects = self
            .owned()
            .join(".claude.json")
            .read_to_string()
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|json| json["projects"].as_object().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|(path, _)| is_run_folder(path))
            .map(|(path, entry)| (path, digest(entry.to_string().as_bytes())))
            .collect();
        let conversations = self
            .owned()
            .join(".claude/projects")
            .names()
            .into_iter()
            .flatten()
            .filter_map(|name| name.into_string().ok())
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
            let mut file = self.history().open()?;
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
    for name in before.conversations.difference(&after.conversations) {
        found.push(format!("~/.claude/projects lost {name}"));
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

/// A file-backed owner login is compared only by its filesystem metadata.
/// A Keychain item has no corresponding file metadata and keeps its existing check.
enum OwnerLogin {
    File(std::result::Result<cf_base::file::Stat, std::io::ErrorKind>),
    Keychain(Value),
}

/// The folder a run's Claude windows run on, once it is made.
struct Made {
    /// The login as it was copied: the machine's, without what renews it ([`without_refresh`]).
    oauth: Value,
    /// The owner login's file metadata, or its Keychain fields when it is not a file.
    login: OwnerLogin,
    /// The person's setup when it was made.
    owner: Option<Seen>,
    /// The Keychain items of Claude Code then; none where they cannot be listed.
    services: Option<BTreeSet<String>>,
    /// What the renewal of the login came to, where the login had run short on a test machine.
    renewed: Option<Renewed>,
    /// The folder, removed when this is dropped: with the run, however it ends.
    folder: crate::door::Made,
}

/// What lets a login renew itself, and is never given to a run's copy: the refresh token and when it
/// ends. A copy that holds only the access token cannot renew it, so it cannot use up the refresh
/// token the machine keeps (T-1067, measured on zeewin2: Claude Code 2.1.316 runs on such a copy,
/// does not look at its expiry, and fails with a 401 where the server refuses the token).
/// The part of `oauth`, the machine's login, that a run's folder is given: all but what renews it.
fn without_renewal(oauth: &Value) -> Value {
    let mut copy = oauth.clone();
    without_refresh(&mut copy);
    copy
}

/// Remove refresh-named keys at every depth of a JSON login, including inside arrays.
pub fn without_refresh(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.retain(|name, _| !name.to_ascii_lowercase().contains("refresh"));
            fields.values_mut().for_each(without_refresh);
        }
        Value::Array(items) => items.iter_mut().for_each(without_refresh),
        _ => {}
    }
}

/// Copy a JSON login only when it is an object whose refresh-named keys can be removed.
/// Errors never include its contents.
pub fn json_copy(text: &str, harness: &str) -> Result<String> {
    let refused = || {
        Error::Daemon(format!(
            "{harness}'s login cannot be shown free of refresh keys, so it is not copied"
        ))
    };
    let mut value: Value = serde_json::from_str(text).map_err(|_| refused())?;
    if !value.is_object() {
        return Err(refused());
    }
    without_refresh(&mut value);
    let mut copy = serde_json::to_string_pretty(&value).map_err(|_| refused())?;
    copy.push('\n');
    Ok(copy)
}

/// Codex's token-backed login needs both fields even though a run must not get the refresh token.
/// An empty string is a non-token that still deserializes as Codex's required `String`. The copy's
/// timestamp delays an age-based refresh; a near-expiry access token can still trigger one.
pub fn codex_copy(text: &str, now: i64) -> Result<String> {
    let refused = || Error::Daemon("Codex's login cannot be safely copied".to_owned());
    let clean = json_copy(text, "Codex")?;
    let mut value: Value = serde_json::from_str(&clean).map_err(|_| refused())?;
    match value.get_mut("tokens") {
        Some(Value::Object(tokens)) => {
            tokens.insert("refresh_token".to_owned(), Value::String(String::new()));
            let time = jiff::Timestamp::from_millisecond(now).map_err(|_| refused())?;
            value["last_refresh"] = Value::String(time.to_string());
        }
        None | Some(Value::Null) => {} // API-key and other non-token-backed logins.
        Some(_) => return Err(refused()),
    }
    let mut copy = serde_json::to_string_pretty(&value).map_err(|_| refused())?;
    copy.push('\n');
    Ok(copy)
}

/// A TOML login is copied whole only if it can be parsed and has no refresh-named key.
/// Refusing an unknown format is safer than copying a token that cannot be removed.
pub fn toml_copy(text: &str, harness: &str) -> Result<String> {
    fn has_refresh(value: &toml::Value) -> bool {
        match value {
            toml::Value::Table(fields) => fields.iter().any(|(name, value)| {
                name.to_ascii_lowercase().contains("refresh") || has_refresh(value)
            }),
            toml::Value::Array(items) => items.iter().any(has_refresh),
            _ => false,
        }
    }
    let refused = || {
        Error::Daemon(format!(
            "{harness}'s login cannot be shown free of refresh keys, so it is not copied"
        ))
    };
    let value: toml::Value = toml::from_str(text).map_err(|_| refused())?;
    if has_refresh(&value) {
        return Err(refused());
    }
    Ok(text.to_owned())
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
        let now = crate::door::reads::read_to_string(self.folder.path().join(".credentials.json"))
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
    /// Whether this case reads the owner's non-login Claude configuration to watch it.
    watch_owner: bool,
    /// Where the folder is made.
    parent: PathBuf,
    /// How long the access token must last more for the folder to be made.
    left: Duration,
    /// Claude Code run once as the owner to renew a login that has run short: only on a test
    /// machine ([`TestMachine::detect`]); none elsewhere, where such a login is refused.
    renewal: Option<Renewal>,
    /// The fields of the login's `claudeAiOauth` the source must hold: both tokens where it is the
    /// machine's, the access token alone where it is a copy the run's caller made
    /// ([`Login::of_folder`]).
    required: &'static [&'static str],
    made: Mutex<Option<Made>>,
}

impl Login {
    /// Use the same login copy without consulting the owner's other Claude configuration.
    /// It cannot check those files for changes, and it cannot renew on a test machine:
    /// the renewal itself reads and runs on that configuration.
    #[must_use]
    pub fn without_owner_config_reads(mut self) -> Self {
        self.watch_owner = false;
        // Test-machine renewal opens the owner's Claude and reads its configuration.
        // A login that does not last the case must instead be refused.
        self.renewal = None;
        self
    }

    fn owner_before(&self) -> Option<Seen> {
        self.watch_owner.then(|| self.owner.look())
    }

    fn owner_after(&self, before: &Option<Seen>) -> Vec<String> {
        before
            .as_ref()
            .map_or_else(Vec::new, |before| self.owner.changes_since(before))
    }

    /// The login of this machine's Claude Code, for a run that takes `left` at the most, margin
    /// included: the folder is made only if the access token lasts that much more (a live case
    /// states its planned length and the margin, [`super::Log::lasting`]). Claude Code refreshes a
    /// token when five minutes of it are left, so a run shorter than the token has left never does. None where the run has no home to look for it in.
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
            watch_owner: true,
            parent,
            left,
            renewal: None,
            required: &MACHINES,
            made: Mutex::new(None),
        }
    }

    /// The login in the Claude config folder `folder` that a run's caller made and copied a login
    /// into (`--claude-config <folder>`), for a run that takes `left` at the most. The folder is
    /// **read and never written**: its `.credentials.json` is the source, and the run's windows run
    /// on a folder of the run's own, made like the machine's copy is (no refresh token, the live
    /// folders trusted as a person's own config trusts them), which is removed with the run. The
    /// folder's own `.claude.json`, settings and records are not carried over: a config the run made
    /// is what its cases are written for. Held to what the machine's copy is held to: the source is
    /// compared with what was copied when the run is over. An error where the run has no home for
    /// the owner's setup, or the folder holds no `.credentials.json` (on a Mac a Claude Code that
    /// ran on a folder keeps its login in the Keychain, not in it).
    pub fn of_folder(folder: &Path, left: Duration) -> Result<Self> {
        let home = machine_home().ok_or_else(|| {
            Error::Daemon(
                "--claude-config: this run has no home to say the live folders below, which the \
                 run's copy of the login trusts"
                    .to_owned(),
            )
        })?;
        Self::of_folder_in(folder, home, std::env::temp_dir(), left)
    }

    /// [`Login::of_folder`] for the owner whose home is `owner_home`, the run's folder made in
    /// `parent`.
    fn of_folder_in(
        folder: &Path,
        owner_home: PathBuf,
        parent: PathBuf,
        left: Duration,
    ) -> Result<Self> {
        let file = folder.join(".credentials.json");
        if !Account::this().named(&file, Purpose::Seed).is_a_file() {
            return Err(Error::Daemon(format!(
                "--claude-config {}: the folder holds no .credentials.json to copy the login from \
                 (a Claude Code that ran on a folder on a Mac keeps its login in the Keychain, not \
                 in it): give a folder the login was copied into, or let the run use its own copy \
                 of the machine's login (no flag)",
                folder.display()
            )));
        }
        let mut login = Self::new(Source::File(file), owner_home, parent, left);
        login.required = &["accessToken"];
        Ok(login)
    }

    /// The login of the machine in `text` as a run may start on it at `now`: renewed first, once,
    /// where it has run short on a test machine. The login and what the renewal came to; or why the
    /// run is refused, with the renewal's part in it said.
    fn usable(&self, text: String, now: i64) -> Result<(Value, Option<Renewed>)> {
        let refused = match oauth_with(&text, now, self.left, self.required) {
            Ok(oauth) => return Ok((oauth, None)),
            Err(refused) => refused,
        };
        let Some(renewal) = self
            .renewal
            .as_ref()
            .filter(|_| renewable(&text, now, self.left))
        else {
            if !self.watch_owner {
                return Err(Error::Daemon(format!(
                    "{refused}. Claude parity does not renew the owner's login; renew it outside this case"
                )));
            }
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
        match oauth_with(&fresh, now, self.left, self.required) {
            Ok(oauth) => Ok((oauth, Some(renewed))),
            Err(still) => Err(Error::Daemon(format!(
                "{still}. {}",
                renewal.ended(&renewed, now)
            ))),
        }
    }

    fn seed_trust(&self, config: &Path) -> Result {
        if self.watch_owner {
            super::machine::seed_claude_trust(config, self.owner.owned())
        } else {
            super::machine::seed_claude_trust_login_only(config, self.owner.owned())
        }
    }

    /// The folder, made as the clock says `now` is (milliseconds since the epoch).
    fn folder_at(&self, now: i64) -> Result<PathBuf> {
        let mut made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(made) = made.as_ref() {
            return Ok(made.folder.path().to_path_buf());
        }
        let (oauth, renewed) = self.usable(self.source.read()?, now)?;
        let owner_login = match &self.source {
            Source::File(file) => OwnerLogin::File(
                Account::this()
                    .named(file, Purpose::Guard)
                    .stat()
                    .map_err(|e| e.kind()),
            ),
            Source::Keychain => OwnerLogin::Keychain(oauth.clone()),
        };
        crate::rig::sweep::say_copies(&self.parent);
        let folder =
            crate::door::made_in(&self.parent, &format!("{PREFIX}{}-", std::process::id()))
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
        self.seed_trust(folder.path())?;
        let path = folder.path().to_path_buf();
        *made = Some(Made {
            folder,
            oauth: copy,
            login: owner_login,
            owner: self.owner_before(),
            services: if keychain::watching() {
                keychain_services()
            } else {
                None
            },
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
    let on = machine.clone();
    Renewal::new(
        machine,
        "Claude Code",
        "only when five minutes of it are left",
        move || {
            let model = super::agents::asked()?.of(Harness::Claude).name.clone();
            let program = program()?;
            let owner = Owner { home: home.clone() };
            let before = owner.look();
            let home = Account::this().named(&home, Purpose::Guard);
            let projects = home.join(".claude/projects");
            let ended = renew::one_shot_in(
                &on,
                Harness::Claude,
                &program,
                &renew::claude_words(&model),
                &renew::account_vars(&home),
                &[&projects],
            )?;
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
        crate::door::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Makes `path` hold `text`, readable by the user alone (mode 600).
pub(super) fn write_private(path: &Path, text: &str) -> Result {
    write_private_bytes(path, text.as_bytes())
}

/// Makes `path` hold `bytes`, readable by the user alone (mode 600): a new file, below a folder this
/// run made.
pub(super) fn write_private_bytes(path: &Path, bytes: &[u8]) -> Result {
    crate::door::create_new_private(path)?
        .write_all(bytes)
        .map_err(Error::file("write", path))
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
    if !crate::door::reads::is_dir(from) {
        return Ok(());
    }
    let not_copied = |source: &Path| {
        source.extension().is_some_and(|ext| ext == "key") || crate::door::reads::is_symlink(source)
    };
    crate::door::copy_tree(from, to, &not_copied)
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
    /// Whether the owner's login file metadata changed, or its Keychain login fields changed.
    /// The latter item also holds the owner's MCP tokens, which are not compared there.
    fn owner_login_changed(&self, copied: &OwnerLogin) -> Option<String> {
        let kept = match (&self.source, copied) {
            (Source::File(file), OwnerLogin::File(Ok(was))) => Account::this()
                .named(file, Purpose::Guard)
                .stat()
                .is_ok_and(|now| now == *was),
            (Source::Keychain, OwnerLogin::Keychain(copied)) => self
                .source
                .read()
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|login| login.get("claudeAiOauth").cloned())
                .is_some_and(|now| {
                    TOKENS
                        .iter()
                        .all(|field| now.get(field) == copied.get(field))
                }),
            _ => false,
        };
        (!kept).then(|| {
            let mut said = format!(
                "the owner's Claude login in {} is not as it was",
                self.source.described()
            );
            if matches!(&self.source, Source::File(_)) {
                said.push_str(
                    " (only the file's size, modification time and identity were compared; the owner's \
                     own Claude Code changes them too, for example when it refreshes its login or an MCP login)",
                );
            }
            said
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
        let mut touched = self.owner_after(&made.owner);
        touched.extend(self.owner_login_changed(&made.login));
        if let Some(note) = made.rewritten() {
            let _ = writeln!(io::stderr(), "note: {note}");
        }
        let mut refreshed: Vec<String> = made.refreshed().into_iter().collect();
        let mut kept = Vec::new();
        if let Some((before, now)) = made
            .services
            .as_ref()
            .and_then(|before| keychain_services().map(|now| (before, now)))
        {
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
            // What was checked, without saying what the login holds.
            let login_check = match &made.login {
                OwnerLogin::File(_) => "metadata".to_owned(),
                OwnerLogin::Keychain(value) => {
                    digest(value.to_string().as_bytes())[..12].to_owned()
                }
            };
            if let Some(owner) = &made.owner {
                let _ = writeln!(
                    io::stderr(),
                    "the owner's Claude setup is as it was: login in {} {}, ~/.claude/settings.json {}; the copy of the login in the run's folder is as it was copied",
                    self.source.described(),
                    login_check,
                    owner.settings.as_deref().map_or("none".to_owned(), |hash| hash[..12].to_owned())
                );
            } else {
                let _ = writeln!(
                    io::stderr(),
                    "the owner's Claude login in {} {} and the run's login copy are as they were; the owner's other Claude configuration was not read or checked",
                    self.source.described(),
                    login_check
                );
            }
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
