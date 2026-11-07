//! What an image file is, read from its header: its size in pixels, its type, and whether it
//! has alpha (designer P1, T-1280). When a design task finishes, the chief gets, after the
//! designer's words, a line for each image file the words name, so the chief learns whether the
//! size and the transparency it ordered were met without opening the picture. Codex's image tool
//! draws at a size of its own (1672x941 for an order of 1280x720: the words set the aspect, not
//! the pixels) and the window may resize or convert the file after.
//!
//! Only headers are read, never pixels, and never more than [`MOST_BYTES`] of a file:
//! - **PNG**: the header chunk says the size and the colour type; a colour type with an alpha
//!   channel (4, 6) or a `tRNS` chunk before the pixel data (a palette's, or one colour made
//!   clear) is alpha. The chunks between are walked by their lengths, not read: the picture
//!   Codex saves has a provenance chunk of its own before the pixels.
//! - **JPEG**: the frame header says the size; a JPEG has no alpha.
//! - **WebP**: the lossy frame says the size and has no alpha; the lossless one says both; the
//!   extended one says both in its flags.
//!
//! What is not one of these, a file that cannot be read, one cut inside its header, and a path
//! that is gone, has no line: ConsensFlow says what it read and nothing else.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;

use cf_ledger::TaskView;
use regex::Regex;

/// The most of a file a reading looks through: a header is in the first bytes of a picture,
/// and the chunks and segments before the pixels are walked by their lengths.
pub const MOST_BYTES: u64 = 4 * 1024 * 1024;

/// The most image files one result gets a line for.
pub const MOST_FILES: usize = 12;

/// An image file's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Png,
    Jpeg,
    Webp,
}

impl Kind {
    /// How the line says it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Png => "PNG",
            Kind::Jpeg => "JPEG",
            Kind::Webp => "WebP",
        }
    }
}

/// What an image file's header says of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    pub width: u32,
    pub height: u32,
    pub kind: Kind,
    pub alpha: bool,
}

/// The facts of the image `file` holds, or none where it is none of the types above, is cut
/// inside its header, or its header says a size of no pixels.
pub fn facts_of<F: Read + Seek>(file: &mut F) -> Option<Facts> {
    let mut start = [0_u8; 12];
    let got = read_up_to(file, &mut start)?;
    let start = &start[..got];
    if start.starts_with(&PNG) {
        png(file)
    } else if start.starts_with(&[0xFF, 0xD8]) {
        jpeg(file)
    } else if start.len() == 12 && &start[..4] == b"RIFF" && &start[8..] == b"WEBP" {
        webp(file)
    } else {
        None
    }
}

/// The facts of the file at `path`, or none where it cannot be opened or is no image.
pub fn read(path: &Path) -> Option<Facts> {
    let mut file = File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    facts_of(&mut file)
}

