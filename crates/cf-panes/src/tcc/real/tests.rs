//! The reader of the log as it runs (T-1194): what it is asked, and that a program's lines
//! come one at a time, its end comes, and a stop ends it.

use std::time::{Duration, Instant};

use super::*;

fn lines_until_the_end(stream: &mut LogStream) -> Vec<String> {
    let mut lines = Vec::new();
    let stop = Instant::now() + Duration::from_secs(20);
    while Instant::now() < stop {
        match stream.recv(Duration::from_millis(200)) {
            Read::Line(line) => lines.push(line),
            Read::Quiet => {}
            Read::End => return lines,
        }
    }
    panic!("the reader never ended: {lines:?}");
}

#[test]
fn the_log_is_asked_for_the_dialogs_and_for_the_requests_in_the_apps_name_alone() {
    let predicate = predicate(1811);
    assert_eq!(
        predicate,
        "subsystem == \"com.apple.TCC\" AND (eventMessage CONTAINS \"AUTHREQ_PROMPTING\" OR \
         (eventMessage CONTAINS \"AUTHREQ_ATTRIBUTION\" AND eventMessage CONTAINS \"pid=1811,\"))"
    );
    let arguments: Vec<String> = log_arguments(1811)
        .into_iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    // The error stream is in the output, so that a refusal comes as a line; the log is named by
    // its whole path, for a shell finds nothing else on the path it is given.
    assert_eq!(&arguments[..2], ["-c", "exec \"$@\" 2>&1"]);
    assert_eq!(
        &arguments[3..10],
        [
            "/usr/bin/log",
            "stream",
            "--level",
            "info",
            "--style",
            "ndjson",
            "--predicate"
        ]
    );
    assert_eq!(arguments.len(), 11);
}

#[test]
fn a_programs_lines_come_one_at_a_time_and_then_its_end() {
    let mut stream = LogStream::start(
        Path::new("/bin/sh"),
        vec!["-c".into(), "echo first; echo second".into()],
    )
    .expect("it starts");
    assert_eq!(lines_until_the_end(&mut stream), ["first", "second"]);
    // Once ended it stays ended.
    assert_eq!(stream.recv(Duration::from_millis(50)), Read::End);
}

#[test]
fn a_program_that_cannot_start_says_why() {
    let why = LogStream::start(Path::new("/no/such/reader"), vec![]).err();
    assert!(
        why.is_some_and(|why| why.contains("/no/such/reader")),
        "it says why"
    );
}

#[test]
fn a_reader_that_is_asked_to_end_ends_though_it_has_nothing_to_say() {
    let mut stream = LogStream::start(
        Path::new("/bin/sh"),
        vec!["-c".into(), "echo started; exec /bin/sleep 600".into()],
    )
    .expect("it starts");
    let stop = Instant::now() + Duration::from_secs(20);
    // Its first line, and then nothing for as long as the test waits.
    loop {
        assert!(Instant::now() < stop, "no first line");
        if stream.recv(Duration::from_millis(200)) == Read::Line("started".to_owned()) {
            break;
        }
    }
    assert_eq!(stream.recv(Duration::from_millis(300)), Read::Quiet);
    let asked = Instant::now();
    stream.end();
    loop {
        assert!(Instant::now() < stop, "it did not end");
        if stream.recv(Duration::from_millis(200)) == Read::End {
            break;
        }
    }
    assert!(
        asked.elapsed() < Duration::from_secs(10),
        "{:?}",
        asked.elapsed()
    );
}
