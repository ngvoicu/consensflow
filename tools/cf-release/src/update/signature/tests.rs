//! The tests of the signature's shape: what the signer writes is taken, and
//! every way a `.sig` can be wrong is refused for the reason it is wrong.

use super::*;

const KEY: &str = "RUTKbp37uB4mh3QuqjizE5qwzEQRTiusW3qwiDtoFuhEUaQ+cRCU+VdP/Ee2JVCrCWCRwJw6e64tAmU8FXPeeubR6Y3+foAANgU=";
const GLOBAL: &str =
    "ZsYY/Z32Np8PS9zvML7LiCnnfxQRC/vGcV5dA8drvzLItPQf1kDn0m0VcO+Q0j9Lhsx6h9sllAP3jpTF6vfbAQ==";
const TRUSTED: &str =
    "trusted comment: timestamp:1791543247\tfile:ConsensFlow_3.0.0-alpha.99_aarch64.app.tar.gz";

/// A `.sig` of these lines: their text, a line break after each, as base64.
fn envelope(lines: &[&str]) -> String {
    STANDARD.encode(format!("{}\n", lines.join("\n")))
}

fn valid() -> String {
    envelope(&[UNTRUSTED_COMMENT, KEY, TRUSTED, GLOBAL])
}

fn checked(signature: &str) -> Result<(), SignatureError> {
    check(signature)
}

#[test]
fn a_signature_of_the_shape_the_signer_writes_is_taken() {
    assert_eq!(checked(&valid()), Ok(()));
    // The signer's tab between the timestamp and the file, or a space, or more.
    for gap in ["\t", " ", " \t ", "  "] {
        let trusted = format!("trusted comment: timestamp:7{gap}file:a.app.tar.gz");
        assert_eq!(
            checked(&envelope(&[UNTRUSTED_COMMENT, KEY, &trusted, GLOBAL])),
            Ok(()),
            "{gap:?}"
        );
    }
    // The last line break is the text's end, not another line.
    let bare = STANDARD.encode([UNTRUSTED_COMMENT, KEY, TRUSTED, GLOBAL].join("\n"));
    assert_eq!(checked(&bare), Ok(()));
    // A line that carries no padding, and one with a single `=`.
    for global in ["QUJD", "QUI="] {
        let lines = [UNTRUSTED_COMMENT, KEY, TRUSTED, global];
        assert_eq!(checked(&envelope(&lines)), Ok(()), "{global}");
    }
}

#[test]
fn what_a_real_signer_wrote_is_taken_and_read_whole() {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/prepare-update/signature.sig"
    );
    let signature = read(Path::new(file)).unwrap();
    assert_eq!(signature, fs::read_to_string(file).unwrap().trim());
}

#[test]
fn the_file_is_read_without_the_blank_around_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.sig");
    // JavaScript's blank: a byte order mark goes, and so does a line break.
    for blank in ["", "\n", "  \r\n", "\u{feff}\n\n\t"] {
        fs::write(&file, format!("{blank}{}{blank}", valid())).unwrap();
        assert_eq!(read(&file), Ok(valid()), "{blank:?}");
    }
}

#[test]
fn a_file_that_cannot_be_read_says_which() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.sig");
    let Err(SignatureError::Unreadable(said)) = read(&missing) else {
        panic!("a missing file was read");
    };
    assert!(said.starts_with(&missing.display().to_string()), "{said}");
    assert!(SignatureError::Unreadable(said)
        .to_string()
        .starts_with("could not read signature: "),);
    // A folder is no file either.
    assert!(matches!(
        read(dir.path()),
        Err(SignatureError::Unreadable(_))
    ));
}

#[test]
fn text_that_is_not_one_line_of_base64_is_refused_as_such() {
    let outer = Err(SignatureError::NotBase64);
    for bad in [
        "",
        "   ",
        "not a signature!!!",
        "short",
        "a b",
        "-_-_",
        "====",
        "ab=c",
        "abcd===",
    ] {
        assert_eq!(checked(bad), outer, "{bad:?}");
    }
    let one_line = valid();
    for broken in [
        format!("{one_line}\n{one_line}"),
        format!("{}\n{}", &one_line[..40], &one_line[40..]),
        format!("{one_line}!"),
        format!(" {one_line}"),
    ] {
        assert_eq!(checked(&broken), outer, "{broken:?}");
    }
}

