//! A project's specs folder as the viewer reads it: what it holds, whether it
//! holds a file at all, one file at a time, and an HTML file with the assets it
//! names, read for the browser's copy; read and never written.
//!
//! Nothing outside the folder is read. The folder is found by a walk from the
//! project's own folder, each of its names opened in the one above and none of
//! them a link ([`open`]). A path the page names is relative to the folder and
//! written in slashes ([`parts`]), and is opened through the
//! [`door`](super::door): each name through the folder above it, never through
//! a link, so the file read is the file checked. A dotfile is never read and
//! never listed (`.DS_Store`), nor, on Windows, a name that may be a dotfile's
//! short (8.3) name ([`is_short_alias`]); neither is a link, nor anything that
//! is not a regular file or a folder; a file of more than one name is refused.
//!
//! What is read has limits, each said in words where it is passed: a tree of at
//! most [`MAX_ENTRIES`] entries, [`MAX_DEPTH`] folders deep, whose answer fits
//! the bridge's frame ([`MAX_TREE_BYTES`] of names), read no further than
//! [`MAX_SCANNED`] names (every name a folder gives counts, those passed over
//! too), so a folder of a million files costs no more than one of ten
//! thousand; and a file of at most [`MAX_FILE`] bytes whose answer fits the
//! frame with room to spare ([`MAX_ANSWER`]). A file over them, or one that is
//! neither text nor an image, is answered all the same: its size, and why it
//! is not shown. An HTML file for the browser is at most [`MAX_FILE`] too, and
//! so is each of the assets it names ([`browser`]).

use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::rc::Rc;

use base64::Engine as _;
use serde::Serialize;

use super::door::{Held, Kind, Opened, Reading, Refused};
use super::names::{is_short_alias, parts};

mod browser;

pub(super) use browser::ForBrowser;

/// The most entries, files and folders together, a tree shows; the rest are cut.
pub(super) const MAX_ENTRIES: usize = 2000;
/// The most names a walk reads, dotfiles and links included, before it stops.
pub(super) const MAX_SCANNED: usize = 10_000;
/// How many folders deep a tree goes: a folder deeper is shown empty.
pub(super) const MAX_DEPTH: usize = 8;
/// The most a tree's names may take in its answer, encoded as the bridge carries
/// them, with what each entry takes besides ([`ENTRY_COST`]): a frame is 1 MiB.
pub(super) const MAX_TREE_BYTES: usize = 640 * 1024;
/// What an entry's answer takes besides its name and its path.
const ENTRY_COST: usize = 64;
/// The largest file shown, in bytes.
pub(super) const MAX_FILE: u64 = 512 * 1024;
/// The most a file's content may take in the answer, encoded as the bridge
/// carries it: JSON can write a text twice as long as it is, and a frame is
/// 1 MiB.
const MAX_ANSWER: usize = 768 * 1024;
/// What a file is shown as an image by, its extension: the page draws it as a
/// picture, never as markup, so an SVG's script never runs.
const IMAGES: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];
/// What a file opens in the browser by, its extension.
const HTML: [&str; 2] = ["html", "htm"];

/// What a tree says where it holds more entries than [`MAX_ENTRIES`].
pub(super) const MORE_ENTRIES: &str =
    "The folder holds more than 2000 files and folders: 2000 of them are shown.";
/// What a tree says where it holds more names than [`MAX_SCANNED`].
pub(super) const MORE_NAMES: &str =
    "The folder holds more than 10000 names: the viewer read no further.";
/// What a tree says where it has folders deeper than [`MAX_DEPTH`].
pub(super) const DEEPER: &str = "Folders more than 8 deep are shown empty.";
/// What a tree says where its names would not fit one answer.
pub(super) const TOO_LONG: &str =
    "The folder's names are too long to send whole: those that fit are shown.";

/// A project's specs folder, found and held open, and the looks at it.
pub(super) struct Folder {
    root: Held,
    reading: Rc<Reading>,
}

/// What a folder holds, by name; a file with its size.
#[derive(Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub(super) enum Entry {
    File {
        name: String,
        path: String,
        size: u64,
    },
    Folder {
        name: String,
        path: String,
        entries: Vec<Entry>,
    },
}

