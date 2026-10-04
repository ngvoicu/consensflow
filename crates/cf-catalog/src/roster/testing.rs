//! What the unit tests of the roster's modules share: a home with an agents
//! file in it, the sentence a file that cannot be used is said with, and a
//! clock that counts how often it was read.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::{tempdir, TempDir};

/// The sentence a file that cannot be used is said with.
pub(crate) fn sentence(path: &Path, why: &str) -> String {
    format!(
        "Your agents file {} {why}: fix it or move it away. ConsensFlow left it as it is.",
        path.display()
    )
}

/// A home holding `agents.json` with these bytes.
pub(crate) fn file_with(bytes: &[u8]) -> (TempDir, PathBuf) {
    let home = tempdir().unwrap();
    let path = home.path().join("agents.json");
    fs::write(&path, bytes).unwrap();
    (home, path)
}

/// A clock at one instant that counts its readings: an add reads it once
/// for both stamps, a refused edit not at all.
pub(crate) struct Counted {
    pub(crate) now: i64,
    pub(crate) readings: usize,
}

impl Counted {
    /// 2026-10-04T12:00:00.000Z, read no times yet.
    pub(crate) fn new() -> Self {
        Self {
            now: 1_791_115_200_000,
            readings: 0,
        }
    }
}

impl cf_base::time::Clock for Counted {
    fn now_ms(&mut self) -> i64 {
        self.readings += 1;
        self.now
    }
}

/// A request's body: the object `json` writes.
pub(crate) fn body(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    match json {
        serde_json::Value::Object(fields) => fields,
        other => panic!("no object: {other}"),
    }
}
