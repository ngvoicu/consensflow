//! A JSONL file read on from where an earlier look stopped, without holding the
//! file between looks.
//!
//! A line is a record once its newline is written. An unterminated last
//! line that is already whole JSON is visited too, and remembered, so that
//! the newline which ends it later adds nothing. One that is not whole yet
//! waits for a later look, and so does one that never can be: a torn line
//! still waiting for its newline.
//!
//! A whole line that is not JSON is torn: a short write (a full disk, a
//! process killed in a large write) left part of a record, and the next
//! append made one line of it and itself. The harness goes on past it, and so
//! does a look, once a record follows it: the line is passed over, once, and
//! the daemon's log told ([`torn`]). One with no record after it yet waits
//! for one, as an unfinished last line does, the records before it read, for
//! [`TORN_WAIT_MS`] at most, and is then passed over all the same, with the
//! torn lines after it. A look says where it passed lines over
//! ([`Seen::passed_over`]), which the reading carries. Whole records run
//! together in one line, a newline lost between them, are each read in their
//! place. An unfinished last line that is no record and has not grown for the
//! wait is said in the daemon's log, once. A line of JSON this build cannot
//! hold is no torn line: it fails the look.
//!
//! The file read so far is known by four things: its identity, the bytes
//! just before where the last look stopped (the edge), the unterminated
//! record it remembered (the tail), and its size and time of writing. A
//! file of another identity, or one that no longer holds the edge or the
//! tail, is not the one read so far: it shrank, or was written over. Its
//! size and time alone say it is unchanged only where that time was
//! [`settled`] when the look that found it began; one within a file time's
//! step of it is checked for its edge and tail.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use cf_base::file::{identity, mtime_ms, settled, Identity};
use cf_base::json::{from_slice_lossy, is_json_lossy, DEEPEST};
use serde::de::IgnoredAny;
use serde_json::Value;

use super::torn;

/// How many bytes before where a look stopped the next look checks are unchanged.
pub(crate) const EDGE_BYTES: u64 = 1024;
/// How much of the file a look reads at a time.
const CHUNK: usize = 64 * 1024;
/// How long a torn line with no record after it waits for one before it is
/// passed over, and an unfinished last line that does not grow before it is
/// said: 5 s. A belt: no shape is known in which a whole line that is not JSON
/// becomes one, since a writer's newline comes after its bytes. It costs
/// nothing, for the records before the line are read while it waits, so a
/// window that tore its last line and went quiet is never blind.
pub(crate) const TORN_WAIT_MS: i64 = 5_000;

/// Where a look at a file stopped: the next look reads on from here.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Seen {
    identity: Identity,
    size: u64,
    mtime_ms: f64,
    /// When the look began, in the clock's milliseconds.
    looked_ms: i64,
    /// The byte after the last whole line.
    offset: u64,
    /// The bytes before `offset`, up to `EDGE_BYTES` of them.
    edge: Vec<u8>,
    /// An unterminated last record already visited: its bytes, after `offset`.
    tail: Vec<u8>,
    /// How many records were visited.
    records: usize,
    /// The torn lines passed over, each by the place among the records it
    /// stood at: how many records were lost, and where.
    passed_over: Vec<usize>,
    /// When the look began that first found the line at `offset` torn, with
    /// no record after it: it waits for one, or for [`TORN_WAIT_MS`].
    torn_since: Option<i64>,
    /// The unfinished last line, at the file's `size`, that is no record.
    unfinished: Option<Unfinished>,
}

/// An unfinished last line that is no record, as long as when it was first found.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Unfinished {
    /// When the look began that first found it so.
    since: i64,
    /// It has not grown for [`TORN_WAIT_MS`], and the daemon's log was told.
    said: bool,
}

impl Seen {
    /// When the file was last written, as the look found it (`mtimeMs`).
    pub(crate) fn mtime_ms(&self) -> f64 {
        self.mtime_ms
    }

