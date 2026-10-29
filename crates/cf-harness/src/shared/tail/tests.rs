//! A record read from its end within a bound.

use std::ops::ControlFlow;

use super::{lines_back, BLOCK};

fn written(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("record.jsonl");
    std::fs::write(&file, text).unwrap();
    (dir, file)
}

fn all_back(file: &std::path::Path, bound: u64) -> Vec<String> {
    let mut seen = Vec::new();
    lines_back(file, bound, |line| {
        seen.push(line.to_owned());
        ControlFlow::Continue(())
    })
    .unwrap();
    seen
}

#[test]
fn gives_every_line_from_the_last_back_and_none_empty() {
    let (_dir, file) = written("one\ntwo\n\nthree\n");
    assert_eq!(all_back(&file, 1 << 20), ["three", "two", "one"]);
    let (_dir, unended) = written("one\ntwo");
    assert_eq!(all_back(&unended, 1 << 20), ["two", "one"]);
}

#[test]
fn a_line_longer_than_a_block_is_given_whole() {
    let long = "x".repeat(usize::try_from(BLOCK).unwrap() * 3 + 7);
    let (_dir, file) = written(&format!("first\n{long}\nlast\n"));
    let seen = all_back(&file, 1 << 30);
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0], "last");
    assert_eq!(seen[1], long);
    assert_eq!(seen[2], "first");
}

#[test]
fn stops_at_the_bound_and_never_gives_the_line_it_cuts() {
    let (_dir, file) = written("aaaa\nbbbb\ncccc\n");
    // The last 7 bytes are "b\ncccc\n": "cccc" is whole, "bbbb" is cut.
    assert_eq!(all_back(&file, 7), ["cccc"]);
    assert_eq!(all_back(&file, 10), ["cccc", "bbbb"]);
}

#[test]
fn stops_where_its_reader_breaks() {
    let (_dir, file) = written("one\ntwo\nthree\n");
    let mut seen = Vec::new();
    lines_back(&file, 1 << 20, |line| {
        seen.push(line.to_owned());
        if line == "two" {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .unwrap();
    assert_eq!(seen, ["three", "two"]);
}

#[test]
fn reads_no_more_of_a_long_record_than_its_end_when_what_it_needs_is_there() {
    // 40 MB of lines, the one wanted last: found at once; the bound past 16 MB is never read.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("long.jsonl");
    let mut text = String::with_capacity(40 << 20);
    while text.len() < 40 << 20 {
        text.push_str(&"y".repeat(1000));
        text.push('\n');
    }
    text.push_str("wanted\n");
    std::fs::write(&file, &text).unwrap();
    let mut visited = 0;
    lines_back(&file, super::TAIL_BOUND, |line| {
        visited += 1;
        if line == "wanted" {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .unwrap();
    assert_eq!(visited, 1);
    let all = all_back(&file, super::TAIL_BOUND);
    assert!(all.len() < 16_800, "{} lines within 16 MB", all.len());
}
