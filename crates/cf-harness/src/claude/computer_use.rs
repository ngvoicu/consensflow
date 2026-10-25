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

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;

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

/// Claude Code's configuration as last read: the folders whose entry enables the
/// server, kept while the file's size and modification time stay as they were. The
/// file carries Claude Code's whole history and grows to many MB, and each Claude
/// launch with a grant missing and each look at Project settings asks of it.
#[derive(Debug, Default)]
pub struct Configuration {
    read: RefCell<Option<Read>>,
}

/// One reading of the file.
#[derive(Debug)]
struct Read {
    file: PathBuf,
    stamp: Stamp,
    on_in: HashSet<String>,
}

/// What says the file changed: its size and modification time.
type Stamp = (u64, Option<SystemTime>);

impl Configuration {
    /// Whether computer use is on for a Claude window opened in `directory`, as the
    /// configuration Claude Code reads in `env` says: off where it says nothing, or
    /// where it cannot be read.
    pub fn computer_use(&self, env: &Env, directory: &str) -> ComputerUse {
        let folder = trusts(directory).folder;
        let on = self.on_in(env).is_some_and(|on_in| on_in.contains(&folder));
        ComputerUse { folder, on }
    }

    /// The folders the file in `env` enables the server for, read again only when
    /// it is another file or it changed; none where it is not there.
    fn on_in(&self, env: &Env) -> Option<std::cell::Ref<'_, HashSet<String>>> {
        let file = configuration_file(env)?;
        let stamp = std::fs::metadata(&file)
            .ok()
            .map(|meta| (meta.len(), meta.modified().ok()))?;
        let kept = self
            .read
            .borrow()
            .as_ref()
            .is_some_and(|read| read.file == file && read.stamp == stamp);
        if !kept {
            let on_in = std::fs::read_to_string(&file)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .map(|configuration| enabled(&configuration))
                .unwrap_or_default();
            *self.read.borrow_mut() = Some(Read { file, stamp, on_in });
        }
        std::cell::Ref::filter_map(self.read.borrow(), |read| {
            read.as_ref().map(|read| &read.on_in)
        })
        .ok()
    }
}

/// Claude Code's configuration file in `env`: `.claude.json` in
/// `CLAUDE_CONFIG_DIR`, else in the home.
fn configuration_file(env: &Env) -> Option<PathBuf> {
    let folder = match env.os("CLAUDE_CONFIG_DIR") {
        Some(root) => root.to_string_lossy().into_owned(),
        None => home(env).ok()?,
    };
    Some(PathBuf::from(path::join(&[&folder, ".claude.json"])))
}

/// The folders whose project entry in `configuration` enables the server.
fn enabled(configuration: &Value) -> HashSet<String> {
    configuration["projects"]
        .as_object()
        .map(|projects| {
            projects
                .iter()
                .filter(|(_, entry)| {
                    entry["enabledMcpServers"]
                        .as_array()
                        .is_some_and(|servers| servers.iter().any(|server| server == SERVER))
                })
                .map(|(folder, _)| folder.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
