//! What Devin's window runs with (`src/devin-install.js`): a config of its
//! launch's own, which is the owner's with the hooks that give a session its
//! role text and answer a member's question from the board, and the first
//! message in a prompt file. Mutable native preferences are per launch;
//! loaded helper code is immutable.
//!
//! Kept from Node on purpose:
//! - the version is always asked: Node skipped the question for a call that
//!   named no executable, which no window's does;
//! - an environment that names no ConsensFlow folder has none to write in,
//!   and fails with `missing home in env` where Node read the process's own
//!   home;
//! - hooks that are a zero, a flag or empty text, which V8 failed to add a
//!   property to, are refused in a sentence of this module's own.

mod config;
#[cfg(test)]
mod tests;
mod version;

use std::fs;
use std::path::Path;

use cf_base::file::{make_folder, write_file, FileError, Mkdir};
use cf_base::home::config_root;
use cf_base::{js, path};
use serde_json::{json, Map, Value};

use crate::seams::processes::{probe, Unanswered};
use crate::seams::Services;
use crate::shared::window_args::Invocation;

/// How long Devin lets the question hook wait for the board's answer.
const QUESTION_HOOK_SECONDS: u32 = 3600;

/// What Devin's window is given of its launch's own folder.
pub(super) struct Integration {
    /// `--config` and the config written for the launch.
    pub(super) args: Vec<String>,
    /// What names Devin's wire log of this launch.
    pub(super) env: Vec<(String, String)>,
    /// Devin's wire log of this launch, which its adapter follows.
    pub(super) wire: String,
    /// The launch's own folder.
    root: String,
}

/// A launch's id as a folder's name is safe to be: `/^[A-Za-z0-9_-]{1,200}$/`.
fn valid(launch: &str) -> bool {
    (1..=200).contains(&launch.len())
        && launch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// `value` as one word of a shell's line, as `quote` quotes it.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// What a failed question of a program says: Node's own words, or the
/// program's.
fn said(unanswered: Unanswered) -> String {
    match unanswered {
        Unanswered::Unread(sentence) => sentence,
        Unanswered::Failed(failed) => failed.message,
    }
}

/// `writeFile(file, bytes, { mode, flag: 'wx' })`: the file is made first as
/// no file there was, so one already there is the failure, in Node's words;
/// then it is written as `write_file` writes one, its close checked. Writing
/// it is a second opening, which `cf_base::file` has no call to spare.
fn write_new(file: &Path, bytes: &[u8], mode: u32) -> Result<(), FileError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, mode);
    options
        .open(file)
        .map_err(|error| FileError::call(error, "open", Some(file)))?;
    write_file(file, bytes, mode)
}

/// Adds `entry`, when there is one, to the hooks Devin runs for `event`,
/// after those it has.
fn extend(hooks: &mut Map<String, Value>, event: &str, entry: Option<Value>) -> Result<(), String> {
    let mut kept = match hooks.get(event) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(kept)) => kept.clone(),
        Some(_) => return Err("Invalid native Devin hook configuration".to_owned()),
    };
    kept.extend(entry);
    hooks.insert(event.to_owned(), Value::Array(kept));
    Ok(())
}

/// Writes the config Devin's window of `launch` runs with, one per launch:
/// the owner's, which is never changed, its hooks first and ours after, and
/// its own updates off. A session the window shows starts with its role text
/// (`cf hook devin-session`), from the bundle's own `cf`, named in full as a
/// shell that reads the user's profile again can find another `cf` first. A
/// member's question tool is answered from the board (`cf hook devin`: the
/// hook holds the call while the question waits for its answer); the chief's
/// shows Devin's own dialog, where the human answers it.
///
/// The version of `executable` is asked once as it is on disk, and a Devin
/// that is too old is refused.
pub(super) async fn integration(
    services: &Services,
    launch: &str,
    executable: &str,
    board_questions: bool,
) -> Result<Integration, String> {
    if !valid(launch) {
        return Err("invalid Devin launch".to_owned());
    }
    let answered = probe(
        &services.probes,
        &services.processes,
        Path::new(executable),
        &["--version"],
        &services.env,
    )
    .await
    .map_err(said)?;
    if !version::supported(&answered.stdout) {
        return Err(version::required());
    }
    let mut configuration = config::native(&services.env)?;
    let home = config_root(&services.env).ok_or_else(|| "missing home in env".to_owned())?;
    let root = path::join(&[&home.to_string_lossy(), "integrations", "devin", launch]);
    make_folder(Path::new(&root), 0o700, Mkdir::Promise).map_err(|error| error.to_string())?;
    if configuration.get("hooks").is_none_or(Value::is_null) {
        configuration.insert("hooks".to_owned(), json!({}));
    }
    let Some(Value::Object(hooks)) = configuration.get_mut("hooks") else {
        return Err("the hooks of the native Devin configuration cannot take a hook".to_owned());
    };
    let session_start = json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": format!("{} hook devin-session", quote(&services.bundle.pane_cf)),
            "timeout": 5,
        }],
    });
    let question = json!({
        "matcher": "ask_user_question",
        "hooks": [{ "type": "command", "command": "cf hook devin", "timeout": QUESTION_HOOK_SECONDS }],
    });
    extend(hooks, "SessionStart", Some(session_start))?;
    extend(hooks, "PreToolUse", board_questions.then_some(question))?;
    configuration.insert("auto_update".to_owned(), Value::Bool(false));
    let file = path::join(&[&root, "config.json"]);
    let text = js::stringify(&Value::Object(configuration));
    write_new(Path::new(&file), text.as_bytes(), 0o600).map_err(|error| error.to_string())?;
    let wire = path::join(&[&root, "wire.jsonl"]);
    Ok(Integration {
        args: vec!["--config".to_owned(), file],
        env: vec![("CHISEL_PURE_ACP_WIRE_LOG".to_owned(), wire.clone())],
        wire,
        root,
    })
}

/// Writes the first message where Devin reads it from, in the launch's own
/// folder beside its wire log, and adds the flag that names it. A window
/// with no first message has no file.
pub(super) fn prepare_prompt(
    invocation: Invocation,
    integration: &Integration,
) -> Result<Invocation, String> {
    let Some(prompt) = invocation.prompt.as_deref() else {
        return Ok(invocation);
    };
    let file = path::join(&[&integration.root, "prompt.txt"]);
    write_new(Path::new(&file), prompt.as_bytes(), 0o600).map_err(|error| error.to_string())?;
    let mut args = invocation.args;
    args.extend(["--prompt-file".to_owned(), file]);
    Ok(Invocation {
        args,
        prompt: None,
        ..invocation
    })
}
