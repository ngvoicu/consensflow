//! A conversation's JSONL transcript followed from look to look
//! (`followedTranscript` and `transcriptReader`,
//! `hosts/lib/completion/shared.js`).
//!
//! A look locates the transcript (again, once its file is gone), reads on
//! from where the last look stopped into the state its parser makes, and
//! says whether it read anything. A transcript that is not the one read so
//! far, or whose parser asked for it again (`Stop::Reread`), is read again
//! from its start, into a fresh state. One that could not be read is not
//! read again until it changes.

use std::path::PathBuf;
use std::sync::Arc;

use cf_base::file::{stat, Stat};
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
    /// A failure, kept until the file changes (`sameFile`).
    broken: Option<(Stat, String)>,
}

/// What a look at a followed transcript read.
pub(crate) struct Read<'a, P> {
    pub(crate) state: &'a mut P,
    /// The transcript the look read.
    pub(crate) file: PathBuf,
    /// Whether the look read anything since the last.
    pub(crate) changed: bool,
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

    /// A look: none when the harness keeps no transcript of the conversation.
    pub(crate) fn read(&mut self) -> Result<Option<Read<'_, P>>, Stop> {
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
        if let Some((stat, reason)) = &self.broken {
            if *stat == current {
                return Err(Stop::Failed(reason.clone()));
            }
        }
        self.broken = None;
        loop {
            let state = self.state.get_or_insert_with(|| (self.parse)());
            let looked = read_on_with(
                &file,
                self.seen.as_ref(),
                P::parse,
                &mut |record, index| state.visit(record, index),
                None,
            )
            .and_then(|looked| match looked {
                Looked::NotTheFile => Ok(looked),
                looked => state.flush().map(|()| looked).map_err(Stop::Failed),
            });
            match looked {
                Ok(Looked::Unchanged) => {
                    return Ok(Some(Read {
                        state: self.state.get_or_insert_with(|| (self.parse)()),
                        file,
                        changed: false,
                    }))
                }
                Ok(Looked::Read(seen)) => {
                    self.seen = Some(seen);
                    return Ok(Some(Read {
                        state: self.state.get_or_insert_with(|| (self.parse)()),
                        file,
                        changed: true,
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
                    self.broken = Some((current, stop.reason()));
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
}

impl<P: Parser + Answer> Look for TranscriptReader<P> {
    /// Takes no options and reads no clock: the transcript alone answers.
    fn look(&mut self, _options: &Options, _now_ms: i64) -> Arc<Reading> {
        match self.transcript.read() {
            // A thread with no transcript yet is unknown: missing history alone proves nothing.
            Ok(None) => {
                self.answer = None;
                Arc::new(Reading::unreadable(&format!("no {}", self.missing)))
            }
            Ok(Some(read)) => {
                if let (false, Some(answer)) = (read.changed, &self.answer) {
                    return Arc::clone(answer);
                }
                match read.state.result() {
                    Ok(record) => {
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
        fs::write(&file, "{\"a\":1}\n{bad}\n").unwrap();
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
        let failed = followed.read().err().unwrap().reason();
        assert_eq!(failed, "malformed JSONL at record 1");
        assert_eq!(counts(), (1, 1));
        assert_eq!(followed.read().err().unwrap().reason(), failed);
        assert_eq!(
            counts(),
            (1, 1),
            "unchanged: the same failure, nothing read"
        );
        let mut appended = fs::OpenOptions::new().append(true).open(&file).unwrap();
        std::io::Write::write_all(&mut appended, b"{\"b\":2}\n").unwrap();
        assert_eq!(followed.read().err().unwrap().reason(), failed);
        assert_eq!(counts(), (2, 2), "changed: read again from its start");
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
        let read = followed.read().unwrap().unwrap();
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
