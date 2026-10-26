//! A conversation's JSONL transcript followed from look to look.
//!
//! A look locates the transcript (again, once its file is gone), reads on
//! from where the last look stopped into the state its parser makes, and
//! says whether it read anything. A transcript that is not the one read so
//! far, or whose parser asked for it again (`Stop::Reread`), is read again
//! from its start, into a fresh state. One that could not be read is not
//! read again until it changes.

use std::path::PathBuf;
use std::sync::Arc;

use cf_base::file::{settled, stat, Stat};
use cf_base::json::from_slice_lossy;
use serde_json::Value;

use super::cache::{Look, Options};
use super::jsonl::{read_on_with, Looked, Seen, Stop};
use super::reading::{Reading, Record};

/// What reads a transcript's records into what they say.
pub(crate) trait Parser {
    /// A line of the transcript, read as a record: as `JSON.parse` reads it.
    /// A parser that reads a part of a record may read a line for that part
    /// alone, so long as it fails where this does.
    fn parse(line: &[u8]) -> serde_json::Result<Value> {
        from_slice_lossy(line)
    }

    /// A record, at its place among the transcript's records. Fails with
    /// `Stop::Reread` to have the transcript read again from its start.
    fn visit(&mut self, record: Value, index: usize) -> Result<(), Stop>;

    /// [`Parser::visit`], told the byte the record's line begins at too, which
    /// a parser that reads no place in the file passes over.
    fn visit_at(&mut self, record: Value, index: usize, offset: u64) -> Result<(), Stop> {
        let _ = offset;
        self.visit(record, index)
    }

    /// The records of a look are all in: what waited for the rest is decided.
    /// Fails with the look's reason; only a record asks for another read.
    fn flush(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Where the transcript is, if the harness keeps one yet: or why it cannot
/// be looked for.
pub(crate) type Locate = Box<dyn FnMut() -> Result<Option<PathBuf>, String> + Send>;

/// A transcript followed from look to look.
pub(crate) struct Followed<P> {
    locate: Locate,
    parse: Box<dyn FnMut() -> P + Send>,
    file: Option<PathBuf>,
    seen: Option<Seen>,
    state: Option<P>,
    /// A failure, kept until the file changes (`sameFile`), with when the look
    /// that found it began: a stat not [`settled`] then may be the same for a
    /// file written again since.
    broken: Option<(Stat, i64, String)>,
}

/// What a look at a followed transcript read.
pub(crate) struct Read<'a, P> {
    pub(crate) state: &'a mut P,
    /// The transcript the look read.
    pub(crate) file: PathBuf,
    /// Whether the look read anything since the last.
    pub(crate) changed: bool,
    /// The torn lines passed over so far, each by the place among the records
    /// it stood at.
    pub(crate) lost: Vec<usize>,
}

impl<P: Parser> Followed<P> {
    pub(crate) fn new(locate: Locate, parse: Box<dyn FnMut() -> P + Send>) -> Self {
        Self {
            locate,
            parse,
            file: None,
            seen: None,
            state: None,
            broken: None,
        }
    }

