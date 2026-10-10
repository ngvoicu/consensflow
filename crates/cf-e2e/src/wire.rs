//! The bridge's wire, as the suites meet it: JSON frames, one to a line, on a
//! program's standard streams. The daemon and the pane host speak it to each
//! other (`{v, id, kind, op, body}`: a `req` is answered by the `res` of its
//! id; an `evt` is answered by none), and the rig stands between them as the
//! app does. This module reads lines off a pipe, with a deadline for the first
//! (the handle a program says when it is ready), and keeps the table of
//! requests a case made and is waiting on the answers of.

use std::collections::HashMap;
use std::io::BufRead;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

/// Reads a stream a line at a time: each line without its end (`\n`, or
/// `\r\n`), what is not UTF-8 read as U+FFFD, and the empty lines skipped, as
/// the rig's parser of frames always did.
pub struct Lines<R> {
    reader: R,
}

impl<R: BufRead> Lines<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    /// The reader, with whatever it has read ahead still in it.
    pub fn into_inner(self) -> R {
        self.reader
    }
}

impl<R: BufRead> Iterator for Lines<R> {
    type Item = String;

    /// The next line, which may wait for ever on a stream that stays open and
    /// says nothing; none once the stream is done (or cannot be read).
    fn next(&mut self) -> Option<String> {
        loop {
            let mut bytes = Vec::new();
            match self.reader.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {}
            }
            while matches!(bytes.last(), Some(b'\n' | b'\r')) {
                bytes.pop();
            }
            if !bytes.is_empty() {
                return Some(String::from_utf8_lossy(&bytes).into_owned());
            }
        }
    }
}

/// What came first on a stream.
#[derive(Debug, PartialEq, Eq)]
pub enum First {
    /// A line.
    Line(String),
    /// The end of the stream, with no line.
    Ended,
    /// Nothing, in the time given: the stream is still open.
    TimedOut,
}

/// Reads the first line of `reader`, waiting `within` for it, and hands the
/// reader back so that the rest can be read from where it stands (what it has
/// read ahead is in it). The first line of a program is the handle it says it
/// is ready with. `None` for the reader when the time ran out: it is on a
/// thread that stays blocked on the stream until the program is ended.
pub fn first_line<R>(reader: R, within: Duration) -> (First, Option<R>)
where
    R: BufRead + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut lines = Lines::new(reader);
        let first = lines.next().map_or(First::Ended, First::Line);
        // The case may have given up; the reader goes with the thread then.
        let _ = sender.send((first, lines.into_inner()));
    });
    match receiver.recv_timeout(within) {
        Ok((first, reader)) => (first, Some(reader)),
        Err(RecvTimeoutError::Timeout) => (First::TimedOut, None),
        Err(RecvTimeoutError::Disconnected) => (First::Ended, None),
    }
}

/// Calls `on_line` with each line of `reader`, on a thread of its own, until
/// the stream is done or `on_line` says to leave (by answering false); then
/// drops the reader, which closes the stream, and calls `done`.
pub fn read_each<R>(
    reader: R,
    mut on_line: impl FnMut(String) -> bool + Send + 'static,
    done: impl FnOnce() + Send + 'static,
) where
    R: BufRead + Send + 'static,
{
    thread::spawn(move || {
        for line in Lines::new(reader) {
            if !on_line(line) {
                break;
            }
        }
        done();
    });
}

/// A request frame, as a line.
pub fn request(id: &str, op: &str, body: &Value) -> String {
    format!(
        "{}\n",
        json!({ "v": 1, "id": id, "kind": "req", "op": op, "body": body })
    )
}

/// A response frame, as a line.
pub fn response(id: &str, op: &str, body: &Value) -> String {
    format!(
        "{}\n",
        json!({ "v": 1, "id": id, "kind": "res", "op": op, "body": body })
    )
}

/// The requests a case has made and waits on the answers of.
#[derive(Debug, Clone, Default)]
pub struct Pending {
    waiting: Arc<Mutex<HashMap<String, Sender<Value>>>>,
}

