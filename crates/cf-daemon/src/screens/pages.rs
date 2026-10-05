//! The two pages, carried as text: the agents (`PAGE`, `agents-server.js:181-660`)
//! and the harness diagnostics (`harnessPage`, `src/harness-page.js`), each
//! with its inline style and script, and what every screen carries for the app's
//! frame (`FRAMED`). Node builds them as template literals; here they are the
//! files they came out as, with `$TOKEN` (inside the quotes the script's
//! `JSON.stringify(token)` writes) and `$VERSION` where Node interpolated, which
//! are the recorded pages (`tests/goldens/daemon/pages`) to the byte: a test
//! holds each to its recording, so a page changed in Node is a failure here
//! until its file is copied over.

use cf_base::js;
use serde_json::Value;

use crate::start::VERSION;

const AGENTS: &str = include_str!("pages/agents.html");
const HARNESSES: &str = include_str!("pages/harnesses.html");

/// The agents page for the UI token `token`, which its script sends with every
/// request, and this build's version in its heading. The token goes in last, so
/// that what it holds is never read as a place for the version.
pub fn agents(token: &str) -> String {
    with_token(&AGENTS.replacen("$VERSION", VERSION, 1), token)
}

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
