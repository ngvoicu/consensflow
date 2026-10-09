//! `cargo xtask clippy-windows` (landing S10): clippy for Windows from a machine
//! that is not one, as the gate runs it here (`--all-targets -- -D warnings`,
//! tests included): what only Windows compiles (the `cfg(windows)` code, and the
//! unix code a Windows build must leave out) is found in seconds, though nothing
//! is run. The crates to lint are named, else all of the workspace but the app
//! (whose build wants the Windows runtime it ships with, which only a Windows
//! build has). The app is linted when it is named, with its resources left out of
//! its Tauri configuration and a resource compiler that compiles nothing, as a
//! check needs neither.
//!
//! It needs the `x86_64-pc-windows-msvc` target (`rustup target add`) and none of
//! Windows' toolchain: the C that SQLite is built from is not compiled (its
//! compiler is `true`) and its archive is made empty by xtask itself, which cc-rs
//! runs as its archiver: `xtask --as-archiver`, the second command here. cc-rs
//! splits the program it is told to run at the spaces, so a checkout under a
//! folder with a space in its name cannot be linted this way; the stand-ins are
//! the unix ones, so the machine is one that is not Windows (on Windows the
//! workspace's own clippy is clippy for Windows, which is why `check` has no
//! step of this there).

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io;
use std::iter;
use std::path::{Path, PathBuf};

use crate::app;
use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};

/// The target linted for.
const TARGET: &str = "x86_64-pc-windows-msvc";
/// The variables cc-rs reads the C compiler and the archiver of that target from.
const CC: &str = "CC_x86_64_pc_windows_msvc";
const AR: &str = "AR_x86_64_pc_windows_msvc";
/// The first word xtask is run with when it is the archiver.
const AS_ARCHIVER: &str = "--as-archiver";

/// Where the resource compiler that compiles nothing is put, from the checkout's
/// root: in the build folder, which nothing else reads.
const STAND_IN_FOLDER: &str = "app/src-tauri/target/clippy-windows";
/// Its file, and what it is: the app's build script runs `llvm-rc` for its icon
/// and its manifest, and a check needs neither.
const STAND_IN: (&str, &str) = ("llvm-rc", "#!/bin/sh\nexit 0\n");

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["clippy-windows"],
        about: "Run clippy for x86_64-pc-windows-msvc, tests included and warnings denied",
        usage: "[CRATE ...]  (all of the workspace but the app by default; `app` names the app)",
        run: Run::Native(run),
    },
    Command {
        words: &[AS_ARCHIVER],
        about: "What clippy-windows has cc-rs run as the archiver: make the empty archive it is asked for",
        usage: "cq ARCHIVE [OBJECT ...]  or  -out:ARCHIVE [OBJECT ...]",
        run: Run::Native(archive),
    },
];

fn run(context: &Context, args: &[OsString], _console: &mut Console) -> Result<i32, Failure> {
    let mut lint = lint(context, args, &archiver()?);
    if names_the_app(args) {
        lint = lint.var("PATH", path_with_stand_in(context)?);
    }
    Ok(process::run(&lint, &context.env)?)
}

/// This xtask's own program: what cc-rs is told to run as the archiver.
pub(crate) fn archiver() -> Result<PathBuf, Failure> {
    std::env::current_exe().map_err(doing("find the xtask that cc-rs is to run as the archiver"))
}

/// The lint for Windows of the `crates` named, or of all of the workspace but
/// the app when none is, from the checkout's root. Every word is a crate. The
/// C compiler is `true`, and the archiver is `archiver` run as one.
pub(crate) fn lint(context: &Context, crates: &[OsString], archiver: &Path) -> Invocation {
    let selection: Vec<OsString> = if crates.is_empty() {
        ["--workspace", "--exclude", "app"]
            .map(OsString::from)
            .to_vec()
    } else {
        crates
            .iter()
            .flat_map(|name| [OsString::from("-p"), name.clone()])
            .collect()
    };
    let mut as_archiver = archiver.as_os_str().to_os_string();
    as_archiver.push(" ");
    as_archiver.push(AS_ARCHIVER);
    let invocation = Invocation::new("cargo", &context.root)
        .args(["clippy", "--offline", "--target", TARGET, "--all-targets"])
        .args(selection)
        .args(["--", "-D", "warnings"])
        .var(CC, "true")
        .var(AR, as_archiver);
    if names_the_app(crates) {
        app::without_resources(invocation)
    } else {
        invocation
    }
}

/// Whether the app is one of the `crates`.
fn names_the_app(crates: &[OsString]) -> bool {
    crates.iter().any(|name| name == "app")
}

/// The PATH a lint of the app runs with: the folder of the resource compiler
/// that compiles nothing (made now) first, then the PATH xtask has.
fn path_with_stand_in(context: &Context) -> Result<OsString, Failure> {
    let folder = context.path(STAND_IN_FOLDER);
    fs::create_dir_all(&folder).map_err(doing(format!("make {}", folder.display())))?;
    let (name, body) = STAND_IN;
    let file = folder.join(name);
    write_program(&file, body).map_err(doing(format!("write {}", file.display())))?;
    let kept = context
        .env
        .os("PATH")
        .map(std::env::split_paths)
        .into_iter()
        .flatten();
    std::env::join_paths(iter::once(folder.clone()).chain(kept)).map_err(|cause| {
        Failure::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("could not put {} on the PATH: {cause}", folder.display()),
        ))
    })
}

/// Writes `body` to `file`, as a program where a program needs a mode to be one.
fn write_program(file: &Path, body: &str) -> io::Result<()> {
    fs::write(file, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// `xtask --as-archiver cq ARCHIVE OBJECT ...` (`ar`'s line) or
/// `xtask --as-archiver -out:ARCHIVE OBJECT ...` (`lib`'s): makes the archive
/// empty when there is none, and leaves it as it is when there is. cc-rs asks
/// for the archive of a C library it was told to build, and then links it:
/// nothing of the C was compiled, so there is nothing to put in it. A line that
/// names no archive makes none.
fn archive(_context: &Context, args: &[OsString], _console: &mut Console) -> Result<i32, Failure> {
    if let Some(archive) = archive_named(args) {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&archive)
            .map_err(doing(format!("make the archive {}", archive.display())))?;
    }
    Ok(0)
}

/// The archive an archiver's `args` ask for: what follows `-out:` (or `/out:`,
/// in any case) in the first word that begins so, else the second word.
fn archive_named(args: &[OsString]) -> Option<PathBuf> {
    let words: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
    let named = words.iter().find_map(|word| {
        let bytes = word.as_bytes();
        let out = bytes.len() >= 5
            && matches!(bytes[0], b'-' | b'/')
            && bytes[1..4].eq_ignore_ascii_case(b"out")
            && bytes[4] == b':';
        out.then(|| &word[5..])
    });
    named
        .or_else(|| words.get(1).map(|word| &**word))
        .map(PathBuf::from)
}

/// How an input/output failure is told: what was being done, and why it was
/// not, with the kind kept for whoever reads it.
fn doing(what: impl std::fmt::Display) -> impl FnOnce(io::Error) -> Failure {
    move |cause| {
        Failure::Io(io::Error::new(
            cause.kind(),
            format!("could not {what}: {cause}"),
        ))
    }
}

#[cfg(test)]
mod tests;
