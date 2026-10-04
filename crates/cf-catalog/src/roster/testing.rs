//! What the unit tests of the roster's modules share: a home with an agents
//! file in it, and the sentence a file that cannot be used is said with.

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
