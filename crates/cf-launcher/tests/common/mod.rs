//! What the launcher's tests share: a throwaway home in the shape of
//! `tempEnv` (`tests/helpers.mjs`), the two forms of a launcher, and the
//! old launcher alpha.78 wrote, which Node's `src/terminal.js` made and
//! nothing but this reads now.

#![allow(dead_code)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use cf_base::env::Env;
use cf_base::path;
use cf_launcher::{Places, Repair, Repaired};
use tempfile::TempDir;

/// A throwaway home: every test runs against a `CONSENSFLOW_HOME` and
/// harness homes of its own, and the environment is handed to every call.
pub struct Home {
    pub dir: TempDir,
    /// The folder `PATH` names, and the stand-in harness CLIs go in.
    pub bin: PathBuf,
}

impl Home {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        Self { dir, bin }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// `CONSENSFLOW_HOME`, a folder of its own.
    pub fn consensflow(&self) -> PathBuf {
        self.root().join("consensflow")
    }

    /// The environment of a test (`tempEnv`): the homes, `PATH` at `bin`, and a
    /// project's own bin override, which nothing here may honour. `windows`
    /// says it is Windows's, as `OS=Windows_NT` does: the form of a launcher
    /// is its environment's, so a `.cmd` is made and read on every system.
    pub fn env(&self, windows: bool) -> Env {
        self.env_with(windows, &[])
    }

    /// The same without a `CONSENSFLOW_HOME`: an ordinary terminal's.
    pub fn plain_env(&self, windows: bool) -> Env {
        self.env_with(windows, &[("CONSENSFLOW_HOME", None)])
    }

    /// The environment with each of `changes` made: a variable set to a
    /// value, or taken away.
    pub fn env_with(&self, windows: bool, changes: &[(&str, Option<&str>)]) -> Env {
        let mut vars = self.vars();
        if windows {
            vars.push(("OS".to_owned(), "Windows_NT".to_owned()));
        }
        for (name, value) in changes {
            vars.retain(|(held, _)| held != name);
            if let Some(value) = value {
                vars.push(((*name).to_owned(), (*value).to_owned()));
            }
        }
        Env::from_vars(vars)
    }

    fn vars(&self) -> Vec<(String, String)> {
        let text = |path: PathBuf| path.to_string_lossy().into_owned();
        let user = self.root().join("home");
        vec![
            ("HOME".to_owned(), text(user.clone())),
            ("CONSENSFLOW_HOME".to_owned(), text(self.consensflow())),
            ("CLAUDE_CONFIG_DIR".to_owned(), text(user.join(".claude"))),
            ("CODEX_HOME".to_owned(), text(user.join(".codex"))),
            ("XDG_CONFIG_HOME".to_owned(), text(user.join(".config"))),
            ("PATH".to_owned(), text(self.bin.clone())),
            (
                "CONSENSFLOW_BIN_DIR".to_owned(),
                text(self.consensflow().join("bin")),
            ),
        ]
    }
}

/// The forms of a launcher this system can make and read: the one of its
/// own, and, anywhere but Windows, a `.cmd` too, as Windows's environment
/// asks for one.
pub fn forms() -> Vec<bool> {
    if cfg!(windows) {
        vec![true]
    } else {
        vec![false, true]
    }
}

/// How a launcher is called: `cf` in the form of `sh`, `cf.cmd` in the form
/// of cmd.exe.
pub fn called(name: &str, windows: bool) -> String {
    if windows {
        format!("{name}.cmd")
    } else {
        name.to_owned()
    }
}

/// A file with `text` in it, its folders made.
pub fn write(file: &Path, text: &str) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// A stand-in `cf` of a bundle under `root`, which is there to be found:
/// `<root>/<app>/Contents/Resources/cli/bin/cf`, and its `cf.mjs` and
/// runtime beside the layout of the flip release, where both still are.
pub struct Bundle {
    pub cf: PathBuf,
    pub cf_mjs: PathBuf,
    pub node: PathBuf,
}