    /// The torn lines passed over, each by the place among the records it stood at.
    pub(crate) fn passed_over(&self) -> &[usize] {
        &self.passed_over
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
    /// The file did not change, as its bytes said where its time was too near the
    /// look that found it to: the look that stopped at `seen` is current, and
    /// this is it with this look's time, which the next look tells by.
    Held(Seen),
    /// The file read on: the next look starts here.
    Read(Seen),
    /// Another file is in its place, or it no longer holds what was read.
    NotTheFile,
}

/// Reads on in the JSONL `file` from `seen` (none reads from the start), in a
/// look that began at `looked_ms` (read before the file is), handing each
/// record to `visit` with its place among the file's records.
/// With `only`, a line that does not hold those bytes is passed over
/// unparsed: a log of many conversations, read for one, parses that one's
/// lines alone.
pub(crate) fn read_on(
    file: &Path,
    seen: Option<&Seen>,
    looked_ms: i64,
    visit: &mut dyn FnMut(Value, usize) -> Result<(), Stop>,
    only: Option<&[u8]>,
) -> Result<Looked, Stop> {
    read_on_with(
        file,
        seen,
        looked_ms,
        from_slice_lossy,
        &mut |record, index, _| visit(record, index),
        only,
    )
}

/// How a line is read into a record: a parser of the JSON of one line.
pub(crate) type Parse = fn(&[u8]) -> serde_json::Result<Value>;

/// [`read_on`], each line read by `parse`, and each record handed to `visit`
/// with the byte its line begins at too. It reads as `from_slice_lossy` does,
/// or fails where that fails: a line it cannot read is torn where that one is
/// malformed, and a failure of the look where it is too much for a value, said
/// as `read_on` says it.
pub(crate) fn read_on_with(
    file: &Path,
    seen: Option<&Seen>,
    looked_ms: i64,
    parse: Parse,
    visit: &mut dyn FnMut(Value, usize, u64) -> Result<(), Stop>,
    only: Option<&[u8]>,
) -> Result<Looked, Stop> {
    let mut handle = File::open(file)?;
    let metadata = handle.metadata()?;
    let file_identity = identity(&handle)?;
    let size = metadata.len();
    let written = mtime_ms(&metadata)?;
    let waited = |since: i64| looked_ms.saturating_sub(since) >= TORN_WAIT_MS;
    // The torn line the last look stopped at, if its wait is over: it is passed
    // over, and every torn line after it with it, the file changed or not.
    let done_waiting = seen
        .filter(|seen| seen.torn_since.is_some_and(waited))
        .map(|seen| seen.offset);
    // An unfinished last line that has not grown for the wait, and is not said yet.
    let unfinished_due = seen.is_some_and(|seen| {
        seen.unfinished
            .is_some_and(|it| !it.said && waited(it.since))
    });
    if let Some(seen) = seen {
        if file_identity != seen.identity {
            return Ok(Looked::NotTheFile);
        }
        let same = size == seen.size
            && written == seen.mtime_ms
            && done_waiting.is_none()
            && !unfinished_due;
        if same && settled(written, seen.looked_ms) {
            return Ok(Looked::Unchanged);
        }
        let edge_start = seen.offset - seen.edge.len() as u64;
        if !holds(&mut handle, edge_start, &seen.edge)?
            || !holds(&mut handle, seen.offset, &seen.tail)?
        {
            return Ok(Looked::NotTheFile);
        }
        // As long and as old as it was, and holding what was read: nothing follows it.
        if same {
            return Ok(Looked::Held(Seen {
                looked_ms,
                ..seen.clone()
            }));
        }
    }
    let mut offset = seen.map_or(0, |seen| seen.offset);
    let mut tail = seen.map(|seen| seen.tail.clone()).unwrap_or_default();
    let mut records = seen.map_or(0, |seen| seen.records);
    let mut passed = Passed {
        file,
        places: seen
            .map(|seen| seen.passed_over.clone())
            .unwrap_or_default(),
    };
    let holds_only = |line: &[u8]| only.is_none_or(|wanted| contains(line, wanted));
    // The torn lines read since the last record, which wait for one; the look
    // stops at the first unless one comes.
    let mut waiting: Vec<Torn> = Vec::new();
    // The torn line whose wait is over was met: the torn lines after it go with it.
    let mut passing = false;
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
            while let Some(newline) = memchr::memchr(b'\n', &chunk[start..read]) {
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
                    let (whole, torn) = whole_line(&line, records, parse)?;
                    let led = !whole.is_empty();
                    for record in whole {
                        passed.all(&mut waiting);
                        passing = false;
                        visit(record, records, offset)?;
                        records += 1;
                    }
                    if torn {
                        passing |= done_waiting == Some(offset);
                        // A rest after whole records cannot become one: it goes at once.
                        if led || passing {
                            passed.one(records, offset);
                        } else {
                            waiting.push(Torn {
                                at: offset,
                                index: records,
                            });
                        }
                    }
                }
                line.clear();
                offset = at + end as u64 + 1;
                start = end + 1;
            }
            line.extend_from_slice(&chunk[start..read]);
            at += read as u64;
        }
    }
    let mut unfinished = None;
    if visited {
        if !is_blank(&line) {
            return Ok(Looked::NotTheFile);
        }
    } else if !is_blank(&line) && holds_only(&line) {
        let text = String::from_utf8_lossy(&line);
        match parse(text.as_bytes()) {
            Ok(record) => {
                passed.all(&mut waiting);
                visit(record, records, offset)?;
                records += 1;
                tail = line;
            }
            Err(_) if is_json_lossy(&line) => return Err(cannot_hold(records)),
            // Not whole yet: a live writer's final append, or a torn line the
            // next is being written into. Either waits for its newline; one
            // that has not grown for the wait is said, once.
            Err(_) => {
                let mut it = match seen {
                    Some(seen) if seen.size == at => seen.unfinished,
                    _ => None,
                }
                .unwrap_or(Unfinished {
                    since: looked_ms,
                    said: false,
                });
                if !it.said && waited(it.since) {
                    torn::unfinished(file, records, offset);
                    it.said = true;
                }
                unfinished = Some(it);
            }
        }
    }
    // A torn line with no record after it: the look stops before it, and it
    // waits from when a look first found it there.
    let mut torn_since = None;
    if let Some(first) = waiting.first() {
        offset = first.at;
        torn_since = Some(match seen {
            Some(seen) if seen.offset == offset => seen.torn_since.unwrap_or(looked_ms),
            _ => looked_ms,
        });
    }
    let edge = match seen {
        Some(seen) if seen.offset == offset => seen.edge.clone(),
        _ => bytes_at(&mut handle, offset.saturating_sub(EDGE_BYTES), offset)?,
    };
    Ok(Looked::Read(Seen {
        identity: file_identity,
        size: at,
        mtime_ms: written,
        looked_ms,
        offset,
        edge,
        tail,
        records,
        passed_over: passed.places,
        torn_since,
        unfinished,
    }))
}

