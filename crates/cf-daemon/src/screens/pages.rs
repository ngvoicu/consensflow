//! The page, carried as text: the harness diagnostics, with its inline style and
//! script, and what every screen carries for the app's frame (`FRAMED`). Node
//! built it as a template literal; here it is the file it came out as, with
//! `$TOKEN` (inside the quotes the script's `JSON.stringify(token)` writes) where
//! Node interpolated, which is the recorded page (`tests/goldens/pages`) to the byte. The files
//! are the source now, edited by hand; the screens' traces hold the page the
//! daemon serves to the recorded copy, so a page and its copy change together.

use cf_base::js;
use serde_json::Value;

const HARNESSES: &str = include_str!("pages/harnesses.html");

/// The harness diagnostics for the UI token `token`.
pub fn harnesses(token: &str) -> String {
    with_token(HARNESSES, token)
}

/// `template` with its token put where the script has it: as the text
/// `JSON.stringify` writes for it, quotes and escapes and all.
fn with_token(template: &str, token: &str) -> String {
    template.replacen(
        r#""$TOKEN""#,
        &js::stringify(&Value::String(token.to_owned())),
        1,
    )
}

#[cfg(test)]
mod tests;
