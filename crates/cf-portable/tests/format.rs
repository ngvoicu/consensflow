//! The footer and the trailer of the payload, byte by byte, and what a file
//! that does not carry them looks like. Each exe here is laid out by hand.
#![allow(clippy::expect_used)]

mod common;

use std::fs;
use std::io::Cursor;

use cf_portable::{footer, inspect, Error, Payload};

fn find(bytes: &[u8]) -> Result<Option<Payload>, Error> {
    Payload::find(&mut Cursor::new(bytes))
}

#[test]
fn the_footer_is_the_length_as_eight_little_endian_bytes_then_the_tag() {
    assert_eq!(
        footer(0x0102_0304_0506_0708),
        *b"\x08\x07\x06\x05\x04\x03\x02\x01CFPAYLD1"
    );
    assert_eq!(footer(0), *b"\0\0\0\0\0\0\0\0CFPAYLD1");
}

/// A payload of the smallest size there is, a gzip header and a trailer, with
/// the trailer's CRC and length as given: `find` reads them and nothing more.
fn payload_with_trailer(crc: u32, tar_length: u32) -> Vec<u8> {
    let mut payload = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    payload.extend_from_slice(&crc.to_le_bytes());
    payload.extend_from_slice(&tar_length.to_le_bytes());
    payload
}

#[test]
fn the_trailer_names_the_crc_then_the_length_and_the_folder_the_crc_in_eight_hex_digits() {
    let exe = common::exe(
        b"MZ the app",
        &payload_with_trailer(0x0000_abcd, 0x0403_0201),
    );

    let found = find(&exe).expect("read").expect("a payload");

    assert_eq!(
        found,
        Payload {
            offset: 10,
            length: 18,
            crc: 0x0000_abcd,
            tar_length: 0x0403_0201,
        }
    );
    assert_eq!(found.crc_hex(), "0000abcd");
    assert_eq!(found.folder("9.9.9"), "9.9.9-0000abcd");
    assert_eq!(found.folder("3.0.0-alpha.83"), "3.0.0-alpha.83-0000abcd");
}

/// The installed app, and the Mac's, carry nothing: they find their runtime
/// beside them.
#[test]
fn a_file_that_does_not_end_with_the_tag_carries_nothing() {
    let with = common::exe(b"MZ the app", &payload_with_trailer(1, 2));
    let mut one_short = with.clone();
    one_short.pop();
    let mut another_tag = with.clone();
    *another_tag.last_mut().expect("a byte") = b'2';
    let mut lower = with.clone();
    let tag = lower.len() - 8;
    lower[tag..].make_ascii_lowercase();
    let mut shifted = with;
    shifted.push(0);

    for bytes in [
        b"".to_vec(),
        b"MZ".to_vec(),
        b"MZ an installed app, longer than a footer".to_vec(),
        one_short,
        another_tag,
        lower,
        shifted,
    ] {
        assert_eq!(find(&bytes).expect("read"), None, "{bytes:?}");
    }
}

/// A footer naming more payload than the file holds, or less than any gzip
/// stream, is a damaged exe, not one that carries nothing.
#[test]
fn a_footer_that_cannot_be_right_is_an_error() {
    let app = b"MZ the app and some";
    for length in [0_u64, 5, 17, 1_000, u64::MAX] {
        let mut bytes = app.to_vec();
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(b"CFPAYLD1");
        let size = bytes.len() as u64;

        let refusal = find(&bytes).expect_err("a damaged exe");

        assert!(
            matches!(refusal, Error::Footer { length: named, size: held } if named == length && held == size),
            "{refusal}"
        );
        assert_eq!(
            refusal.to_string(),
            format!("its footer names {length} bytes of payload in a file of {size}")
        );
    }
}

#[test]
fn the_smallest_payload_and_one_that_fills_the_file_are_found_and_one_more_than_fits_is_not() {
    // Eighteen bytes, a gzip header and trailer, and nothing before them.
    let exe = common::exe(b"", &payload_with_trailer(7, 9));
    let found = find(&exe).expect("read").expect("a payload");
    assert_eq!((found.offset, found.length), (0, 18));

    let length_at = exe.len() - 16;
    for (length, fits) in [(17_u64, false), (18, true), (19, false)] {
        let mut bytes = exe.clone();
        bytes[length_at..length_at + 8].copy_from_slice(&length.to_le_bytes());
        assert_eq!(
            find(&bytes).is_ok_and(|found| found.is_some()),
            fits,
            "{length}"
        );
    }
}

/// An exe cut short in its payload, its footer kept, names more than it holds.
#[test]
fn an_exe_cut_inside_its_payload_names_more_than_it_holds() {
    let mut bytes = common::exe(b"MZ the app", &[7; 300]);
    bytes.drain(20..120);

    let refusal = find(&bytes).expect_err("cut short");

    assert!(
        matches!(refusal, Error::Footer { length: 300, size } if size == bytes.len() as u64),
        "{refusal}"
    );
}

#[test]
fn inspect_names_a_file_that_carries_no_runtime_and_one_that_is_not_there() {
    let dir = tempfile::tempdir().expect("dir");
    let exe = dir.path().join("ConsensFlow.exe");
    fs::write(&exe, b"MZ an installed app, longer than a footer").expect("write");

    let refusal = inspect(&exe).expect_err("no footer");
    assert!(matches!(refusal, Error::NoFooter { .. }), "{refusal}");
    assert_eq!(
        refusal.to_string(),
        format!("{} does not end with the portable footer", exe.display())
    );

    let absent = dir.path().join("absent.exe");
    let refusal = inspect(&absent).expect_err("no file");
    assert!(
        matches!(refusal, Error::Files { what: "open", .. }),
        "{refusal}"
    );
    assert!(refusal
        .to_string()
        .starts_with(&format!("could not open {}: ", absent.display())));
}

/// What a person reads of a refusal has the file in it, whichever way the file
/// is wrong.
#[test]
fn inspect_names_a_file_whose_footer_cannot_be_right() {
    let dir = tempfile::tempdir().expect("dir");
    let exe = dir.path().join("ConsensFlow.exe");
    let mut bytes = b"MZ the app and some".to_vec();
    bytes.extend_from_slice(&footer(1_000));
    fs::write(&exe, &bytes).expect("write");

    let refusal = inspect(&exe).expect_err("a damaged exe");

    assert!(
        matches!(refusal, Error::Files { what: "read", .. }),
        "{refusal}"
    );
    assert_eq!(
        refusal.to_string(),
        format!(
            "could not read {}: its footer names 1000 bytes of payload in a file of {}",
            exe.display(),
            bytes.len()
        )
    );
}
