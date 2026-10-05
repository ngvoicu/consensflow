//! The portable Windows app is one file: the app's own exe, then the runtime
//! it needs (Node, the CLI, and the terminals' console host) as a payload,
//! then a footer that finds it.
//! `app/scripts/portable.mjs` packs it; this module reads and unpacks it. The
//! layout, written down here once:
//!
//! ```text
//! ConsensFlow_<version>_x64-portable.exe
//!   the built ConsensFlow.exe, byte for byte
//!   the payload: a gzip-compressed tar of node.exe, cli/, conpty.dll and
//!     OpenConsole.exe
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

/// The runtime `exe` carries, unpacked under `root` by its first start and
/// reused by every later one; `None` when it carries none. Every other
/// runtime under `root` goes, best effort.
pub(crate) fn unpacked_runtime(
    exe: &Path,
    root: &Path,
    version: &str,
) -> Result<Option<PathBuf>, String> {
    runtime(exe, root, version)
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
/// what an unpack that stopped left behind. One whose node.exe runs stays,
/// whole, for its daemon: Windows does not let a running program be opened
/// for writing.
fn remove_other_runtimes(root: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path != keep && !node_runs(&path) {
            remove_aside(root, &path);
        }
    }
}

/// Whether `folder`'s node.exe cannot be opened for writing: on Windows,
/// because it runs.
fn node_runs(folder: &Path) -> bool {
    let node = folder.join("node.exe");
    node.is_file() && OpenOptions::new().append(true).open(node).is_err()
}

/// Moves `path` into a folder of its own under `root`, then removes that
/// folder: what goes is never left half there, and what Windows holds on to
/// stays where it was.
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
mod tests {
    use super::*;
    use std::sync::Arc;

    use flate2::write::GzEncoder;
    use flate2::Compression;

    /// A runtime as the packer packs it: node.exe and the CLI, in a
    /// gzip-compressed tar.
    fn payload(node: &[u8]) -> Vec<u8> {
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        let cli: &[(&str, &[u8])] = &[
            ("node.exe", node),
            ("cli/bin/cf.mjs", b"the cli"),
            ("cli/src/core/daemon.js", b"the core"),
        ];
        for (path, body) in cli {
            let mut header = tar::Header::new_ustar();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            tar.append_data(&mut header, path, *body)
                .expect("add a file");
        }
        tar.into_inner()
            .expect("end the tar")
            .finish()
            .expect("end the gzip stream")
    }

    /// An exe as the packer writes it: the app, its payload, the footer.
    fn packed(dir: &Path, payload: &[u8]) -> PathBuf {
        let exe = dir.join("ConsensFlow_9.9.9_x64-portable.exe");
        let mut bytes = b"MZ the app".to_vec();
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes.extend_from_slice(b"CFPAYLD1");
        fs::write(&exe, bytes).expect("write the exe");
        exe
    }

    fn crc_of(payload: &[u8]) -> u32 {
        let mut tar = Vec::new();
        GzDecoder::new(payload)
            .read_to_end(&mut tar)
            .expect("gunzip the payload");
        let mut crc = flate2::Crc::new();
        crc.update(&tar);
        crc.sum()
    }

    fn entries(root: &Path) -> Vec<String> {
        let mut names = fs::read_dir(root)
            .expect("read the runtime root")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .into_string()
                    .expect("a name")
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn the_footer_finds_the_payload_and_names_its_folder_by_crc() {
        let dir = tempfile::tempdir().expect("dir");
        let payload = payload(b"node");
        let exe = packed(dir.path(), &payload);
        let found = Payload::find(&mut File::open(&exe).expect("open"))
            .expect("read")
            .expect("a payload");
        assert_eq!(
            found,
            Payload {
                offset: 10,
                length: payload.len() as u64,
                crc: crc_of(&payload),
            }
        );
        assert_eq!(
            found.folder("9.9.9"),
            format!("9.9.9-{:08x}", crc_of(&payload))
        );
    }

    /// The installed app, and the Mac's, carry nothing: they find their
    /// runtime beside them, and nothing is unpacked.
    #[test]
    fn an_exe_without_the_footer_carries_no_runtime() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().join("runtime");
        for bytes in [
            &b"MZ"[..],
            &b"MZ an installed app, longer than a footer"[..],
        ] {
            let exe = dir.path().join("ConsensFlow.exe");
            fs::write(&exe, bytes).expect("write");
            assert_eq!(unpacked_runtime(&exe, &root, "9.9.9"), Ok(None));
        }
        assert!(!root.exists());
    }

