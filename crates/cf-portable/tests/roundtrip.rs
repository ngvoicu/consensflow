//! What `pack` writes, read back: by the library, and by hand.
#![allow(clippy::expect_used)]

mod common;

use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cf_portable::{inspect, pack, Error, Packed, Payload};

/// A fake release folder in `dir`, and the exe that `pack` makes of it, in a
/// folder of `dir` that `pack` makes.
fn packed(dir: &Path) -> (PathBuf, Packed) {
    let release = dir.join("release");
    common::release(&release, &[]);
    let out = dir
        .join("out")
        .join("ConsensFlow_3.0.0-alpha.99_x64-portable.exe");
    let packed = pack(&release.join("ConsensFlow.exe"), &release, &out).expect("packed");
    (out, packed)
}

/// The layout is `lib.rs`'s, read here with nothing of the library's.
#[test]
fn a_packed_exe_is_the_app_then_its_runtime_as_a_gzip_compressed_tar_then_the_length_and_the_tag() {
    let dir = tempfile::tempdir().expect("dir");
    let (out, _) = packed(dir.path());

    let exe = fs::read(out).expect("the exe");
    assert_eq!(&exe[..3], common::APP, "the app first, byte for byte");
    assert_eq!(&exe[exe.len() - 8..], b"CFPAYLD1");
    let length = u64::from_le_bytes(
        exe[exe.len() - 16..exe.len() - 8]
            .try_into()
            .expect("eight bytes"),
    ) as usize;
    assert_eq!(3 + length + 16, exe.len());

    let payload = &exe[3..3 + length];
    let tar = common::gunzip(payload);
    // The app names its runtime folder by this CRC, the tar's, from the gzip
    // trailer.
    let trailer = &payload[payload.len() - 8..];
    assert_eq!(
        u32::from_le_bytes(trailer[..4].try_into().expect("four bytes")),
        common::crc32(&tar)
    );
    assert_eq!(
        u32::from_le_bytes(trailer[4..].try_into().expect("four bytes")) as usize,
        tar.len()
    );
    // The `cf`, the console host and its license: no Node, and no sources.
    assert_eq!(
        common::file_names(&tar),
        [
            "OpenConsole-LICENSE.txt",
            "OpenConsole.exe",
            "cli/bin/cf.exe",
            "conpty.dll"
        ]
    );
}

#[test]
fn what_pack_says_of_the_payload_is_what_a_reader_finds() {
    let dir = tempfile::tempdir().expect("dir");
    let (out, packed) = packed(dir.path());

    assert_eq!(packed.path, out);
    assert_eq!(packed.payload.offset, 3);
    assert_eq!(inspect(&out).expect("inspected"), packed.payload);
    let found = Payload::find(&mut File::open(&out).expect("open"))
        .expect("read the footer")
        .expect("a payload");
    assert_eq!(found, packed.payload);
    let exe = fs::read(&out).expect("the exe");
    let tar = common::gunzip(common::payload_of(&exe, 3));
    assert_eq!(found.crc, common::crc32(&tar));
    assert_eq!(found.tar_length as usize, tar.len());
}

#[test]
fn the_runtime_unpacks_as_it_was_packed_and_the_builds_leftovers_are_not_in_it() {
    let dir = tempfile::tempdir().expect("dir");
    let (out, packed) = packed(dir.path());
    let unpacked = dir.path().join("unpacked");

    let mut file = File::open(&out).expect("open");
    packed
        .payload
        .extract(&mut file, &unpacked)
        .expect("unpacked");

    let expected = common::RUNTIME
        .iter()
        .map(|(path, body)| (path.to_string(), body.as_bytes().to_vec()))
        .collect::<Vec<_>>();
    assert_eq!(common::files_under(&unpacked), expected);
}

