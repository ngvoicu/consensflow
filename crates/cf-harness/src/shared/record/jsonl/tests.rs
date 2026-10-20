use super::*;
use crate::testing::{EPOCH_MS, LONG_AFTER_MS};
use std::fs;
use std::time::{Duration, SystemTime};

/// Every record a look visits, with its place, the look long after the file was
/// written.
fn collect(
    file: &Path,
    seen: Option<&Seen>,
    only: Option<&[u8]>,
) -> (Result<Looked, Stop>, Vec<(Value, usize)>) {
    collect_at(file, seen, LONG_AFTER_MS, only)
}

/// Every record a look that began at `looked_ms` visits, with its place.
fn collect_at(
    file: &Path,
    seen: Option<&Seen>,
    looked_ms: i64,
    only: Option<&[u8]>,
) -> (Result<Looked, Stop>, Vec<(Value, usize)>) {
    let mut visited = Vec::new();
    let looked = read_on(
        file,
        seen,
        looked_ms,
        &mut |record, index| {
            visited.push((record, index));
            Ok(())
        },
        only,
    );
    (looked, visited)
}

/// The place where a look stopped; fails a test whose look did not read.
fn read(looked: Result<Looked, Stop>) -> Seen {
    match looked {
        Ok(Looked::Read(seen)) => seen,
        other => panic!("not read: {other:?}"),
    }
}

/// The file's time set `ms` after the epoch, as the scenarios set it.
fn stamp(path: &Path, ms: u64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_millis(ms))
        .unwrap();
}

#[test]
fn read_on_passes_over_unparsed_the_lines_that_do_not_hold_only() {
    // A Devin window's wire log holds its own session alone, and every Devin
    // session's reader reads every window's: each parses its own lines.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("wire.jsonl");
    fs::write(
        &file,
        "{\"sessionId\":\"calm-river\",\"n\":1}\nnot JSON, nor calm\n{\"sessionId\":\"quiet-lake\",\"n\":2}\n{\"sessionId\":\"calm-river\",\"n\":3}",
    )
    .unwrap();
    let only: &[u8] = b"\"calm-river\"";
    let (looked, visited) = collect(&file, None, Some(only));
    let ns: Vec<_> = visited
        .iter()
        .map(|(record, _)| record["n"].clone())
        .collect();
    assert_eq!(ns, [1, 3]);
    let first = read(looked);
    let mut appended = fs::OpenOptions::new().append(true).open(&file).unwrap();
    std::io::Write::write_all(
        &mut appended,
        b"\n{\"sessionId\":\"quiet-lake\",\"n\":4}\n{\"sessionId\":\"calm-river\",\"n\":5}\n{\"sessionId\":\"quiet",
    )
    .unwrap();
    let (_, visited) = collect(&file, Some(&first), Some(only));
    let ns: Vec<_> = visited
        .iter()
        .map(|(record, _)| record["n"].clone())
        .collect();
    assert_eq!(ns, [5]);
}

#[test]
fn a_look_reads_the_bytes_after_where_the_last_stopped_and_no_others() {
    // Only bytes past where the last look stopped are read: a record made
    // malformed in place, before the edge the next look checks, would fail
    // any look that read it again.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    let padding = "x".repeat(40);
    let mut text = format!("{{\"a\":0,\"pad\":\"{padding}\"}}\n");
    for n in 0..40 {
        text.push_str(&format!("{{\"n\":{n},\"pad\":\"{padding}\"}}\n"));
    }
    fs::write(&file, &text).unwrap();
    stamp(&file, 1_000);
    let seen = read(collect(&file, None, None).0);
    assert_eq!(seen.records, 41);
    let mut rewritten = fs::File::options().write(true).open(&file).unwrap();
    std::io::Write::write_all(&mut rewritten, b"{bad::").unwrap();
    drop(rewritten);
    let mut appended = fs::OpenOptions::new().append(true).open(&file).unwrap();
    std::io::Write::write_all(&mut appended, b"{\"n\":99}\n").unwrap();
    stamp(&file, 2_000);
    let (looked, visited) = collect(&file, Some(&seen), None);
    assert_eq!(visited, [(serde_json::json!({ "n": 99 }), 41)]);
    assert_eq!(read(looked).records, 42);
}