#[test]
fn base64_must_be_canonical_to_be_the_signers() {
    // 82 bytes: one over a multiple of three, so the last group is "xx==".
    let bytes = [b'x'; 82];
    let canonical = STANDARD.encode(bytes);
    assert!(canonical.ends_with("=="), "{canonical}");
    // The text decodes either way; only the text of the signer's envelope would not be it.
    assert_eq!(checked(&canonical), Err(SignatureError::NotEnvelope));

    // Its padding left off.
    assert_eq!(
        checked(canonical.trim_end_matches('=')),
        Err(SignatureError::NotBase64)
    );
    // Its last character carrying bits the bytes do not: a different text for the same bytes.
    let last = canonical.len() - 3;
    let mut stray = canonical.clone().into_bytes();
    stray[last] += 1;
    assert_eq!(
        checked(&String::from_utf8(stray).unwrap()),
        Err(SignatureError::NotBase64)
    );
    // More padding than there is to pad.
    assert_eq!(
        checked(&format!("{canonical}=")),
        Err(SignatureError::NotBase64)
    );
}

#[test]
fn text_too_short_or_not_text_is_refused_before_its_lines_are_counted() {
    // Valid base64 of a few words: no envelope, but it does not get as far as being one.
    assert_eq!(
        checked(&STANDARD.encode("short")),
        Err(SignatureError::NotBase64)
    );
    assert_eq!(
        checked(&STANDARD.encode([b'x'; SHORTEST - 1])),
        Err(SignatureError::NotBase64)
    );
    assert_eq!(
        checked(&STANDARD.encode([b'x'; SHORTEST])),
        Err(SignatureError::NotEnvelope)
    );
    // Bytes that are no UTF-8, long enough.
    assert_eq!(
        checked(&STANDARD.encode([0xff_u8; 90])),
        Err(SignatureError::NotBase64)
    );
}

#[test]
fn four_lines_of_the_right_kinds_make_the_envelope() {
    let envelope_error = Err(SignatureError::NotEnvelope);
    let (u, k, t, g) = (UNTRUSTED_COMMENT, KEY, TRUSTED, GLOBAL);
    let wrong: &[&[&str]] = &[
        &[u, k, t],
        &[u, k, t, g, g],
        &[u, k, "", t, g],
        &[k, k, t, g],
        &["untrusted comment: signature from another key", k, t, g],
        &[
            " untrusted comment: signature from tauri secret key",
            k,
            t,
            g,
        ],
        &[u, "", t, g],
        &[u, "not base64!", t, g],
        &[u, "a-b_c", t, g],
        &[u, k, "", g],
        &[u, k, t, ""],
        &[u, k, t, "no spaces allowed"],
        &[u, k, t, "padding====="],
        &[u, g, g, g],
    ];
    for lines in wrong {
        assert_eq!(checked(&envelope(lines)), envelope_error, "{lines:?}");
    }
}

#[test]
fn the_trusted_comment_has_a_timestamp_a_gap_and_a_file_name() {
    let shapes = [
        ("trusted comment: timestamp:1\tfile:a", true),
        (
            "trusted comment: timestamp:1700000000\tfile:A_b-c.1.app.tar.gz",
            true,
        ),
        // The timestamp, its digits and its gap.
        ("trusted comment: timestamp:\tfile:a", false),
        ("trusted comment: timestamp:x1\tfile:a", false),
        ("trusted comment: timestamp:1x\tfile:a", false),
        ("trusted comment: timestamp:1file:a", false),
        ("trusted comment: timestamp: 1\tfile:a", false),
        ("trusted comment:timestamp:1\tfile:a", false),
        ("Trusted comment: timestamp:1\tfile:a", false),
        // The file.
        ("trusted comment: timestamp:1\tfile:", false),
        ("trusted comment: timestamp:1\tfile: a", false),
        ("trusted comment: timestamp:1\tFile:a", false),
        ("trusted comment: timestamp:1\tfile:a/b", false),
        ("trusted comment: timestamp:1\tfile:a b", false),
        ("trusted comment: timestamp:1\tfile:a\r", false),
        ("trusted comment: timestamp:1\tfile:é", false),
        ("trusted comment: timestamp:1\tfile:a\thashed", false),
    ];
    for (line, taken) in shapes {
        let lines = [UNTRUSTED_COMMENT, KEY, line, GLOBAL];
        let expected = if taken {
            Ok(())
        } else {
            Err(SignatureError::NotEnvelope)
        };
        assert_eq!(checked(&envelope(&lines)), expected, "{line:?}");
    }
}