/// The sizes of a real build: a few megabytes of the app and of the `cf`, which
/// no fixture of a few bytes shows to be copied whole.
#[test]
fn a_runtime_of_the_size_of_a_real_one_survives_the_trip() {
    let dir = tempfile::tempdir().expect("dir");
    let release = dir.path().join("release");
    common::release(&release, &[]);
    let pattern = |length: usize, seed: usize| -> Vec<u8> {
        (0..length)
            .map(|at| ((at * 31 + seed * 7 + at / 251) % 256) as u8)
            .collect()
    };
    let app = pattern(3 * 1024 * 1024 + 5, 1);
    let cf = pattern(2 * 1024 * 1024 + 3, 2);
    fs::write(release.join("ConsensFlow.exe"), &app).expect("the app");
    fs::write(release.join("cli").join("bin").join("cf.exe"), &cf).expect("the cf");
    let out = dir.path().join("out.exe");

    let packed = pack(&release.join("ConsensFlow.exe"), &release, &out).expect("packed");

    let exe = fs::read(&out).expect("the exe");
    assert_eq!(&exe[..app.len()], &app[..], "the app, byte for byte");
    assert_eq!(packed.payload.offset, app.len() as u64);
    assert_eq!(
        exe.len() as u64,
        packed.payload.offset + packed.payload.length + 16
    );
    let unpacked = dir.path().join("unpacked");
    packed
        .payload
        .extract(&mut Cursor::new(&exe), &unpacked)
        .expect("unpacked");
    let files = common::files_under(&unpacked);
    let cf_back = files
        .iter()
        .find(|(path, _)| path == "cli/bin/cf.exe")
        .expect("the cf");
    assert_eq!(cf_back.1, cf);
}

/// `cli` is a folder of the one file in a real build, but it is packed as the
/// folder it is: all that is in it, folders and all, by name.
#[test]
fn a_folder_of_the_runtime_is_packed_whole_and_by_name() {
    let dir = tempfile::tempdir().expect("dir");
    let release = dir.path().join("release");
    common::release(&release, &[]);
    for (path, body) in [
        ("cli/README", "read me"),
        ("cli/bin/zz", "last"),
        ("cli/bin/aa", "first"),
        ("cli/lib/deeper/mid.txt", "mid"),
    ] {
        let file = release.join(path);
        fs::create_dir_all(file.parent().expect("a folder")).expect("make the folder");
        fs::write(file, body).expect("write the file");
    }
    let out = dir.path().join("out.exe");

    pack(&release.join("ConsensFlow.exe"), &release, &out).expect("packed");

    let exe = fs::read(&out).expect("the exe");
    assert_eq!(
        common::entry_names(&common::gunzip(common::payload_of(&exe, 3))),
        [
            "cli",
            "cli/README",
            "cli/bin",
            "cli/bin/aa",
            "cli/bin/cf.exe",
            "cli/bin/zz",
            "cli/lib",
            "cli/lib/deeper",
            "cli/lib/deeper/mid.txt",
            "conpty.dll",
            "OpenConsole.exe",
            "OpenConsole-LICENSE.txt",
        ]
    );
}

#[test]
fn one_release_folder_packs_to_the_same_bytes_every_time() {
    let dir = tempfile::tempdir().expect("dir");
    let release = dir.path().join("release");
    common::release(&release, &[]);
    for path in ["cli/zz", "cli/bin/aa", "cli/mm/x"] {
        let file = release.join(path);
        fs::create_dir_all(file.parent().expect("a folder")).expect("make the folder");
        fs::write(file, path).expect("write the file");
    }
    let app = release.join("ConsensFlow.exe");

    pack(&app, &release, &dir.path().join("first.exe")).expect("packed");
    pack(&app, &release, &dir.path().join("second.exe")).expect("packed");

    assert_eq!(
        fs::read(dir.path().join("first.exe")).expect("first"),
        fs::read(dir.path().join("second.exe")).expect("second")
    );
}

#[test]
fn the_folder_the_exe_is_written_to_is_made_and_an_older_exe_is_replaced() {
    let dir = tempfile::tempdir().expect("dir");
    let release = dir.path().join("release");
    common::release(&release, &[]);
    let out = dir.path().join("a").join("b").join("c.exe");

    pack(&release.join("ConsensFlow.exe"), &release, &out).expect("packed");
    fs::write(release.join("conpty.dll"), "a newer conpty").expect("a newer file");
    let again = pack(&release.join("ConsensFlow.exe"), &release, &out).expect("packed again");

    let exe = fs::read(&out).expect("the exe");
    let tar = common::gunzip(common::payload_of(&exe, 3));
    assert!(
        tar.windows(14).any(|part| part == b"a newer conpty"),
        "the older exe is gone"
    );
    assert_eq!(inspect(&out).expect("inspected"), again.payload);
}

