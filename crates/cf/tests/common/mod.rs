//! `cf` as the tests run it: the binary, with none of ConsensFlow's
//! variables from a window this test may itself run in, a home of its own
//! unless the case names one, and only the variables the case gives.

// The tests' own helper: a failure in it is the test's.
#![allow(clippy::expect_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// `cf args` with `env` added and `input` on its standard input.
#[allow(dead_code)] // Each test file takes what it needs of this.
pub fn cf<S: AsRef<str>>(args: &[S], env: &[(&str, &str)], input: &str) -> Output {
    cf_at(Path::new(env!("CARGO_BIN_EXE_cf")), args, env, input)
}

/// The `cf` at `program`, run as [`cf`] runs one. A tokenless `cf` looks in the
/// home for the way back to Node (`cf_base::way_back`), so the home is a
/// folder of this run's own unless the case names one: the machine's own
/// `~/.consensflow` is nobody's to look in.
#[allow(clippy::disallowed_methods)] // The tests start cf themselves.
pub fn cf_at<S: AsRef<str>>(
    program: &Path,
    args: &[S],
    env: &[(&str, &str)],
    input: &str,
) -> Output {
    let home = tempfile::tempdir().expect("a home");
    let mut command = Command::new(program);
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if ["CONSENSFLOW_", "CF_", "CHISEL_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            command.env_remove(&*name);
        }
    }
    command.env("CONSENSFLOW_HOME", home.path());
    let mut child = command
        .args(args.iter().map(AsRef::as_ref))
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cf starts");
    let mut stdin = child.stdin.take().expect("its standard input");
    // A command that reads no input may have ended before it is written.
    let _ = stdin.write_all(input.as_bytes());
    drop(stdin);
    child.wait_with_output().expect("cf ends")
}

/// A bundle laid out as the app's is, with a `cf` of this build where the
/// app's is, a `cf.mjs` beside it and, when asked for, a Node of the bundle's
/// own where the bundle keeps its Node:
///
/// ```text
/// unix      <dir>/Contents/Resources/cli/bin/{cf, cf.mjs}   <dir>/Contents/MacOS/node
/// Windows   <dir>\cli\bin\{cf.exe, cf.mjs}                  <dir>\node.exe
/// ```
#[allow(dead_code)] // Each test file takes what it needs of this.
pub struct Bundle {
    pub dir: tempfile::TempDir,
    pub cf: PathBuf,
    pub cf_mjs: PathBuf,
    pub node: PathBuf,
}

#[allow(dead_code)]
impl Bundle {
    /// The bundle, with the stand-in Node `node` runs as, a shell script (a
    /// Windows stand-in cannot be one, and there only its place is held).
    pub fn new(node: Option<&str>) -> Self {
        let dir = tempfile::tempdir().expect("a bundle");
        // The folder as the running `cf` will name it: `current_exe` is
        // resolved (`/var` is `/private/var` on a Mac), and plain on Windows.
        let root = plain(&std::fs::canonicalize(dir.path()).expect("the bundle's folder"));
        let (resources, node_at) = if cfg!(windows) {
            (root.clone(), root.join("node.exe"))
        } else {
            (
                root.join("Contents").join("Resources"),
                root.join("Contents").join("MacOS").join("node"),
            )
        };
        let bin = resources.join("cli").join("bin");
        std::fs::create_dir_all(&bin).expect("the bundle's folders");
        let cf = bin.join(if cfg!(windows) { "cf.exe" } else { "cf" });
        std::fs::copy(env!("CARGO_BIN_EXE_cf"), &cf).expect("a cf in the bundle");
        let cf_mjs = bin.join("cf.mjs");
        std::fs::write(&cf_mjs, "// the CLI's sources\n").expect("a cf.mjs");
        if let Some(body) = node {
            std::fs::create_dir_all(node_at.parent().expect("a folder")).expect("its folder");
            std::fs::write(&node_at, format!("#!/bin/sh\n{body}\n")).expect("a node");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&node_at, std::fs::Permissions::from_mode(0o755))
                    .expect("a node that runs");
            }
        }
        Self {
            dir,
            cf,
            cf_mjs,
            node: node_at,
        }
    }

    /// `cf args` as this bundle's `cf` runs them, in `home`, which has the way
    /// back's file when `way_back` says so, with `env` added.
    pub fn cf(&self, args: &[&str], home: &Home, env: &[(&str, &str)]) -> Output {
        let mut given = vec![("CONSENSFLOW_HOME", home.path_text())];
        given.extend(env.iter().copied());
        cf_at(&self.cf, args, &given, "")
    }
}

/// A path as Windows' `GetModuleFileName` writes one, without the prefix a
/// resolved path has; any other path as it is.
#[allow(dead_code)]
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{share}"));
    }
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

/// A home, with the file that is the way back to Node in it or not.
#[allow(dead_code)]
pub struct Home {
    dir: tempfile::TempDir,
    text: String,
}

#[allow(dead_code)]
impl Home {
    /// A home that has taken the way back when `way_back`.
    pub fn new(way_back: bool) -> Self {
        let dir = tempfile::tempdir().expect("a home");
        if way_back {
            std::fs::write(dir.path().join(cf_base::way_back::FILE), "").expect("the file");
        }
        let text = dir.path().to_string_lossy().into_owned();
        Self { dir, text }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn path_text(&self) -> &str {
        &self.text
    }
}
