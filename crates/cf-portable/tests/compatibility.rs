//! The exe the JavaScript packer wrote, read by this library. A writer and a
//! reader can agree on the same mistake; bytes that the shipped packer made
//! cannot.
//!
//! `fixtures/javascript-packer-3.0.0-alpha.99.bin` was written once, by the
//! JavaScript packer that this library replaced (`app/scripts/portable.mjs`:
//! in the tree at 8c889331, deleted with the landing that moved the build to
//! `cargo xtask portable pack`), on macOS, whose `tar` (bsdtar) is what
//! Windows' own `tar.exe` is, the packer of the shipped exe:
//!
//! ```text
//! node app/scripts/portable.mjs --release R --out O --version 3.0.0-alpha.99
//! ```
//!
//! from a release folder `R` as `tauri build` leaves it, with these files
//! (their bodies in brackets): `ConsensFlow.exe` [app], `consensflow-bridge.exe`
//! [bridge], `cli/bin/cf.exe` [cf], `conpty.dll` [conpty], `OpenConsole.exe`
//! [openconsole], `OpenConsole-LICENSE.txt` [MIT], and the build's leftovers
//! `app.pdb` [debug], `deps/app.d` [dep], `nsis/installer.nsi` [nsis]. The
//! file is `O/ConsensFlow_3.0.0-alpha.99_x64-portable.exe`, kept as it is: the
//! times in its tar are the day it was made, so making it again would not give
//! the same bytes. It has NUL bytes in it (the footer's length, to begin with),
//! so git takes it for binary and never converts its line endings.
#![allow(clippy::expect_used)]

mod common;

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use cf_portable::{inspect, pack, Payload};

/// The version the packer was told (`--version`), which names its file.
const VERSION: &str = "3.0.0-alpha.99";

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("javascript-packer-3.0.0-alpha.99.bin")
}

/// The fixture, and where its payload is: `app` (three bytes), the payload,
/// the footer's sixteen.
fn bytes() -> Vec<u8> {
    fs::read(fixture()).expect("the fixture")
}

#[test]
fn the_footer_of_the_exe_the_javascript_packer_wrote_is_found() {
    let bytes = bytes();
    assert_eq!(&bytes[..3], common::APP, "the app first, byte for byte");
    assert_eq!(&bytes[bytes.len() - 8..], b"CFPAYLD1");

    let found = Payload::find(&mut Cursor::new(&bytes))
        .expect("read the footer")
        .expect("a payload");

    let payload = common::payload_of(&bytes, 3);
    let tar = common::gunzip(payload);
    assert_eq!(found.offset, 3);
    assert_eq!(found.length, payload.len() as u64);
    // The app names its runtime folder by this CRC, the tar's.
    assert_eq!(found.crc, common::crc32(&tar));
    assert_eq!(found.tar_length as usize, tar.len());
    assert_eq!(
        found.folder(VERSION),
        format!("{VERSION}-{:08x}", common::crc32(&tar))
    );
    assert_eq!(inspect(&fixture()).expect("inspected"), found);
}

#[test]
fn the_runtime_the_javascript_packer_wrote_unpacks_and_the_builds_leftovers_are_not_in_it() {
    let bytes = bytes();
    let dir = tempfile::tempdir().expect("dir");
    let mut file = Cursor::new(&bytes);
    let payload = Payload::find(&mut file)
        .expect("read the footer")
        .expect("a payload");

    payload.extract(&mut file, dir.path()).expect("unpacked");

    let unpacked = common::files_under(dir.path());
    let expected = common::RUNTIME
        .iter()
        .map(|(path, body)| (path.to_string(), body.as_bytes().to_vec()))
        .collect::<Vec<_>>();
    assert_eq!(unpacked, expected);
}

/// The packer of this library is the JavaScript packer's, entry for entry: the
/// same runtime, nothing else, folders where it had them.
#[test]
fn this_packer_writes_the_entries_the_javascript_packer_did() {
    let bytes = bytes();
    let theirs = common::entry_names(&common::gunzip(common::payload_of(&bytes, 3)));
    assert!(theirs.contains(&"cli/bin".to_string()), "{theirs:?}");

    let dir = tempfile::tempdir().expect("dir");
    let release = dir.path().join("release");
    common::release(&release, &[]);
    let out = dir.path().join("out.exe");
    pack(&release.join("ConsensFlow.exe"), &release, &out).expect("packed");
    let packed = fs::read(&out).expect("the exe");
    let ours = common::entry_names(&common::gunzip(common::payload_of(&packed, 3)));

    assert_eq!(ours, theirs);
}
