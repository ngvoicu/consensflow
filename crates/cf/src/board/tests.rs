//! What the board commands that print answers say they wrote: `cf` tells the
//! board which answers it carried whole, once its output is complete, and the
//! board receives those. A read by itself is no receipt, so a command that
//! failed, was refused, was cut off, printed no body in full, or printed more
//! than a harness shows its model (`cut`) says nothing.

use std::cell::Cell;
use std::io::{self, Write};
use std::rc::Rc;

use cf_board::scripted::{hang_up, reply, scripted, Reply, ScriptedApi};
use cf_board::Board;
use serde_json::{json, Value};

use super::run;

/// Where a command's output goes, which notes how many requests the board had
/// received when the output was flushed, and can be made to fail.
struct Output<'a> {
    board: &'a ScriptedApi,
    written: Vec<u8>,
    flushed_at: Rc<Cell<Option<usize>>>,
    fails: Option<io::ErrorKind>,
}

impl<'a> Output<'a> {
    fn new(board: &'a ScriptedApi) -> Self {
        Self {
            board,
            written: Vec::new(),
            flushed_at: Rc::default(),
            fails: None,
        }
    }
}

impl Write for Output<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(kind) = self.fails {
            return Err(kind.into());
        }
        self.written.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(kind) = self.fails {
            return Err(kind.into());
        }
        self.flushed_at.set(Some(self.board.received().len()));
        Ok(())
    }
}

/// What one run of a command did.
struct Ran {
    /// The exit code, or the kind of the failure to write.
    code: Result<u8, io::ErrorKind>,
    out: String,
    err: String,
    /// The requests the board received: method, path, and body as JSON.
    asked: Vec<(String, String, Option<Value>)>,
    /// How many requests the board had when the output was flushed.
    flushed_at: Option<usize>,
}

fn cf_with(words: &[&str], json: bool, replies: Vec<Reply>, fails: Option<io::ErrorKind>) -> Ran {
    let api = scripted(replies);
    let board = Board::new(Some(&api.url), "tok");
    let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
    let mut out = Output::new(&api);
    out.fails = fails;
    let mut err = Vec::new();
    let code = run(&words, json, &board, &mut io::empty(), &mut out, &mut err)
        .map_err(|failure| failure.kind());
    Ran {
        code,
        out: String::from_utf8_lossy(&out.written).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
        asked: api
            .received()
            .into_iter()
            .map(|received| {
                let body = received.json();
                (received.method, received.path, body)
            })
            .collect(),
        flushed_at: out.flushed_at.get(),
    }
}

fn cf(words: &[&str], replies: Vec<Reply>) -> Ran {
    cf_with(words, false, replies, None)
}

/// Task 3 as the board gives it: the brief received, an answer waiting to be
/// pasted, one read already, and a note waiting.
fn thread() -> Value {
    json!({ "task": {
        "number": 3, "state": "waiting", "assignee": "zeus", "requester": "chief",
        "title": "Parser", "blockedBy": [], "pool": "worker", "tier": "standard",
        "deletedAt": null,
        "messages": [
            { "id": 11, "kind": "task", "state": "delivered", "task": 3, "sender": "chief", "body": "Parser" },
            { "id": 12, "kind": "answer", "state": "queued", "task": 3, "sender": "chief", "body": "JSON\nand then YAML" },
            { "id": 13, "kind": "answer", "state": "read", "task": 3, "sender": "chief", "body": "Tabs" },
            { "id": 14, "kind": "note", "state": "queued", "task": 3, "sender": "chief", "body": "Mind the tests" },
            { "id": 15, "kind": "answer", "state": "queued", "task": 3, "sender": "chief", "body": "Two spaces" },
        ],
    } })
}

fn asked(ran: &Ran) -> Vec<(&str, &str)> {
    ran.asked
        .iter()
        .map(|(method, path, _)| (method.as_str(), path.as_str()))
        .collect()
}

