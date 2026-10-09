//! What the tests of the portable exe share.
//!
//! An exe is laid out here by hand, byte by byte, and its tar read with the tar
//! crate itself: a test that packed and read with the library alone could not
//! tell its layout from the one the shipped app reads.
// Each test file uses some of this, and not all of it.
#![allow(dead_code)]
#![allow(clippy::expect_used)]

use std::fs;
use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::{Compression, Crc};

/// The app's exe in the release folder `release` makes.
pub const APP: &[u8] = b"app";

/// What the runtime of that release folder unpacks to, by path (with `/`) and
/// body: the `cf`, the console host and its license.
pub const RUNTIME: [(&str, &str); 4] = [
    ("OpenConsole-LICENSE.txt", "MIT"),
    ("OpenConsole.exe", "openconsole"),
    ("cli/bin/cf.exe", "cf"),
    ("conpty.dll", "conpty"),
];

/// A release folder as `tauri build` leaves it on Windows, build leftovers
/// included, less the pieces named in `missing`: the test helper is there only
/// when someone built it. The terminals' console host and its license are the
/// bundle's resources there.
pub fn release(dir: &Path, missing: &[&str]) {
    let files = [
        ("ConsensFlow.exe", "app"),
        ("consensflow-bridge.exe", "bridge"),
        ("cli/bin/cf.exe", "cf"),
        ("conpty.dll", "conpty"),
        ("OpenConsole.exe", "openconsole"),
        ("OpenConsole-LICENSE.txt", "MIT"),
        ("app.pdb", "debug"),
        ("deps/app.d", "dep"),
        ("nsis/installer.nsi", "nsis"),
    ];
    for (path, body) in files {
        if missing.contains(&path) {
            continue;
        }
        let file = dir.join(path);
        fs::create_dir_all(file.parent().expect("a folder")).expect("make the folder");
        fs::write(file, body).expect("write the file");
    }
}

/// An exe laid out by hand: the app, the payload, and the footer, which is the
/// payload's length as eight little-endian bytes, then the tag.
pub fn exe(app: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut bytes = app.to_vec();
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(b"CFPAYLD1");
    bytes
}

/// The payload of a packed exe that begins with `app_len` bytes of the app: all
/// but those and the sixteen of the footer.
pub fn payload_of(exe: &[u8], app_len: usize) -> &[u8] {
    &exe[app_len..exe.len() - 16]
}

pub fn gunzip(payload: &[u8]) -> Vec<u8> {
    let mut tar = Vec::new();
    GzDecoder::new(payload)
        .read_to_end(&mut tar)
        .expect("gunzip the payload");
    tar
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = Crc::new();
    crc.update(bytes);
    crc.sum()
}

/// The paths a tar names, a folder's without the slash that ends it, in the
/// order they were written.
pub fn entry_names(tar: &[u8]) -> Vec<String> {
    tar::Archive::new(tar)
        .entries()
        .expect("read the tar")
        .map(|entry| {
            let entry = entry.expect("an entry");
            let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
            name.trim_end_matches('/').to_string()
        })
        .collect()
}

/// The paths of a tar's files (its folders left out), sorted.
pub fn file_names(tar: &[u8]) -> Vec<String> {
    let mut names = tar::Archive::new(tar)
        .entries()
        .expect("read the tar")
        .filter_map(|entry| {
            let entry = entry.expect("an entry");
            entry
                .header()
                .entry_type()
                .is_file()
                .then(|| String::from_utf8_lossy(&entry.path_bytes()).into_owned())
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// Every file under `root`, by its path from there (with `/`) and its bytes,
/// sorted by path.
pub fn files_under(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, folder: &Path, found: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(folder).expect("read the folder") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                walk(root, &path, found);
                continue;
            }
            let from_root = path.strip_prefix(root).expect("under the root");
            let name = from_root
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            found.push((name, fs::read(&path).expect("read the file")));
        }
    }
    let mut found = Vec::new();
    walk(root, root, &mut found);
    found.sort();
    found
}

/// An entry of a tar as `tar_gz` writes it, name and all.
pub struct Entry<'a> {
    pub name: &'a str,
    pub kind: tar::EntryType,
    pub body: &'a [u8],
    /// Where a link points.
    pub link: Option<&'a str>,
}

impl<'a> Entry<'a> {
    pub fn file(name: &'a str, body: &'a [u8]) -> Self {
        Self {
            name,
            kind: tar::EntryType::Regular,
            body,
            link: None,
        }
    }

    pub fn of(kind: tar::EntryType, name: &'a str) -> Self {
        Self {
            name,
            kind,
            body: b"",
            link: None,
        }
    }

    pub fn pointing_to(self, link: &'a str) -> Self {
        Self {
            link: Some(link),
            ..self
        }
    }
}

/// A gzip-compressed tar of `entries`, in the order given. Their names are
/// written into the headers as they are, which the tar crate's own builder
/// refuses to do for an absolute path or one with a `..` in it: this is how a
/// damaged or hostile payload is made.
pub fn tar_gz(entries: &[Entry]) -> Vec<u8> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for entry in entries {
        let mut header = tar::Header::new_ustar();
        let name = entry.name.as_bytes();
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_entry_type(entry.kind);
        header.set_size(entry.body.len() as u64);
        header.set_mode(if entry.kind.is_dir() { 0o755 } else { 0o644 });
        if let Some(link) = entry.link {
            header.set_link_name(link).expect("a link name");
        }
        header.set_cksum();
        tar.append(&header, entry.body).expect("add the entry");
    }
    tar.into_inner()
        .expect("end the tar")
        .finish()
        .expect("end the gzip stream")
}