    /// A look that began at `now_ms`: none when the harness keeps no transcript
    /// of the conversation.
    pub(crate) fn read(&mut self, now_ms: i64) -> Result<Option<Read<'_, P>>, Stop> {
        let mut current = self.file.as_deref().and_then(|file| stat(file).ok());
        if current.is_none() {
            let located = (self.locate)().map_err(Stop::Failed)?;
            if located != self.file {
                self.seen = None;
                self.state = None;
                self.broken = None;
            }
            self.file = located;
            let Some(file) = self.file.as_deref() else {
                return Ok(None);
            };
            current = Some(stat(file)?);
        }
        let (Some(current), Some(file)) = (current, self.file.clone()) else {
            return Ok(None);
        };
        if let Some((stat, looked_ms, reason)) = &self.broken {
            if *stat == current && settled(stat.mtime_ms, *looked_ms) {
                return Err(Stop::Failed(reason.clone()));
            }
        }
        self.broken = None;
        loop {
            let state = self.state.get_or_insert_with(|| (self.parse)());
            let looked = read_on_with(
                &file,
                self.seen.as_ref(),
                now_ms,
                P::parse,
                &mut |record, index, offset| state.visit_at(record, index, offset),
                None,
            )
            .and_then(|looked| match looked {
                Looked::NotTheFile => Ok(looked),
                looked => state.flush().map(|()| looked).map_err(Stop::Failed),
            });
            match looked {
                Ok(looked @ (Looked::Unchanged | Looked::Held(_) | Looked::Read(_))) => {
                    let changed = matches!(looked, Looked::Read(_));
                    if let Looked::Held(seen) | Looked::Read(seen) = looked {
                        self.seen = Some(seen);
                    }
                    let lost = self
                        .seen
                        .as_ref()
                        .map(|seen| seen.passed_over().to_vec())
                        .unwrap_or_default();
                    return Ok(Some(Read {
                        state: self.state.get_or_insert_with(|| (self.parse)()),
                        file,
                        changed,
                        lost,
                    }));
                }
                // Another file, or a parser that wants every record again: the
                // same look reads from the start into a fresh state, and the
                // transcript is no more broken than it was.
                Ok(Looked::NotTheFile) | Err(Stop::Reread) => {
                    self.seen = None;
                    self.state = None;
                }
                Err(stop) => {
                    self.seen = None;
                    self.state = None;
                    self.broken = Some((current, now_ms, stop.reason()));
                    return Err(stop);
                }
            }
        }
    }
}

/// What a parser makes of what it read: the record's answer, or why it has none.
pub(crate) trait Answer {
    fn result(&self) -> Result<Record, String>;
}

/// The reader of a harness whose answer is its transcript's alone (Codex,
/// Claude Code): a look that read nothing new answers what the last did,
/// the same reading.
pub(crate) struct TranscriptReader<P> {
    transcript: Followed<P>,
    /// What there is no transcript of: `codex rollout for <session>`.
    missing: String,
    answer: Option<Arc<Reading>>,
}

impl<P> TranscriptReader<P> {
    pub(crate) fn new(transcript: Followed<P>, missing: String) -> Self {
        Self {
            transcript,
            missing,
            answer: None,
        }
    }

    /// Tells the state read so far, if there is one, what `tell` tells it, and
    /// has the next look answer anew: from that state where `tell` says it
    /// took it, else from the transcript read again from its start into a
    /// fresh state.
    pub(crate) fn retell(&mut self, tell: impl FnOnce(&mut P) -> bool) {
        self.answer = None;
        if let Some(state) = self.transcript.state.as_mut() {
            if !tell(state) {
                self.transcript.seen = None;
                self.transcript.state = None;
            }
        }
    }
}

