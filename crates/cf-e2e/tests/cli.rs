//! The CLI's black-box suite: the built `cf` run as a process, in a home of
//! each case's own, and judged by what it prints, writes and exits with. These
//! are the cases that held the CLI's rewrite to what Node's CLI said, ported
//! one for one from the JavaScript suites `tests/cli.test.mjs` and
//! `tests/cf-commands.test.mjs` (gone with the port); the module of a case is
//! the JavaScript `describe` it came from.
//!
//! A case makes its own home (the JavaScript suites shared one among the cases
//! of a `describe`, and none of them leans on what another left), so the cases
//! run in any order and at once.
//!
//!     cargo test -p cf-e2e --test cli        (`npm run test:clis`)

// The cases are in `tests/cli/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "cli/catalog.rs"]
mod catalog;
#[path = "cli/commands.rs"]
mod commands;
#[path = "cli/host_verbs.rs"]
mod host_verbs;
#[path = "cli/role_files.rs"]
mod role_files;
#[path = "cli/roster.rs"]
mod roster;
#[path = "cli/saved_data.rs"]
mod saved_data;
#[path = "cli/switch_over.rs"]
mod switch_over;

use std::path::PathBuf;

use cf_e2e::ScratchHome;
use serde_json::Value;

/// What a case ends with: nothing, or why it could not be run. A check that
/// does not hold is a panic, which is the case's failure.
type Outcome = Result<(), Box<dyn std::error::Error>>;

/// The terminal's command `cf setup` makes: `cf`, or `cf.cmd` on Windows.
fn launcher() -> &'static str {
    if cfg!(windows) {
        "cf.cmd"
    } else {
        "cf"
    }
}

/// A launch's own role file, where a window writes it
/// (`crates/cf-harness/src/shared/role.rs`): a file `cf` leaves alone.
fn role_file(home: &ScratchHome) -> PathBuf {
    ["integrations", "claude", "launch-canary", "role"]
        .into_iter()
        .chain([".claude", "skills", "consensflow-chief", "SKILL.md"])
        .fold(home.consensflow(), |folder, part| folder.join(part))
}

/// The agent `name` in the answer of `cf agent list --json`, or null when it
/// is not listed there (so a check on it fails by what it finds, not by a panic
/// of its own).
fn agent<'a>(listed: &'a Value, name: &str) -> &'a Value {
    listed["agents"]
        .as_array()
        .and_then(|agents| agents.iter().find(|agent| agent["name"] == name))
        .unwrap_or(&Value::Null)
}
