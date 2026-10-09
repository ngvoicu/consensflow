//! The tests of the feed's entry: its bytes, the notes it carries, and where it
//! is written.

use super::*;

fn build<'a>(notes: &'a str, signature: &'a str) -> Build<'a> {
    Build {
        version: "3.0.0-alpha.99",
        notes,
        date: "2026-09-09T12:00:00Z",
        archive: "ConsensFlow_3.0.0-alpha.99_aarch64.app.tar.gz",
        signature,
    }
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("prepare-update")
        .join(name)
}

#[test]
fn the_entry_is_one_line_in_the_order_the_updater_documents() {
    assert_eq!(
        render(&build("Fixes.", "c2ln")),
        concat!(
            r#"{"version":"3.0.0-alpha.99","notes":"Fixes.","pub_date":"2026-09-09T12:00:00Z","#,
            r#""platforms":{"darwin-aarch64":{"url":"https://github.com/ngvoicu/consensflow/releases/download/"#,
            r#"v3.0.0-alpha.99/ConsensFlow_3.0.0-alpha.99_aarch64.app.tar.gz","signature":"c2ln"}}}"#,
            "\n"
        )
    );
}

#[test]
fn the_notes_are_escaped_as_javascript_escapes_text() {
    let notes = "a \"quote\", a \\ and a / and a\ttab\nand \u{1} \u{1f} \u{7f} \u{2028} é 😀";
    let entry = render(&build(notes, "s"));
    assert!(
        entry.contains(r#""notes":"a \"quote\", a \\ and a / and a\ttab\nand \u0001 \u001f "#),
        "{entry}"
    );
    assert!(entry.contains("\u{7f} \u{2028} é 😀\","), "{entry}");
    // What it says is what was given.
    let read: Value = serde_json::from_str(&entry).unwrap();
    assert_eq!(read["notes"], notes);
}

#[test]
fn a_url_in_the_notes_does_not_touch_the_one_the_entry_makes() {
    let entry = render(&build("See https://evil.example/notes for details.", "s"));
    let read: Value = serde_json::from_str(&entry).unwrap();
    assert_eq!(read["notes"], "See https://evil.example/notes for details.");
    assert_eq!(
        read["platforms"]["darwin-aarch64"]["url"],
        "https://github.com/ngvoicu/consensflow/releases/download/v3.0.0-alpha.99/ConsensFlow_3.0.0-alpha.99_aarch64.app.tar.gz"
    );
    assert!(read.get("consensflow").is_none());
}

#[test]
fn what_the_script_this_replaced_wrote_for_a_build_is_what_this_writes() {
    // The files `app/scripts/prepare-update.mjs` read, and the line it wrote from
    // them (3.0.0-alpha.99, the archive named for it, on 2026-10-09 at 06:30:15.123
    // in a zone two hours east): the notes' white space trimmed from either end, a
    // byte order mark and a line break; a NEL and a tab kept inside.
    let written = fs::read_to_string(fixture("latest.json")).unwrap();
    let notes = notes(&fixture("notes.txt")).unwrap();
    let signature = read_signature(&fixture("signature.sig"));
    let entry = render(&Build {
        date: "2026-10-09T06:30:15.123+02:00",
        ..build(&notes, &signature)
    });
    assert_eq!(entry, written);
}

fn read_signature(path: &Path) -> String {
    fs::read_to_string(path).unwrap().trim().to_string()
}

#[test]
fn the_notes_are_the_text_of_the_file_without_javascripts_blank_around_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    for (given, expected) in [
        (
            "Alpha 99 fixes delivery races.\n",
            "Alpha 99 fixes delivery races.",
        ),
        ("", ""),
        (" \t\r\n ", ""),
        (
            "\u{feff}  two\n\nparagraphs \u{a0}\u{2029}",
            "two\n\nparagraphs",
        ),
        // JavaScript keeps a NEL, which Rust's own trim would take off.
        ("\u{85}x\u{85}\n", "\u{85}x\u{85}"),
        ("a\0\n", "a\0"),
    ] {
        fs::write(&file, given).unwrap();
        assert_eq!(notes(&file).unwrap(), expected, "{given:?}");
    }
}

#[test]
fn text_that_is_not_utf8_is_read_as_replacement_characters() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    fs::write(&file, b"caf\xe9 \xf0\x9f").unwrap();
    assert_eq!(notes(&file).unwrap(), "caf\u{fffd} \u{fffd}");
}

#[test]
fn notes_of_64_kib_are_the_most_there_can_be_counted_in_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    fs::write(&file, "n".repeat(NOTES_LIMIT)).unwrap();
    assert_eq!(notes(&file).unwrap().len(), NOTES_LIMIT);

    fs::write(&file, "n".repeat(NOTES_LIMIT + 1)).unwrap();
    let refused = notes(&file).unwrap_err();
    assert!(matches!(refused, FeedError::NotesTooLong), "{refused}");
    assert_eq!(refused.to_string(), "release notes exceed 64 KiB");

    // The white space around them is not counted, and the bytes of a letter are.
    fs::write(&file, format!("\n\n{}\n  ", "n".repeat(NOTES_LIMIT))).unwrap();
    assert_eq!(notes(&file).unwrap().len(), NOTES_LIMIT);
    let two_bytes = "é".repeat(NOTES_LIMIT / 2);
    fs::write(&file, &two_bytes).unwrap();
    assert_eq!(notes(&file).unwrap(), two_bytes);
    fs::write(&file, format!("{two_bytes}é")).unwrap();
    assert!(matches!(notes(&file), Err(FeedError::NotesTooLong)));
}

#[test]
fn notes_that_cannot_be_read_say_which_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("none.txt");
    let refused = notes(&missing).unwrap_err();
    assert!(
        refused.to_string().starts_with(&format!(
            "could not read release notes: {}: ",
            missing.display()
        )),
        "{refused}"
    );
    assert!(matches!(
        notes(dir.path()),
        Err(FeedError::NotesUnreadable(_))
    ));
}

#[test]
fn the_entry_replaces_what_the_file_held_and_a_file_that_cannot_be_written_says_which() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("latest.json");
    fs::write(&file, "old entry that is longer than the new one\n").unwrap();
    write(&file, "new\n").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "new\n");

    let nowhere = dir.path().join("no").join("such").join("latest.json");
    let refused = write(&nowhere, "x\n").unwrap_err();
    assert!(
        refused
            .to_string()
            .starts_with(&format!("could not write {}: ", nowhere.display())),
        "{refused}"
    );
    assert!(!nowhere.exists());
}
