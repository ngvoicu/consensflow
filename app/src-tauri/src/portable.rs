//! The portable Windows app is one file: the app's own exe, then the runtime
//! it needs (the `cf` the daemon and every window run, and the terminals'
//! console host) as a payload, then a footer that finds it.
//! `app/scripts/portable.mjs` packs it; this module reads and unpacks it. The
//! layout, written down here once:
//!
//! ```text
//! ConsensFlow_<version>_x64-portable.exe
//!   the built ConsensFlow.exe, byte for byte
//!   the payload: a gzip-compressed tar of cli/ (its bin/cf.exe), conpty.dll,
//!     OpenConsole.exe and OpenConsole-LICENSE.txt
//!   the footer, 16 bytes: the payload's length in bytes, as an unsigned
//!     64-bit little-endian integer, then the tag "CFPAYLD1"
//! ```
//!
//! An exe that ends with the tag carries its runtime; one that does not (the
//! installed app, the Mac's) finds its runtime beside it, as before. The
//! first start unpacks the payload into `<root>/<version>-<crc>`, the crc
//! being the CRC32 of the tar, in eight hex digits, from the gzip trailer: a
//! folder per build, which every later start reuses once the marker written
//! last into it says it is complete. gzip's own CRC checks the payload as it
//! is unpacked.
//!
//! `<root>` is [`RUNTIME_PARENT`] under the app's local data folder
//! (`%LOCALAPPDATA%\<identifier>`). The apps before the flip release kept
//! their runtimes in `runtime` there, and each start of theirs removed every
//! runtime in it whose `node.exe` was not running, which a runtime whose
//! daemon is the native `cf.exe` never is. A collector already out cannot be
//! taught, so the runtimes of this app, and of those after it, live in a
//! folder of their own, out of its reach, where every start keeps a runtime
//! while a program of it runs ([`PROGRAMS`]). The old folder is left as it
//! is.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use flate2::read::GzDecoder;

/// The footer's tag: the last eight bytes of an exe that carries its runtime.
const TAG: &[u8; 8] = b"CFPAYLD1";
/// The footer: the payload's length, then the tag.
const FOOTER_BYTES: u64 = 16;
/// The smallest gzip stream there is: its header and its trailer.
const SMALLEST_GZIP: u64 = 18;
/// Written last into a runtime folder: everything else is there.
const MARKER: &str = ".unpacked";
/// Where, under the app's local data folder, the runtimes are unpacked: not
/// `runtime`, which is the apps before the flip release's, and which their
/// collector empties of every runtime whose `node.exe` is not running.
const RUNTIME_PARENT: &str = "portable-runtime";
/// The programs of a runtime that run on their own, as the parts of their
/// paths in it: `node.exe`, of which this release's payload has none, and
/// `cf.exe`, which is the daemon and every window's `cf`. The runtimes of the
/// flip release share this parent with this release's, and the daemon of one
/// is Node's in a home that took the way back: a start of this release must
/// not take such a runtime from under it, so a runtime whose `node.exe` runs
/// stays as one whose `cf.exe` does. Dropped when no flip release can be
/// running.
const PROGRAMS: [&[&str]; 2] = [&["node.exe"], &["cli", "bin", "cf.exe"]];

/// Where an exe carries its runtime.
#[derive(Debug, PartialEq)]
struct Payload {
    offset: u64,
    length: u64,
    /// The CRC32 of the tar inside, from the gzip trailer.
    crc: u32,
}

impl Payload {
    /// The payload `file` carries, read from its footer; `None` when it does
    /// not end with the tag.
    fn find(file: &mut File) -> io::Result<Option<Self>> {
        let size = file.metadata()?.len();
        if size < FOOTER_BYTES {
            return Ok(None);
        }
        let mut footer = [0; FOOTER_BYTES as usize];
        file.seek(SeekFrom::Start(size - FOOTER_BYTES))?;
        file.read_exact(&mut footer)?;
        let (length, tag) = footer.split_at(8);
        if tag != TAG {
            return Ok(None);
        }
        let length = u64::from_le_bytes(length.try_into().expect("eight bytes"));
        if length < SMALLEST_GZIP || length > size - FOOTER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("its footer names {length} bytes of payload in a file of {size}"),
            ));
        }
        let offset = size - FOOTER_BYTES - length;
        let mut crc = [0; 4];
        file.seek(SeekFrom::Start(offset + length - 8))?;
        file.read_exact(&mut crc)?;
        Ok(Some(Self {
            offset,
            length,
            crc: u32::from_le_bytes(crc),
        }))
    }

    /// The runtime folder's name: this version's, and this payload's.
    fn folder(&self, version: &str) -> String {
        format!("{version}-{:08x}", self.crc)
    }
}

/// The runtime `exe` carries, unpacked under `local_data`'s
/// [`RUNTIME_PARENT`] by its first start and reused by every later one;
/// `None` when it carries none. Every other runtime there goes, best effort,
/// but one that runs.
pub(crate) fn unpacked_runtime(
    exe: &Path,
    local_data: &Path,
    version: &str,
) -> Result<Option<PathBuf>, String> {
    runtime(exe, &local_data.join(RUNTIME_PARENT), version)
        .map_err(|error| format!("the bundled runtime could not be unpacked: {error}"))
}

