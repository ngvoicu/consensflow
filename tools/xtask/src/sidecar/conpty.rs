//! `cargo xtask conpty --into DIR`: Microsoft's own console host for the app's
//! terminals on Windows, `conpty.dll` and `OpenConsole.exe`, built from the
//! Windows Terminal repository (microsoft/terminal, MIT) and published by
//! Microsoft on NuGet as Microsoft.Windows.Console.ConPTY. Beside the program
//! that opens the terminals, portable-pty loads them instead of the system's
//! console host, so every Windows the app runs on (Windows 10's builds too) has
//! the same, current one, as VS Code and WezTerm ship theirs.
//!
//! The package is pinned, and checked against its SHA-256 before anything in it
//! is used. It is kept in `app/.cache` and checked each time it is used: one
//! that is not the pinned package (cut short, changed) is fetched again, and one
//! that was just fetched and is not is refused. The fetching is `curl`, which
//! Windows has had since 10.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use zip::result::ZipError;
use zip::ZipArchive;

use super::{files, finish, ran, Error, Machine, System};
use crate::context::Context;
use crate::dispatch::{Console, Failure};
use crate::process::Invocation;

/// A version of the package, and its SHA-256 as nuget.org serves it (the
/// `.nupkg`, signed by Microsoft).
pub(super) struct Package<'a> {
    pub version: &'a str,
    pub sha256: &'a str,
}

/// The package the app ships.
pub(super) const PINNED: Package<'static> = Package {
    version: "1.25.260930003",
    sha256: "02b07b349af66d801159bdf9e440d4a1ce78bb951f37fc8609731665afdae7ee",
};

/// What the app takes of it, for x64 Windows: where the package keeps each, and
/// its name beside the app.
const FILES: [(&str, &str); 2] = [
    ("runtimes/win-x64/native/conpty.dll", "conpty.dll"),
    (
        "build/native/runtimes/x64/OpenConsole.exe",
        "OpenConsole.exe",
    ),
];

impl Package<'_> {
    fn name(&self) -> String {
        format!("microsoft.windows.console.conpty.{}", self.version)
    }

    /// The package's file.
    fn archive(&self) -> String {
        format!("{}.nupkg", self.name())
    }

    fn url(&self) -> String {
        format!(
            "https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/{}/{}",
            self.version,
            self.archive()
        )
    }
}

pub(super) fn run(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    let into = target(context, args)?;
    let mut machine = Machine { env: &context.env };
    let cache = context.path("app/.cache");
    let result = prepare(&PINNED, &cache, &into, &mut machine, console.out);
    finish(result, console)
}

/// The folder `--into` names (`--into DIR` or `--into=DIR`): a relative one is
/// from the checkout's root, where the script this replaces ran.
fn target(context: &Context, args: &[OsString]) -> Result<PathBuf, Failure> {
    let dir = match args {
        [flag, dir] if flag == "--into" => Some(dir.as_os_str()),
        [joined] => joined
            .to_str()
            .and_then(|text| text.strip_prefix("--into="))
            .map(OsStr::new),
        _ => None,
    };
    dir.filter(|dir| !dir.is_empty())
        .map(|dir| context.root.join(dir))
        .ok_or_else(|| {
            Failure::Usage(
                "conpty takes --into DIR, the folder for the console host's files".into(),
            )
        })
}

/// Puts the console host's two files into `into`, the package fetched and
/// checked first when it is not in `cache`, and says each on `out`.
pub(super) fn prepare(
    package: &Package,
    cache: &Path,
    into: &Path,
    system: &mut dyn System,
    out: &mut dyn Write,
) -> Result<(), Error> {
    fs::create_dir_all(cache).map_err(files("make", cache))?;
    let archive = cache.join(package.archive());
    let bytes = match kept(package, &archive)? {
        Some(bytes) => bytes,
        None => fetch(package, cache, &archive, system, out)?,
    };
    // All read before any is written: a package without one of them leaves nothing.
    let unzipped = unzip(&archive, &bytes)?;
    fs::create_dir_all(into).map_err(files("make", into))?;
    for (named, contents) in unzipped {
        let path = into.join(named);
        fs::write(&path, contents).map_err(files("write", &path))?;
        writeln!(out, "conpty: {}", path.display())?;
    }
    Ok(())
}

/// The package in the cache, when it is there and is the pinned one. One that
/// is not is deleted, to be fetched again.
fn kept(package: &Package, archive: &Path) -> Result<Option<Vec<u8>>, Error> {
    let bytes = match fs::read(archive) {
        Ok(bytes) => bytes,
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(cause) => return Err(files("read", archive)(cause)),
    };
    if sha256(&bytes) == package.sha256 {
        return Ok(Some(bytes));
    }
    fs::remove_file(archive).map_err(files("remove", archive))?;
    Ok(None)
}

/// Fetches the package into the cache and checks it. A download cut short is a
/// file that fails its check the next time, and is fetched again then.
fn fetch(
    package: &Package,
    cache: &Path,
    archive: &Path,
    system: &mut dyn System,
    out: &mut dyn Write,
) -> Result<Vec<u8>, Error> {
    let url = package.url();
    writeln!(out, "fetching {url}")?;
    let curl = Invocation::new("curl", cache)
        .args(["-fsSL", "-o"])
        .arg(archive)
        .arg(&url);
    ran(system, &curl)?;
    let bytes = fs::read(archive).map_err(files("read", archive))?;
    let actual = sha256(&bytes);
    if actual != package.sha256 {
        fs::remove_file(archive).map_err(files("remove", archive))?;
        return Err(Error::NotThePackage {
            archive: archive.to_path_buf(),
            actual,
            pinned: package.sha256.to_string(),
        });
    }
    Ok(bytes)
}

/// The SHA-256 of `bytes` in hex, as nuget.org shows one.
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The files the app takes from the package `bytes`, each with the name it has
/// beside the app. `archive` is where the package is, for what is said.
fn unzip(archive: &Path, bytes: &[u8]) -> Result<Vec<(&'static str, Vec<u8>)>, Error> {
    let unreadable = |cause: String| Error::Unzip {
        archive: archive.to_path_buf(),
        cause,
    };
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|cause| unreadable(told(&cause)))?;
    FILES
        .iter()
        .map(|&(inside, named)| {
            let mut entry = zip.by_name(inside).map_err(|cause| match cause {
                ZipError::FileNotFound => Error::NotInThePackage {
                    archive: archive.to_path_buf(),
                    inside,
                },
                other => unreadable(told(&other)),
            })?;
            let mut contents = Vec::new();
            entry
                .read_to_end(&mut contents)
                .map_err(|cause| unreadable(format!("{inside}: {cause}")))?;
            Ok((named, contents))
        })
        .collect()
}

/// What went wrong in the zip, in words: its i/o error says "i/o error" and
/// keeps what the system said as its source.
fn told(error: &ZipError) -> String {
    match error {
        ZipError::Io(cause) => cause.to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
