//! Whether Claude Code's computer use is on for a project's folder, read from
//! Claude Code's own configuration and never written (T-540, 2026-10-25).
//!
//! Claude Code 2.1.310 starts its built-in `computer-use` server only for a
//! project whose entry in its configuration (`~/.claude.json`, or `.claude.json`
//! in `CLAUDE_CONFIG_DIR`) lists it among `enabledMcpServers`, which `/mcp`
//! writes when the human turns it on; nothing on its command line or in a
//! settings file turns it on, and the same server under another name loses what
//! Claude Code gives the built-in, an organisation's HIPAA block included. So
//! ConsensFlow only says whether it is on, and where the human turns it on.
//!
//! A project's entry is kept under the folder Claude Code takes for the
//! project, which is the one a yes to its folder-trust question trusts
//! ([`trusts`]): the root of the git repository the folder is in, the checkout
//! a linked worktree belongs to, a nested repository's own root, and the folder
//! itself out of any repository.

use std::path::Path;

use cf_base::env::Env;
use cf_base::path;
use serde_json::Value;

use crate::shared::record::home;
use crate::shared::trust::trusts;

/// The name of Claude Code's built-in computer-use server.
const SERVER: &str = "computer-use";

/// What Claude Code's configuration says of computer use for a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputerUse {
    /// The folder Claude Code keeps the project's entry under, where the human
    /// turns it on.
    pub folder: String,
    /// Whether that entry lists the built-in server among the enabled ones.
    pub on: bool,
}

/// Whether computer use is on for a Claude window opened in `directory`, as the
/// configuration Claude Code reads in `env` says: off where it says nothing, or
/// where it cannot be read.
pub fn computer_use(env: &Env, directory: &str) -> ComputerUse {
    let folder = trusts(directory).folder;
    let on = configuration(env)
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|configuration| enabled(&configuration, &folder));
    ComputerUse { folder, on }
}

/// Claude Code's configuration file in `env`: `.claude.json` in
/// `CLAUDE_CONFIG_DIR`, else in the home, as its text; none where it is not there.
fn configuration(env: &Env) -> Option<String> {
    let folder = match env.os("CLAUDE_CONFIG_DIR") {
        Some(root) => root.to_string_lossy().into_owned(),
        None => home(env).ok()?,
    };
    std::fs::read_to_string(Path::new(&path::join(&[&folder, ".claude.json"]))).ok()
}

/// Whether the project entry of `folder` in `configuration` enables the server.
fn enabled(configuration: &Value, folder: &str) -> bool {
    configuration["projects"][folder]["enabledMcpServers"]
        .as_array()
        .is_some_and(|servers| servers.iter().any(|server| server == SERVER))
}

#[cfg(test)]
mod tests;
