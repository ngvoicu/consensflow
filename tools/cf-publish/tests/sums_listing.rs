//! The one reading of a SHA256SUMS (`cf_publish::sums::listed_in`) that the
//! publisher's checks and `cargo xtask release verify` both hold a release to:
//! each row as the release writes it, any other an error that names it.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use cf_publish::sums::{listed_in, write_sums};
use cf_publish::testing::{built_files, folder_of, v};

const A: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const B: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

/// What `listed_in` makes of a list: the names and hashes it lists, or the
/// rows it refuses.
type Read = Result<Vec<(String, String)>, Vec<String>>;

fn read(sums: &[u8]) -> Read {
    listed_in(sums).map(|listed| listed.into_iter().collect())
}

fn listing(rows: &[(&str, &str)]) -> Read {
    Ok(rows
        .iter()
        .map(|(name, hash)| ((*name).to_owned(), (*hash).to_owned()))
        .collect())
}

fn refused(rows: &[&str]) -> Read {
    Err(rows.iter().map(|row| (*row).to_owned()).collect())
}

#[test]
fn reads_each_row_as_the_release_writes_it_and_refuses_any_other_naming_it() {
    let upper = A.to_ascii_uppercase();
    let cases: Vec<(&str, Vec<u8>, Read)> = vec![
        (
            "rows as sums_of writes them",
            format!("{A}  a.dmg\n{B}  b.exe\n").into_bytes(),
            listing(&[("a.dmg", A), ("b.exe", B)]),
        ),
        (
            "the last row without its line feed",
            format!("{A}  a.dmg\n{B}  b.exe").into_bytes(),
            listing(&[("a.dmg", A), ("b.exe", B)]),
        ),
        ("an empty list", Vec::new(), listing(&[])),
        (
            "a hash in capitals, which the installers do not read",
            format!("{upper}  a.dmg\n").into_bytes(),
            refused(&[&format!(
                "row 1 is not \"<sha-256>  <name>\": \"{upper}  a.dmg\""
            )]),
        ),
        (
            "a name listed twice, with the same hash",
            format!("{A}  a.dmg\n{A}  a.dmg\n").into_bytes(),
            refused(&["row 2 lists a.dmg again"]),
        ),
        (
            "a name listed twice, with another hash",
            format!("{A}  a.dmg\n{B}  b.exe\n{B}  a.dmg\n").into_bytes(),
            refused(&["row 3 lists a.dmg again"]),
        ),
        (
            "one space",
            format!("{A} a.dmg\n").into_bytes(),
            refused(&[&format!(
                "row 1 is not \"<sha-256>  <name>\": \"{A} a.dmg\""
            )]),
        ),
        (
            "sha256sum's binary mode, which the release never writes",
            format!("{A} *a.dmg\n").into_bytes(),
            refused(&[&format!(
                "row 1 is not \"<sha-256>  <name>\": \"{A} *a.dmg\""
            )]),
        ),
        (
            "a tab",
            format!("{A}\ta.dmg\n").into_bytes(),
            refused(&[&format!(
                "row 1 is not \"<sha-256>  <name>\": \"{A}\\ta.dmg\""
            )]),
        ),
        (
            "rows ending in a carriage return",
            format!("{A}  a.dmg\r\n").into_bytes(),
            refused(&[&format!(
                "row 1 is not \"<sha-256>  <name>\": \"{A}  a.dmg\\r\""
            )]),
        ),
        (
            "a blank row",
            format!("{A}  a.dmg\n\n{B}  b.exe\n").into_bytes(),
            refused(&["row 2 is not \"<sha-256>  <name>\": \"\""]),
        ),
        (
            "a hash a digit short",
            format!("{}  a.dmg\n", &A[1..]).into_bytes(),
            refused(&[&format!(
                "row 1 is not \"<sha-256>  <name>\": \"{}  a.dmg\"",
                &A[1..]
            )]),
        ),
        (
            "a hash a digit long",
            format!("0{A}  a.dmg\n").into_bytes(),
            refused(&[&format!(
                "row 1 is not \"<sha-256>  <name>\": \"0{A}  a.dmg\""
            )]),
        ),
        (
            "no name",
            format!("{A}  \n").into_bytes(),
            refused(&[&format!("row 1 is not \"<sha-256>  <name>\": \"{A}  \"")]),
        ),
        (
            "a name with a space",
            format!("{A}  a b.dmg\n").into_bytes(),
            refused(&[&format!(
                "row 1 is not \"<sha-256>  <name>\": \"{A}  a b.dmg\""
            )]),
        ),
        (
            "a row that is not UTF-8",
            [format!("{A}  a").as_bytes(), &[0xff], b".dmg\n"].concat(),
            refused(&[&format!(
                "row 1 is not \"<sha-256>  <name>\": \"{A}  a\u{fffd}.dmg\""
            )]),
        ),
        (
            "each wrong row, not the first alone",
            format!("garbage\n{A}  a.dmg\n{B} b.exe\n{A}  a.dmg\n").into_bytes(),
            refused(&[
                "row 1 is not \"<sha-256>  <name>\": \"garbage\"",
                &format!("row 3 is not \"<sha-256>  <name>\": \"{B} b.exe\""),
                "row 4 lists a.dmg again",
            ]),
        ),
    ];
    for (case, sums, wanted) in cases {
        assert_eq!(read(&sums), wanted, "{case}");
    }
}

#[test]
fn reads_what_the_publisher_writes_and_the_installers_signed_fixture() {
    let version = "3.0.7-alpha.5";
    let files = built_files("https://example.invalid", version)
        .without("SHA256SUMS")
        .without("SHA256SUMS.sig");
    let dir = folder_of(&files);
    write_sums(dir.path(), &v(version)).unwrap();
    let written = fs::read(dir.path().join("SHA256SUMS")).unwrap();
    let listed = listed_in(&written).unwrap();
    assert_eq!(
        listed.len(),
        String::from_utf8(written).unwrap().lines().count()
    );

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/install/tauri-signed/ConsensFlow_9.9.9-test.1_SHA256SUMS");
    let listed = listed_in(&fs::read(fixture).unwrap()).unwrap();
    assert_eq!(
        listed.into_iter().collect::<Vec<_>>(),
        [
            (
                "ConsensFlow_9.9.9-test.1_aarch64.dmg".to_owned(),
                "1".repeat(64)
            ),
            (
                "ConsensFlow_9.9.9-test.1_x64-setup.exe".to_owned(),
                "2".repeat(64)
            ),
        ]
    );
}