fn acknowledged(answers: &[i64], via: &str) -> Option<Value> {
    Some(json!({ "answers": answers, "via": via }))
}

#[test]
fn a_task_thread_printed_whole_is_acknowledged_once_after_the_output_is_flushed_in_text_and_in_json(
) {
    for json in [false, true] {
        let ran = cf_with(
            &["task", "get", "T-3"],
            json,
            vec![reply(200, thread()), reply(200, json!({}))],
            None,
        );
        assert_eq!(ran.code.unwrap(), 0);
        assert!(ran.out.contains("and then YAML"), "{}", ran.out);
        assert_eq!(
            asked(&ran),
            [("GET", "/api/tasks/3"), ("POST", "/api/answers/read")],
            "json: {json}"
        );
        assert_eq!(
            ran.asked[1].2,
            acknowledged(&[12, 15], "task"),
            "the answers still waiting, and no other message"
        );
        assert_eq!(
            ran.flushed_at,
            Some(1),
            "the output was complete when the board was told: it had been asked only for the thread"
        );
    }
}

#[test]
fn a_thread_with_no_answer_waiting_says_nothing() {
    let mut task = thread();
    task["task"]["messages"] = json!([
        { "id": 11, "kind": "task", "state": "delivered", "task": 3, "sender": "chief", "body": "Parser" },
        { "id": 13, "kind": "answer", "state": "read", "task": 3, "sender": "chief", "body": "Tabs" },
    ]);
    let ran = cf(&["task", "get", "T-3"], vec![reply(200, task)]);
    assert_eq!(ran.code.unwrap(), 0);
    assert_eq!(asked(&ran), [("GET", "/api/tasks/3")]);
}

#[test]
fn a_transcript_says_nothing_of_any_answer_whether_it_prints_the_thread_or_not() {
    for json in [false, true] {
        let ran = cf_with(
            &["task", "get", "T-3", "--transcript", "--last", "2"],
            json,
            vec![
                reply(200, thread()),
                reply(200, json!({ "total": 0, "items": [] })),
                reply(200, json!({})),
            ],
            None,
        );
        assert_eq!(ran.code.unwrap(), 0);
        assert_eq!(
            asked(&ran),
            [
                ("GET", "/api/tasks/3"),
                ("GET", "/api/tasks/3/transcript?last=2")
            ],
            "json: {json}: no acknowledgement"
        );
    }
}

#[test]
fn a_bad_last_asks_the_board_nothing_and_so_receives_nothing() {
    for last in ["0", "x", "1.5", "-2", "Infinity", "NaN"] {
        let ran = cf(
            &["task", "get", "T-3", "--transcript", "--last", last],
            vec![reply(200, thread()), reply(200, json!({}))],
        );
        assert_eq!(ran.code.unwrap(), 2, "--last {last}");
        assert!(ran.asked.is_empty(), "--last {last}: {:?}", ran.asked);
        assert_eq!(ran.err, "cf: --last takes a number of items\n");
        assert_eq!(ran.out, "");
    }
    // Without --transcript, --last is not read, as it never was.
    let ran = cf(
        &["task", "get", "T-3", "--last", "x"],
        vec![reply(200, thread()), reply(200, json!({}))],
    );
    assert_eq!(ran.code.unwrap(), 0);
    assert_eq!(
        asked(&ran),
        [("GET", "/api/tasks/3"), ("POST", "/api/answers/read")]
    );
}

#[test]
fn a_task_number_that_is_no_number_asks_the_board_nothing() {
    let ran = cf(&["task", "get", "T-x"], vec![reply(200, thread())]);
    assert_eq!(ran.code.unwrap(), 2);
    assert!(ran.asked.is_empty());
}

