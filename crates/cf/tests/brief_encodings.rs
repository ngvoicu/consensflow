//! What encoding a text may come in, for every command that takes one, whether it is read from a
//! file (`--file`) or from standard input (`-`). Windows PowerShell 5.1 writes what `>` and
//! `Out-File` send to a file as UTF-16 with a byte order mark; `cf` once read those bytes as
//! UTF-8 with what was not UTF-8 replaced, and the board took `��W\0r\0i\0…` as a brief
//! (the review T-1447 of T-1435, finding 1: a wrong result that is silent).
//!
//! The rule: UTF-8, with or without its mark, is the text; UTF-16 with a mark, in either byte
//! order, is decoded; anything else that is not valid UTF-8 (UTF-16 without a mark, a NUL byte,
//! an invalid sequence) is refused, with words that say what was found and what to do, and
//! nothing is posted. The class-wide cases (every command of `cf help` that takes a text) are
//! `text_commands.rs`'s.

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use cf_board::scripted::{reply, scripted, ScriptedApi};
use common::{cf, cf_bytes};
use serde_json::{json, Value};

/// A text with a letter outside ASCII, a letter outside the Basic Multilingual Plane (a surrogate
/// pair in UTF-16), a backtick, `$( )` and a Windows line break.
const TEXT: &str = "Scrie parserul: Întrebarea 1, 漢字 🙂 `cargo test` $(nimic).\r\nA doua linie.";

fn added() -> ScriptedApi {
    scripted(vec![reply(
        201,
        json!({
            "task": {
                "number": 1, "state": "open", "assignee": null, "requester": "chief",
                "title": "Parser", "blockedBy": [], "pool": "worker", "tier": "standard",
            },
            "message": null, "gated": false,
        }),
    )])
}

fn utf16(text: &str, mark: Option<[u8; 2]>, little: bool) -> Vec<u8> {
    let mut bytes: Vec<u8> = mark.map(|mark| mark.to_vec()).unwrap_or_default();
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&if little {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        });
    }
    bytes
}

/// What PowerShell 5.1's `>` and `Out-File` write: a mark, UTF-16 little-endian, and a Windows
/// line break to end the file.
fn powershell_5_1(text: &str) -> Vec<u8> {
    utf16(&format!("{text}\r\n"), Some([0xff, 0xfe]), true)
}

/// What `cf` did with `bytes` given as a file: its exit code, the body it posted (none when it
/// posted nothing), and what it said on the error output.
fn from_file(bytes: &[u8]) -> (Option<i32>, Option<Value>, String) {
    let folder = tempfile::tempdir().expect("a folder");
    let path = folder.path().join("brief.md");
    std::fs::write(&path, bytes).expect("the file");
    let api = added();
    let ran = cf(
        &[
            "task",
            "add",
            "--tier",
            "standard",
            "--file",
            &path.to_string_lossy(),
        ],
        &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
        "",
    );
    outcome(&api, &ran, Some(&path.to_string_lossy()))
}

/// What `cf` did with `bytes` given on standard input.
fn from_input(bytes: &[u8]) -> (Option<i32>, Option<Value>, String) {
    let api = added();
    let ran = cf_bytes(
        &["task", "add", "--tier", "standard", "-"],
        &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
        bytes,
    );
    outcome(&api, &ran, None)
}

fn outcome(
    api: &ScriptedApi,
    ran: &std::process::Output,
    path: Option<&str>,
) -> (Option<i32>, Option<Value>, String) {
    let body = api
        .received()
        .into_iter()
        .next()
        .and_then(|got| got.json())
        .and_then(|sent| sent.get("body").cloned());
    let said = String::from_utf8_lossy(&ran.stderr).into_owned();
    // Where the file was is the case's own; what is said of it is the words'.
    let said = path.map_or(said.clone(), |path| said.replace(path, "<file>"));
    (ran.status.code(), body, said)
}

#[test]
fn utf16_with_a_mark_in_either_byte_order_is_decoded_and_posted_as_its_text() {
    let want = Some(json!(TEXT));
    let big = utf16(&format!("{TEXT}\r\n"), Some([0xfe, 0xff]), false);
    let cases = [
        ("PowerShell 5.1's own > and Out-File", powershell_5_1(TEXT)),
        ("little-endian, a line feed to end", {
            utf16(&format!("{TEXT}\n"), Some([0xff, 0xfe]), true)
        }),
        ("big-endian", big),
        ("little-endian, no final line break", {
            utf16(TEXT, Some([0xff, 0xfe]), true)
        }),
    ];
    for (name, bytes) in cases {
        let (code, body, said) = from_file(&bytes);
        assert_eq!(
            (code, body.clone(), said.as_str()),
            (Some(0), want.clone(), ""),
            "a file, {name}"
        );
        let (code, body, said) = from_input(&bytes);
        assert_eq!(
            (code, body, said.as_str()),
            (Some(0), want.clone(), ""),
            "input, {name}"
        );
    }
}

