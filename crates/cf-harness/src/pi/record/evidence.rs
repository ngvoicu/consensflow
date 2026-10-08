//! What ConsensFlow's Pi extension writes beside a session, in the folder of
//! one launch: the `<launchId>.json` it writes when Pi settles, and the
//! `<launchId>.working.json` it writes when a turn starts and removes when Pi
//! settles.
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

/// The leaf entry the extension saw Pi settle at: the id its file for this
/// launch and session names, if it names one.
pub(super) fn frontier(
    session: &str,
    env: &Env,
    options: &Options,
) -> Result<Option<String>, String> {
    let Some(launch) = launch(env, options) else {
        return Ok(None);
    };
    let Some(evidence) = launch.read(".json")? else {
        return Ok(None);
    };
    if !launch.is_of(&evidence, session) {
        return Ok(None);
    }
    Ok(evidence
        .get("frontier")
        .and_then(|frontier| frontier.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned))
}

/// Whether the extension saw a turn of this session start, and has not seen
/// Pi settle.
pub(super) fn working(session: &str, env: &Env, options: &Options) -> Result<bool, String> {
    let Some(launch) = launch(env, options) else {
        return Ok(false);
    };
    Ok(launch
        .read(".working.json")?
        .is_some_and(|marker| launch.is_of(&marker, session)))
}
