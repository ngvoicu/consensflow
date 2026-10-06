//! What the app-preparation tests share: a throwaway home in the shape of
//! `tempEnv` (`tests/helpers.mjs`) and the stand-in harness CLIs a test puts
//! on its `PATH`.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use tempfile::TempDir;

/// A throwaway home: every test runs against a `CONSENSFLOW_HOME` and
/// harness homes of its own, and the environment is handed to every call.
pub struct Home {
    pub dir: TempDir,
}

impl Home {
    pub fn new() -> Self {
        let home = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(home.path_dir()).unwrap();
        home
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The folder `PATH` names, where the stand-in CLIs go.
    pub fn path_dir(&self) -> PathBuf {
        self.root().join("bin")
    }

    /// `CONSENSFLOW_HOME`.
    pub fn consensflow(&self) -> PathBuf {
        self.root().join("consensflow")
    }

    /// The user's home.
    pub fn user(&self) -> PathBuf {
        self.root().join("home")
    }

    /// The environment of a test (`tempEnv`).
    pub fn env(&self) -> Env {
        let text = |path: PathBuf| path.to_string_lossy().into_owned();
        let user = self.user();
        Env::from_vars([
            ("HOME", text(user.clone())),
            ("CONSENSFLOW_HOME", text(self.consensflow())),
            ("CLAUDE_CONFIG_DIR", text(user.join(".claude"))),
            ("CODEX_HOME", text(user.join(".codex"))),
            ("XDG_CONFIG_HOME", text(user.join(".config"))),
            ("PATH", text(self.path_dir())),
            ("CONSENSFLOW_BIN_DIR", text(self.consensflow().join("bin"))),
        ])
    }

    /// A stand-in CLI called `name` on `PATH`: a file this user can run, which
    /// is all that finding one asks (`fakeExecutable`).
    pub fn stub_cli(&self, name: &str) -> PathBuf {
        let file = self.path_dir().join(if cfg!(windows) {
            format!("{name}.cmd")
        } else {
            name.to_owned()
        });
        fs::write(
            &file,
            if cfg!(windows) {
                "@echo off\r\nexit /b 0\r\n"
            } else {
                "#!/bin/sh\nexit 0\n"
            },
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        }
        file
    }
}

/// A launcher is `cf` on POSIX and `cf.cmd` on Windows.
pub fn launcher_name() -> &'static str {
    if cfg!(windows) {
        "cf.cmd"
    } else {
        "cf"
    }
}