fn runtime(exe: &Path, root: &Path, version: &str) -> io::Result<Option<PathBuf>> {
    let mut file = File::open(exe)?;
    let Some(payload) = Payload::find(&mut file)? else {
        return Ok(None);
    };
    let folder = root.join(payload.folder(version));
    if !complete(&folder) {
        unpack(&mut file, &payload, root, &folder)?;
    }
    remove_other_runtimes(root, &folder);
    Ok(Some(folder))
}

fn complete(folder: &Path) -> bool {
    folder.join(MARKER).is_file()
}

/// Unpacks into a sibling of `folder`, which takes `folder`'s name once its
/// marker is written: `folder` is never there half unpacked. A copy of the
/// app that started at the same time may get there first, and this one uses
/// its folder.
fn unpack(file: &mut File, payload: &Payload, root: &Path, folder: &Path) -> io::Result<()> {
    fs::create_dir_all(root)?;
    let staging = tempfile::Builder::new()
        .prefix(".unpacking-")
        .tempdir_in(root)?;
    let unpacked = extract(file, payload, staging.path())
        .and_then(|()| File::create(staging.path().join(MARKER)).map(drop));
    if let Err(error) = unpacked {
        return if complete(folder) { Ok(()) } else { Err(error) };
    }
    let staged = staging.keep();
    let mut placed = place(&staged, folder);
    if placed.is_err() && !complete(folder) {
        // No start of the app leaves a folder without its marker; something
        // else damaged this one, and it goes.
        remove_aside(root, folder);
        placed = place(&staged, folder);
    }
    if placed.is_err() {
        let _ = fs::remove_dir_all(&staged);
    }
    if complete(folder) {
        Ok(())
    } else {
        placed
    }
}

/// Renames `staged` to `folder`, and again for a few seconds while Windows
/// denies it: an antivirus scanning what was just unpacked holds on to it for
/// a moment. A `folder` already there ends it at once: Windows denies a
/// rename onto a folder too, and that one waits on nothing.
fn place(staged: &Path, folder: &Path) -> io::Result<()> {
    let mut wait = Duration::from_millis(50);
    loop {
        match fs::rename(staged, folder) {
            Err(error)
                if error.kind() == io::ErrorKind::PermissionDenied
                    && wait <= Duration::from_secs(2)
                    && !folder.exists() =>
            {
                thread::sleep(wait);
                wait *= 2;
            }
            placed => return placed,
        }
    }
}

/// The payload's tar, into `into`: its files and folders, nothing else.
/// Reading the gzip stream to its end checks its CRC, which unpacking alone
/// would not: tar stops reading at the archive's end marker.
fn extract(file: &mut File, payload: &Payload, into: &Path) -> io::Result<()> {
    file.seek(SeekFrom::Start(payload.offset))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file.by_ref().take(payload.length)));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() {
            continue;
        }
        if !(kind.is_file() || kind.is_dir()) || !entry.unpack_in(into)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the payload holds a link, or a path outside its folder",
            ));
        }
    }
    io::copy(&mut archive.into_inner(), &mut io::sink())?;
    Ok(())
}

/// Every runtime under `root` but `keep` goes, best effort: an older one, and
/// what an unpack that stopped left behind. One of which a program runs
/// ([`runs`]) stays, whole, for its daemon and its windows.
fn remove_other_runtimes(root: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path != keep && !runs(&path) {
            remove_aside(root, &path);
        }
    }
}

/// Whether a program of `folder`'s runtime, its `node.exe` or its
/// `cli/bin/cf.exe`, cannot be opened for writing: on Windows, because it
/// runs. For writing, and not for appending: Windows refuses a program that
/// runs the right to write its data (`FILE_WRITE_DATA`, which `write` asks
/// for) and grants it the right to append (`FILE_APPEND_DATA`, which is all
/// `append` asks for: std takes `FILE_WRITE_DATA` out of it). An open for
/// appending succeeds on a program that runs, and so tells nothing of it.
fn runs(folder: &Path) -> bool {
    PROGRAMS.iter().any(|parts| {
        let program = parts
            .iter()
            .fold(folder.to_path_buf(), |path, part| path.join(part));
        program.is_file() && OpenOptions::new().write(true).open(program).is_err()
    })
}

/// Moves `path` into a folder of its own under `root`, then removes that
/// folder: what goes is never left half there, and what Windows holds on to
/// stays where it was. A program that runs is not held so: its folder moves,
/// and the removal deletes all of it but the program. [`runs`] is what keeps
/// such a folder, not this.
fn remove_aside(root: &Path, path: &Path) {
    let Ok(aside) = tempfile::Builder::new()
        .prefix(".removing-")
        .tempdir_in(root)
    else {
        return;
    };
    let _ = fs::rename(path, aside.path().join("removed"));
    // `aside` goes when it drops, with whatever was moved into it.
}

/// Names the runtime folder to Windows' search for libraries: the terminals'
/// console host is there (`conpty.dll`, which runs `OpenConsole.exe` beside
/// it; `app/scripts/conpty.mjs`), and Windows looks beside the exe first,
/// which for a portable exe is wherever it was saved. Called before the
/// first terminal opens, which loads the console host once for the app.
#[cfg(windows)]
pub(crate) fn find_libraries_in(runtime: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::SetDllDirectoryW;

    let wide: Vec<u16> = runtime
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is a NUL-terminated wide string that outlives the call,
    // which copies it.
    if unsafe { SetDllDirectoryW(wide.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
