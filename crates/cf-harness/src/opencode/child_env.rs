//! The environment a program of a window runs with: a base, plus what the
//! window declares, minus what it must not see. OpenCode's is its only caller:
//! the throwaway server that makes a fresh window's conversation runs in the
//! engine's environment with no declared keys.
//!
//! Kept from Node on purpose: on Windows a variable's name is read in upper
//! case (`Env`), where Node copied the environment into a plain object, in
//! which a key is dropped by its own case alone.

use std::ffi::OsStr;

use cf_base::env::Env;

/// What a window declares of its environment besides its base.
#[derive(Debug, Default)]
pub(super) struct Declared<'a> {
    /// What it adds, over the base's own.
    pub(super) env: &'a [(&'a str, &'a str)],
    /// The keys it must not inherit, which would silently switch its billing.
    pub(super) drop_env: &'a [&'a str],
}

/// `base` and what `declared` adds, less the keys it drops and the ones of
/// cmux, the terminal whose own sockets and hook a window must not reach
/// (`CMUX_SOCKET*` and `CMUX_CLAUDE_HOOK_CMUX_BIN`).
pub(super) fn child_env(base: &Env, declared: &Declared<'_>) -> Env {
    let added = declared
        .env
        .iter()
        .map(|&(name, value)| (name.into(), value.into()));
    let merged = Env::from_vars(
        base.iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain(added),
    );
    let dropped = |name: &OsStr| {
        let text = name.to_string_lossy();
        declared.drop_env.iter().any(|&key| key == text)
            || text.starts_with("CMUX_SOCKET")
            || text == "CMUX_CLAUDE_HOOK_CMUX_BIN"
    };
    Env::from_vars(merged.iter().filter(|(name, _)| !dropped(name)))
}

#[cfg(test)]
mod tests;
