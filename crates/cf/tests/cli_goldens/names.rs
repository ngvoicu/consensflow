//! The names the recording writes for what is of the machine that made it
//! (`$ROOT`, `$VERSION`, `$NODE`, `$REPO`, `$HASH` and `$PAYLOAD`, and the one
//! this player adds, `$CF`), and what this run puts in their places. The format
//! says what each stands for: tests/goldens/FORMAT.md.
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

use super::plain;

/// The version of the build, which the recording writes as `$VERSION`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where each name is this run's.
pub struct Names {
    /// The binary under test as the system spells it: a program that is run by
    /// this path reads the same path back as its own.
    pub cf: String,
    /// The `cf.mjs` beside it.
    mjs: String,
    /// A program that is there, for `$NODE`.
    runtime: String,
    /// The bytes of the shim beside the `cf`, which `setup` copies on Windows: the
    /// recording names them `$SHIM` ([`Names::is_shim`]).
    shim: Vec<u8>,
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
        let shim = fs::read(env!("CARGO_BIN_EXE_cf-shim")).unwrap();
        Self {
            cf,
            mjs,
            runtime,
            shim,
        }
    }

    /// Whether `bytes` are the shim a bundle carries beside its `cf`.
    pub fn is_shim(&self, bytes: &[u8]) -> bool {
        bytes == self.shim
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
/// (`extensions/pi/<hash>/<its path in the repository>`): the
/// path as the recording writes it, the hash as `$HASH`, and the file's own
/// path in the repository. None for any other file.
pub fn bundle_entry(path: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = path.split('/').collect();
    let at = parts.windows(3).position(|three| {
        three[0] == "extensions"
            && three[1] == "pi"
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
/// They are the repository's file at the file's `own` path.
pub fn payload(own: &str, bytes: &[u8]) -> String {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(own);
    if fs::read(source).is_ok_and(|held| held == bytes) {
        "$PAYLOAD".to_owned()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}
