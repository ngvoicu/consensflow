//! A home of a case's own: a folder made for it and removed with it, and the
//! environment that points `cf` at the folders in it. Every case runs against a
//! throwaway ConsensFlow home and throwaway harness homes, never the machine's
//! own `~/.consensflow`.
//!
//! The environment is these variables and no others: `HOME`,
//! `CONSENSFLOW_HOME`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_CONFIG_HOME`, a
//! `PATH` of one folder (empty until a case puts a stand-in command in it) and
//! `CONSENSFLOW_BIN_DIR`, all under the folder. Nothing is made in it: `cf`
//! makes what it needs.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::process::{Ran, Run};
use crate::{cf, stand_in, Error, Result};

/// A throwaway home, removed when it goes out of scope.
#[derive(Debug)]
pub struct ScratchHome {
    folder: TempDir,
}

impl ScratchHome {
    /// A new home, in a folder of its own under the system's temporary one.
    pub fn new() -> Result<Self> {
        let folder = tempfile::Builder::new()
            .prefix("cf-e2e-")
            .tempdir()
            .map_err(|source| Error::File {
                action: "make a folder in",
                path: std::env::temp_dir(),
                source,
            })?;
        Ok(Self { folder })
    }

    /// The folder everything of the home is under.
    pub fn root(&self) -> &Path {
        self.folder.path()
    }

    /// ConsensFlow's home, `CONSENSFLOW_HOME`.
    pub fn consensflow(&self) -> PathBuf {
        self.root().join("consensflow")
    }

    /// Where `cf setup` puts the terminal's command, `CONSENSFLOW_BIN_DIR`.
    pub fn bin(&self) -> PathBuf {
        self.consensflow().join("bin")
    }

    /// The one folder on the `PATH`, where the harnesses' commands are looked for.
    pub fn path_dir(&self) -> PathBuf {
        self.root().join("bin")
    }

    /// The environment of the home, variable by variable.
    pub fn vars(&self) -> Vec<(&'static str, PathBuf)> {
        let home = self.root().join("home");
        vec![
            ("HOME", home.clone()),
            ("CONSENSFLOW_HOME", self.consensflow()),
            ("CLAUDE_CONFIG_DIR", home.join(".claude")),
            ("CODEX_HOME", home.join(".codex")),
            ("XDG_CONFIG_HOME", home.join(".config")),
            ("PATH", self.path_dir()),
            ("CONSENSFLOW_BIN_DIR", self.bin()),
        ]
    }

    /// `program` to run with the environment of the home and no other.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Run {
        Run::new(program).vars(self.vars())
    }

    /// Puts a stand-in for the harness command `name` on the `PATH`.
    pub fn stand_in(&self, name: &str) -> Result<PathBuf> {
        stand_in::install(&self.path_dir(), name)
    }

    /// `cf` run with `args` in the home.
    pub fn cf<I, S>(&self, args: I) -> Result<Ran>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command(cf::binary()?).args(args).run()
    }

    /// `cf` run with `args` in the home, with `vars` set besides its environment.
    pub fn cf_with<V, K, X, I, S>(&self, vars: V, args: I) -> Result<Ran>
    where
        V: IntoIterator<Item = (K, X)>,
        K: AsRef<OsStr>,
        X: AsRef<OsStr>,
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command(cf::binary()?).vars(vars).args(args).run()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_is_these_variables_all_under_the_folder_of_the_home() {
        let home = ScratchHome::new().unwrap();
        let vars = home.vars();
        let names: Vec<_> = vars.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                "HOME",
                "CONSENSFLOW_HOME",
                "CLAUDE_CONFIG_DIR",
                "CODEX_HOME",
                "XDG_CONFIG_HOME",
                "PATH",
                "CONSENSFLOW_BIN_DIR",
            ]
        );
        for (name, value) in &vars {
            assert!(
                value.starts_with(home.root()),
                "{name} is {}",
                value.display()
            );
        }
        // The terminal's command goes in ConsensFlow's home; the PATH is a folder of its own.
        assert_eq!(home.bin(), home.consensflow().join("bin"));
        assert_ne!(home.path_dir(), home.bin());
    }

    #[test]
    fn a_home_has_a_folder_of_its_own_that_goes_with_it() {
        let (first, second) = (ScratchHome::new().unwrap(), ScratchHome::new().unwrap());
        assert_ne!(first.root(), second.root());
        let root = first.root().to_path_buf();
        assert!(root.is_dir());
        drop(first);
        assert!(!root.exists());
    }

    #[test]
    fn a_stand_in_is_put_on_the_path_of_the_home() {
        let home = ScratchHome::new().unwrap();
        let found = home.stand_in("claude").unwrap();
        assert_eq!(found.parent(), Some(home.path_dir().as_path()));
        assert!(found.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_program_run_by_the_home_sees_its_variables_and_none_of_the_tests() {
        let home = ScratchHome::new().unwrap();
        let ran = home
            .command("/bin/sh")
            .args([
                "-c",
                r#"printf '%s|%s|%s|%s' "$HOME" "$CONSENSFLOW_HOME" "$PATH" "${CARGO_MANIFEST_DIR-unset}""#,
            ])
            .run()
            .unwrap();
        let root = home.root().display();
        assert_eq!(
            ran.stdout,
            format!("{root}/home|{root}/consensflow|{root}/bin|unset")
        );
    }
}
