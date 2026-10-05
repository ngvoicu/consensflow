//! What a first look at a big transcript costs: the memory it peaks at, and
//! the time it takes in its parts. The transcript is a synthetic one built
//! like a 324 MB one (`synthetic`: 125,000 lines, a third of a gigabyte, most
//! of it under keys no reader reads), so that a run on any machine is the
//! same run. Ignored by `cargo test`; each measure is a test of its own, to be
//! run alone, in a process of its own, since a process remembers the most
//! memory it ever held:
//!
//! ```text
//! cargo test --offline --release -p cf-harness --lib claude::record::tests::memory::first_look -- --ignored --nocapture --exact
//! ```
//!
//! and one of the others in the place of `first_look`. `npm run
//! bench:records-memory` runs them all, each alone. Each prints what it
//! measured, and the first two the digest of the reading they made, so that
//! the same digest before and after a change says the answer is the same.
//!
//! - `first_look`: a new reader's look at the whole transcript, as the cache
//!   makes it: the time it takes, and the memory it peaks at, with what the
//!   process held before the look alongside.
//! - `first_look_parts`: the same look in the parts it is made of, each
//!   timed apart: reading and parsing the lines (visiting them), replaying
//!   the records, dropping what waited for the replay, and making the answer.
//!   A look drops each record as it replays it; here they are all kept to be
//!   dropped last, so as to time it, which makes this look's memory its own
//!   and not the look's: only what is held once the lines are in is said.
//! - `unchanged_look`: the look after a first, when nothing was written.
//! - `reread`: the look after a record claiming a uuid a decision looked up,
//!   which has the transcript read again from its start.
//!
//! `CF_RECORDS_MEMORY_LINES` sets the size of the transcript made, and
//! `CF_RECORDS_MEMORY_TRANSCRIPT` names a real one to read in its place (read
//! only, its name is its session, and `reread` is not measured on it).

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::synthetic::{self, Plan};
use super::*;
use crate::shared::record::jsonl::read_on_with;

/// The session the synthetic transcript is of.
const SESSION: &str = "bench";
const LINES: usize = 125_000;
const MEGABYTE: f64 = 1_048_576.0;

/// A transcript to look at, and what is known of it.
struct Subject {
    _folder: Option<TempDir>,
    file: PathBuf,
    session: String,
    lines: Option<usize>,
    bytes: u64,
    watched: Option<String>,
}

impl Subject {
    fn new() -> Self {
        let process = Env::from_process();
        if let Some(real) = process.path("CF_RECORDS_MEMORY_TRANSCRIPT") {
            let session = real
                .file_stem()
                .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
            return Self {
                _folder: None,
                file: real.to_path_buf(),
                session,
                lines: None,
                bytes: fs::metadata(real).map_or(0, |metadata| metadata.len()),
                watched: None,
            };
        }
        let lines = process
            .text("CF_RECORDS_MEMORY_LINES")
            .and_then(|text| text.parse().ok())
            .unwrap_or(LINES);
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join(format!("{SESSION}.jsonl"));
        let out = BufWriter::new(File::create(&file).unwrap());
        let made = synthetic::write(out, SESSION, &Plan { lines, seed: 7 }).unwrap();
        Self {
            _folder: Some(folder),
            file,
            session: SESSION.to_owned(),
            lines: Some(made.lines),
            bytes: made.bytes,
            watched: Some(made.watched),
        }
    }

    fn transcript(&self) -> Transcript {
        Transcript::new(Arc::from(self.session.as_str()), local())
    }

    /// A transcript followed from look to look.
    fn followed(&self) -> Followed<Transcript> {
        let (file, session) = (self.file.clone(), self.session.clone());
        Followed::new(
            Box::new(move || Ok(Some(file.clone()))),
            Box::new(move || Transcript::new(Arc::from(session.as_str()), local())),
        )
    }

    /// A reader of the transcript, as the cache keeps one.
    fn reader(&self) -> TranscriptReader<Transcript> {
        TranscriptReader::new(self.followed(), "bench".to_owned())
    }

    fn describe(&self) {
        let lines = self.lines.map_or_else(
            || "an unknown number of".to_owned(),
            |lines| lines.to_string(),
        );
        println!(
            "transcript: {lines} lines, {:.1} MB",
            self.bytes as f64 / MEGABYTE
        );
    }
}

/// The most memory this process ever held resident, in bytes: none where this
/// system does not say.
#[cfg(unix)]
fn peak_resident() -> Option<u64> {
    use nix::sys::resource::{getrusage, UsageWho};
    let most = u64::try_from(getrusage(UsageWho::RUSAGE_SELF).ok()?.max_rss()).ok()?;
    // macOS counts bytes, the others kilobytes.
    Some(if cfg!(target_os = "macos") {
        most
    } else {
        most * 1024
    })
}