#[test]
fn an_unchanged_file_is_unchanged_and_an_appended_one_reads_its_new_lines_alone() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    fs::write(&file, "{\"a\":1}\n{\"a\":2}\n").unwrap();
    stamp(&file, 1_000);
    let (looked, visited) = collect(&file, None, None);
    assert_eq!(
        visited.iter().map(|(_, at)| *at).collect::<Vec<_>>(),
        [0, 1]
    );
    let seen = read(looked);
    assert!(matches!(
        collect(&file, Some(&seen), None).0,
        Ok(Looked::Unchanged)
    ));
    let mut appended = fs::OpenOptions::new().append(true).open(&file).unwrap();
    std::io::Write::write_all(&mut appended, b"{\"a\":3}\r\n\n  \n").unwrap();
    stamp(&file, 2_000);
    let (looked, visited) = collect(&file, Some(&seen), None);
    assert_eq!(
        visited,
        [(serde_json::json!({ "a": 3 }), 2)],
        "a carriage return ends the line, blank lines are no records"
    );
    assert_eq!(read(looked).records, 3);
}

#[test]
fn a_whole_unterminated_record_is_visited_once_and_an_unfinished_one_waits() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    fs::write(&file, "{\"a\":1}\n{\"a\":2}").unwrap();
    stamp(&file, 1_000);
    let (looked, visited) = collect(&file, None, None);
    assert_eq!(visited.len(), 2, "the whole last record is visited");
    let seen = read(looked);
    fs::write(&file, "{\"a\":1}\n{\"a\":2}\n{\"a\":").unwrap();
    stamp(&file, 2_000);
    let (looked, visited) = collect(&file, Some(&seen), None);
    assert!(
        visited.is_empty(),
        "its newline adds nothing, and the unfinished one waits"
    );
    let seen = read(looked);
    fs::write(&file, "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n").unwrap();
    stamp(&file, 3_000);
    let (_, visited) = collect(&file, Some(&seen), None);
    assert_eq!(visited, [(serde_json::json!({ "a": 3 }), 2)]);
}

#[test]
fn a_malformed_line_fails_with_its_place_and_so_does_a_last_line_that_cannot_become_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    fs::write(&file, "{\"a\":1}\n{\"malformed\":\n{\"a\":2}\n").unwrap();
    assert_eq!(
        collect(&file, None, None).0.unwrap_err().reason(),
        "malformed JSONL at record 1"
    );
    for last in [
        "definitely-not-json",
        "{\"type\":!}",
        "\u{a0}",
        "{\"type\":\u{b}",
    ] {
        fs::write(&file, format!("{{\"a\":1}}\n{last}")).unwrap();
        assert_eq!(
            collect(&file, None, None).0.unwrap_err().reason(),
            "malformed JSONL at record 1",
            "{last:?}"
        );
    }
}

#[test]
fn a_file_that_shrank_was_written_over_or_was_replaced_is_not_the_one_read() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    fs::write(&file, "{\"a\":1}\n{\"a\":2}\n").unwrap();
    stamp(&file, 1_000);
    let seen = read(collect(&file, None, None).0);
    // Shorter.
    fs::write(&file, "{\"a\":1}\n").unwrap();
    stamp(&file, 2_000);
    assert!(matches!(
        collect(&file, Some(&seen), None).0,
        Ok(Looked::NotTheFile)
    ));
    // As long, but other bytes before where the look stopped.
    fs::write(&file, "{\"b\":1}\n{\"b\":2}\n{\"a\":3}\n").unwrap();
    stamp(&file, 3_000);
    assert!(matches!(
        collect(&file, Some(&seen), None).0,
        Ok(Looked::NotTheFile)
    ));
    // Another file renamed into its place, the same bytes and more.
    let beside = dir.path().join("t.jsonl.new");
    fs::write(&beside, "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n").unwrap();
    fs::rename(&beside, &file).unwrap();
    assert!(matches!(
        collect(&file, Some(&seen), None).0,
        Ok(Looked::NotTheFile)
    ));
}

