//! Whether Codex's computer use is set up on this machine, read from Codex's own
//! configuration and never written (T-548, 2026-10-25).
//!
//! Codex's computer use comes from the ChatGPT app: it turns on Codex's
//! `unified-computer-use@openai-bundled` plugin in `config.toml` (in
//! `CODEX_HOME`), whose `cua_repl` server runs on the app's own Node, and puts
//! `Codex Computer Use.app` in `computer-use` beside it. The service that app
//! runs is opened through LaunchServices, so the macOS grants it works through
//! are its own, not the terminal's and not ConsensFlow's. Codex started in a
//! terminal and a ConsensFlow member get the same tools from it (probed on Codex
//! 0.164.0: a read-only run listed the same running apps both ways). The
//! `[mcp_servers.computer-use]` entry the app also writes names its program by
//! a path relative to a folder only the app's own Codex runs in, and fails in
//! every other ("No such file or directory"), so it is no sign of anything.
//!
//! It is the machine's and not a project's: the plugin is set in the user's
//! configuration, for every folder. `config.toml` is read again only when it is
//! another file or its size or modification time changed: it is asked of at every
//! look at Project settings, on the daemon's thread, and holds the user's whole
//! configuration (a table for every folder Codex trusted).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cf_base::env::Env;
use cf_base::path;
use serde_json::Value;

use super::paths::codex_home;

/// The plugin that gives Codex its computer use, as `config.toml` names it.
const PLUGIN: &str = "unified-computer-use@openai-bundled";

/// Where Codex keeps that plugin's installed versions, under its home.
const PLUGIN_CACHE: [&str; 4] = ["plugins", "cache", "openai-bundled", "unified-computer-use"];

/// The server of the plugin's that does the work.
const SERVER: &str = "cua_repl";

/// The app whose service controls the screen, under Codex's home.
const SERVICE_APP: [&str; 2] = ["computer-use", "Codex Computer Use.app"];

/// What Codex's configuration says of its computer use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputerUse {
    /// Whether the plugin is on, an installed version of it names a program
    /// that is there, and the service's app is in place.
    pub set_up: bool,
}

/// Whether Codex's computer use is set up for the Codex `env` names: off where
/// anything it needs is missing or cannot be read.
pub fn computer_use(env: &Env) -> ComputerUse {
    let set_up = codex_home(env).is_ok_and(|home| {
        plugin_on(&home)
            && program_installed(&home)
            && Path::new(&path::join(&[&home, SERVICE_APP[0], SERVICE_APP[1]])).is_dir()
    });
    ComputerUse { set_up }
}

/// What says the file changed: its size and modification time.
type Stamp = (u64, Option<SystemTime>);

/// One reading of `config.toml`: whether it turned the plugin on.
struct Read {
    file: PathBuf,
    stamp: Stamp,
    on: bool,
}

thread_local! {
    /// `config.toml` as this thread last read it.
    static READ: RefCell<Option<Read>> = const { RefCell::new(None) };
}

/// Whether `config.toml` in `home` turns the plugin on, as it was last read where
/// it has not changed since; off where it is not there.
fn plugin_on(home: &str) -> bool {
    let file = PathBuf::from(path::join(&[home, "config.toml"]));
    let Some(stamp) = std::fs::metadata(&file)
        .ok()
        .map(|meta| (meta.len(), meta.modified().ok()))
    else {
        return false;
    };
    READ.with(|read| {
        let mut read = read.borrow_mut();
        if let Some(kept) = read
            .as_ref()
            .filter(|kept| kept.file == file && kept.stamp == stamp)
        {
            return kept.on;
        }
        let on = enabled_in(&file);
        *read = Some(Read { file, stamp, on });
        on
    })
}

/// Whether the configuration in `file` turns the plugin on.
fn enabled_in(file: &Path) -> bool {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|config| {
            config
                .get("plugins")?
                .get(PLUGIN)?
                .get("enabled")?
                .as_bool()
        })
        .unwrap_or(false)
}

/// Whether a version of the plugin Codex installed names, for its server, a
/// program that is there: the ChatGPT app's Node, which goes with the app.
fn program_installed(home: &str) -> bool {
    let cache = path::join(&[&[home][..], &PLUGIN_CACHE[..]].concat());
    let Ok(versions) = std::fs::read_dir(cache) else {
        return false;
    };
    versions.flatten().any(|version| {
        std::fs::read(version.path().join(".mcp.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|servers| {
                servers["mcpServers"][SERVER]["command"]
                    .as_str()
                    .map(|program| Path::new(program).is_file())
            })
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests;
