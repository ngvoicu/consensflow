//! What ConsensFlow's Pi extension writes beside a session, in the folder of
//! one launch: its word of Pi's run state, `<launchId>.state.json`.
//!
//! A file that is not there, or is no JSON, is no evidence. A file that
//! cannot be read for any other reason fails the look, as it threw in Node.
//! JSON nested deeper than serde_json reads ([`DEEPEST`] levels) is no JSON
//! here, where Node read it: a difference kept, since the extension writes
//! none.
//!
//! [`DEEPEST`]: cf_base::json::DEEPEST

use std::fs;

use cf_base::env::Env;
use cf_base::file::is_missing;
use cf_base::json::from_slice_lossy;
use cf_base::path;
use serde_json::Value;

use crate::shared::record::cache::Options;

/// Where one launch's evidence is: its folder, and the launch's id, which
/// names the files in it.
struct Launch {
    directory: String,
    id: String,
}

/// The launch a look reads the evidence of: what the look's options say, and
/// the environment for what they leave none (`config.directory ??
/// env.CF_DELIVERY_SETTLED`, and the launch's id the same). None when either
/// is missing, or the id is no one path segment, before any file is read.
fn launch(env: &Env, options: &Options) -> Option<Launch> {
    let config = options.pi_settlement.as_ref();
    let from_env = |name: &str| Some(env.os(name)?.to_string_lossy().into_owned());
    let directory = match config.and_then(|config| config.directory.clone()) {
        Some(directory) => directory,
        None => from_env("CF_DELIVERY_SETTLED")?,
    };
    let id = match config.and_then(|config| config.launch_id.clone()) {
        Some(id) => id,
        None => from_env("CF_DELIVERY_LAUNCH_ID")?,
    };
    is_one_segment(&id).then_some(Launch { directory, id })
}

/// `/^[A-Za-z0-9._-]+$/`: a name that is one path segment, and no more.
fn is_one_segment(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

impl Launch {
    /// The JSON of the file `<id><suffix>`: none when it is not there or is no
    /// JSON.
    fn read(&self, suffix: &str) -> Result<Option<Value>, String> {
        let file = path::join(&[&self.directory, &format!("{}{suffix}", self.id)]);
        match fs::read(&file) {
            Ok(bytes) => Ok(from_slice_lossy(&bytes).ok()),
            Err(error) if is_missing(&error) => Ok(None),
            Err(error) => Err(format!("{file}: {error}")),
        }
    }

    /// Whether `evidence` says it is this launch's and this session's: each
    /// the same text, and no other value.
    fn is_of(&self, evidence: &Value, session: &str) -> bool {
        evidence.get("launchId").and_then(Value::as_str) == Some(&self.id)
            && evidence.get("sessionId").and_then(Value::as_str) == Some(session)
    }
}

/// Pi's run state, as ConsensFlow's extension follows it and writes it in one word
/// (`<launchId>.state.json`, the brain's `architecture/pi-turn-state.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// A run is on.
    Working,
    /// No run, and Pi is not free: it compacts, sums up a branch, or holds a prompt.
    Busy,
    /// Pi is idle, with nothing in hand.
    Rest,
}

/// The last run that ended in the session: the session's leaf at its end, and Pi's own flag of an
/// abort (none where Pi gave none, or the run was dropped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct End {
    pub(crate) leaf: Option<String>,
    pub(crate) aborted: Option<bool>,
}

/// The extension's word for this launch and session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Word {
    pub(crate) state: State,
    /// The session's leaf when it was written: the one Pi has selected.
    pub(crate) leaf: Option<String>,
    /// How many runs of the launch began, and how many ended.
    pub(crate) begun: u64,
    pub(crate) ended: u64,
    /// The last run that ended in this session, if one did.
    pub(crate) last: Option<End>,
    /// The process that wrote it: Pi's own.
    pub(crate) pid: Option<u32>,
}

/// The extension's word for `session`, where it wrote one for this launch: none where there is
/// none, it is another launch's or session's, or it is no word (a state it does not say, a count
/// that is no whole number, the extension's mark that the launch has none). A file that cannot be
/// read for any other reason fails the look.
pub(crate) fn word(session: &str, env: &Env, options: &Options) -> Result<Option<Word>, String> {
    let Some(launch) = launch(env, options) else {
        return Ok(None);
    };
    let Some(word) = launch.read(".state.json")? else {
        return Ok(None);
    };
    if !launch.is_of(&word, session) {
        return Ok(None);
    }
    let state = match word.get("state").and_then(Value::as_str) {
        Some("working") => State::Working,
        Some("busy") => State::Busy,
        Some("rest") => State::Rest,
        _ => return Ok(None),
    };
    let leaf = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
    };
    let Some(ended) = word.get("ended").and_then(Value::as_u64) else {
        return Ok(None);
    };
    let last = word
        .get("last")
        .filter(|last| last.is_object())
        .map(|last| End {
            leaf: leaf(last.get("leaf")),
            aborted: last.get("aborted").and_then(Value::as_bool),
        });
    let begun = word.get("begun").and_then(Value::as_u64).unwrap_or(ended);
    Ok(Some(Word {
        state,
        leaf: leaf(word.get("leaf")),
        begun,
        ended,
        last,
        pid: word
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok()),
    }))
}