    /// A footer naming more payload than the file holds, or less than any
    /// gzip stream, is a damaged exe, not one that carries nothing.
    #[test]
    fn a_footer_that_cannot_be_right_is_an_error() {
        let dir = tempfile::tempdir().expect("dir");
        let exe = dir.path().join("ConsensFlow.exe");
        for length in [5_u64, 1_000] {
            let mut bytes = b"MZ the app and some".to_vec();
            bytes.extend_from_slice(&length.to_le_bytes());
            bytes.extend_from_slice(b"CFPAYLD1");
            fs::write(&exe, bytes).expect("write");
            let error = unpacked_runtime(&exe, &dir.path().join("runtime"), "9.9.9")
                .expect_err("a damaged exe");
            assert!(
                error.starts_with("the bundled runtime could not be unpacked: its footer names"),
                "{error}"
            );
        }
    }

    /// The first start unpacks node.exe and the CLI, marker last; a later
    /// start reuses the folder without reading the payload at all.
    #[test]
    fn the_first_start_unpacks_the_runtime_and_later_ones_reuse_it() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().join("runtime");
        let mut payload = payload(b"node");
        let exe = packed(dir.path(), &payload);

        let folder = unpacked_runtime(&exe, &root, "9.9.9")
            .expect("unpacked")
            .expect("a runtime");
        assert_eq!(folder, root.join(format!("9.9.9-{:08x}", crc_of(&payload))));
        assert_eq!(fs::read(folder.join("node.exe")).expect("node"), b"node");
        assert_eq!(
            fs::read(folder.join("cli/bin/cf.mjs")).expect("cli"),
            b"the cli"
        );
        assert!(folder.join(".unpacked").is_file());
        assert_eq!(
            entries(&root),
            [folder.file_name().unwrap().to_str().unwrap()]
        );