#[test]
fn utf8_with_or_without_its_mark_is_the_text() {
    let want = Some(json!(TEXT));
    let plain = format!("{TEXT}\r\n").into_bytes();
    let mut marked = vec![0xef, 0xbb, 0xbf];
    marked.extend_from_slice(&plain);
    for (name, bytes) in [("without a mark", plain), ("with a mark", marked)] {
        let (code, body, said) = from_file(&bytes);
        assert_eq!(
            (code, body, said.as_str()),
            (Some(0), want.clone(), ""),
            "a file, {name}"
        );
        let (code, body, said) = from_input(&bytes);
        assert_eq!(
            (code, body, said.as_str()),
            (Some(0), want.clone(), ""),
            "input, {name}"
        );
    }
}

#[test]
fn a_text_that_is_not_utf8_or_utf16_with_a_mark_is_refused_and_nothing_is_posted() {
    // What each case is, the bytes, and the words of what was found.
    let cases: [(&str, Vec<u8>, &str); 9] = [
        (
            "UTF-16 little-endian without a mark",
            utf16("Write the parser.\r\n", None, true),
            "it holds a NUL byte (at offset 1)",
        ),
        (
            "UTF-16 big-endian without a mark",
            utf16("Write the parser.\r\n", None, false),
            "it holds a NUL byte (at offset 0)",
        ),
        (
            "a Windows code page (PowerShell's -Encoding Default): é is one byte",
            b"Intrebarea: \xe9\r\n".to_vec(),
            "it is not UTF-8: the byte at offset 12 is not valid",
        ),
        (
            "a NUL byte in text that is otherwise UTF-8",
            b"Write\0 the parser.".to_vec(),
            "it holds a NUL byte (at offset 5)",
        ),
        (
            "an overlong sequence",
            b"Write the parser \xc0\x80.".to_vec(),
            "it is not UTF-8: the byte at offset 17 is not valid",
        ),
        (
            "UTF-8's mark, then bytes that are not UTF-8",
            b"\xef\xbb\xbfWrite \xff the parser.".to_vec(),
            "it is not UTF-8: the byte at offset 9 is not valid",
        ),
        (
            "a UTF-16 mark, then an odd number of bytes",
            {
                let mut bytes = powershell_5_1("Write");
                bytes.pop();
                bytes
            },
            "it begins with a UTF-16 byte order mark but is cut or damaged",
        ),
        (
            "a UTF-16 mark, then half a surrogate pair",
            {
                let mut bytes = vec![0xff, 0xfe];
                bytes.extend_from_slice(&0xd83d_u16.to_le_bytes());
                bytes.extend_from_slice(&u16::from(b'x').to_le_bytes());
                bytes
            },
            "it begins with a UTF-16 byte order mark but is cut or damaged",
        ),
        (
            "a UTF-32 mark (little-endian): its characters are four bytes wide",
            {
                let mut bytes = vec![0xff, 0xfe, 0, 0];
                bytes.extend_from_slice(&u32::from(b'W').to_le_bytes());
                bytes
            },
            "it begins with a UTF-16 byte order mark but holds a NUL",
        ),
    ];
    for (name, bytes, found) in cases {
        let (code, body, said) = from_file(&bytes);
        assert_eq!((code, &body), (Some(1), &None), "a file, {name}: {said}");
        assert!(
            said.starts_with("cf: cannot read <file>: ") && said.contains(found),
            "a file, {name}: {said}"
        );
        assert!(
            said.contains("Write it as UTF-8")
                && said.contains("file tool")
                && said.ends_with("Nothing was sent.\n"),
            "a file, {name}: what to do: {said}"
        );
        let (code, body, said) = from_input(&bytes);
        assert_eq!((code, &body), (Some(1), &None), "input, {name}: {said}");
        assert!(
            said.starts_with("cf: cannot read standard input: ") && said.contains(found),
            "input, {name}: {said}"
        );
        assert!(
            said.contains("Write it as UTF-8") && said.ends_with("Nothing was sent.\n"),
            "input, {name}: what to do: {said}"
        );
    }
}