/// What every PNG starts with.
const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// As many bytes as `file` has for `buffer`, up to its length; none on an error.
fn read_up_to<F: Read>(file: &mut F, buffer: &mut [u8]) -> Option<usize> {
    let mut got = 0;
    while got < buffer.len() {
        match file.read(&mut buffer[got..]) {
            Ok(0) => break,
            Ok(count) => got += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(got)
}

/// Exactly `buffer.len()` bytes, or none where the file ends first.
fn read_exactly<F: Read>(file: &mut F, buffer: &mut [u8]) -> Option<()> {
    (read_up_to(file, buffer)? == buffer.len()).then_some(())
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// A picture of a size a picture can have: some pixels in each direction.
fn sized(width: u32, height: u32) -> Option<(u32, u32)> {
    (width > 0 && height > 0).then_some((width, height))
}

/// A PNG, `file` read past its signature: the header chunk says the size and the colour type,
/// and the chunks after it are walked by their lengths to the pixels, looking for a
/// transparency chunk.
fn png<F: Read + Seek>(file: &mut F) -> Option<Facts> {
    file.seek(SeekFrom::Start(PNG.len() as u64)).ok()?;
    let mut head = [0_u8; 8 + 13];
    read_exactly(file, &mut head)?;
    if &head[4..8] != b"IHDR" || be32(&head) != 13 {
        return None;
    }
    let (width, height) = sized(be32(&head[8..]), be32(&head[12..]))?;
    let colour_type = head[17];
    // Colour types 4 and 6 have an alpha channel of their own; the others have it where a
    // transparency chunk says so, which comes before the pixels.
    let mut alpha = matches!(colour_type, 4 | 6);
    if !alpha {
        // The header's checksum, then the chunks.
        let mut at = PNG.len() as u64 + 8 + 13 + 4;
        let mut chunk = [0_u8; 8];
        while at < MOST_BYTES {
            file.seek(SeekFrom::Start(at)).ok()?;
            if read_exactly(file, &mut chunk).is_none() {
                break;
            }
            match &chunk[4..] {
                b"tRNS" => {
                    alpha = true;
                    break;
                }
                b"IDAT" | b"IEND" => break,
                _ => {}
            }
            at += 8 + u64::from(be32(&chunk)) + 4;
        }
    }
    Some(Facts {
        width,
        height,
        kind: Kind::Png,
        alpha,
    })
}

/// A JPEG: its segments walked by their lengths to the frame header, which says the size.
fn jpeg<F: Read + Seek>(file: &mut F) -> Option<Facts> {
    let mut at = 2_u64;
    let mut marker = [0_u8; 4];
    while at < MOST_BYTES {
        file.seek(SeekFrom::Start(at)).ok()?;
        read_exactly(file, &mut marker)?;
        if marker[0] != 0xFF {
            return None;
        }
        match marker[1] {
            // Fill bytes before a marker.
            0xFF => at += 1,
            // A marker with no length: a restart, the start of an image, a temporary one.
            0x01 | 0xD0..=0xD7 | 0xD8 => at += 2,
            // The frame headers: every start of frame but the tables that share their numbers.
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                let mut frame = [0_u8; 5];
                read_exactly(file, &mut frame)?;
                let (width, height) = (
                    u32::from(u16::from_be_bytes([frame[3], frame[4]])),
                    u32::from(u16::from_be_bytes([frame[1], frame[2]])),
                );
                let (width, height) = sized(width, height)?;
                return Some(Facts {
                    width,
                    height,
                    kind: Kind::Jpeg,
                    alpha: false,
                });
            }
            // The start of the scan, or the end: no frame header came before.
            0xDA | 0xD9 => return None,
            _ => at += 2 + u64::from(u16::from_be_bytes([marker[2], marker[3]])),
        }
    }
    None
}

/// A WebP: the first chunk after the signature says it, in one of three forms.
fn webp<F: Read + Seek>(file: &mut F) -> Option<Facts> {
    file.seek(SeekFrom::Start(12)).ok()?;
    let mut head = [0_u8; 8];
    read_exactly(file, &mut head)?;
    let little = |bytes: &[u8]| {
        bytes
            .iter()
            .rev()
            .fold(0_u32, |all, byte| all << 8 | u32::from(*byte))
    };
    let (width, height, alpha) = match &head[..4] {
        b"VP8 " => {
            let mut frame = [0_u8; 10];
            read_exactly(file, &mut frame)?;
            if frame[3..6] != [0x9D, 0x01, 0x2A] {
                return None;
            }
            (
                little(&frame[6..8]) & 0x3FFF,
                little(&frame[8..10]) & 0x3FFF,
                false,
            )
        }
        b"VP8L" => {
            let mut frame = [0_u8; 5];
            read_exactly(file, &mut frame)?;
            if frame[0] != 0x2F {
                return None;
            }
            let bits = little(&frame[1..]);
            (
                (bits & 0x3FFF) + 1,
                ((bits >> 14) & 0x3FFF) + 1,
                bits >> 28 & 1 == 1,
            )
        }
        b"VP8X" => {
            let mut frame = [0_u8; 10];
            read_exactly(file, &mut frame)?;
            (
                little(&frame[4..7]) + 1,
                little(&frame[7..10]) + 1,
                frame[0] & 0x10 != 0,
            )
        }
        _ => return None,
    };
    let (width, height) = sized(width, height)?;
    Some(Facts {
        width,
        height,
        kind: Kind::Webp,
        alpha,
    })
}

/// A path that starts at a root: `/`, or a drive (`C:\`, `D:/`), after the start of a line or a
/// character that sets it off, and runs to the first image extension. Lazy, so that a line with
/// two paths gives two, and so that a path may hold a space.
#[allow(clippy::expect_used)]
static PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:^|[\s"'`(\[<:=,])((?:[a-z]:[\\/]|/)[^\n\r"'`<>|*?]*?\.(?:png|jpe?g|webp))(?:$|[^a-z0-9_])"#,
    )
    .expect("a pattern that compiles")
});

/// The absolute paths of image files `words` name (`.png`, `.jpg`, `.jpeg`, `.webp`), in order,
/// each once, at most [`MOST_FILES`]. A path may hold spaces; one that is not absolute is not
/// one a designer was told to give, and has no file to read.
pub fn files_named(words: &str) -> Vec<String> {
    let mut named: Vec<String> = Vec::new();
    for line in words.lines() {
        // A match ends before the character that follows its extension, which a following
        // path may start with, so each is looked for from where the last one ended.
        let mut rest = line;
        while let Some(found) = PATH.captures(rest) {
            let Some(path) = found.get(1) else { break };
            if !named.iter().any(|seen| seen == path.as_str()) {
                named.push(path.as_str().to_owned());
                if named.len() == MOST_FILES {
                    return named;
                }
            }
            rest = &rest[path.end()..];
        }
    }
    named
}

/// What ConsensFlow adds after a designer's `words`: one line for each image file they name that
/// can be read, none where no file is. Marked as ConsensFlow's, with the words before the path,
/// as a line of a path alone is cut from what a Claude Code window is pasted.
pub fn lines_for(words: &str) -> String {
    files_named(words)
        .into_iter()
        .filter_map(|path| {
            let facts = read(Path::new(&path))?;
            Some(format!(
                "(ConsensFlow read {path}: {}x{} pixels, {}, {})",
                facts.width,
                facts.height,
                facts.kind.name(),
                if facts.alpha {
                    "with alpha"
                } else {
                    "no alpha"
                }
            ))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `words`, and after them the lines of [`lines_for`], a line apart.
pub(crate) fn with_images(words: String) -> String {
    let lines = lines_for(&words);
    if lines.is_empty() {
        words
    } else {
        format!("{words}\n\n{lines}")
    }
}

/// The result `words` of `task`: for a design task, with the lines of the images it names; any
/// other task's is its words alone.
pub(crate) fn for_task(task: &TaskView, words: String) -> String {
    if task.pool.as_deref() == Some(DESIGNER_POOL) {
        with_images(words)
    } else {
        words
    }
}

/// The pool a design task (`cf task add --design`) is asked for.
const DESIGNER_POOL: &str = "designer";

#[cfg(test)]
mod tests;