/// A torn line read and not yet passed over: the byte it begins at, and the
/// place among the records it stands at, which the record after it takes.
struct Torn {
    at: u64,
    index: usize,
}

/// The torn lines of `file` passed over: where each stood among the records.
struct Passed<'a> {
    file: &'a Path,
    places: Vec<usize>,
}

impl Passed<'_> {
    /// The torn line at the byte `at`, where the record `index` would be, is passed over.
    fn one(&mut self, index: usize, at: u64) {
        self.places.push(index);
        torn::passed_over(self.file, index, at);
    }

    /// A record has come after the torn lines `waiting`: each is passed over.
    fn all(&mut self, waiting: &mut Vec<Torn>) {
        for line in waiting.drain(..) {
            self.one(line.index, line.at);
        }
    }
}

/// One whole line, which would be the record `index`: the records it holds,
/// and whether the rest of it is torn (not JSON, where Node failed the look).
/// A line of one record is that record; a blank one holds none. One that is
/// not JSON holds the whole records it begins with, run together, as a short
/// write that dropped only a newline leaves two, each in the place it would
/// have had; the rest of it, if any, is torn. JSON this build cannot hold is a
/// failure. A carriage return before the newline is JSON's white space: a line
/// ended `\r\n` reads as one ended `\n`.
fn whole_line(raw: &[u8], index: usize, parse: Parse) -> Result<(Vec<Value>, bool), Stop> {
    if is_blank(raw) {
        return Ok((Vec::new(), false));
    }
    match parse(raw) {
        Ok(record) => Ok((vec![record], false)),
        Err(_) if is_json_lossy(raw) => Err(cannot_hold(index)),
        Err(_) => {
            let mut whole = Vec::new();
            let mut values = serde_json::Deserializer::from_slice(raw).into_iter::<IgnoredAny>();
            let mut start = 0;
            while let Some(Ok(_)) = values.next() {
                let end = values.byte_offset();
                let Ok(record) = parse(&raw[start..end]) else {
                    break;
                };
                whole.push(record);
                start = end;
            }
            Ok((whole, !is_blank(&raw[start..])))
        }
    }
}

/// Why the line at `index`, JSON a value here cannot hold, nested past
/// [`DEEPEST`] levels or with a number past a double's range, fails its look.
/// Node read such a line, and this does not: a difference kept on purpose,
/// said in a sentence of its own so that a store holding one can be counted.
fn cannot_hold(index: usize) -> Stop {
    Stop::Failed(format!(
        "JSON this build cannot hold at record {index}: nested past {DEEPEST} levels, or a number past a double's range"
    ))
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
mod parsers;
#[cfg(test)]
mod tests;
