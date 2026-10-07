//! The names the recording writes for what is of the machine that made it
//! (`$ROOT`, `$VERSION`, `$NODE`, `$REPO`, `$HASH` and `$PAYLOAD`, and the one
//! this player adds, `$CF`), and what this run puts in their places. The format
//! says what each stands for: tests/goldens/cli/FORMAT.md.
//!
//! - `$NODE` and `$REPO` are what a launcher of Node's names: the runtime that
//!   ran the CLI, and the checkout it ran from. A native `cf` has neither, so a
//!   launcher that names them is made here with a program that is there (this
//!   test's own) and the `cf.mjs` beside the binary under test, which is the
//!   copy a native `cf` reads such a launcher as running; and what the binary
//!   says of them is written back as the names.
//! - `$HASH` and `$PAYLOAD` are an extension's bundle: the folder that is
//!   named by a hash of its bytes, and the bytes, which are the repository's
//!   own. Here the bytes are compared with the repository's files, so the
//!   name says something.
//! - `$CF` is the binary under test, which the launcher this build writes runs.

use std::fs;
use std::path::Path;

use serde_json::Value;

use super::plain;

/// The version of the build, which the recording writes as `$VERSION`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// OpenCode's list of the plugins its TUI loads, in the bundle.
const TUI: &str = "hosts/opencode-extension/tui.json";

/// Where each name is this run's.
pub struct Names {
    /// The binary under test as the system spells it: a program that is run by
    /// this path reads the same path back as its own.
    pub cf: String,
    /// The `cf.mjs` beside it.
    mjs: String,
    /// A program that is there, for `$NODE`.
    runtime: String,
}

impl Names {
    pub fn new() -> Self {
        let cf = plain(
            &fs::canonicalize(env!("CARGO_BIN_EXE_cf"))
                .unwrap()
                .to_string_lossy(),
        );
        let mjs = plain(&Path::new(&cf).with_file_name("cf.mjs").to_string_lossy());
        let runtime = plain(&std::env::current_exe().unwrap().to_string_lossy());
        Self { cf, mjs, runtime }
    }

    /// A text of a case's folder as it is made here: the names put as this
    /// run's, and `root` for the folder.
    pub fn made(&self, text: &str, root: &str) -> String {
        text.replace("$REPO/bin/cf.mjs", &self.mjs)
            .replace("$NODE", &self.runtime)
            .replace("$ROOT", root)
    }

    /// A text as the recording writes it: what is of this run, written as its
    /// name. The longer place first, since `cf` is the start of `cf.mjs`.
    pub fn recorded(&self, text: &str, root: &str) -> String {
        text.replace(&self.mjs, "$REPO/bin/cf.mjs")
            .replace(&self.cf, "$CF")
            .replace(&self.runtime, "$NODE")
            .replace(root, "$ROOT")
            .replace(VERSION, "$VERSION")
    }
}

/// A file of an extension's bundle, by its path in the folder
/// (`extensions/<pi or opencode>/<hash>/<its path in the repository>`): the
/// path as the recording writes it, the hash as `$HASH`, and the file's own
/// path in the repository. None for any other file.
pub fn bundle_entry(path: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = path.split('/').collect();
    let at = parts.windows(3).position(|three| {
        three[0] == "extensions"
            && matches!(three[1], "pi" | "opencode")
            && three[2].len() == 64
            && three[2]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })?;
    let own = parts[at + 3..].join("/");
    let masked = format!("{}/$HASH/{own}", parts[..at + 2].join("/"));
    Some((masked, own))
}

/// The text of a file of a bundle as the recording writes it: `$PAYLOAD` when
/// its bytes are what they must be, and what they are when they are not.
///
/// They are the repository's file at the file's `own` path, but for OpenCode's
/// `tui.json`, which is made where it is put: it names the session plugin
/// beside it by its file URL, so it is held to that shape and not to a file.
pub fn payload(own: &str, bytes: &[u8]) -> String {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(own);
    let right = if own == TUI {
        names_the_session_plugin(bytes)
    } else {
        fs::read(source).is_ok_and(|held| held == bytes)
    };
    if right {
        "$PAYLOAD".to_owned()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Whether `bytes` are `{"plugin":["<file URL of consensflow-session.mjs>"]}`
/// as `JSON.stringify` writes it, with nothing else in it. The URL's escapes
/// of a folder's name are the harness crate's to hold against Node's.
fn names_the_session_plugin(bytes: &[u8]) -> bool {
    let Ok(held) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    let named = held["plugin"]
        .as_array()
        .and_then(|plugins| match plugins.as_slice() {
            [plugin] => plugin.as_str(),
            _ => None,
        })
        .is_some_and(|url| {
            url.starts_with("file:///")
                && url.ends_with("/hosts/opencode-extension/consensflow-session.mjs")
        });
    named && serde_json::to_vec(&held).is_ok_and(|compact| compact == bytes)
}