impl Bundle {
    pub fn new(root: &Path, app: &str) -> Self {
        let contents = root.join(format!("{app}.app")).join("Contents");
        let bin = contents.join("Resources").join("cli").join("bin");
        let name = |name: &str| bin.join(name);
        let bundle = Self {
            cf: name(if cfg!(windows) { "cf.exe" } else { "cf" }),
            cf_mjs: name("cf.mjs"),
            node: contents.join("MacOS").join("node"),
        };
        for file in [&bundle.cf, &bundle.cf_mjs, &bundle.node] {
            write(file, "");
        }
        bundle
    }
}

/// The launcher alpha.78 and every build before it wrote (`launcher`,
/// `src/terminal.js`), in the form of cmd.exe when `windows`: Node's own
/// text, which the repair's cases and the goldens begin from.
pub fn old_launcher(windows: bool, runtime: &Path, cli: &Path, home: Option<&str>) -> String {
    let (runtime, cli) = (runtime.display(), cli.display());
    if windows {
        let pin = home.map_or_else(String::new, |home| {
            format!("setlocal\r\nset \"CONSENSFLOW_HOME={home}\"\r\n")
        });
        return format!("@echo off\r\nREM Installed by ConsensFlow. Runs the app's own runtime and its own copy of\r\nREM the CLI, so the terminal and the window never drift apart.\r\n{pin}\"{runtime}\" \"{cli}\" %*\r\n");
    }
    let pin = home.map_or_else(String::new, |home| {
        format!("export CONSENSFLOW_HOME=\"{home}\"\n")
    });
    format!("#!/bin/sh\n# Installed by ConsensFlow. Runs the app's own runtime and its own copy of\n# the CLI, so the terminal and the window never drift apart.\n{pin}exec \"{runtime}\" \"{cli}\" \"$@\"\n")
}

/// `folder` joined with `name` as Node's `path.join` does.
pub fn join(folder: &Path, name: &str) -> PathBuf {
    PathBuf::from(path::join(&[&folder.to_string_lossy(), name]))
}

/// What the file says.
pub fn read(file: &Path) -> String {
    fs::read_to_string(file).unwrap()
}

/// Where a test's launchers are: a folder of the home's of its own, which an
/// install makes, as Node's tests named theirs (`candidates: [bin]`).
pub fn launchers(home: &Home) -> PathBuf {
    home.root().join("launchers")
}

/// That folder as the place a test installs in and looks in.
pub fn at(home: &Home) -> Places {
    Places::at(vec![launchers(home)])
}

/// The two names a launcher goes by here, in that folder.
pub fn files(home: &Home, windows: bool) -> [PathBuf; 2] {
    ["consensflow", "cf"].map(|name| join(&launchers(home), &called(name, windows)))
}

/// What the repair must write for `cf`, spelled out: the new shape of each
/// form, pinned to `home` when there is one.
pub fn expected(windows: bool, cf: &Path, home: Option<&str>) -> String {
    let cf = cf.display();
    if windows {
        let pin = home.map_or_else(String::new, |home| {
            format!("setlocal\r\nset \"CONSENSFLOW_HOME={home}\"\r\n")
        });
        return format!("@echo off\r\nREM Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\r\nREM window never drift apart.\r\n{pin}\"{cf}\" %*\r\n");
    }
    let pin = home.map_or_else(String::new, |home| {
        format!("export CONSENSFLOW_HOME=\"{home}\"\n")
    });
    format!("#!/bin/sh\n# Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\n# window never drift apart.\n{pin}exec \"{cf}\" \"$@\"\n")
}

/// The outcomes of a repair, by name and in order.
pub fn outcomes(repaired: &[Repaired]) -> Vec<Repair> {
    repaired.iter().map(|each| each.outcome.clone()).collect()
}

/// A file's time of writing, set far in the past so that a write, however
/// fast, shows.
pub fn aged(file: &Path) -> SystemTime {
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(old)
        .unwrap();
    old
}