impl<P: Parser + Answer> Look for TranscriptReader<P> {
    /// Takes no options, and the clock says only when the look began: the
    /// transcript alone answers.
    fn look(&mut self, _options: &Options, now_ms: i64) -> Arc<Reading> {
        match self.transcript.read(now_ms) {
            // A thread with no transcript yet is unknown: missing history alone proves nothing.
            Ok(None) => {
                self.answer = None;
                Arc::new(Reading::missing(&self.missing))
            }
            Ok(Some(read)) => {
                if let (false, Some(answer)) = (read.changed, &self.answer) {
                    return Arc::clone(answer);
                }
                match read.state.result() {
                    Ok(mut record) => {
                        record.lines.lost = read.lost;
                        let answer = Arc::new(Reading::Known(record));
                        self.answer = Some(Arc::clone(&answer));
                        answer
                    }
                    Err(reason) => {
                        self.answer = None;
                        Arc::new(Reading::unreadable(&reason))
                    }
                }
            }
            Err(stop) => {
                self.answer = None;
                Arc::new(Reading::unreadable(&stop.reason()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{EPOCH_MS, LONG_AFTER_MS};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A parser that counts the records it is given.
    struct Counting(Arc<AtomicUsize>);

    impl Parser for Counting {
        fn visit(&mut self, _: Value, _: usize) -> Result<(), Stop> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    fn a_transcript_that_could_not_be_read_is_not_read_again_until_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.jsonl");
        fs::write(&file, "{\"a\":1}\n{\"x\":1e400}\n").unwrap();
        let made = Arc::new(AtomicUsize::new(0));
        let visits = Arc::new(AtomicUsize::new(0));
        let located = file.clone();
        let (making, visiting) = (Arc::clone(&made), Arc::clone(&visits));
        let mut followed = Followed::new(
            Box::new(move || Ok(Some(located.clone()))),
            Box::new(move || {
                making.fetch_add(1, Ordering::Relaxed);
                Counting(Arc::clone(&visiting))
            }),
        );
        let counts = || (made.load(Ordering::Relaxed), visits.load(Ordering::Relaxed));
        let failed = followed.read(LONG_AFTER_MS).err().unwrap().reason();
        assert!(
            failed.starts_with("JSON this build cannot hold at record 1"),
            "{failed}"
        );
        assert_eq!(counts(), (1, 1));
        assert_eq!(followed.read(LONG_AFTER_MS).err().unwrap().reason(), failed);
        assert_eq!(
            counts(),
            (1, 1),
            "unchanged: the same failure, nothing read"
        );
        let mut appended = fs::OpenOptions::new().append(true).open(&file).unwrap();
        std::io::Write::write_all(&mut appended, b"{\"b\":2}\n").unwrap();
        assert_eq!(followed.read(LONG_AFTER_MS).err().unwrap().reason(), failed);
        assert_eq!(counts(), (2, 2), "changed: read again from its start");
    }

    #[test]
    fn a_torn_line_breaks_nothing_the_state_goes_on_and_reads_the_records_after_it() {
        // A short write left part of a record, and the next append made one line of
        // it and itself: the state read so far is kept, and given what comes after.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.jsonl");
        fs::write(&file, "{\"a\":1}\n{\"a\":2,\"b\":\"cut sh").unwrap();
        let made = Arc::new(AtomicUsize::new(0));
        let visits = Arc::new(AtomicUsize::new(0));
        let located = file.clone();
        let (making, visiting) = (Arc::clone(&made), Arc::clone(&visits));
        let mut followed = Followed::new(
            Box::new(move || Ok(Some(located.clone()))),
            Box::new(move || {
                making.fetch_add(1, Ordering::Relaxed);
                Counting(Arc::clone(&visiting))
            }),
        );
        let counts = || (made.load(Ordering::Relaxed), visits.load(Ordering::Relaxed));
        let append = |text: &str| {
            let mut appended = fs::OpenOptions::new().append(true).open(&file).unwrap();
            std::io::Write::write_all(&mut appended, text.as_bytes()).unwrap();
        };
        assert!(followed.read(LONG_AFTER_MS).is_ok());
        assert_eq!(counts(), (1, 1), "the piece waits");
        append("{\"a\":3}\n");
        assert!(followed.read(LONG_AFTER_MS).is_ok(), "torn: it waits");
        assert_eq!(counts(), (1, 1));
        append("{\"a\":4}\n");
        assert!(followed.read(LONG_AFTER_MS).unwrap().unwrap().changed);
        assert_eq!(
            counts(),
            (1, 2),
            "the same state, given the record after it"
        );
        assert!(followed.broken.is_none());
    }

    /// Writes `text` to `file`, last written at `at_ms` whatever the file system's clock.
    fn write_at(file: &std::path::Path, text: &str, at_ms: i64) {
        fs::write(file, text).unwrap();
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_millis(at_ms as u64);
        fs::File::options()
            .write(true)
            .open(file)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    #[test]
    fn a_broken_transcript_mended_at_its_size_within_a_file_times_step_is_read_again() {
        // Mended in place a second before the look that found it broken, as Windows'
        // clock gives a file written again within its tick: its stat is the same, and
        // only its bytes tell.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.jsonl");
        let looked = EPOCH_MS;
        let (broken, mended) = ("{\"a\":1}\n{\"b\":1e999}\n", "{\"a\":1}\n{\"b\":22222}\n");
        assert_eq!(broken.len(), mended.len());
        write_at(&file, broken, looked - 1_000);
        let located = file.clone();
        let mut followed = Followed::new(
            Box::new(move || Ok(Some(located.clone()))),
            Box::new(|| Counting(Arc::new(AtomicUsize::new(0)))),
        );
        assert!(followed.read(looked).is_err());
        write_at(&file, mended, looked - 1_000);
        assert!(followed.read(looked + 10).is_ok());
        // Broken and settled when the look found it, it is not read again while it stays.
        write_at(&file, broken, looked - 60_000);
        assert!(followed.read(looked + 20).is_err());
        write_at(&file, mended, looked - 60_000);
        assert!(followed.read(looked + 30).is_err());
    }

    #[test]
    fn a_transcript_read_a_moment_after_its_last_write_is_read_to_tell_once_and_then_by_its_time() {
        // A look each second reads a transcript a moment after it was written: the
        // look after one that read its bytes to tell takes its time, as for a transcript
        // last written long before. Written over at its size and time then, it is the
        // one read (as git takes a clean entry).
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.jsonl");
        let written = EPOCH_MS;
        write_at(&file, "{\"a\":1}\n{\"a\":2}\n", written);
        let visits = Arc::new(AtomicUsize::new(0));
        let (located, visiting) = (file.clone(), Arc::clone(&visits));
        let mut followed = Followed::new(
            Box::new(move || Ok(Some(located.clone()))),
            Box::new(move || Counting(Arc::clone(&visiting))),
        );
        let mut look = |at: i64| followed.read(written + at).unwrap().unwrap().changed;
        assert!(look(500));
        assert!(!look(2_500), "its bytes as they were");
        write_at(&file, "{\"b\":1}\n{\"b\":2}\n", written);
        assert!(!look(3_000), "its time settled: not read");
        assert_eq!(visits.load(Ordering::Relaxed), 2);
    }

    /// A parser that numbers its states, as made, and keeps the places of the
    /// records it is given. The first state asks to read again at the record
    /// `asks_at`.
    struct Rereading {
        number: usize,
        visited: Vec<usize>,
        asks_at: usize,
    }

    impl Parser for Rereading {
        fn visit(&mut self, _: Value, index: usize) -> Result<(), Stop> {
            self.visited.push(index);
            if self.number == 1 && index == self.asks_at {
                return Err(Stop::Reread);
            }
            Ok(())
        }
    }

    /// A transcript of two records, followed by `Rereading` parsers, and the
    /// file that holds it.
    fn rereading(asks_at: usize) -> (tempfile::TempDir, PathBuf, Followed<Rereading>) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.jsonl");
        fs::write(&file, "{\"a\":0}\n{\"a\":1}\n").unwrap();
        let made = Arc::new(AtomicUsize::new(0));
        let located = file.clone();
        let followed = Followed::new(
            Box::new(move || Ok(Some(located.clone()))),
            Box::new(move || Rereading {
                number: made.fetch_add(1, Ordering::Relaxed) + 1,
                visited: Vec::new(),
                asks_at,
            }),
        );
        (dir, file, followed)
    }

    /// The state a look read into, as it numbered itself, the places of the
    /// records it was given, and whether the look read anything.
    fn read_into(followed: &mut Followed<Rereading>) -> (usize, Vec<usize>, bool) {
        let read = followed.read(LONG_AFTER_MS).unwrap().unwrap();
        (read.state.number, read.state.visited.clone(), read.changed)
    }

    #[test]
    fn a_reread_reads_the_transcript_again_in_the_same_look_into_a_fresh_state_and_breaks_nothing()
    {
        let (_dir, _file, mut followed) = rereading(1);
        let (number, visited, changed) = read_into(&mut followed);
        assert_eq!(number, 2, "the first state was dropped for a fresh one");
        assert_eq!(visited, [0, 1], "read from the start, whole");
        assert!(changed, "the look read");
        assert!(followed.broken.is_none(), "a reread is no failure");
        // The look after it, nothing written since, reads nothing, and fails nothing.
        assert_eq!(read_into(&mut followed), (2, vec![0, 1], false));
    }

    #[test]
    fn a_reread_in_a_later_look_reads_from_the_start_and_not_from_where_the_last_look_stopped() {
        let (_dir, file, mut followed) = rereading(2);
        assert_eq!(read_into(&mut followed), (1, vec![0, 1], true));
        let mut appended = fs::OpenOptions::new().append(true).open(&file).unwrap();
        std::io::Write::write_all(&mut appended, b"{\"a\":2}\n").unwrap();
        // The first state is given the third record alone, and asks: the
        // fresh one is given all three.
        assert_eq!(read_into(&mut followed), (2, vec![0, 1, 2], true));
        assert!(followed.broken.is_none());
        assert_eq!(read_into(&mut followed), (2, vec![0, 1, 2], false));
    }
}