#[test]
fn a_request_the_board_refused_says_nothing_of_any_answer() {
    for (status, body) in [
        (
            404,
            json!({ "error": "unknown-task", "message": "no task T-3 in this project" }),
        ),
        (500, json!({})),
    ] {
        let ran = cf(
            &["task", "get", "T-3"],
            vec![reply(status, body), reply(200, json!({}))],
        );
        assert_eq!(ran.code.unwrap(), 1, "{status}");
        assert_eq!(asked(&ran), [("GET", "/api/tasks/3")], "{status}");
        assert_eq!(ran.out, "");
    }
}

#[test]
fn an_output_that_could_not_be_written_says_nothing_of_the_answers_in_it() {
    for fails in [io::ErrorKind::BrokenPipe, io::ErrorKind::WriteZero] {
        for json in [false, true] {
            let ran = cf_with(
                &["task", "get", "T-3"],
                json,
                vec![reply(200, thread()), reply(200, json!({}))],
                Some(fails),
            );
            assert_eq!(ran.code.unwrap_err(), fails);
            assert_eq!(asked(&ran), [("GET", "/api/tasks/3")], "json: {json}");
        }
    }
}

#[test]
fn an_acknowledgement_that_fails_leaves_what_was_printed_and_the_exit_as_they_were() {
    // A daemon of Node's has no such route; another's reply was lost on its way.
    let node = reply(
        404,
        json!({ "error": "unknown-route", "message": "no such command: POST /api/answers/read" }),
    );
    for lost in [node, hang_up(), reply(500, json!({}))] {
        let ran = cf(&["task", "get", "T-3"], vec![reply(200, thread()), lost]);
        assert_eq!(ran.code.unwrap(), 0);
        assert!(ran.out.contains("JSON\nand then YAML"));
        assert_eq!(ran.err, "");
        assert_eq!(
            asked(&ran),
            [("GET", "/api/tasks/3"), ("POST", "/api/answers/read")],
            "it was said, and nothing came of it"
        );
    }
    // The board gone by then: nothing is raised either.
    let ran = cf(&["task", "get", "T-3"], vec![reply(200, thread())]);
    assert_eq!(ran.code.unwrap(), 0);
    assert_eq!(ran.err, "");
}

/// A message `cf inbox read` printed whole.
fn message(kind: &str, state: &str) -> Value {
    json!({ "message": {
        "id": 12, "kind": kind, "state": state, "taskNumber": 3, "sender": "chief",
        "body": "JSON\nand then YAML",
    } })
}

#[test]
fn an_answer_cf_inbox_read_printed_whole_is_acknowledged_and_nothing_else_is() {
    for json in [false, true] {
        let ran = cf_with(
            &["inbox", "read", "m-12"],
            json,
            vec![
                reply(200, message("answer", "queued")),
                reply(200, json!({})),
            ],
            None,
        );
        assert_eq!(ran.code.unwrap(), 0);
        assert!(ran.out.contains("and then YAML"));
        assert_eq!(
            asked(&ran),
            [("GET", "/api/inbox/12"), ("POST", "/api/answers/read")]
        );
        assert_eq!(ran.asked[1].2, acknowledged(&[12], "inbox"));
        assert_eq!(ran.flushed_at, Some(1));
    }
    for (kind, state) in [
        ("note", "queued"),
        ("answer", "read"),
        ("question", "queued"),
    ] {
        let ran = cf(
            &["inbox", "read", "m-12"],
            vec![reply(200, message(kind, state)), reply(200, json!({}))],
        );
        assert_eq!(ran.code.unwrap(), 0);
        assert_eq!(asked(&ran), [("GET", "/api/inbox/12")], "{kind} {state}");
    }
    // A message that is no number asks nothing.
    let ran = cf(&["inbox", "read", "T-3"], vec![reply(200, json!({}))]);
    assert_eq!(ran.code.unwrap(), 2);
    assert!(ran.asked.is_empty());
}

