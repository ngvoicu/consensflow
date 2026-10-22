//! What Claude Code's input box makes of a paste that names image files, as Claude
//! Code 2.1.308 does (read in its binary, and shown by the live probe of 2026-10-22,
//! `tests/live_image_paste.rs`, on a designer's result that starts with the paths of
//! its pictures; the field bug T-215, seen on 2026-10-21 in 2.1.306). The paste is
//! split at its line breaks and at a space that a `/`, or a drive letter and a
//! backslash, follows; each piece that ends in an image extension, once its quotes
//! are off and its backslash escapes undone, names a file. The files that are whole
//! images become attachments: a `[Image #N]` for each at the head of the message,
//! one space between them, and the other pieces after them as lines, with nothing
//! between. A paste none of whose images could be read stays as it was; an image
//! that could not be read, beside one that could, is gone from the words.
//!
//! The record is two user's records: the message, with an image part for each file
//! (the pixels are not copied), and a companion of the window's own that says where
//! each came from, `[Image: source: <path>]`. This stand-in knows PNGs only
//! (`cf_e2e::png`), counts its placeholders from 1 in each window, and does not wrap
//! a long paste in `<pasted_content>`, as Claude does from about four lines on: the
//! shape the field showed was a short result. Only a paste is made so: the first message
//! of a launch, given as an argument of the program, is kept as it is (Claude Code 2.1.308,
//! the live probe of 2026-10-22, T-265), and the window says so (`Window::seeded`).

use serde_json::{json, Value};

/// A paste that named images, as the window keeps it.
#[derive(Debug, PartialEq, Eq)]
struct Attached {
    /// The message's words: the placeholders, then the pieces that were no images.
    text: String,
    /// The number in each placeholder, and the file it stands for, in order.
    numbers: Vec<u32>,
    files: Vec<String>,
}

/// Whether `rest`, what follows a space, begins a path: a `/`, or a drive letter, a
/// colon and a backslash.
fn path_follows(rest: &[u8]) -> bool {
    rest.first() == Some(&b'/')
        || matches!(rest, [letter, b':', b'\\', ..] if letter.is_ascii_alphabetic())
}

/// The pieces of a paste that Claude looks for paths in: cut at each space that a path
/// follows and at each line break, those that are blank left out.
fn pieces(paste: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    for (at, byte) in paste.bytes().enumerate() {
        if byte == b' ' && path_follows(&paste.as_bytes()[at + 1..]) {
            pieces.push(&paste[start..at]);
            start = at + 1;
        }
    }
    pieces.push(&paste[start..]);
    pieces
        .into_iter()
        .flat_map(|piece| piece.split('\n'))
        .filter(|line| !line.trim().is_empty())
        .collect()
}

/// `path` with a backslash and the character after it as that character alone, and
/// two backslashes as one: how Claude reads a path a terminal escaped.
fn unescaped(path: &str) -> String {
    let mut chars = path.chars();
    let mut plain = String::new();
    while let Some(at) = chars.next() {
        if at == '\\' {
            plain.push(chars.next().unwrap_or('\\'));
        } else {
            plain.push(at);
        }
    }
    plain
}

/// The path `piece` names, if it ends in an image extension: its quotes, if it is
/// quoted whole, off, and its escapes undone.
fn image_path(piece: &str) -> Option<String> {
    let piece = piece.trim();
    let quoted = ['"', '\'']
        .into_iter()
        .any(|quote| piece.len() > 1 && piece.starts_with(quote) && piece.ends_with(quote));
    let path = unescaped(if quoted {
        &piece[1..piece.len() - 1]
    } else {
        piece
    });
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp"]
        .iter()
        .any(|extension| lower.ends_with(extension))
        .then_some(path)
}

/// What the window keeps of `paste` if it names images it can read, numbering the
/// placeholders with `number`; none where it names none, or none that are whole.
fn attach(paste: &str, mut number: impl FnMut() -> u32) -> Option<Attached> {
    let (named, rest): (Vec<_>, Vec<_>) = pieces(paste)
        .into_iter()
        .partition(|piece| image_path(piece).is_some());
    let files: Vec<String> = named
        .iter()
        .filter_map(|piece| image_path(piece))
        .filter(|path| std::fs::read(path).is_ok_and(|bytes| cf_e2e::png::inspect(&bytes).is_ok()))
        .collect();
    if files.is_empty() {
        return None;
    }
    let numbers: Vec<u32> = files.iter().map(|_| number()).collect();
    let marks: Vec<String> = numbers.iter().map(|at| format!("[Image #{at}]")).collect();
    Some(Attached {
        text: format!("{}{}", marks.join(" "), rest.join("\n")),
        numbers,
        files,
    })
}

/// The user's record of a message with no attachment made of it: its words as they are.
pub fn unattached(text: &str) -> Value {
    json!({ "type": "user", "message": { "role": "user", "content": text } })
}