#[test]
fn a_file_written_over_at_its_size_within_a_file_times_step_of_the_look_is_not_the_one_read() {
    // Written over a second before the look that read it, and its time kept, as
    // Windows' clock gives a file written again within its tick: only its bytes tell.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    let looked = EPOCH_MS;
    fs::write(&file, "{\"a\":1}\n{\"a\":2}\n").unwrap();
    stamp(&file, (looked - 1_000) as u64);
    let seen = read(collect_at(&file, None, looked, None).0);
    fs::write(&file, "{\"b\":1}\n{\"b\":2}\n").unwrap();
    stamp(&file, (looked - 1_000) as u64);
    assert!(matches!(
        collect_at(&file, Some(&seen), looked + 10, None).0,
        Ok(Looked::NotTheFile)
    ));
    // Not written over, it holds what was read, and nothing is read.
    fs::write(&file, "{\"a\":1}\n{\"a\":2}\n").unwrap();
    stamp(&file, (looked - 1_000) as u64);
    let (looked_again, visited) = collect_at(&file, Some(&seen), looked + 20, None);
    assert!(matches!(looked_again, Ok(Looked::Held(_))) && visited.is_empty());
}

#[test]
fn a_file_found_holding_what_was_read_once_its_time_is_settled_is_not_read_again() {
    // Read a moment after its last write, as a look each second reads a transcript
    // being written, and quiet since: one look reads it to tell, and those after it
    // take its time.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    let written = EPOCH_MS;
    fs::write(&file, "{\"a\":1}\n{\"a\":2}\n").unwrap();
    stamp(&file, written as u64);
    let seen = read(collect_at(&file, None, written + 500, None).0);
    let held = match collect_at(&file, Some(&seen), written + 2_500, None).0 {
        Ok(Looked::Held(held)) => held,
        other => panic!("not held: {other:?}"),
    };
    assert!(matches!(
        collect_at(&file, Some(&held), written + 3_000, None).0,
        Ok(Looked::Unchanged)
    ));
}

#[test]
fn a_file_last_written_well_before_the_look_that_read_it_is_not_read_again_while_its_time_stays() {
    // Its size and time as the look found them, and that time older than the look by
    // more than a file time's step: nothing of it is read again. Written over so, it
    // is the file read before (as git takes a clean entry).
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    let looked = EPOCH_MS;
    fs::write(&file, "{\"a\":1}\n{\"a\":2}\n").unwrap();
    stamp(&file, (looked - 60_000) as u64);
    let seen = read(collect_at(&file, None, looked, None).0);
    fs::write(&file, "{\"b\":1}\n{\"b\":2}\n").unwrap();
    stamp(&file, (looked - 60_000) as u64);
    assert!(matches!(
        collect_at(&file, Some(&seen), looked + 10, None).0,
        Ok(Looked::Unchanged)
    ));
}

#[test]
fn a_file_that_is_not_there_is_an_io_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (looked, _) = collect(&dir.path().join("none.jsonl"), None, None);
    assert!(matches!(looked, Err(Stop::Io(ref error)) if cf_base::file::is_missing(error)));
}

/// A seeded sequence of numbers.
struct Seeded(u64);

impl Seeded {
    fn below(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as usize % bound.max(1)
    }
}

/// Every record of the file read whole, or why a whole read fails.
fn whole(file: &Path) -> Result<Vec<Value>, String> {
    let (looked, visited) = collect(file, None, None);
    looked.map_err(|stop| stop.reason())?;
    Ok(visited.into_iter().map(|(record, _)| record).collect())
}