        // Its compressed bytes damaged, its trailer kept: only a start that
        // unpacked again would notice.
        payload[12] ^= 0xff;
        let exe = packed(dir.path(), &payload);
        assert_eq!(unpacked_runtime(&exe, &root, "9.9.9"), Ok(Some(folder)));
    }

    /// gzip's CRC checks the payload: one whose tar does not match the CRC in
    /// its trailer unpacks nothing and leaves nothing behind.
    #[test]
    fn a_payload_that_does_not_match_its_crc_unpacks_nothing() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().join("runtime");
        let mut payload = payload(b"node");
        let trailer = payload.len() - 8;
        payload[trailer] ^= 0xff;
        let exe = packed(dir.path(), &payload);

        let error = unpacked_runtime(&exe, &root, "9.9.9").expect_err("a damaged payload");
        assert!(
            error.starts_with("the bundled runtime could not be unpacked:"),
            "{error}"
        );
        assert_eq!(entries(&root), Vec::<String>::new());
    }

    /// Copies of the app started at once each unpack, and the first to
    /// finish names the folder; the others use it.
    #[test]
    fn copies_started_at_once_share_one_runtime() {
        let dir = tempfile::tempdir().expect("dir");
        let root = Arc::new(dir.path().join("runtime"));
        let exe = Arc::new(packed(dir.path(), &payload(&vec![7; 512 * 1024])));
        let copies = (0..4)
            .map(|_| {
                let (exe, root) = (Arc::clone(&exe), Arc::clone(&root));
                thread::spawn(move || unpacked_runtime(&exe, &root, "9.9.9"))
            })
            .collect::<Vec<_>>();
        let folders = copies
            .into_iter()
            .map(|copy| {
                copy.join()
                    .expect("a copy")
                    .expect("unpacked")
                    .expect("a runtime")
            })
            .collect::<Vec<_>>();

        assert!(folders.iter().all(|folder| *folder == folders[0]));
        assert!(folders[0].join(".unpacked").is_file());
        assert_eq!(
            fs::read(folders[0].join("node.exe")).expect("node").len(),
            512 * 1024
        );
        assert_eq!(
            entries(&root),
            [folders[0].file_name().unwrap().to_str().unwrap()]
        );
    }

    /// An unpacked runtime that cannot take its name for a moment (on
    /// Windows, while an antivirus scans it) takes it once it can.
    #[cfg(unix)]
    #[test]
    fn a_runtime_denied_its_name_for_a_moment_is_placed_once_it_can_be() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().join("runtime");
        fs::create_dir_all(root.join(".unpacking-done")).expect("an unpacked runtime");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).expect("deny");
        let allowed = {
            let root = root.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(200));
                fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).expect("allow");
            })
        };
        let started = Instant::now();
        let placed = place(&root.join(".unpacking-done"), &root.join("9.9.9-00000001"));
        allowed.join().expect("allowed");

        placed.expect("placed");
        assert!(started.elapsed() >= Duration::from_millis(200));
        assert_eq!(entries(&root), ["9.9.9-00000001"]);
    }

    /// A folder under the runtime's name without its marker, which no start
    /// of the app leaves, is unpacked again.
    #[test]
    fn a_runtime_folder_without_its_marker_is_unpacked_again() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().join("runtime");
        let payload = payload(b"node");
        let exe = packed(dir.path(), &payload);
        let damaged = root.join(format!("9.9.9-{:08x}", crc_of(&payload)));
        fs::create_dir_all(damaged.join("cli")).expect("a damaged runtime");
        fs::write(damaged.join("stray"), b"left").expect("a stray file");

        let folder = unpacked_runtime(&exe, &root, "9.9.9")
            .expect("unpacked")
            .expect("a runtime");
        assert_eq!(folder, damaged);
        assert!(folder.join(".unpacked").is_file());
        assert!(folder.join("node.exe").is_file());
        assert!(!folder.join("stray").exists());
        assert_eq!(
            entries(&root),
            [folder.file_name().unwrap().to_str().unwrap()]
        );
    }

    /// Older runtimes, and what a stopped unpack left, go once this one is in
    /// place; a runtime whose node.exe cannot be written, as a running one
    /// cannot on Windows, stays whole.
    #[test]
    fn other_runtimes_go_but_one_whose_node_runs_stays() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().join("runtime");
        for old in ["9.9.8-00000001", "9.9.8-00000002", ".unpacking-stopped"] {
            fs::create_dir_all(root.join(old).join("cli")).expect("an old runtime");
            fs::write(root.join(old).join("node.exe"), b"old node").expect("its node");
        }
        let running = root.join("9.9.8-00000002").join("node.exe");
        let mut permissions = fs::metadata(&running).expect("node").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&running, permissions.clone()).expect("read-only node");

        let exe = packed(dir.path(), &payload(b"node"));
        let folder = unpacked_runtime(&exe, &root, "9.9.9")
            .expect("unpacked")
            .expect("a runtime");
        let kept = entries(&root);
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&running, permissions).expect("writable again");

        assert_eq!(
            kept,
            [
                "9.9.8-00000002",
                folder.file_name().unwrap().to_str().unwrap()
            ]
        );
        assert!(
            root.join("9.9.8-00000002").join("cli").is_dir(),
            "left whole"
        );
    }
}