/// The thread of task 3 with the note in it (the fourth message) as long as `body`.
fn thread_with_a_note(body: &str) -> Value {
    let mut task = thread();
    task["task"]["messages"][3]["body"] = json!(body);
    task
}

/// What `cf task get T-3` asks the board and says of it, with `body` in its thread.
fn get_with_a_note(body: &str, json: bool) -> Vec<(String, String)> {
    let ran = cf_with(
        &["task", "get", "T-3"],
        json,
        vec![reply(200, thread_with_a_note(body)), reply(200, json!({}))],
        None,
    );
    assert_eq!(ran.code.unwrap(), 0);
    ran.asked
        .into_iter()
        .map(|(method, path, _)| (method, path))
        .collect()
}

fn only_the_thread() -> Vec<(String, String)> {
    vec![("GET".to_owned(), "/api/tasks/3".to_owned())]
}

fn the_thread_and_what_it_carried_whole() -> Vec<(String, String)> {
    vec![
        ("GET".to_owned(), "/api/tasks/3".to_owned()),
        ("POST".to_owned(), "/api/answers/read".to_owned()),
    ]
}

#[test]
fn a_thread_a_harness_may_cut_says_nothing_of_the_answers_in_it_and_they_come_as_text() {
    for json in [false, true] {
        for (what, note) in [
            ("a note of 30,000 characters", "x".repeat(30_000)),
            (
                "a note of 20,000, past Codex's cut though under Claude's",
                "x".repeat(20_000),
            ),
            (
                "one of 6,000 two-byte characters: bytes, not characters",
                "é".repeat(6_000),
            ),
        ] {
            assert_eq!(
                get_with_a_note(&note, json),
                only_the_thread(),
                "json: {json}: {what}: printed past what a harness shows"
            );
        }
        assert_eq!(
            get_with_a_note(&"x".repeat(8_000), json),
            the_thread_and_what_it_carried_whole(),
            "json: {json}: a long thread that a harness still shows whole"
        );
    }
}

#[test]
fn what_is_measured_is_what_was_printed_so_lines_count_in_the_text_and_not_in_the_json() {
    // 2,100 lines of nothing: a few bytes, and more lines than a harness shows.
    let lines = "\n".repeat(2_100);
    assert_eq!(get_with_a_note(&lines, false), only_the_thread());
    assert_eq!(
        get_with_a_note(&lines, true),
        the_thread_and_what_it_carried_whole(),
        "the JSON writes them in one line of escapes"
    );
}

#[test]
fn an_answer_cf_inbox_read_printed_past_what_a_harness_shows_is_not_acknowledged() {
    for json in [false, true] {
        for (body, acknowledged) in [("x".repeat(20_000), false), ("x".repeat(8_000), true)] {
            let mut answer = message("answer", "queued");
            answer["message"]["body"] = json!(body);
            let ran = cf_with(
                &["inbox", "read", "m-12"],
                json,
                vec![reply(200, answer), reply(200, json!({}))],
                None,
            );
            assert_eq!(ran.code.unwrap(), 0);
            assert_eq!(
                asked(&ran).len(),
                if acknowledged { 2 } else { 1 },
                "json: {json}: {} characters",
                body.len()
            );
        }
    }
}

#[test]
fn a_list_of_the_inbox_prints_first_lines_and_says_nothing_of_the_answers_in_it() {
    let list = json!({ "messages": [
        { "id": 12, "kind": "answer", "state": "queued", "task": 3, "sender": "chief", "preview": "JSON" },
        { "id": 14, "kind": "answer", "state": "queued", "task": 3, "sender": "chief", "preview": "Two spaces" },
    ] });
    for json in [false, true] {
        let ran = cf_with(
            &["inbox"],
            json,
            vec![reply(200, list.clone()), reply(200, json!({}))],
            None,
        );
        assert_eq!(ran.code.unwrap(), 0);
        assert_eq!(asked(&ran), [("GET", "/api/inbox")], "json: {json}");
    }
}
