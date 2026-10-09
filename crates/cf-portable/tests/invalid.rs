//! A payload that is not as it should be: cut short, not what its trailer
//! says, or holding an entry that could land where it should not. Each is
//! refused when the payload is unpacked, in words that say what is wrong, and
//! nothing is put outside the folder it is unpacked into.
#![allow(clippy::expect_used)]

mod common;

use std::fs;
use std::io::{self, Cursor};
use std::path::Path;

use cf_portable::{pack, Error, Payload};
use common::Entry;
use tar::EntryType;

/// An exe as `pack` writes it from the fake release folder: three bytes of app,
/// then the payload and the footer.
fn packed_exe() -> Vec<u8> {
    let dir = tempfile::tempdir().expect("dir");
    let release = dir.path().join("release");
    common::release(&release, &[]);
    let out = dir.path().join("out.exe");
    pack(&release.join("ConsensFlow.exe"), &release, &out).expect("packed");
    fs::read(out).expect("the exe")
}

/// What the app does with an exe that carries a payload: find it and unpack it.
fn unpack(bytes: &[u8], into: &Path) -> Result<(), Error> {
    let mut file = Cursor::new(bytes);
    let payload = Payload::find(&mut file)?.expect("an exe that carries a payload");
    payload.extract(&mut file, into)
}

#[test]
fn a_gzip_stream_cut_short_under_a_footer_that_fits_it_is_refused_when_unpacked() {
    let packed = packed_exe();
    let payload = common::payload_of(&packed, 3);
    let bytes = common::exe(common::APP, &payload[..payload.len() - 20]);
    let dir = tempfile::tempdir().expect("dir");

    // The footer is right about what is there; only reading it all tells.
    assert!(Payload::find(&mut Cursor::new(&bytes))
        .expect("read")
        .is_some());
    let refusal = unpack(&bytes, dir.path()).expect_err("cut short");

    assert!(matches!(refusal, Error::Io(_)), "{refusal}");
}

/// gzip's trailer is the CRC32 of the tar, then its length. The footer finds the
/// trailer and does not check it: an exe whose trailer lies is found as any
/// other, and reading the payload to its end is what refuses it.
#[test]
fn a_payload_that_does_not_match_its_crc_or_its_length_is_refused_when_unpacked() {
    for (what, from_the_end) in [("crc", 8_usize), ("length", 4)] {
        let mut bytes = packed_exe();
        let at = bytes.len() - 16 - from_the_end;
        bytes[at] ^= 0xff;
        let dir = tempfile::tempdir().expect("dir");

        assert!(
            Payload::find(&mut Cursor::new(&bytes))
                .expect("read")
                .is_some(),
            "{what}: still found"
        );
        let refusal = unpack(&bytes, dir.path()).expect_err(what);

        assert!(matches!(refusal, Error::Io(_)), "{what}: {refusal}");
    }
}

/// Its compressed bytes damaged and its trailer kept: found as any other.
#[test]
fn compressed_bytes_that_are_damaged_under_a_trailer_that_is_kept_are_refused_when_unpacked() {
    let mut bytes = packed_exe();
    // The app is three bytes, the gzip header ten: the third byte of the stream.
    bytes[3 + 12] ^= 0xff;
    let dir = tempfile::tempdir().expect("dir");

    assert!(Payload::find(&mut Cursor::new(&bytes))
        .expect("read")
        .is_some());

    assert!(unpack(&bytes, dir.path()).is_err());
}

/// A payload of `entries` unpacked into a folder of its own in a fresh
/// directory: what came of it, and the directory to look in.
fn unpacked(entries: &[Entry]) -> (Result<(), Error>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let into = dir.path().join("into");
    fs::create_dir(&into).expect("the folder");
    let bytes = common::exe(common::APP, &common::tar_gz(entries));
    (unpack(&bytes, &into), dir)
}

fn is_empty(folder: &Path) -> bool {
    fs::read_dir(folder).expect("read the folder").count() == 0
}