#[test]
fn reading_on_through_appends_truncations_and_replacements_reads_what_a_whole_read_does() {
    let lines = [
        "{\"a\":1}",
        "{\"text\":\"ü\\n\"}",
        "",
        "  ",
        "{\"b\":[1,2,{\"c\":null}]}",
        "{\"long\":\"0123456789012345678901234567890123456789\"}",
    ];
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    let mut random = Seeded(20_261_004);
    for run in 0..60 {
        fs::write(&file, "").unwrap();
        let mut text = String::new();
        let mut seen: Option<Seen> = None;
        let mut records: Vec<Value> = Vec::new();
        let mut written = 1_000;
        for _ in 0..40 {
            match random.below(10) {
                // Mostly appends, a line or a piece of one at a time.
                0..=6 => {
                    let line = lines[random.below(lines.len())];
                    let piece = if random.below(3) == 0 {
                        &line[..line.len() / 2]
                    } else {
                        line
                    };
                    text.push_str(piece);
                    if random.below(2) == 0 {
                        text.push('\n');
                    }
                }
                // A truncation, somewhere in the last few bytes.
                7 => {
                    let cut = text.len().saturating_sub(random.below(12));
                    let cut = (0..=cut)
                        .rev()
                        .find(|at| text.is_char_boundary(*at))
                        .unwrap_or(0);
                    text.truncate(cut);
                }
                // Another file in its place, the same text.
                _ => {
                    let beside = dir.path().join("t.jsonl.new");
                    fs::write(&beside, &text).unwrap();
                    fs::rename(&beside, &file).unwrap();
                    written += 1;
                    stamp(&file, written);
                    continue;
                }
            }
            fs::write(&file, &text).unwrap();
            written += 1;
            stamp(&file, written);
            // A look that reads on, read again from the start when the file is not the one read.
            let (looked, visited) = match collect(&file, seen.as_ref(), None) {
                (Ok(Looked::NotTheFile), _) => {
                    records.clear();
                    collect(&file, None, None)
                }
                other => other,
            };
            let visited: Vec<Value> = visited.into_iter().map(|(record, _)| record).collect();
            let expected = whole(&file);
            match looked {
                Ok(Looked::Read(next)) => {
                    records.extend(visited);
                    seen = Some(next);
                    assert_eq!(Ok(records.clone()), expected, "run {run}: {text:?}");
                }
                Ok(Looked::Unchanged) => assert_eq!(Ok(records.clone()), expected, "run {run}"),
                Ok(Looked::Held(next)) => {
                    seen = Some(next);
                    assert_eq!(Ok(records.clone()), expected, "run {run}");
                }
                Ok(Looked::NotTheFile) => panic!("a read from the start is the file read"),
                Err(stop) => {
                    assert_eq!(Err(stop.reason()), expected, "run {run}: {text:?}");
                    seen = None;
                    records.clear();
                }
            }
        }
    }
}

#[test]
fn json_this_build_cannot_hold_fails_its_look_in_a_sentence_of_its_own() {
    // Node reads both lines; serde_json reads neither. Kept on purpose.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    let deep = format!("{}{}", "[".repeat(128), "]".repeat(128));
    for (line, said) in [
        (
            "{\"x\":1e400}".to_owned(),
            "JSON this build cannot hold at record 1",
        ),
        (deep, "JSON this build cannot hold at record 1"),
        ("{bad}".to_owned(), "malformed JSONL at record 1"),
    ] {
        for ended in ["\n", ""] {
            fs::write(&file, format!("{{\"a\":1}}\n{line}{ended}")).unwrap();
            let Err(Stop::Failed(reason)) = collect(&file, None, None).0 else {
                panic!("{line}: read");
            };
            assert!(reason.starts_with(said), "{line:?}{ended:?}: {reason}");
        }
    }
}

#[test]
fn a_lone_surrogate_reads_as_the_replacement_character_and_two_halves_as_one_text() {
    // Kept on purpose: JavaScript holds each half apart, Rust text cannot.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    fs::write(&file, "{\"a\":\"\\ud800\",\"b\":\"x\\udfff\"}\n").unwrap();
    let (_, visited) = collect(&file, None, None);
    assert_eq!(
        visited,
        [(serde_json::json!({ "a": "\u{FFFD}", "b": "x\u{FFFD}" }), 0)]
    );
}
