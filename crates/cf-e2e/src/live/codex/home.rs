//! A Codex home of a run's own: a copy of the machine's login and nothing else of
//! the owner's, which a check writes its own configuration into. A run on the
//! owner's `CODEX_HOME` saves a trust table for its folder in his `config.toml`
//! and its threads among his; a run on a copy leaves his home as it was. Codex
//! may refresh the login it is given, which rotates the owner's refresh token
//! with it: [`CodexHome::rotated`] says so, for the run to say at once.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::super::machine::Installed;
use crate::{files, Error, Result};

/// The file Codex keeps its login in, in its home.
const LOGIN: &str = "auth.json";

/// A Codex home made for a run, gone with it.
pub struct CodexHome {
    folder: TempDir,
    /// The login as it was copied.
    login: Vec<u8>,
}

impl CodexHome {
    /// A home holding a copy of the login of `codex` (the machine's Codex, whose
    /// home is where its login is read from), readable by its owner alone.
    pub fn copying_login_of(codex: &Installed) -> Result<Self> {
        let from = codex.home().join(LOGIN);
        let login = std::fs::read(&from).map_err(|source| Error::File {
            action: "read",
            path: from.clone(),
            source,
        })?;
        let folder = tempfile::Builder::new()
            .prefix("cf-codex-home-")
            .tempdir()
            .map_err(|source| Error::File {
                action: "make a folder in",
                path: std::env::temp_dir(),
                source,
            })?;
        let home = Self { folder, login };
        home.write_private(LOGIN, &home.login)?;
        Ok(home)
    }

    /// Where it is: what `CODEX_HOME` names.
    pub fn path(&self) -> &Path {
        self.folder.path()
    }

    /// The file `name` of the home, made with the folders it is in, holding `text`.
    pub fn write(&self, name: &str, text: &str) -> Result {
        files::write(&self.folder.path().join(name), text)
    }

    /// The file `name`, holding `bytes`, readable by its owner alone.
    fn write_private(&self, name: &str, bytes: &[u8]) -> Result {
        let path: PathBuf = self.folder.path().join(name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let failed = |source| Error::File {
            action: "write",
            path: path.clone(),
            source,
        };
        let mut file = options.open(&path).map_err(failed)?;
        std::io::Write::write_all(&mut file, bytes).map_err(failed)
    }

    /// Whether Codex wrote the login again while the run used the copy: a refresh,
    /// after which the owner's own login may be stale.
    pub fn rotated(&self) -> bool {
        std::fs::read(self.folder.path().join(LOGIN)).map_or(true, |now| now != self.login)
    }
}