/// An absolute path the tar crate would unpack under the folder, its root taken
/// off; a payload that holds one is damaged, and is refused.
const ABSOLUTE: &str = if cfg!(windows) {
    "C:\\cf-portable-escape.txt"
} else {
    "/cf-portable-escape.txt"
};

#[test]
fn an_entry_with_a_path_outside_the_folder_is_refused_and_nothing_is_unpacked_for_it() {
    let mut names = vec![
        ABSOLUTE,
        "../escape.txt",
        "cli/../../escape.txt",
        "cli/bin/../../../escape.txt",
    ];
    if cfg!(windows) {
        names.extend(["..\\escape.txt", "C:escape.txt", "\\escape.txt"]);
    }
    for name in names {
        let (result, dir) = unpacked(&[Entry::file(name, b"escaped")]);

        let refusal = result.expect_err(name);

        assert!(
            matches!(&refusal, Error::Outside { path } if path == name),
            "{name}: {refusal}"
        );
        assert_eq!(
            refusal.to_string(),
            format!("the payload holds {name}, which is outside the folder it unpacks into")
        );
        assert!(!dir.path().join("escape.txt").exists(), "{name}");
        assert!(
            is_empty(&dir.path().join("into")),
            "{name}: refused, not unpacked inside"
        );
    }
}

/// A link is how a payload reaches a place it names only later; the reader
/// takes files and folders and nothing else.
#[test]
fn an_entry_that_is_not_a_plain_file_or_folder_is_refused() {
    for (kind, name, points_to) in [
        (EntryType::Symlink, "cli/link", Some("../../outside")),
        (EntryType::Link, "cli/hard", Some("conpty.dll")),
        (EntryType::Char, "cli/tty", None),
        (EntryType::Block, "cli/disk", None),
        (EntryType::Fifo, "cli/pipe", None),
    ] {
        let entry = match points_to {
            Some(target) => Entry::of(kind, name).pointing_to(target),
            None => Entry::of(kind, name),
        };

        let (result, dir) = unpacked(&[Entry::file("conpty.dll", b"conpty"), entry]);

        let refusal = result.expect_err(name);
        assert!(
            matches!(&refusal, Error::NotPlain { path } if path == name),
            "{name}: {refusal}"
        );
        assert_eq!(
            refusal.to_string(),
            format!("{name} is not a plain file or folder, which is all a payload holds")
        );
        assert!(
            fs::symlink_metadata(dir.path().join("into").join(name)).is_err(),
            "{name} is not there"
        );
    }
}

/// bsdtar's output may begin with a global header of pax, and a name may begin
/// with `./`: neither is a reason to refuse a payload.
#[test]
fn a_global_header_of_pax_and_names_that_begin_with_a_dot_are_unpacked() {
    let (result, dir) = unpacked(&[
        Entry {
            name: "pax_global_header",
            kind: EntryType::XGlobalHeader,
            body: b"25 comment=a global note\n",
            link: None,
        },
        Entry::of(EntryType::Directory, "./cli/"),
        Entry::file("./cli/bin/cf.exe", b"cf"),
        Entry::file("./conpty.dll", b"conpty"),
    ]);

    result.expect("unpacked");

    assert_eq!(
        common::files_under(&dir.path().join("into")),
        [
            ("cli/bin/cf.exe".to_string(), b"cf".to_vec()),
            ("conpty.dll".to_string(), b"conpty".to_vec())
        ]
    );
}

#[test]
fn an_error_becomes_an_io_error_with_the_same_words() {
    let damaged = Error::Footer {
        length: 7,
        size: 20,
    };
    let words = damaged.to_string();
    let as_io = io::Error::from(damaged);
    assert_eq!(as_io.kind(), io::ErrorKind::InvalidData);
    assert_eq!(as_io.to_string(), words);

    let denied = Error::Io(io::Error::from(io::ErrorKind::PermissionDenied));
    let words = denied.to_string();
    let as_io = io::Error::from(denied);
    assert_eq!(as_io.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(as_io.to_string(), words);
}
