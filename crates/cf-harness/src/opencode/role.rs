//! The role an OpenCode window is given (`roleConfiguration`, its `opencode`
//! branch, `src/role-skills.js`). The text is written in a file of the
//! launch's own (`shared::role`), and OpenCode is told of it through
//! `OPENCODE_CONFIG_CONTENT`, the JSON configuration it reads from its
//! environment: merged into the one the human may have set, the role's
//! skills folder added to the paths its skills are looked for in and the
//! file to its instructions, each once.
//!
//! Kept from Node on purpose:
//! - a configuration that is no JSON, or is nested past 127 levels, is
//!   refused in a sentence of this module's own, where V8 threw its
//!   `SyntaxError`;
//! - a `skills` that is a text with a character past the Basic Multilingual
//!   Plane, which Node spreads into one entry per UTF-16 unit, half of a
//!   pair in each, has each half written as U+FFFD, as a Rust text holds
//!   no half of one.

use cf_base::env::Env;
use cf_base::json::from_slice_lossy;
use cf_base::{js, path};
use cf_proto::agents::Harness;
use serde_json::{Map, Value};

use crate::contract::LaunchId;
use crate::shared::paths::set;
use crate::shared::role::write_role;

/// The `OPENCODE_CONFIG_CONTENT` a window of `role` runs with: what the
/// environment names (nothing, `{}`) with the role added. The role's text is
/// written first, so a configuration that is refused leaves it in its
/// launch's folder, as it did in Node.
pub(super) fn configure(
    env: &Env,
    launch: &LaunchId,
    role: &str,
    content: &str,
) -> Result<String, String> {
    let written = write_role(Harness::Opencode, role, env, launch, content)?;
    let skills = path::join(&[&written.root, ".claude", "skills"]);
    let current = set(env, "OPENCODE_CONFIG_CONTENT");
    merged(current.as_deref().unwrap_or("{}"), &skills, &written.file)
}

/// `current`, a configuration, with `skills` among the paths its skills are
/// looked for in and `file` among its instructions.
fn merged(current: &str, skills: &str, file: &str) -> Result<String, String> {
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
    configuration.insert("instructions".to_owned(), once(instructions, file));
    Ok(js::stringify(&Value::Object(configuration)))
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

/// `[...new Set([...paths, path])]`: each path once, where it first is.
fn once(mut paths: Vec<String>, path: &str) -> Value {
    paths.push(path.to_owned());
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
