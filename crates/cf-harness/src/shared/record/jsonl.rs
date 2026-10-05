//! A JSONL file read on from where an earlier look stopped (`readOn`,
//! `hosts/lib/completion/shared.js`), without holding the file between
//! looks.
//!
//! A line is a record once its newline is written. An unterminated last
//! line that is already whole JSON is visited too, and remembered, so that
//! the newline which ends it later adds nothing. One that is not whole yet
//! waits for a later look, and one that never can be fails, as a malformed
//! whole line does.
//!
//! The file read so far is known by four things: its identity, the bytes
//! just before where the last look stopped (the edge), the unterminated
//! record it remembered (the tail), and its size and time of writing. A
//! file of another identity, or one that no longer holds the edge or the
//! tail, is not the one read so far: it shrank, or was written over.

mod prefix;

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use cf_base::file::{identity, mtime_ms, Identity};
use cf_base::json::{from_slice_lossy, is_json_lossy, DEEPEST};
use serde_json::Value;

use prefix::{json_prefix_state, Prefix};

/// How many bytes before where a look stopped the next look checks are unchanged.
const EDGE_BYTES: u64 = 1024;
/// How much of the file a look reads at a time.
const CHUNK: usize = 64 * 1024;

/// Where a look at a file stopped: the next look reads on from here.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Seen {
    identity: Identity,
    size: u64,
    mtime_ms: f64,
    /// The byte after the last whole line.
    offset: u64,
    /// The bytes before `offset`, up to `EDGE_BYTES` of them.
    edge: Vec<u8>,
    /// An unterminated last record already visited: its bytes, after `offset`.
    tail: Vec<u8>,
    /// How many records were visited.
    records: usize,
}

impl Seen {
    /// When the file was last written, as the look found it (`mtimeMs`).
    pub(crate) fn mtime_ms(&self) -> f64 {
        self.mtime_ms
    }
}

/// Why a look stopped short.
#[derive(Debug)]
pub(crate) enum Stop {
    /// The record cannot be read, for this reason.
    Failed(String),
    /// The file could not be read.
    Io(io::Error),
    /// What a parser's `visit` returns to have the transcript read again from
    /// its start (`REREAD`): not a failure, and no reading says it.
    Reread,
}

impl Stop {
    /// What a reading says of it, after `unreadable: `.
    pub(crate) fn reason(&self) -> String {
        match self {
            Stop::Failed(reason) => reason.clone(),
            Stop::Io(error) => error.to_string(),
            Stop::Reread => "read the transcript again".to_owned(),
        }
    }
}

impl From<io::Error> for Stop {
    fn from(error: io::Error) -> Self {
        Stop::Io(error)
    }
}

/// What a look found.
#[derive(Debug)]
pub(crate) enum Looked {
    /// The file did not change: the look that stopped at `seen` is current.
    Unchanged,
    /// The file read on: the next look starts here.
    Read(Seen),
    /// Another file is in its place, or it no longer holds what was read.
    NotTheFile,
}