impl Pending {
    /// Says the answer to the request `id` is wanted; the answer comes on the
    /// receiver.
    pub fn expect(&self, id: &str) -> Receiver<Value> {
        let (sender, receiver) = mpsc::channel();
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.to_owned(), sender);
        receiver
    }

    /// Gives up on the answer to `id`.
    pub fn forget(&self, id: &str) {
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }

    /// Hands the answer `body` to whoever waits for `id`; whether anyone did.
    pub fn answer(&self, id: &str, body: &Value) -> bool {
        let waiting = self
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
        waiting.is_some_and(|waiting| waiting.send(body.clone()).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn lines(text: &[u8]) -> Vec<String> {
        Lines::new(Cursor::new(text.to_vec())).collect()
    }

    #[test]
    fn a_stream_is_read_a_line_at_a_time_without_the_ends_of_its_lines() {
        assert_eq!(lines(b"one\ntwo\r\nthree"), ["one", "two", "three"]);
        assert_eq!(lines(b""), Vec::<String>::new());
    }

    #[test]
    fn an_empty_line_is_skipped_and_what_is_not_text_is_read_as_the_replacement_character() {
        assert_eq!(lines(b"\n\r\nfirst\n\n\nsecond\n"), ["first", "second"]);
        assert_eq!(lines(b"caf\xc3\xa9 \xff!\n"), ["caf\u{e9} \u{fffd}!"]);
    }

    #[test]
    fn the_first_line_is_waited_for_and_the_reader_goes_on_from_there() {
        let (first, reader) = first_line(
            Cursor::new(b"handle\nframe one\nframe two\n".to_vec()),
            Duration::from_secs(10),
        );
        assert_eq!(first, First::Line("handle".into()));
        let rest: Vec<String> = Lines::new(reader.unwrap()).collect();
        assert_eq!(rest, ["frame one", "frame two"]);
    }

    #[test]
    fn a_stream_that_ends_before_a_line_says_so() {
        let (first, reader) = first_line(Cursor::new(Vec::new()), Duration::from_secs(10));
        assert_eq!(first, First::Ended);
        assert!(reader.is_some());
    }

    #[test]
    fn a_first_line_that_does_not_come_in_time_gives_up_the_reader() {
        /// A stream that has nothing to say until it is let go.
        struct Silent(Receiver<()>);
        impl std::io::Read for Silent {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                let _ = self.0.recv();
                Ok(0)
            }
        }
        let (letting_go, silent) = mpsc::channel();
        let (first, reader) = first_line(
            std::io::BufReader::new(Silent(silent)),
            Duration::from_millis(100),
        );
        assert_eq!(first, First::TimedOut);
        assert!(reader.is_none());
        let _ = letting_go.send(());
    }

    #[test]
    fn every_line_is_given_to_the_reader_of_each_and_the_end_is_told() {
        let (sender, receiver) = mpsc::channel();
        let ended = sender.clone();
        read_each(
            Cursor::new(b"a\nb\n".to_vec()),
            move |line| {
                let _ = sender.send(line);
                true
            },
            move || {
                let _ = ended.send("done".to_owned());
            },
        );
        let seen: Vec<String> = receiver.iter().take(3).collect();
        assert_eq!(seen, ["a", "b", "done"]);
    }

    #[test]
    fn a_reader_of_each_that_says_to_leave_reads_no_more_and_the_end_is_told() {
        let (sender, receiver) = mpsc::channel();
        let ended = sender.clone();
        read_each(
            Cursor::new(b"a\nb\nc\n".to_vec()),
            move |line| {
                let _ = sender.send(line.clone());
                line != "b"
            },
            move || {
                let _ = ended.send("done".to_owned());
            },
        );
        let seen: Vec<String> = receiver.iter().take(3).collect();
        assert_eq!(seen, ["a", "b", "done"]);
    }

    #[test]
    fn a_request_and_a_response_are_frames_of_the_wire_in_the_order_of_its_keys() {
        assert_eq!(
            request("n-test-1", "ping", &json!({})),
            "{\"v\":1,\"id\":\"n-test-1\",\"kind\":\"req\",\"op\":\"ping\",\"body\":{}}\n"
        );
        assert_eq!(
            response("r-1", "ping", &json!({"ok": true})),
            "{\"v\":1,\"id\":\"r-1\",\"kind\":\"res\",\"op\":\"ping\",\"body\":{\"ok\":true}}\n"
        );
    }

    #[test]
    fn an_answer_goes_to_whoever_waits_for_it_once_and_to_no_one_else() {
        let pending = Pending::default();
        let waiting = pending.expect("r-1");
        assert!(!pending.answer("r-2", &json!(1)), "no one waits for r-2");
        assert!(pending.answer("r-1", &json!({"ok": true})));
        assert_eq!(
            waiting.recv_timeout(Duration::from_secs(1)).unwrap(),
            json!({"ok": true})
        );
        assert!(!pending.answer("r-1", &json!(2)), "answered once");
        let given_up = pending.expect("r-3");
        pending.forget("r-3");
        assert!(!pending.answer("r-3", &json!(3)));
        assert!(given_up.recv_timeout(Duration::from_millis(20)).is_err());
    }
}
