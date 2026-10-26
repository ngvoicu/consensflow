//! The role an OpenCode window is given. The text is written in a file of the
//! launch's own (`shared::role`), and OpenCode is told of it through
//! `OPENCODE_CONFIG_CONTENT`, the JSON configuration it reads from its
//! environment: merged into the one the human may have set, the role's skills
//! folder added to the paths its skills are looked for in and the file to its
//! instructions, each once. The skills folder of ConsensFlow's own copy of
//! Specmint, where the chief is given one, goes after the role's.
//!
//! The paths come after the ones OpenCode's own files give the window's folder
//! (its global and project configurations), asked of OpenCode itself
//! (`opencode debug config`, [`configured_paths`]): it reads this configuration
//! last and a list in it replaces the files' own, so a member that was told only
//! of the role's folder lost the skills the human keeps elsewhere, which the same
//! OpenCode opened in a terminal there has (T-540, 2026-10-25). A path is kept as
//! OpenCode says it: it resolves a relative one against the window's folder from
//! whichever layer it came. An OpenCode that cannot be asked, or answers no
//! configuration, gives none, and the window goes on as it did before.
//!
//! The window is also told not to update itself (`"autoupdate": false`), as
//! Devin's is: OpenCode updates itself to the latest release when it starts,
//! which is how a working 1.x became the 2 that no window opens on (`version`).
//! A configuration the human set that names an `autoupdate` keeps it, whatever
//! it is: on updates the human's word is the last, and a null is none.
//!
//! Kept from Node on purpose:
//! - a configuration that is no JSON, or is nested past 127 levels, is
//!   refused in a sentence of this module's own, where V8 threw its
//!   `SyntaxError`;
//! - a `skills` that is a text with a character past the Basic Multilingual
//!   Plane, which Node spreads into one entry per UTF-16 unit, half of a
//!   pair in each, has each half written as U+FFFD, as a Rust text holds
//!   no half of one.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::json::from_slice_lossy;
use cf_base::{js, path};
use cf_proto::agents::Harness;
use serde_json::{Map, Value};

use super::child_env::{child_env, Declared};
use crate::contract::LaunchId;
use crate::seams::processes::{Limits, Processes, Program};
use crate::shared::paths::set;
use crate::shared::role::write_role;

/// How long OpenCode has to say its configuration, and how much it may say:
/// it answers in about a second (1.19.4).
const ASKED: Limits = Limits {
    timeout: Duration::from_secs(30),
    max_buffer: 4 * 1024 * 1024,
};

/// The skill paths OpenCode's own configuration gives a window opened in
/// `directory`, as the OpenCode at `executable` resolves it there (`opencode
/// debug config`), with the configuration the environment names: the one a
/// terminal's OpenCode would read. It is asked with its updates off, as the
/// window will be, so that asking never updates it. None where it cannot be
/// asked, or answers no configuration whose paths are a list of texts.
pub(super) async fn configured_paths(
    processes: &Rc<dyn Processes>,
    env: &Env,
    executable: &str,
    directory: &str,
) -> Vec<String> {
    let current = set(env, "OPENCODE_CONFIG_CONTENT");
    let content = match from_slice_lossy(current.as_deref().unwrap_or("{}").as_bytes()) {
        Ok(Value::Object(mut configuration)) => {
            not_to_update(&mut configuration);
            js::stringify(&Value::Object(configuration))
        }
        // Refused as the window's own configuration is (`merged`).
        _ => current.unwrap_or_default(),
    };
    let asked = Env::from_vars(
        env.iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([("OPENCODE_CONFIG_CONTENT".into(), content.into())]),
    );
    let program = Program {
        executable: PathBuf::from(executable),
        args: vec!["debug".to_owned(), "config".to_owned()],
        cwd: Some(PathBuf::from(directory)),
        env: child_env(&asked, &Declared::default()),
    };
    let Ok(said) = processes.run(program, ASKED).await else {
        return Vec::new();
    };
    from_slice_lossy(said.as_bytes())
        .ok()
        .and_then(|configuration| {
            paths(
                configuration
                    .get("skills")
                    .and_then(|skills| skills.get("paths")),
                "",
            )
            .ok()
        })
        .unwrap_or_default()
}