/// Reads on in the JSONL `file` from `seen` (none reads from the start),
/// handing each record to `visit` with its place among the file's records.
/// With `only`, a line that does not hold those bytes is passed over
/// unparsed: a log of many conversations, read for one, parses that one's
/// lines alone.
pub(crate) fn read_on(
    file: &Path,
    seen: Option<&Seen>,
    visit: &mut dyn FnMut(Value, usize) -> Result<(), Stop>,
    only: Option<&[u8]>,
) -> Result<Looked, Stop> {
    let mut handle = File::open(file)?;
    let metadata = handle.metadata()?;
    let file_identity = identity(&handle)?;
    let size = metadata.len();
    let written = mtime_ms(&metadata)?;
    if let Some(seen) = seen {
        if file_identity != seen.identity {
            return Ok(Looked::NotTheFile);
        }
        if size == seen.size && written == seen.mtime_ms {
            return Ok(Looked::Unchanged);
        }
        let edge_start = seen.offset - seen.edge.len() as u64;
        if !holds(&mut handle, edge_start, &seen.edge)?
            || !holds(&mut handle, seen.offset, &seen.tail)?
        {
            return Ok(Looked::NotTheFile);
        }
    }
    let mut offset = seen.map_or(0, |seen| seen.offset);
    let mut tail = seen.map(|seen| seen.tail.clone()).unwrap_or_default();
    let mut records = seen.map_or(0, |seen| seen.records);
    let holds_only = |line: &[u8]| only.is_none_or(|wanted| contains(line, wanted));
    // The line being read; `visited` while it is the rest of `tail`'s line.
    let mut line: Vec<u8> = Vec::new();
    let mut visited = !tail.is_empty();
    let mut at = offset + tail.len() as u64;
    if at < size {
        handle.seek(SeekFrom::Start(at))?;
        let mut rest = (&mut handle).take(size - at);
        let mut chunk = vec![0; CHUNK];
        loop {
            let read = rest.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            let mut start = 0;
            while let Some(newline) = chunk[start..read].iter().position(|byte| *byte == b'\n') {
                let end = start + newline;
                line.extend_from_slice(&chunk[start..end]);
                if visited {
                    // A record visited whole may be followed by nothing but white space.
                    if !is_blank(&line) {
                        return Ok(Looked::NotTheFile);
                    }
                    visited = false;
                    tail.clear();
                } else if holds_only(&line) {
                    records = consume_line(&line, records, visit)?;
                }
                line.clear();
                offset = at + end as u64 + 1;
                start = end + 1;
            }
            line.extend_from_slice(&chunk[start..read]);
            at += read as u64;
        }
    }
    if visited {
        if !is_blank(&line) {
            return Ok(Looked::NotTheFile);
        }
    } else if !is_blank(&line) && holds_only(&line) {
        let text = String::from_utf8_lossy(&line);
        match from_slice_lossy(text.as_bytes()) {
            Ok(record) => {
                visit(record, records)?;
                records += 1;
                tail = line;
            }
            // A live writer may have left only the final, unterminated append.
            Err(_) if json_prefix_state(&text) == Prefix::Incomplete => {}
            Err(_) => return Err(unread(&line, records)),
        }
    }
    let edge = match seen {
        Some(seen) if seen.offset == offset => seen.edge.clone(),
        _ => bytes_at(&mut handle, offset.saturating_sub(EDGE_BYTES), offset)?,
    };
    Ok(Looked::Read(Seen {
        identity: file_identity,
        size: at,
        mtime_ms: written,
        offset,
        edge,
        tail,
        records,
    }))
}

/// One whole line: a record for `visit` at `index`, nothing when blank, a
/// failure when malformed. A carriage return before the newline is JSON's
/// white space: a line ended `\r\n` reads as one ended `\n`.
fn consume_line(
    raw: &[u8],
    index: usize,
    visit: &mut dyn FnMut(Value, usize) -> Result<(), Stop>,
) -> Result<usize, Stop> {
    if is_blank(raw) {
        return Ok(index);
    }
    let record = from_slice_lossy(raw).map_err(|_| unread(raw, index))?;
    visit(record, index)?;
    Ok(index + 1)
}

/// Why the line at `index`, which is no record here, fails its look: it is
/// malformed, as Node said; or it is JSON a value here cannot hold, nested
/// past [`DEEPEST`] levels or with a number past a double's range. Node
/// read such a line, and this does not: a difference kept on purpose, said
/// in a sentence of its own so that a store holding one can be counted.
fn unread(line: &[u8], index: usize) -> Stop {
    if is_json_lossy(line) {
        Stop::Failed(format!(
            "JSON this build cannot hold at record {index}: nested past {DEEPEST} levels, or a number past a double's range"
        ))
    } else {
        Stop::Failed(format!("malformed JSONL at record {index}"))
    }
}

/// JSON's own white space: four bytes, where JavaScript's `trim` takes many more.
const fn is_json_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

fn is_blank(bytes: &[u8]) -> bool {
    bytes.iter().copied().all(is_json_space)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// The bytes of `handle` from `from` to `to`, fewer where the file ends first.
fn bytes_at(handle: &mut File, from: u64, to: u64) -> io::Result<Vec<u8>> {
    handle.seek(SeekFrom::Start(from))?;
    let mut bytes = Vec::new();
    handle.take(to - from).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Whether the file still holds `bytes` at `at`.
fn holds(handle: &mut File, at: u64, bytes: &[u8]) -> io::Result<bool> {
    if bytes.is_empty() {
        return Ok(true);
    }
    Ok(bytes_at(handle, at, at + bytes.len() as u64)? == bytes)
}

#[cfg(test)]
mod tests;
