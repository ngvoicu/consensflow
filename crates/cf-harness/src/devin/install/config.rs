//! The owner's own config of Devin, read as Devin reads it: where it is, what
//! it may hold besides JSON, and what shape it must have. Only a copy of it is
//! ever written, so the original is never touched.
//!
//! Kept from Node on purpose:
//! - an environment that names no home has no config folder, and fails with
//!   `missing home in env` where Node read the process's own home;
//! - JSON nested deeper than serde_json reads, or holding a number past a
//!   double's range, is refused as no config, where Node read it
//!   (`cf_base::json::from_slice_lossy`); and a lone surrogate's escape is
//!   written back as U+FFFD, where Node wrote it back as it was.

use std::path::Path;
use std::sync::LazyLock;

use cf_base::env::Env;
use cf_base::file::read_file;
use cf_base::json::from_slice_lossy;
use cf_base::{js, path};
use regex::{Captures, Regex};
use serde_json::{Map, Value};

use crate::devin::paths;
use crate::shared::pattern::compile;

/// What is said of a config that cannot be used: Devin's own is never
/// changed.
const UNREADABLE: &str = "Cannot read native Devin configuration; the original was preserved";

/// A string as a token of its own, whatever it holds that looks like a
/// comment: `"(?:\\[\s\S]|[^"\\])*"`, where `[\s\S]` is any character, which
/// `\s` and `\S` together are not here (JavaScript's `\s` is its own set).
const STRING: &str = r#""(?:\\(?s:.)|[^"\\])*""#;

/// `/("(?:\\[\s\S]|[^"\\])*")|\/\/[^\r\n]*|\/\*[\s\S]*?\*\//g`: a string, kept
/// whole; or a comment, of a line or of a block.
static COMMENTS: LazyLock<Regex> =
    LazyLock::new(|| compile(&format!(r"({STRING})|//[^\r\n]*|/\*(?s:.)*?\*/")));

/// `/("(?:\\[\s\S]|[^"\\])*")|,\s*(?=[}\]])/g`: a string, kept whole; or a
/// comma and the white space after it, before a closer. The `regex` crate has
/// no look-ahead, so the closer is matched and given back.
static COMMAS: LazyLock<Regex> = LazyLock::new(|| compile(&format!(r"({STRING})|,\s*([}}\]])")));

/// The text of a JSONC config as JSON: its comments become a space, and a
/// comma before a closer goes. A quoted string is consumed as a whole token,
/// so URLs, escapes and comment-like text in it survive.
fn strip(source: &str) -> String {
    let uncommented = COMMENTS.replace_all(source, |found: &Captures| {
        found
            .get(1)
            .map_or(" ", |string| string.as_str())
            .to_owned()
    });
    COMMAS
        .replace_all(&uncommented, |found: &Captures| {
            found
                .get(1)
                .or_else(|| found.get(2))
                .map_or("", |kept| kept.as_str())
                .to_owned()
        })
        .into_owned()
}

/// The owner's config: an empty one when Devin has none, else what it says,
/// an object whose hooks, when it has any, are an object too. A config that
/// cannot be read for any other reason than being missing says why, as
/// Node's `fs` does.
pub(super) fn native(env: &Env) -> Result<Map<String, Value>, String> {
    let file = path::join(&[&paths::config(env)?, "config.json"]);
    let source = match read_file(Path::new(&file)) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) if error.code() == "ENOENT" => return Ok(Map::new()),
        Err(error) => return Err(error.to_string()),
    };
    let Ok(Value::Object(configuration)) = from_slice_lossy(strip(&source).as_bytes()) else {
        return Err(UNREADABLE.to_owned());
    };
    let hooks = configuration.get("hooks");
    if js::truthy(hooks) && !matches!(hooks, Some(Value::Object(_))) {
        return Err(UNREADABLE.to_owned());
    }
    Ok(configuration)
}

#[cfg(test)]
mod tests;