/// The `OPENCODE_CONFIG_CONTENT` a window of `role` runs with: what the
/// environment names (nothing, `{}`) with the role added, its skills
/// folder after the paths OpenCode's files give the window (`configured`,
/// [`configured_paths`]), the skills folder `specmint` of a copy of Specmint
/// that is kept for the window, and its own updates off. The role's text is
/// written first, so a configuration that is refused leaves it in its
/// launch's folder, as it did in Node.
pub(super) fn configure(
    env: &Env,
    launch: &LaunchId,
    role: &str,
    content: &str,
    specmint: Option<String>,
    configured: &[String],
) -> Result<String, String> {
    let written = write_role(Harness::Opencode, role, env, launch, content)?;
    let skills = path::join(&[&written.root, ".claude", "skills"]);
    let current = set(env, "OPENCODE_CONFIG_CONTENT");
    let mut folders: Vec<&str> = configured.iter().map(String::as_str).collect();
    folders.push(&skills);
    folders.extend(specmint.as_deref());
    let mut configuration = merged(current.as_deref().unwrap_or("{}"), &folders, &written.file)?;
    not_to_update(&mut configuration);
    Ok(js::stringify(&Value::Object(configuration)))
}

/// Tells the window not to update itself, unless the configuration names an
/// `autoupdate` already: any value but null is the human's word, and null is
/// none, as it is for the skill paths.
fn not_to_update(configuration: &mut Map<String, Value>) {
    if configuration.get("autoupdate").is_none_or(Value::is_null) {
        configuration.insert("autoupdate".to_owned(), Value::Bool(false));
    }
}

/// `current`, a configuration, with the `skills` folders among the paths its
/// skills are looked for in and `file` among its instructions.
fn merged(current: &str, skills: &[&str], file: &str) -> Result<Map<String, Value>, String> {
    let parsed = from_slice_lossy(current.as_bytes())
        .map_err(|_| "OpenCode process configuration must be JSON".to_owned())?;
    let Value::Object(mut configuration) = parsed else {
        return Err("OpenCode process configuration must be an object".to_owned());
    };
    // `skills?.paths ?? []`: a null is as much none as nothing is.
    let previous = paths(
        configuration
            .get("skills")
            .and_then(|found| found.get("paths"))
            .filter(|found| !found.is_null()),
        "OpenCode skill paths must be an array of paths",
    )?;
    let mut merged = spread(configuration.get("skills"));
    merged.insert("paths".to_owned(), once(previous, skills));
    configuration.insert("skills".to_owned(), Value::Object(merged));
    let instructions = paths(
        configuration.get("instructions"),
        "OpenCode instructions must be an array of paths",
    )?;
    configuration.insert("instructions".to_owned(), once(instructions, &[file]));
    Ok(configuration)
}

/// The paths a list of them holds, none where it is not there, or `refused`
/// where it is anything but a list of texts.
fn paths(list: Option<&Value>, refused: &str) -> Result<Vec<String>, String> {
    match list {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| refused.to_owned()),
        Some(_) => Err(refused.to_owned()),
    }
}

/// `[...new Set([...paths, ...added])]`: each path once, where it first is.
fn once(mut paths: Vec<String>, added: &[&str]) -> Value {
    paths.extend(added.iter().map(|path| (*path).to_owned()));
    let mut kept: Vec<String> = Vec::new();
    for each in paths {
        if !kept.contains(&each) {
            kept.push(each);
        }
    }
    Value::Array(kept.into_iter().map(Value::String).collect())
}

/// What `{...value}` makes of a value, as an object's entries: an object's
/// own, a list's items by their index, a text's characters by theirs, and
/// nothing of any other value.
fn spread(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(fields)) => fields.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(at, item)| (at.to_string(), item.clone()))
            .collect(),
        Some(Value::String(text)) => text
            .encode_utf16()
            .enumerate()
            .map(|(at, unit)| {
                let character = char::from_u32(u32::from(unit)).unwrap_or('\u{FFFD}');
                (at.to_string(), Value::String(character.to_string()))
            })
            .collect(),
        _ => Map::new(),
    }
}

#[cfg(test)]
mod tests;