/// The folder's tree, and what was cut of it, in words, where a limit was passed.
#[derive(Debug, Serialize)]
pub(super) struct Tree {
    pub(super) entries: Vec<Entry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) cut: Option<String>,
}

/// One file as the viewer is given it: its path and size, and its text, its
/// bytes as an image (base64), or why it is withheld.
#[derive(Debug, Serialize)]
pub(super) struct Shown {
    pub(super) path: String,
    pub(super) size: u64,
    #[serde(flatten)]
    pub(super) content: Content,
}

/// What a file is shown as.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Content {
    Text(String),
    Image(String),
    Withheld(String),
}

/// The folder `folder` (as the ledger keeps it: relative, in slashes) of the
/// project in `project`: none where it is not there. It is found by a walk
/// from the project's own folder, each of its names opened in the one above,
/// none of them a link and each a folder, so it is inside the project for as
/// long as it is held, whatever its path names meanwhile. `between` is called
/// once the project's folder is held (where a test swaps a folder on the way).
pub(super) fn open(
    project: &Path,
    folder: &str,
    between: &dyn Fn(),
) -> Result<Option<Folder>, String> {
    let named = parts(folder, cfg!(windows))?;
    let reading = Reading::new();
    reading.begin(MAX_SCANNED);
    let mut held = match Held::project(project, &reading) {
        Ok(held) => held,
        Err(Refused::Missing) => return Ok(None),
        Err(Refused::Io(error)) => {
            return Err(format!("the project's folder cannot be read: {error}"))
        }
        Err(_) => return Err("the project's folder is no folder".to_owned()),
    };
    between();
    for (at, name) in named.iter().enumerate() {
        let last = at + 1 == named.len();
        let on_the_way = named[..=at].join("/");
        held = match held.folder(name) {
            Ok(next) => next,
            Err(Refused::Missing) => return Ok(None),
            Err(Refused::Link) if last => {
                return Err(format!(
                    "the specs folder {folder} is a link: the viewer follows none"
                ))
            }
            Err(Refused::Link) => {
                return Err(format!(
                    "the specs folder {folder} goes through {on_the_way}, a link: the viewer \
                     follows none"
                ))
            }
            Err(Refused::Io(error)) => {
                return Err(format!("the specs folder {folder} cannot be read: {error}"))
            }
            Err(_) if last => {
                return Err(format!("the specs folder {folder} is a file, not a folder"))
            }
            Err(_) => {
                return Err(format!(
                    "the specs folder {folder} goes through {on_the_way}, which is no folder"
                ))
            }
        };
    }
    Ok(Some(Folder {
        root: held,
        reading,
    }))
}

impl Folder {
    /// A new look at the folder begins (a tree, whether it holds a file, a
    /// file, an HTML file with its assets), with a budget of [`MAX_SCANNED`]
    /// names, which its walk and its listings take from.
    fn look(&self) -> &Reading {
        self.reading.begin(MAX_SCANNED);
        &self.reading
    }

    /// Everything the folder holds but dotfiles, links and what is neither a
    /// file nor a folder, each folder by name, within the limits.
    pub(super) fn tree(&self) -> Tree {
        let mut walk = Walk {
            shown: MAX_ENTRIES,
            reading: self.look(),
            bytes: MAX_TREE_BYTES,
            cut: Vec::new(),
        };
        let entries = walk.folder(&self.root, "", 0);
        Tree {
            entries,
            cut: (!walk.cut.is_empty()).then(|| walk.cut.join(" ")),
        }
    }

    /// Whether the folder holds a file anywhere a tree would show it: a walk
    /// that ends at the first, within the same limits of names and depth.
    pub(super) fn present(&self) -> bool {
        holds(&self.root, 0, self.look())
    }

    /// The file at `path`, as the page names one; `between` is called between
    /// the look at the file and its opening (a test swaps it there).
    pub(super) fn read(&self, path: &str, between: &dyn Fn()) -> Result<Shown, String> {
        self.look();
        let opened = self.open_file(path, between)?;
        let content =
            content(opened.file, path, opened.size).map_err(|error| cannot(path, &error))?;
        Ok(Shown {
            path: path.to_owned(),
            size: opened.size,
            content,
        })
    }

