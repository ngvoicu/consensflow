//! Writing the exe: the app, the runtime as a gzip-compressed tar, the footer.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use flate2::Compression;

use crate::error::{files, Error};
use crate::format::{footer, inspect, Payload};

/// What of the build's release folder the payload holds, by the name each has
/// there: `cli`, a folder, whose `bin/cf.exe` is the daemon and every window's
/// `cf`; and the terminals' console host (`conpty.dll`, `OpenConsole.exe`) with
/// Microsoft's license for it.
const RUNTIME: [&str; 4] = [
    "cli",
    "conpty.dll",
    "OpenConsole.exe",
    "OpenConsole-LICENSE.txt",
];

/// A portable exe written by [`pack`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    /// The file written.
    pub path: PathBuf,
    /// The payload it carries, as its footer names it.
    pub payload: Payload,
}

/// Writes the portable exe at `out`: the app's `exe` byte for byte, then the
/// runtime from the build's `release` folder as a gzip-compressed tar, then the
/// footer. The folder `out` is in is made. Nothing is written when a piece of
/// the runtime is missing, and the file that is left is a whole one.
///
/// The file is read back for the answer, so that what is said of the payload is
/// what the app will find.
pub fn pack(exe: &Path, release: &Path, out: &Path) -> Result<Packed, Error> {
    check(exe, release)?;
    let payload = payload(release)?;
    write(exe, &payload, out)?;
    Ok(Packed {
        path: out.to_path_buf(),
        payload: inspect(out)?,
    })
}

/// What has to be there to pack the runtime: `cli` is a folder of the one file
/// `bin/cf.exe`, without which the app has no daemon to start and a window has
/// no `cf` in PowerShell.
fn required() -> impl Iterator<Item = PathBuf> {
    RUNTIME.into_iter().map(|name| {
        if name == "cli" {
            Path::new(name).join("bin").join("cf.exe")
        } else {
            PathBuf::from(name)
        }
    })
}

/// The app's exe, and then each piece of the runtime, are there: the first that
/// is not is the one the refusal names.
fn check(exe: &Path, release: &Path) -> Result<(), Error> {
    if !exe.is_file() {
        let piece = exe.file_name().map_or_else(|| exe.into(), PathBuf::from);
        let folder = exe
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        return Err(Error::Missing {
            piece,
            folder: folder.to_path_buf(),
        });
    }
    match required().find(|piece| !release.join(piece).is_file()) {
        Some(piece) => Err(Error::Missing {
            piece,
            folder: release.to_path_buf(),
        }),
        None => Ok(()),
    }
}

/// The runtime as a gzip-compressed tar.
fn payload(release: &Path) -> Result<Vec<u8>, Error> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for name in RUNTIME {
        append(&mut tar, Path::new(name), &release.join(name))?;
    }
    Ok(tar.into_inner()?.finish()?)
}

/// Adds `path` to the tar as `name`: a folder with all that is in it, by name,
/// so that one folder always packs to the same bytes. A file is a plain file,
/// and anything else (a socket, say) is refused, as the reader would.
fn append<W: Write>(tar: &mut tar::Builder<W>, name: &Path, path: &Path) -> Result<(), Error> {
    let kind = fs::metadata(path).map_err(files("read", path))?;
    if kind.is_dir() {
        tar.append_dir(name, path).map_err(files("pack", path))?;
        let mut children = fs::read_dir(path)
            .and_then(|entries| {
                entries
                    .map(|entry| entry.map(|entry| entry.file_name()))
                    .collect::<io::Result<Vec<_>>>()
            })
            .map_err(files("read", path))?;
        children.sort();
        for child in children {
            append(tar, &name.join(&child), &path.join(&child))?;
        }
    } else if kind.is_file() {
        tar.append_path_with_name(path, name)
            .map_err(files("pack", path))?;
    } else {
        return Err(Error::NotPlain {
            path: name.display().to_string(),
        });
    }
    Ok(())
}

/// The app's exe, the payload and the footer, in `out`. A file that could not be
/// written whole is removed: one that ends short of a footer would be taken for
/// an exe that carries no runtime.
fn write(exe: &Path, payload: &[u8], out: &Path) -> Result<(), Error> {
    if let Some(folder) = out.parent().filter(|folder| !folder.as_os_str().is_empty()) {
        fs::create_dir_all(folder).map_err(files("make", folder))?;
    }
    let mut app = File::open(exe).map_err(files("open", exe))?;
    let mut file = File::create(out).map_err(files("make", out))?;
    let written = io::copy(&mut app, &mut file)
        .and_then(|_| file.write_all(payload))
        .and_then(|()| file.write_all(&footer(payload.len() as u64)));
    drop(file);
    written.map_err(|cause| {
        let _ = fs::remove_file(out);
        files("write", out)(cause)
    })
}