/// The user's records a message `paste` is written as: the message alone, or, where
/// it names images the window can read, the message with its attachments and the
/// companion that says where they came from. `number` numbers the placeholders.
pub fn user_records(paste: &str, number: impl FnMut() -> u32) -> Vec<Value> {
    let Some(attached) = attach(paste, number) else {
        return vec![unattached(paste)];
    };
    let mut content = vec![json!({ "type": "text", "text": attached.text })];
    content.extend(attached.files.iter().map(|_| {
        json!({ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "" } })
    }));
    let sources: Vec<Value> = attached
        .files
        .iter()
        .map(|file| json!({ "type": "text", "text": format!("[Image: source: {file}]") }))
        .collect();
    vec![
        json!({
            "type": "user",
            "message": { "role": "user", "content": content },
            "imagePasteIds": attached.numbers,
        }),
        json!({
            "type": "user", "isMeta": true, "turnCompanion": true,
            "message": { "role": "user", "content": sources },
        }),
    ]
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// A folder with the pictures a paste names: two PNGs, a text file, and a file
    /// that is called a PNG and is not one.
    fn folder() -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        for name in ["desk.png", "hero.png"] {
            std::fs::write(folder.path().join(name), cf_e2e::png::SMALL).unwrap();
        }
        std::fs::create_dir(folder.path().join("my images")).unwrap();
        std::fs::write(folder.path().join("my images/art.png"), cf_e2e::png::SMALL).unwrap();
        std::fs::write(folder.path().join("notes.txt"), "notes").unwrap();
        std::fs::write(folder.path().join("fake.png"), "no picture").unwrap();
        folder
    }

    fn at(folder: &Path, name: &str) -> String {
        folder.join(name).display().to_string()
    }

    /// The words `paste` is kept as, its placeholders numbered from `from`.
    fn kept(paste: &str, from: u32) -> Option<String> {
        let mut at = from - 1;
        attach(paste, || {
            at += 1;
            at
        })
        .map(|attached| attached.text)
    }

    #[test]
    fn a_result_that_starts_with_the_paths_of_its_pictures_is_kept_as_the_field_showed_it() {
        let folder = folder();
        let (desk, hero) = (at(folder.path(), "desk.png"), at(folder.path(), "hero.png"));
        let header = "[ConsensFlow m-2619 · T-45 · result from @pygmalion-jolly-brook]";
        assert_eq!(
            kept(&format!("{header}\n{desk}\nWarm editorial desk scene."), 4).unwrap(),
            format!("[Image #4]{header}\nWarm editorial desk scene.")
        );
        // Two pictures: a space between the placeholders, and none before the header.
        assert_eq!(
            kept(&format!("{header}\n{desk}\n{hero}\nWarm."), 2).unwrap(),
            format!("[Image #2] [Image #3]{header}\nWarm.")
        );
        // On one line, and mid body: the same, the placeholders at the head.
        assert_eq!(
            kept(&format!("{header}\nSaved:\n{desk} {hero}\nWarm."), 1).unwrap(),
            format!("[Image #1] [Image #2]{header}\nSaved:\nWarm.")
        );
    }

    #[test]
    fn a_path_in_a_sentence_a_missing_image_and_a_file_that_is_no_image_leave_the_paste_as_it_was()
    {
        let folder = folder();
        let desk = at(folder.path(), "desk.png");
        for paste in [
            format!("Saved to {desk} as asked."),
            format!("{}\nWarm.", at(folder.path(), "gone.png")),
            format!("{}\nWarm.", at(folder.path(), "notes.txt")),
            format!("{}\nWarm.", at(folder.path(), "fake.png")),
        ] {
            assert_eq!(kept(&paste, 1), None, "{paste}");
        }
    }

    #[test]
    fn an_image_that_could_not_be_read_is_gone_from_the_words_beside_one_that_could() {
        let folder = folder();
        let paste = format!(
            "Done\n{}\n{}",
            at(folder.path(), "gone.png"),
            at(folder.path(), "desk.png")
        );
        assert_eq!(kept(&paste, 1).unwrap(), "[Image #1]Done");
    }

    #[test]
    fn a_quoted_path_and_a_path_with_an_escaped_space_name_their_files() {
        let folder = folder();
        let spaced = at(folder.path(), "my images/art.png");
        let quoted = format!("'{spaced}'");
        let escaped = spaced.replace(' ', "\\ ");
        for paste in [quoted, escaped] {
            assert_eq!(
                kept(&format!("Head\n{paste}"), 1).unwrap(),
                "[Image #1]Head"
            );
        }
    }

    #[test]
    fn the_message_has_an_image_for_each_file_and_a_companion_that_says_where_they_came_from() {
        let folder = folder();
        let (desk, hero) = (at(folder.path(), "desk.png"), at(folder.path(), "hero.png"));
        let mut at = 0;
        let records = user_records(&format!("Head\n{desk}\n{hero}\nTail"), || {
            at += 1;
            at
        });
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0]["message"]["content"][0]["text"],
            "[Image #1] [Image #2]Head\nTail"
        );
        assert_eq!(records[0]["message"]["content"][1]["type"], "image");
        assert_eq!(records[0]["message"]["content"][2]["type"], "image");
        assert_eq!(records[0]["imagePasteIds"], json!([1, 2]));
        assert_eq!(records[1]["isMeta"], true);
        assert_eq!(
            records[1]["message"]["content"],
            json!([
                { "type": "text", "text": format!("[Image: source: {desk}]") },
                { "type": "text", "text": format!("[Image: source: {hero}]") },
            ])
        );
    }

    #[test]
    fn a_message_with_no_attachment_made_of_it_is_the_record_of_its_words_as_they_are() {
        let folder = folder();
        let words = format!("Head\n{}\nTail", at(folder.path(), "desk.png"));
        assert_eq!(
            unattached(&words),
            json!({ "type": "user", "message": { "role": "user", "content": words } })
        );
    }

    #[test]
    fn a_paste_that_names_no_readable_image_is_one_record_of_its_own_words() {
        let records = user_records("Reply with exactly: X", || unreachable!());
        assert_eq!(
            records,
            [
                json!({ "type": "user", "message": { "role": "user", "content": "Reply with exactly: X" } })
            ]
        );
    }

    #[test]
    fn a_drive_letter_path_after_a_space_starts_a_piece_of_its_own() {
        assert_eq!(
            pieces("See C:\\work\\a.png and /tmp/b.png\nend"),
            ["See", "C:\\work\\a.png and", "/tmp/b.png", "end"]
        );
    }
}