#[cfg(not(unix))]
fn peak_resident() -> Option<u64> {
    None
}

fn megabytes(bytes: Option<u64>) -> String {
    bytes.map_or_else(
        || "not measured on this system".to_owned(),
        |bytes| format!("{:.0} MB", bytes as f64 / MEGABYTE),
    )
}

fn millis(time: Duration) -> String {
    if time < Duration::from_millis(1) {
        format!("{} us", time.as_micros())
    } else {
        format!("{:.1} ms", time.as_secs_f64() * 1000.0)
    }
}

/// What a reading says, as one digest of the JSON it is written as.
fn digest(reading: &Reading) -> String {
    struct Hashing(Sha256);
    impl Write for Hashing {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut hashing = Hashing(Sha256::new());
    serde_json::to_writer(&mut hashing, reading).unwrap();
    hashing
        .0
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
#[ignore = "a measurement: see this module's doc"]
fn first_look() {
    let subject = Subject::new();
    subject.describe();
    let mut reader = subject.reader();
    let before = peak_resident();
    let started = Instant::now();
    let reading = reader.look(&Options::default(), 0);
    let took = started.elapsed();
    if let Reading::Known(record) = &*reading {
        println!("items in the answer: {}", record.items.len());
    }
    println!("digest of the reading: {}", digest(&reading));
    println!("first look: {}", millis(took));
    println!(
        "memory held before the look, at most: {}",
        megabytes(before)
    );
    println!(
        "memory held by the look, at most: {}",
        megabytes(peak_resident())
    );
}

#[test]
#[ignore = "a measurement: see this module's doc"]
fn first_look_parts() {
    let subject = Subject::new();
    subject.describe();
    let mut transcript = subject.transcript();

    let started = Instant::now();
    let looked = read_on_with(
        &subject.file,
        None,
        Transcript::parse,
        &mut |record, index| transcript.visit(record, index),
        None,
    );
    let parse = started.elapsed();
    assert!(looked.is_ok(), "the transcript is read");
    let parsed = peak_resident();

    let waiting = std::mem::take(&mut transcript.pending);
    let records = waiting.len();
    let started = Instant::now();
    for record in &waiting {
        transcript
            .replay(&record.record, record.place, record.seq)
            .unwrap();
    }
    let replay = started.elapsed();

    let started = Instant::now();
    drop(waiting);
    let dropped = started.elapsed();

    let started = Instant::now();
    let record = transcript.result().unwrap();
    let answer = started.elapsed();

    println!(
        "records: {records}, items in the answer: {}",
        record.items.len()
    );
    println!("digest of the reading: {}", digest(&Reading::Known(record)));
    println!("parse (read, parse and visit the lines): {}", millis(parse));
    println!("replay: {}", millis(replay));
    println!("drop (what waited for the replay): {}", millis(dropped));
    println!("answer: {}", millis(answer));
    println!("in all: {}", millis(parse + replay + dropped + answer));
    println!(
        "memory held once the lines are in, at most: {}",
        megabytes(parsed)
    );
}

#[test]
#[ignore = "a measurement: see this module's doc"]
fn unchanged_look() {
    let subject = Subject::new();
    subject.describe();
    let mut reader = subject.reader();
    let first = reader.look(&Options::default(), 0);
    let started = Instant::now();
    let second = reader.look(&Options::default(), 0);
    let unchanged = started.elapsed();
    assert!(Arc::ptr_eq(&first, &second), "the same reading");
    println!("unchanged look: {}", millis(unchanged));
}

#[test]
#[ignore = "a measurement: see this module's doc"]
fn reread() {
    let subject = Subject::new();
    subject.describe();
    let Some(watched) = &subject.watched else {
        println!("reread: not measured on a transcript that is not synthetic");
        return;
    };
    let mut followed = subject.followed();
    assert!(followed.read().unwrap().is_some_and(|read| read.changed));
    let claim = format!(
        r#"{{"type":"attachment","uuid":"{watched}","sessionId":"{}","isSidechain":false}}"#,
        subject.session
    );
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&subject.file)
        .unwrap();
    writeln!(file, "{claim}").unwrap();
    drop(file);
    let started = Instant::now();
    let read = followed.read().unwrap().unwrap();
    let reread = started.elapsed();
    assert_eq!(
        Some(read.state.count),
        subject.lines.map(|lines| lines + 1),
        "every record was visited again"
    );
    println!("reread, from the claim to the answer: {}", millis(reread));
    println!("memory held, at most: {}", megabytes(peak_resident()));
}