/// What the packer refuses names the piece, as the system writes its path, and
/// the build that makes it; and nothing is written for it.
#[test]
fn a_release_folder_missing_a_piece_is_refused_naming_the_piece_and_the_build() {
    for piece in [
        "cli/bin/cf.exe",
        "conpty.dll",
        "OpenConsole.exe",
        "OpenConsole-LICENSE.txt",
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let release = dir.path().join("release");
        common::release(&release, &[piece]);
        let out = dir.path().join("out").join("portable.exe");

        let refusal = pack(&release.join("ConsensFlow.exe"), &release, &out)
            .expect_err("a missing piece is refused");

        let at_home: PathBuf = piece.split('/').collect();
        match &refusal {
            Error::Missing { piece, folder } => {
                assert_eq!(piece, &at_home);
                assert_eq!(folder, &release);
            }
            other => panic!("{piece}: {other}"),
        }
        let said = refusal.to_string();
        assert_eq!(
            said,
            format!(
                "{} is missing from {}; build first with npm --prefix app run build",
                at_home.display(),
                release.display()
            )
        );
        assert!(!out.exists(), "{piece}: nothing is written");
        assert!(
            !out.parent().expect("a folder").exists(),
            "{piece}: nor its folder"
        );
    }
}

#[test]
fn the_first_piece_missing_is_the_one_named_and_the_app_comes_first() {
    let dir = tempfile::tempdir().expect("dir");
    let release = dir.path().join("release");
    common::release(
        &release,
        &["conpty.dll", "OpenConsole.exe", "ConsensFlow.exe"],
    );
    let out = dir.path().join("out.exe");

    let refusal = pack(&release.join("ConsensFlow.exe"), &release, &out).expect_err("refused");
    assert_eq!(
        refusal.to_string(),
        format!(
            "ConsensFlow.exe is missing from {}; build first with npm --prefix app run build",
            release.display()
        )
    );

    fs::write(release.join("ConsensFlow.exe"), "app").expect("the app");
    let refusal = pack(&release.join("ConsensFlow.exe"), &release, &out).expect_err("refused");
    assert!(
        refusal
            .to_string()
            .starts_with("conpty.dll is missing from "),
        "{refusal}"
    );
}

/// A folder where a file belongs is no file.
#[test]
fn a_folder_in_the_place_of_a_file_is_a_missing_file() {
    let dir = tempfile::tempdir().expect("dir");
    let release = dir.path().join("release");
    common::release(&release, &["OpenConsole.exe"]);
    fs::create_dir_all(release.join("OpenConsole.exe")).expect("a folder");

    let refusal = pack(
        &release.join("ConsensFlow.exe"),
        &release,
        &dir.path().join("out.exe"),
    )
    .expect_err("refused");

    assert!(
        refusal
            .to_string()
            .starts_with("OpenConsole.exe is missing from "),
        "{refusal}"
    );
}

/// The reader takes files and folders only, so the packer writes no other.
#[cfg(unix)]
#[test]
fn a_special_file_in_the_runtime_is_refused_and_nothing_is_written() {
    let dir = tempfile::tempdir().expect("dir");
    let release = dir.path().join("release");
    common::release(&release, &[]);
    let _socket = std::os::unix::net::UnixListener::bind(release.join("cli").join("socket"))
        .expect("a socket");
    let out = dir.path().join("out.exe");

    let refusal = pack(&release.join("ConsensFlow.exe"), &release, &out).expect_err("refused");

    assert!(
        matches!(&refusal, Error::NotPlain { path } if path == "cli/socket"),
        "{refusal}"
    );
    assert!(!out.exists());
}

/// An exe that cannot be made is refused naming it, and nothing else is made
/// or removed for it.
#[test]
fn an_exe_that_cannot_be_made_is_refused_naming_it_and_what_was_in_the_way_stays() {
    let dir = tempfile::tempdir().expect("dir");
    let release = dir.path().join("release");
    common::release(&release, &[]);
    // A folder where the exe goes.
    let out = dir.path().join("out.exe");
    fs::create_dir_all(&out).expect("a folder in the way");

    let refusal = pack(&release.join("ConsensFlow.exe"), &release, &out).expect_err("refused");

    assert!(
        matches!(&refusal, Error::Files { what: "make", path, .. } if *path == out),
        "{refusal}"
    );
    assert!(out.is_dir(), "what was in the way is left as it was");
    let mut left = fs::read_dir(dir.path())
        .expect("read the folder")
        .map(|entry| entry.expect("an entry").file_name())
        .collect::<Vec<_>>();
    left.sort();
    assert_eq!(left, ["out.exe", "release"]);
}
