//! Claude Code's items in the login Keychain, and what keeps a run from leaving one (T-1102).
//!
//! **The hazard.** On a Mac Claude Code keeps what it stores securely in the login Keychain, not
//! in its config folder: its login (`claudeAiOauth`) and what MCP servers ask of OAuth
//! (`mcpOAuth`). The item is `Claude Code-credentials` for the default folder, and for a
//! `CLAUDE_CONFIG_DIR` of any other path `Claude Code-credentials-<8 hex>`, the first eight
//! digits of the SHA-256 of that path (2.1.316 and 2.1.317, read from the binary and shown by
//! `live keychain`). A throwaway config folder is never used twice, so every one that Claude
//! Code writes secure storage for leaves an item behind that nothing removes.
//!
//! **What writes it.** The first write to the folder's secure storage, which is no more than
//! connecting to an MCP server that asks for OAuth: Claude Code saves the discovery state of the
//! server (`mcpOAuth`, no token in it) at once, in `claude mcp list` and in a session alike, and
//! the servers it connects to are the folder's user-scope servers, the project's `.mcp.json`,
//! the plugins' servers and the account's connectors. The write creates the item. If the
//! folder held a login in `.credentials.json` (the kit's copy of the owner's), the same write
//! carries the login into the item and **deletes the file**: the run's copy of the owner's
//! login, refresh token and all, then outlives the folder in the owner's Keychain. (T-1044's
//! item held `mcpOAuth` alone, four entries: a Claude Code with no login, in a folder that had
//! the owner's plugins, whose four remote servers ask for OAuth.)
//!
//! **What keeps a run from it.**
//!
//! - A Claude Code started with [`super::ISOLATION`] (`--strict-mcp-config --no-chrome`) connects
//!   to no MCP server it was not given, so it writes none of that: shown on the machine's own by
//!   `live keychain`, for the user-scope servers, the project's, and the plugins'. Every window
//!   of a live run and of an eval has it (the wrappers of [`super::machine`]).
//! - A probe that cannot take the flag (`claude mcp list`, which connects to every server on
//!   purpose) runs with [`without_keychain`] first on its `PATH`: Claude Code calls `security` by
//!   its bare name, so a `security` that finds no item and writes none sends it to the folder's
//!   `.credentials.json`, where everything it stores goes with the folder. Only for a folder of
//!   the run's own: on the owner's config it would hide the owner's login from the run.
//! - The guard of the launchers ([`super::launcher::Guard`]) looks at the Keychain's names around
//!   every rig that seats Claude Code, and fails the rig that ends if one was gained
//!   ([`gained_words`]); [`super::login::Login`] names the item of its own folder.
//!
//! The kit never deletes an item that is not its own: the words name the command, and the owner
//! runs it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::login::SERVICE;
use crate::process::{own_var, Run};
use crate::{Error, Result};

/// The names of the Keychain items of the service `Claude Code-credentials` now, and of the ones
/// Claude Code names after a config folder; none where the machine has no Keychain, or it cannot
/// be listed (`security dump-keychain` prints the items' attributes and none of their passwords).
pub use super::login::keychain_services as names;

/// The exit code of `security` for an item that is not there (`errSecItemNotFound`): what Claude
/// Code reads as "no item", whatever else the Keychain holds.
const NOT_FOUND: i32 = 44;

/// The name of the Keychain item Claude Code keeps the secure storage of `folder`, a
/// `CLAUDE_CONFIG_DIR`, under: [`SERVICE`] and the first eight hex digits of the SHA-256 of the
/// path as it is spelled (Claude Code normalizes it to NFC first; the folders of the kit are
/// ASCII, and a path that is not may name another item than this).
pub fn item_of(folder: &Path) -> String {
    let digest = Sha256::digest(folder.to_string_lossy().as_bytes());
    let hex: String = digest
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{SERVICE}-{hex}")
}

/// What the guard says of a Keychain item that a run gained: where it comes from, and the command
/// that deletes it (never run here).
pub fn gained_words(name: &str) -> String {
    let mut words = format!("the Keychain gained the item `{name}`");
    if name.starts_with(&format!("{SERVICE}-")) {
        let _ = write!(
            words,
            ", which Claude Code makes for a config folder of any path but the default (the 8 hex digits \
             are those of the folder's path) when it writes secure storage there: the OAuth state of an MCP \
             server it connected to, or the login a folder held, which it carries into the item and deletes \
             from the folder"
        );
    }
    let _ = write!(
        words,
        "; the Keychain is the whole machine's, so it may be another run's or the owner's own Claude Code's \
         (delete it with `security delete-generic-password -s \"{name}\"`; the kit never does)"
    );
    words
}

/// Deletes the item Claude Code made for `folder`, a config folder that the caller made itself for a
/// Claude Code that it ran on purpose to show the hazard: that item and no other, by the name
/// [`item_of`] makes of it, for the account of the user. An item with a folder's hash is never the
/// owner's own (that one has none), and the kit deletes no other (a run that leaves one is failed
/// by the guard, which names it for the owner to delete). Whether it was there; none where the
/// machine has no Keychain.
pub fn forget(folder: &Path) -> Result<bool> {
    if !cfg!(target_os = "macos") {
        return Ok(false);
    }
    let name = item_of(folder);
    let user = own_var("USER")
        .filter(|user| !user.is_empty())
        .ok_or_else(|| {
            Error::Daemon("this run has no USER, which names the account of the item".to_owned())
        })?;
    let ran = Run::new("/usr/bin/security")
        .args(["delete-generic-password", "-s", &name, "-a", &user])
        .inheriting_env()
        .run()?;
    Ok(ran.code == Some(0))
}

/// The name of the program the stand-in for `security` is.
const PROGRAM: &str = "security";

/// What the stand-in for `security` is: it finds no item and writes none.
fn script() -> String {
    format!(
        "#!/bin/sh\n\
         # Written by the kit (cf_e2e::live::keychain): a Claude Code with a config folder of its own keeps\n\
         # what it stores in that folder, and leaves no item in the owner's login Keychain.\n\
         case \"$1\" in\n  find-generic-password|delete-generic-password) exit {NOT_FOUND} ;;\n  *) exit 1 ;;\nesac\n"
    )
}

/// A folder, made in `into`, that holds a `security` which finds no Keychain item and writes none
/// (the exit code Claude Code reads as "no item", and a failure for anything else): put first on
/// the `PATH` of a Claude Code that has a config folder of the run's own, it keeps what it stores
/// in that folder's `.credentials.json` and makes no Keychain item. The folder to put on the
/// `PATH`; none where the machine has no such Keychain (Claude Code keeps its login in a file
/// there already).
pub fn without_keychain(into: &Path) -> Result<Option<PathBuf>> {
    if !cfg!(unix) {
        return Ok(None);
    }
    let folder = into.join("no-keychain");
    crate::door::make_dir(&folder)?;
    crate::door::write_executable(folder.join(PROGRAM), script())?;
    Ok(Some(folder))
}

#[cfg(test)]
mod tests;