    /// The regular file at `path`, opened through each folder above it.
    fn open_file(&self, path: &str, between: &dyn Fn()) -> Result<Opened, String> {
        let named = parts(path, cfg!(windows))?;
        left_out(path, &named, cfg!(windows))?;
        let Some((file, folders)) = named.split_last() else {
            return Err(format!("{path} names no file"));
        };
        let mut held: Vec<Held> = Vec::with_capacity(folders.len());
        for (at, name) in folders.iter().enumerate() {
            let next = held
                .last()
                .unwrap_or(&self.root)
                .folder(name)
                .map_err(|refused| through(path, &folders[..=at].join("/"), refused))?;
            held.push(next);
        }
        held.last()
            .unwrap_or(&self.root)
            .file(file, between)
            .map_err(|refused| not_read(path, refused))
    }
}

/// Why the viewer leaves out `path` (its names `named`), in words: a path
/// through a dotfile, which is never listed, and on Windows (`windows`) one
/// through a name that may be a dotfile's short (8.3) name (`GIT~1`).
fn left_out(path: &str, named: &[&str], windows: bool) -> Result<(), String> {
    if named.iter().any(|part| part.starts_with('.')) {
        return Err(format!("{path} is a dotfile, which the viewer leaves out"));
    }
    if let Some(part) = named.iter().find(|part| is_short_alias(part, windows)) {
        return Err(format!(
            "{path} names {part}, which may be the short (8.3) name of a dotfile: the viewer \
             reads no such name on Windows"
        ));
    }
    Ok(())
}

/// Why the file at `path` was not read, in words.
fn not_read(path: &str, refused: Refused) -> String {
    match refused {
        Refused::Missing => format!("no file {path} in the specs folder"),
        Refused::Link => format!("{path} is a link: the viewer opens no link"),
        Refused::Folder => format!("{path} is a folder, not a file"),
        Refused::NotFolder => format!("{path} cannot be read: it changed while it was opened"),
        Refused::Other(what) => format!("{path} is {what}, not a regular file"),
        Refused::Names(names) => format!(
            "{path} has {names} names (hard links): the viewer reads only a file of one name, \
             which no path outside the folder can share"
        ),
        Refused::Io(error) => cannot(path, &error),
    }
}

/// Why the file at `path` was not read where the folder `folder` on its way was
/// not opened, in words.
fn through(path: &str, folder: &str, refused: Refused) -> String {
    match refused {
        Refused::Missing => format!("no file {path} in the specs folder"),
        Refused::Link => format!("{path} goes through {folder}, a link: the viewer follows none"),
        Refused::Other(what) => format!("{path} goes through {folder}, which is {what}"),
        Refused::Io(error) => cannot(path, &error),
        _ => format!("{path} goes through {folder}, which is no folder"),
    }
}

fn cannot(path: &str, error: &io::Error) -> String {
    format!("{path} cannot be read: {error}")
}

/// What the opened `file` (`path` in the folder, `size` bytes) is shown as,
/// read through its handle.
fn content(file: fs::File, path: &str, size: u64) -> io::Result<Content> {
    let too_large = || {
        Content::Withheld(format!(
            "{path} is {}, more than the {} the viewer shows",
            measure(size),
            measure(MAX_FILE)
        ))
    };
    if size > MAX_FILE {
        return Ok(too_large());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    // Grown since it was measured.
    if bytes.len() as u64 > MAX_FILE {
        return Ok(too_large());
    }
    let encoded = if has_extension(path, &IMAGES) {
        Content::Image(base64::engine::general_purpose::STANDARD.encode(&bytes))
    } else {
        match String::from_utf8(bytes) {
            Ok(text) if !text.contains('\0') => Content::Text(text),
            _ => {
                return Ok(Content::Withheld(format!(
                    "{path} is {} that is neither text nor an image",
                    measure(size)
                )))
            }
        }
    };
    let answer = match &encoded {
        Content::Text(text) => encoded_len(text),
        Content::Image(image) => image.len(),
        Content::Withheld(_) => 0,
    };
    Ok(if answer > MAX_ANSWER {
        too_large()
    } else {
        encoded
    })
}

/// Whether `path` ends in one of `extensions`, whatever its case.
fn has_extension(path: &str, extensions: &[&str]) -> bool {
    path.rsplit_once('.').is_some_and(|(_, extension)| {
        extensions
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
    })
}

/// How long `text` is as JSON writes it.
fn encoded_len(text: &str) -> usize {
    serde_json::to_string(text).map_or(usize::MAX, |json| json.len())
}

/// A size as it is said: `812 bytes`, `12.5 KB`, `3.2 MB` (of 1024).
pub(super) fn measure(size: u64) -> String {
    const KB: u64 = 1024;
    let at = |unit: u64| size as f64 / unit as f64;
    if size < KB {
        format!("{size} bytes")
    } else if size < KB * KB {
        format!("{:.1} KB", at(KB))
    } else {
        format!("{:.1} MB", at(KB * KB))
    }
}

/// A walk of the folder: what is left of each budget (of names, the look's),
/// and the limits it passed.
struct Walk<'a> {
    shown: usize,
    reading: &'a Reading,
    bytes: usize,
    cut: Vec<&'static str>,
}

impl Walk<'_> {
    /// Says a limit was passed, once.
    fn passed(&mut self, limit: &'static str) {
        if !self.cut.contains(&limit) {
            self.cut.push(limit);
        }
    }

    /// What the held folder (`at` in the tree, `depth` folders down) holds, by
    /// name: its names read until the budget of names is spent, then each kept
    /// one given its slot and its bytes before a folder of them is walked in its
    /// turn. One that cannot be read holds nothing.
    fn folder(&mut self, held: &Held, at: &str, depth: usize) -> Vec<Entry> {
        let mut found = Vec::new();
        if let Ok(names) = held.entries() {
            for named in names {
                if !self.reading.charge() {
                    self.passed(MORE_NAMES);
                    break;
                }
                match named {
                    Ok(Some((name, kind @ (Kind::File { .. } | Kind::Folder))))
                        if !name.starts_with('.') =>
                    {
                        found.push((name, kind));
                    }
                    _ => {}
                }
            }
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        let mut entries = Vec::new();
        for (name, kind) in found {
            if self.shown == 0 {
                self.passed(MORE_ENTRIES);
                break;
            }
            let path = if at.is_empty() {
                name.clone()
            } else {
                format!("{at}/{name}")
            };
            let cost = encoded_len(&name) + encoded_len(&path) + ENTRY_COST;
            if cost > self.bytes {
                self.passed(TOO_LONG);
                break;
            }
            // The entry's slot is its own before anything inside it is walked.
            self.shown -= 1;
            self.bytes -= cost;
            match kind {
                Kind::File { size } => entries.push(Entry::File { name, path, size }),
                _ => {
                    let inside = if depth + 1 >= MAX_DEPTH {
                        self.passed(DEEPER);
                        Vec::new()
                    } else {
                        match held.listed_folder(&name) {
                            Ok(inner) => self.folder(&inner, &path, depth + 1),
                            // Swapped since it was listed: a link, or gone. It is left out.
                            Err(_) => {
                                self.shown += 1;
                                self.bytes += cost;
                                continue;
                            }
                        }
                    };
                    entries.push(Entry::Folder {
                        name,
                        path,
                        entries: inside,
                    });
                }
            }
        }
        entries
    }
}

/// Whether the held folder (`depth` folders down) holds a file a tree would
/// show, its own first, then its folders' by name; no more names are read than
/// the look's budget allows.
fn holds(held: &Held, depth: usize, reading: &Reading) -> bool {
    let mut folders = Vec::new();
    let Ok(names) = held.entries() else {
        return false;
    };
    for named in names {
        if !reading.charge() {
            return false;
        }
        match named {
            Ok(Some((name, _))) if name.starts_with('.') => {}
            Ok(Some((_, Kind::File { .. }))) => return true,
            Ok(Some((name, Kind::Folder))) => folders.push(name),
            _ => {}
        }
    }
    if depth + 1 >= MAX_DEPTH {
        return false;
    }
    folders.sort();
    folders.iter().any(|name| {
        held.listed_folder(name)
            .is_ok_and(|inner| holds(&inner, depth + 1, reading))
    })
}

#[cfg(test)]
mod tests;
