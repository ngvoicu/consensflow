//! What the board commands that print answers say they wrote: `cf` tells the
//! board which answers it carried whole, once its output is complete, and the
//! board receives those. A read by itself is no receipt, so a command that
//! failed, was refused, was cut off, printed no body in full, or printed more
//! than a harness shows its model (`cut`) says nothing.

use std::cell::Cell;
use std::ffi::OsString;
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
    let words: Vec<OsString> = words.iter().map(OsString::from).collect();
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

/// T-942: a message whose text waits (for the chief's meeting, or for the Resume) is in the
/// thread as a line that says so, in its body; it is printed under its message's line, and
/// it is no answer written whole, so nothing is received by the print.
#[test]
fn a_thread_prints_the_line_that_stands_where_a_waiting_message_is_and_receives_nothing() {
    let waits = "(m-16 waits until your meeting with the human ends: it comes to you then)";
    let mut task = thread();
    task["task"]["messages"] = json!([
        { "id": 11, "kind": "task", "state": "delivered", "task": 3, "sender": "chief", "body": "Parser" },
        { "id": 16, "kind": "result", "state": "queued", "task": 3, "sender": "zeus", "body": waits },
    ]);
    for json in [false, true] {
        // A reply for the acknowledgement too, so that one that was wrongly sent is seen.
        let ran = cf_with(
            &["task", "get", "T-3"],
            json,
            vec![reply(200, task.clone()), reply(200, json!({}))],
            None,
        );
        assert_eq!(ran.code.unwrap(), 0);
        assert!(ran.out.contains(waits), "{}", ran.out);
        if !json {
            assert!(
                ran.out
                    .contains(&format!("m-16 [queued] result T-3 from @zeus\n{waits}")),
                "{}",
                ran.out
            );
        }
        assert_eq!(asked(&ran), [("GET", "/api/tasks/3")], "json: {json}");
    }
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

/// An answer of the thread whose paste is on its way is acknowledged with the queued ones: a
/// window runs `cf task get` as soon as it has the answer, before the board has seen it arrive.
#[test]
fn a_task_thread_printed_whole_acknowledges_the_answer_being_pasted_with_the_queued_ones() {
    let mut pasted = thread();
    pasted["task"]["messages"].as_array_mut().unwrap().push(
        json!({ "id": 16, "kind": "answer", "state": "delivering", "task": 3,
            "sender": "chief", "body": "Four spaces" }),
    );
    let ran = cf(
        &["task", "get", "T-3"],
        vec![reply(200, pasted), reply(200, json!({}))],
    );
    assert_eq!(ran.code.unwrap(), 0);
    assert_eq!(
        ran.asked[1].2,
        acknowledged(&[12, 15, 16], "task"),
        "the answers still waiting or being pasted, and no other message"
    );
}

/// A message `cf inbox read` printed whole.
fn message(kind: &str, state: &str) -> Value {
    json!({ "message": {
        "id": 12, "kind": kind, "state": state, "taskNumber": 3, "sender": "chief",
        "body": "JSON\nand then YAML",
    } })
}

/// A message `cf inbox read` printed whole while it still waited to be pasted, or
/// while its paste was on its way (a window reads a message the moment it has it,
/// before the board has seen it arrive), is acknowledged, whatever its kind: the
/// board decides what a read in the inbox receives, and a question, a result or a
/// note read there is not typed in after it (T-260). One that is delivered, read or
/// withdrawn is not.
#[test]
fn a_message_cf_inbox_read_printed_whole_while_it_waited_or_was_pasted_is_acknowledged_and_nothing_else_is(
) {
    for kind in ["answer", "question", "result", "note"] {
        for (state, json) in [
            ("queued", false),
            ("queued", true),
            ("delivering", false),
            ("delivering", true),
        ] {
            let ran = cf_with(
                &["inbox", "read", "m-12"],
                json,
                vec![reply(200, message(kind, state)), reply(200, json!({}))],
                None,
            );
            assert_eq!(ran.code.unwrap(), 0);
            assert!(ran.out.contains("and then YAML"));
            assert_eq!(
                asked(&ran),
                [("GET", "/api/inbox/12"), ("POST", "/api/answers/read")],
                "{kind} {state}"
            );
            assert_eq!(ran.asked[1].2, acknowledged(&[12], "inbox"));
            assert_eq!(ran.flushed_at, Some(1));
        }
    }
    for (kind, state) in [
        ("note", "delivered"),
        ("answer", "read"),
        ("question", "failed"),
        ("result", "cancelled"),
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

/// Where the read in the inbox (T-260) meets the Resume after the Close: a
/// message whose text waits for the Resume is refused to `cf inbox read` (409
/// `waits`), whatever its kind, so `cf` prints none of it and says it received
/// nothing. Only the board's refusal is said. So too one a meeting with the chief
/// holds, which waits for the meeting's end (3.0.7 chief-meeting, CMT-14).
#[test]
fn a_message_that_waits_for_the_resume_is_not_printed_and_nothing_is_acknowledged() {
    for until in [
        "until the human resumes the project",
        "until your meeting with the human ends",
    ] {
        let waits = json!({
            "error": "waits",
            "message": format!("m-12 waits {until}: it comes to you then. Do not ask for it again now."),
        });
        for json in [false, true] {
            let ran = cf_with(
                &["inbox", "read", "m-12"],
                json,
                vec![reply(409, waits.clone()), reply(200, json!({}))],
                None,
            );
            assert_ne!(ran.code.unwrap(), 0, "json: {json}");
            assert!(!ran.out.contains("and then YAML"), "json: {json}");
            assert!(
                ran.err.contains(until) || ran.out.contains(until),
                "json: {json}: {} {}",
                ran.out,
                ran.err
            );
            assert_eq!(asked(&ran), [("GET", "/api/inbox/12")], "json: {json}");
        }
    }
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

/// A brief as a person writes it, with the things a shell would change in it.
const BRIEF: &str = "Parser\nRun `echo $HOME` and \"$(date)\" as written.\nKeep the last line.";

/// Task 17 as the board gives an open one: its brief whole, no window, no thread.
fn open_task(brief: &str) -> Value {
    json!({ "task": {
        "number": 17, "state": "open", "assignee": null, "requester": "chief",
        "title": "Parser", "blockedBy": [], "pool": "worker", "tier": "standard",
        "deletedAt": null, "body": brief, "messages": [],
    } })
}

#[test]
fn an_open_task_prints_its_whole_brief_under_its_header() {
    let ran = cf(&["task", "get", "T-17"], vec![reply(200, open_task(BRIEF))]);
    assert_eq!(ran.code.unwrap(), 0);
    assert_eq!(
        ran.out,
        format!("T-17 [open] for a standard worker ← @chief: Parser\n\n{BRIEF}\n")
    );
    assert_eq!(asked(&ran), [("GET", "/api/tasks/17")]);
}

#[test]
fn an_open_task_as_json_carries_its_brief_and_an_empty_thread() {
    let ran = cf_with(
        &["task", "get", "T-17"],
        true,
        vec![reply(200, open_task(BRIEF))],
        None,
    );
    let got: Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(got["body"], BRIEF);
    assert_eq!(got["messages"], json!([]));
}

#[test]
fn a_given_task_prints_its_brief_once_in_the_message_that_carries_it() {
    let mut task = open_task(BRIEF);
    task["task"]["state"] = json!("working");
    task["task"]["assignee"] = json!("zeus");
    task["task"]["messages"] = json!([
        { "id": 12, "kind": "task", "state": "delivered", "sender": "chief", "recipient": "zeus",
          "task": 17, "body": BRIEF },
        { "id": 30, "kind": "result", "state": "queued", "sender": "zeus", "recipient": "chief",
          "task": 17, "body": "Done." },
    ]);
    let ran = cf(&["task", "get", "T-17"], vec![reply(200, task)]);
    assert_eq!(
        ran.out,
        format!(
            "T-17 [working] @zeus ← @chief: Parser\n\nm-12 [delivered] task T-17 from @chief\n{BRIEF}\n\nm-30 [queued] result T-17 from @zeus\nDone.\n"
        )
    );
}

#[test]
fn an_open_task_given_before_prints_the_brief_it_has_now_and_its_old_messages_after_it() {
    // Taken back to the board: its brief grew after the window it had ended.
    let now = format!("{BRIEF}\n\nResumed: the lexer too.");
    let mut task = open_task(&now);
    task["task"]["messages"] = json!([
        { "id": 12, "kind": "task", "state": "read", "sender": "chief", "recipient": "zeus",
          "task": 17, "body": BRIEF },
    ]);
    let ran = cf(&["task", "get", "T-17"], vec![reply(200, task)]);
    assert_eq!(
        ran.out,
        format!(
            "T-17 [open] for a standard worker ← @chief: Parser\n\n{now}\n\nm-12 [read] task T-17 from @chief\n{BRIEF}\n"
        )
    );
}

#[test]
fn a_task_its_window_has_no_message_of_prints_its_brief_under_its_header() {
    // A task given by name to @zeus while it is blocked on its needs: the ledger queues no
    // message for its window until they are accepted and the window is free (giving.rs,
    // finishing.rs's release_ready). `working` is only what the header prints here: a
    // blocked task is `open`.
    let mut task = open_task(BRIEF);
    task["task"]["state"] = json!("working");
    task["task"]["assignee"] = json!("zeus");
    let ran = cf(&["task", "get", "T-17"], vec![reply(200, task)]);
    assert_eq!(
        ran.out,
        format!("T-17 [working] @zeus ← @chief: Parser\n\n{BRIEF}\n")
    );
}

#[test]
fn a_task_whose_only_task_message_went_to_another_window_prints_its_brief_under_its_header() {
    // Given to @zeus, which has no task message of its own: @athena's copy is not one.
    let mut task = open_task(BRIEF);
    task["task"]["assignee"] = json!("zeus");
    task["task"]["messages"] = json!([
        { "id": 12, "kind": "task", "state": "read", "sender": "chief", "recipient": "athena",
          "task": 17, "body": "Parser, the first draft" },
    ]);
    let ran = cf(&["task", "get", "T-17"], vec![reply(200, task)]);
    assert_eq!(
        ran.out,
        format!(
            "T-17 [open] @zeus ← @chief: Parser\n\n{BRIEF}\n\nm-12 [read] task T-17 from @chief\nParser, the first draft\n"
        )
    );
}

#[test]
fn a_brief_the_board_withholds_prints_no_brief_block() {
    // The daemon sends an empty body only to a member reading an assigned task whose brief
    // waits at the human's gate; the chief and whoever gave the task read the brief, and the
    // message itself is left out for all (routes/task.rs:60-67). This test models the CLI's
    // rule for an empty body on an open task, which the daemon never serves.
    let ran = cf(&["task", "get", "T-17"], vec![reply(200, open_task(""))]);
    assert_eq!(
        ran.out,
        "T-17 [open] for a standard worker ← @chief: Parser\n\n\n"
    );
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

/// A role too long for one message went into its window in part, and `cf inbox
/// read` printed its note whole once it was delivered, while its delivery was
/// still being confirmed (the note tells the window to read it now, before the
/// daemon has seen the paste arrive), or back in the queue (its paste given back,
/// T-1521): it says so to the board, up to the bound a
/// role is read whole at by the windows a role goes to by a note
/// (`role_read_whole`), past the one every harness shows. A note the board does not
/// say tells a role, or one read already, is as any message.
#[test]
fn a_role_cf_inbox_read_printed_whole_once_delivered_or_while_delivering_is_acknowledged_up_to_the_bound_of_a_role(
) {
    let role = |state: &str, body: String, said: bool| {
        let mut answer = message("note", state);
        answer["message"]["body"] = json!(body);
        answer["message"]["taskNumber"] = Value::Null;
        if said {
            answer["role"] = json!(true);
        }
        answer
    };
    for json in [false, true] {
        // The lines of a JSON output are one: its newlines are escapes. Printed as text, a
        // role past the bound is printed in parts (T-1513), and the first part is said read
        // as the first of them; JSON prints it whole, said read only within the bound.
        let in_parts = |of: usize| {
            Some(if json {
                acknowledged(&[12], "inbox").unwrap()
            } else {
                json!({ "answers": [12], "via": "inbox", "part": 1, "parts": of })
            })
        };
        for state in ["delivered", "delivering", "queued"] {
            for (body, taken) in [
                ("x".repeat(20_000), acknowledged(&[12], "inbox")),
                (
                    format!("{}\n", "x".repeat(40)).repeat(400),
                    acknowledged(&[12], "inbox"),
                ),
                ("x".repeat(30_000), if json { None } else { in_parts(2) }),
                ("\n".repeat(600), in_parts(2)),
            ] {
                let ran = cf_with(
                    &["inbox", "read", "m-12"],
                    json,
                    vec![
                        reply(200, role(state, body.clone(), true)),
                        reply(200, json!({})),
                    ],
                    None,
                );
                assert_eq!(ran.code.unwrap(), 0);
                let case = format!("{} bytes {state} json {json}", body.len());
                match taken {
                    Some(said) => {
                        assert_eq!(
                            asked(&ran),
                            [("GET", "/api/inbox/12"), ("POST", "/api/answers/read")],
                            "{case}"
                        );
                        assert_eq!(ran.asked[1].2, Some(said), "{case}");
                    }
                    None => assert_eq!(asked(&ran), [("GET", "/api/inbox/12")], "{case}"),
                }
            }
        }
    }
    for (state, said) in [("delivered", false), ("read", true)] {
        let ran = cf(
            &["inbox", "read", "m-12"],
            vec![
                reply(200, role(state, "x".repeat(20_000), said)),
                reply(200, json!({})),
            ],
        );
        assert_eq!(ran.code.unwrap(), 0);
        assert_eq!(asked(&ran), [("GET", "/api/inbox/12")], "{state} {said}");
    }
}

/// A note that tells its reader a role (`"role": true`), from ConsensFlow, in `state`.
fn a_role_note(state: &str, body: &str) -> Value {
    json!({
        "message": {
            "id": 12, "kind": "note", "state": state, "taskNumber": null, "sender": null,
            "body": body,
        },
        "role": true,
    })
}

/// A role over the bound a role is read whole at: a paragraph, then a table of the staff
/// longer than a part, then a paragraph.
fn a_role_over_the_bound() -> String {
    let rows: String = (0..700)
        .map(|at| format!("| member-{at:03} | worker, reviewer | standard |\n"))
        .collect();
    format!(
        "Your role.\n\n{}\n\n| Member | Roles | Tier |\n|---|---|---|\n{rows}\nThe last paragraph.",
        "An opening paragraph. ".repeat(100)
    )
}

/// What `cf` printed: the message's line, the words, and the line under them.
fn printed_in_part(out: &str) -> (&str, &str, &str) {
    let (head, rest) = out.split_once("\n\n").expect("the message's line");
    let (words, line) = rest
        .strip_suffix('\n')
        .unwrap_or(rest)
        .rsplit_once("\n\n")
        .expect("the part's line");
    (head, words, line)
}

/// The two branches' marks in one inbox (T-1556): a role read in parts (T-1513) and notes
/// shown, not typed (RS-25), one of them over. The list says each by its own state, a role
/// by its state as any note, a shown note `[not typed]` or `[not typed · over]`; `cf inbox
/// read` prints the role in parts with its part's line, and a shown note whole with its mark
/// and no part's line, and has no part 2 of it.
#[test]
fn an_inbox_that_holds_a_role_in_parts_and_shown_notes_prints_each_with_its_own_marks() {
    let list = json!({ "messages": [
        { "id": 21, "kind": "note", "state": "read", "task": 3, "sender": null,
          "preview": "T-3 is still working in the background: 1 command.", "shown": true },
        { "id": 20, "kind": "note", "state": "read", "task": 3, "sender": null,
          "preview": "T-3 was taken back from @zeus (ran out of quota) and waits for another standard worker.",
          "shown": true, "over": true },
        { "id": 12, "kind": "note", "state": "delivered", "task": null, "sender": null,
          "preview": "Your role." },
    ] });
    let ran = cf(&["inbox"], vec![reply(200, list)]);
    assert_eq!(ran.code.unwrap(), 0);
    assert_eq!(
        ran.out.lines().collect::<Vec<_>>(),
        [
            "m-21 [not typed] note T-3 from ConsensFlow: T-3 is still working in the background: 1 command.",
            "m-20 [not typed · over] note T-3 from ConsensFlow: T-3 was taken back from @zeus (ran out of quota) and waits for another standard worker.",
            "m-12 [delivered] note from ConsensFlow: Your role.",
        ]
    );
    let ran = cf(
        &["inbox", "read", "m-12"],
        vec![
            reply(200, a_role_note("delivered", &a_role_over_the_bound())),
            reply(200, json!({})),
        ],
    );
    assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
    let (head, _, line) = printed_in_part(&ran.out);
    assert_eq!(head, "m-12 [delivered] note from ConsensFlow");
    assert!(line.starts_with("(Part 1 of 2 of m-12."), "{line}");
    let over = json!({ "message": { "id": 20, "kind": "note", "state": "read", "taskNumber": 3,
        "sender": null, "body": "T-3 was taken back from @zeus (ran out of quota) and waits for another standard worker.",
        "receipt": { "shown": "was taken back from @zeus", "over": "2026-11-13T08:00:00.000Z" } } });
    let ran = cf(&["inbox", "read", "m-20"], vec![reply(200, over.clone())]);
    assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
    assert_eq!(
        ran.out,
        "m-20 [not typed · over] note T-3 from ConsensFlow\n\nT-3 was taken back from @zeus (ran out of quota) and waits for another standard worker.\n"
    );
    assert_eq!(
        asked(&ran),
        [("GET", "/api/inbox/20")],
        "a shown note read is nothing to receive"
    );
    let ran = cf(
        &["inbox", "read", "m-20", "--part", "2"],
        vec![reply(200, over)],
    );
    assert_eq!(ran.code.unwrap(), 2);
    assert!(ran.err.contains("--part"), "{}", ran.err);
}

/// T-1513 (review T-1510): `cf inbox read` prints a role over the bound a role is read
/// whole at (`role_read_whole`) in parts, each within it, the command the paste gives
/// printing the first, `--part N` the others. Each but the last ends with a line that says
/// which part it is and the command for the next, and that a table goes on where the part
/// ends in one; the last says it is the last. Each part printed is said to the board by its
/// number and how many there are, for the ledger to tell the role once every part was
/// printed. A part there is not is said wrong. In JSON a role is
/// printed whole, as it always was, and said read only within the bound.
#[test]
fn a_role_over_the_bound_is_printed_in_parts_each_within_it_and_each_said_to_the_board_by_its_number(
) {
    let body = a_role_over_the_bound();
    assert!(!cf_base::role_read::role_read_whole(&body));
    let mut words = Vec::new();
    for state in ["delivered", "delivering"] {
        words.clear();
        for (part, last_line) in [
            (None, "(Part 1 of 2 of m-12. The table above goes on in part 2. Print part 2 with: cf inbox read m-12 --part 2)"),
            (Some("2"), "(Part 2 of 2 of m-12, the last: you have read all of it once you have read every part.)"),
        ] {
            let mut command = vec!["inbox", "read", "m-12"];
            command.extend(part.map(|part| ["--part", part]).into_iter().flatten());
            let ran = cf(
                &command,
                vec![reply(200, a_role_note(state, &body)), reply(200, json!({}))],
            );
            assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
            assert!(
                cf_base::role_read::role_read_whole(&ran.out),
                "{state} {part:?}: {} bytes, {} lines",
                ran.out.len(),
                ran.out.matches('\n').count()
            );
            let (head, said, line) = printed_in_part(&ran.out);
            assert_eq!(head, format!("m-12 [{state}] note from ConsensFlow"));
            assert_eq!(line, last_line, "{state}");
            words.push(said.to_owned());
            assert_eq!(
                asked(&ran),
                [("GET", "/api/inbox/12"), ("POST", "/api/answers/read")]
            );
            let number = part.map_or(1, |part| part.parse::<usize>().unwrap());
            assert_eq!(
                ran.asked[1].2,
                Some(json!({ "answers": [12], "via": "inbox", "part": number, "parts": 2 })),
                "{state}"
            );
        }
        assert!(words[0].starts_with("Your role.\n\nAn opening paragraph."));
        assert!(words[0].ends_with(" | worker, reviewer | standard |"));
        assert!(
            words[1].starts_with("| member-"),
            "the table goes on in part 2"
        );
        assert!(words[1].ends_with("\n\nThe last paragraph."));
        assert_eq!(
            format!("{}\n{}", words[0], words[1]),
            body,
            "the parts are the role"
        );
    }
    // A part there is not, or none: said wrong, and nothing said to the board.
    for part in ["3", "0", "two"] {
        let ran = cf(
            &["inbox", "read", "m-12", "--part", part],
            vec![reply(200, a_role_note("delivered", &body))],
        );
        assert_eq!(ran.code.unwrap(), 2, "{part}");
        assert!(ran.err.contains("--part"), "{part}: {}", ran.err);
        assert_eq!(ran.out, "");
        assert!(asked(&ran).len() <= 1, "{part}");
    }
    // In JSON: the role whole, as it always was, and over the bound, said read to nobody.
    let ran = cf_with(
        &["inbox", "read", "m-12"],
        true,
        vec![
            reply(200, a_role_note("delivered", &body)),
            reply(200, json!({})),
        ],
        None,
    );
    assert_eq!(ran.code.unwrap(), 0);
    assert!(ran.out.contains("The last paragraph."));
    assert_eq!(asked(&ran), [("GET", "/api/inbox/12")]);
    // A PowerShell window's line names its own `cf`.
    let ran = cf_as(
        "& 'C:/Program Files/ConsensFlow/bin/cf.exe'",
        &["inbox", "read", "m-12"],
        false,
        vec![
            reply(200, a_role_note("delivered", &body)),
            reply(200, json!({})),
        ],
    );
    assert!(
        ran.out.ends_with("Print part 2 with: & 'C:/Program Files/ConsensFlow/bin/cf.exe' inbox read m-12 --part 2)\n"),
        "{}",
        &ran.out[ran.out.len() - 200..]
    );
}

/// A role back in the queue (its paste given back after the window had it) is said read
/// by each part as the part it is, by the role's own bound and not the one every harness
/// shows whole: a part past 10,000 bytes and within the role's bound is said read as any
/// part of a role delivered is, and never as the whole role (T-1513, review T-1521).
#[test]
fn a_part_of_a_role_back_in_the_queue_is_said_as_a_part_and_never_as_the_whole_role() {
    let body = a_role_over_the_bound();
    for part in ["1", "2"] {
        let ran = cf(
            &["inbox", "read", "m-12", "--part", part],
            vec![
                reply(200, a_role_note("queued", &body)),
                reply(200, json!({})),
            ],
        );
        assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
        assert!(cf_base::role_read::role_read_whole(&ran.out), "part {part}");
        if part == "1" {
            assert!(
                ran.out.len() > 10_000,
                "part 1 is past the bound every harness shows: {} bytes",
                ran.out.len()
            );
        }
        let number: usize = part.parse().unwrap();
        assert_eq!(
            asked(&ran),
            [("GET", "/api/inbox/12"), ("POST", "/api/answers/read")],
            "part {part}"
        );
        assert_eq!(
            ran.asked[1].2,
            Some(json!({ "answers": [12], "via": "inbox", "part": number, "parts": 2 })),
            "part {part}"
        );
    }
}

/// A role within the bound is printed as it always was, by `--part 1` too: its line and
/// its words, no line of a part, and said read as a whole. It has no part 2.
#[test]
fn a_role_within_the_bound_is_printed_as_it_always_was_with_no_part_line() {
    let body = format!("{}\n", "x".repeat(40)).repeat(400);
    for command in [
        vec!["inbox", "read", "m-12"],
        vec!["inbox", "read", "m-12", "--part", "1"],
    ] {
        let ran = cf(
            &command,
            vec![
                reply(200, a_role_note("delivered", &body)),
                reply(200, json!({})),
            ],
        );
        assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
        assert_eq!(
            ran.out,
            format!("m-12 [delivered] note from ConsensFlow\n\n{body}\n")
        );
        assert_eq!(ran.asked[1].2, acknowledged(&[12], "inbox"));
    }
    let ran = cf(
        &["inbox", "read", "m-12", "--part", "2"],
        vec![reply(200, a_role_note("delivered", &body))],
    );
    assert_eq!(ran.code.unwrap(), 2);
    assert!(ran.err.contains("--part"), "{}", ran.err);
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

/// A note of ConsensFlow's own shown, not typed, whose line is over (its task moved on: another
/// worker took the task it said was taken back and waits) reads so wherever `cf` prints its line:
/// `[not typed · over]`, where one that still holds reads `[not typed]`. The list has the
/// summary's `over`; `cf inbox read` and `cf task get` have the receipt's (RS-25, T-1556).
#[test]
fn a_shown_note_whose_line_is_over_reads_so_in_the_inbox_its_read_and_its_tasks_thread() {
    let taken_back =
        "T-3 was taken back from @zeus (ran out of quota) and waits for another standard worker.";
    let list = json!({ "messages": [
        { "id": 21, "kind": "note", "state": "read", "task": 3, "sender": null,
          "preview": "T-3 waits with @diana: out of quota until noon", "shown": true },
        { "id": 20, "kind": "note", "state": "read", "task": 3, "sender": null,
          "preview": taken_back, "shown": true, "over": true },
    ] });
    let ran = cf(&["inbox"], vec![reply(200, list)]);
    assert_eq!(ran.code.unwrap(), 0);
    assert_eq!(
        ran.out.lines().collect::<Vec<_>>(),
        [
            "m-21 [not typed] note T-3 from ConsensFlow: T-3 waits with @diana: out of quota until noon",
            &format!("m-20 [not typed · over] note T-3 from ConsensFlow: {taken_back}"),
        ]
    );
    let over = json!({ "id": 20, "kind": "note", "state": "read", "taskNumber": 3, "sender": null,
        "body": taken_back,
        "receipt": { "shown": "was taken back from @zeus", "over": "2026-11-13T08:00:00.000Z" } });
    let ran = cf(
        &["inbox", "read", "m-20"],
        vec![reply(200, json!({ "message": over.clone() }))],
    );
    assert_eq!(ran.code.unwrap(), 0);
    assert!(
        ran.out
            .starts_with("m-20 [not typed · over] note T-3 from ConsensFlow\n"),
        "{}",
        ran.out
    );
    let mut task = thread();
    task["task"]["messages"][3] = over;
    let ran = cf(
        &["task", "get", "T-3"],
        vec![reply(200, task), reply(200, json!({}))],
    );
    assert_eq!(ran.code.unwrap(), 0);
    assert!(
        ran.out.contains(&format!(
            "m-20 [not typed · over] note T-3 from ConsensFlow\n{taken_back}"
        )),
        "{}",
        ran.out
    );
}

/// `cf words…` run in the window whose `cf` is `word`, the board answering with `replies`.
fn cf_as(word: &str, words: &[&str], json: bool, replies: Vec<Reply>) -> Ran {
    let api = scripted(replies);
    let board = Board::new(Some(&api.url), "tok")
        .with_cf(cf_proto::cf_word::CfWord::of_word(word).unwrap());
    let words: Vec<OsString> = words.iter().map(OsString::from).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(&words, json, &board, &mut io::empty(), &mut out, &mut err)
        .map_err(|failure| failure.kind());
    Ran {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
        asked: api
            .received()
            .into_iter()
            .map(|received| {
                let body = received.json();
                (received.method, received.path, body)
            })
            .collect(),
        flushed_at: None,
    }
}

const WORD: &str = "& 'C:/Program Files/ConsensFlow/bin/cf.exe'";

/// A usage names its commands (`cf task add …`): in the window of a `cf` by a path, they start
/// with it, in the sentence and in the JSON (T-1049: a shell can find another `cf` first).
#[test]
fn a_usage_starts_the_commands_it_names_with_the_cf_of_the_window() {
    for json in [false, true] {
        let ran = cf_as(WORD, &["help"], json, vec![]);
        assert_eq!(ran.code.unwrap(), 0);
        assert!(
            ran.out.contains(&format!("{WORD} task add --tier")),
            "{}",
            ran.out
        );
        let left = ran.out.replace(WORD, "");
        for command in [
            "task", "ask", "inbox", "answer", "note", "tell", "staff", "history",
        ] {
            assert!(
                !left.contains(&format!("cf {command}")),
                "cf {command} in {}",
                ran.out
            );
        }
        let task = cf_as(WORD, &["task", "--help"], json, vec![]);
        assert!(task.out.contains(&format!("{WORD} task ")), "{}", task.out);
    }
    // In a window that was told no word, it is as it always was.
    let plain = cf(&["help"], vec![]);
    assert!(plain.out.contains("cf task add --tier"));
    // A command written wrong says its usage the same way, on stderr.
    let wrong = cf_as(WORD, &["task", "done", "T-3"], false, vec![]);
    assert_eq!(wrong.code.unwrap(), 2);
    assert!(
        wrong.err.contains(&format!("{WORD} task done T-3")),
        "{}",
        wrong.err
    );
}

/// What the board refuses in its own words (the ledger's and the daemon's, which Node's traces
/// hold with the word `cf`) is printed with the window's own.
#[test]
fn a_refusal_starts_the_commands_it_names_with_the_cf_of_the_window() {
    let ran = cf_as(
        WORD,
        &["task", "cancel", "T-3"],
        false,
        vec![reply(
            409,
            json!({ "error": "refused", "message": "T-3 failed, so nobody waits: reopen it (cf task reopen T-3 \"…\")" }),
        )],
    );
    assert_eq!(ran.code.unwrap(), 1);
    assert_eq!(
        ran.err,
        format!("cf: T-3 failed, so nobody waits: reopen it ({WORD} task reopen T-3 \"…\")\n")
    );
}

/// A note ConsensFlow wrote (no sender) is printed with the window's `cf`, as it was pasted; what
/// a participant wrote is as it was written.
#[test]
fn the_notes_of_consensflow_a_thread_and_the_inbox_print_start_their_commands_with_the_cf_of_the_window(
) {
    let mut task = thread();
    task["task"]["messages"][3]["sender"] = Value::Null;
    task["task"]["messages"][3]["body"] = json!("Resume it with: cf task resume T-3 \"…\"");
    task["task"]["messages"][1]["body"] = json!("Run cf task get T-3 yourself");
    let ran = cf_as(
        WORD,
        &["task", "get", "T-3"],
        false,
        vec![reply(200, task), reply(200, json!({}))],
    );
    assert_eq!(ran.code.unwrap(), 0);
    assert!(
        ran.out
            .contains(&format!("Resume it with: {WORD} task resume T-3")),
        "{}",
        ran.out
    );
    assert!(
        ran.out.contains("\nRun cf task get T-3 yourself"),
        "{}",
        ran.out
    );
    let mut note = message("note", "read");
    note["message"]["sender"] = Value::Null;
    note["message"]["body"] = json!("Answer it again: cf answer m-9 \"…\"");
    let read = cf_as(
        WORD,
        &["inbox", "read", "m-12"],
        false,
        vec![reply(200, note)],
    );
    assert!(
        read.out
            .ends_with(&format!("Answer it again: {WORD} answer m-9 \"…\"\n")),
        "{}",
        read.out
    );
}

/// `cf history` says the `cf` of its window to the board, so that the page it asks for names its
/// commands with it and is measured so; a window with no word says none.
#[test]
fn cf_history_says_the_cf_of_its_window_to_the_board() {
    let page = json!({ "page": 1, "pages": 1, "text": "The chief's history." });
    let ran = cf_as(
        "/x/bin/cf",
        &["history", "--page", "2"],
        false,
        vec![reply(200, page.clone())],
    );
    assert_eq!(ran.code.unwrap(), 0);
    assert_eq!(
        asked(&ran),
        [("GET", "/api/history?page=2&cf=%2Fx%2Fbin%2Fcf")]
    );
    let plain = cf(&["history"], vec![reply(200, page)]);
    assert_eq!(asked(&plain), [("GET", "/api/history")]);
}

/// Task 45 as the board answers a `task add` with: on the board for a light
/// worker, waiting on `blocked`.
fn added_task(blocked: &[u64]) -> Value {
    json!({ "task": {
        "number": 45, "state": "open", "assignee": null, "requester": "chief",
        "title": "Fix it", "blockedBy": blocked, "pool": "worker", "tier": "light",
        "deletedAt": null,
    } })
}

/// The body `cf task add` posted, when it posted.
fn posted_needs(ran: &Ran, key: &str) -> Value {
    assert_eq!(asked(ran), [("POST", "/api/tasks")]);
    ran.asked[0].2.as_ref().unwrap()[key].clone()
}

/// The field bug the owner found (2026-11-02): `--needs` given again dropped what
/// the flags before it named, and T-45 went on the board waiting on T-44 alone.
/// Every flag of the command adds to the list, as every comma list's parts do.
#[test]
fn every_needs_flag_of_a_task_add_adds_to_the_list() {
    let ran = cf(
        &[
            "task", "add", "--tier", "light", "--needs", "T-41", "--needs", "T-42", "--needs",
            "T-43", "--needs", "T-44", "Fix it",
        ],
        vec![reply(200, added_task(&[41, 42, 43, 44]))],
    );
    assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
    assert_eq!(posted_needs(&ran, "needs"), json!([41, 42, 43, 44]));
}

/// A list flag and a comma list mix in the order the command gave them, a task
/// named twice kept once.
#[test]
fn needs_flags_and_comma_lists_mix_in_the_order_given_each_task_once() {
    let ran = cf(
        &[
            "task",
            "add",
            "--tier",
            "light",
            "--needs",
            "T-41,T-42",
            "--needs",
            "T-43",
            "--needs",
            "T-42,T-44",
            "Fix it",
        ],
        vec![reply(200, added_task(&[41, 42, 43, 44]))],
    );
    assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
    assert_eq!(posted_needs(&ran, "needs"), json!([41, 42, 43, 44]));
}

/// `--before`, the board's other list of tasks, takes repeats the same way.
#[test]
fn a_before_flag_given_again_holds_every_task_that_waits() {
    let ran = cf(
        &[
            "task",
            "add",
            "--tier",
            "light",
            "--before",
            "T-9",
            "--before",
            "T-10,T-11",
            "Fix it",
        ],
        vec![reply(200, added_task(&[]))],
    );
    assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
    assert_eq!(posted_needs(&ran, "before"), json!([9, 10, 11]));
}

/// A flag that takes one value, given twice, is refused with a plain message
/// before the board is asked anything; the last one's word is never silently
/// taken.
#[test]
fn a_flag_of_one_value_given_twice_is_refused() {
    for (words, flag) in [
        (
            &[
                "task", "add", "--tier", "light", "--tier", "standard", "Fix it",
            ][..],
            "--tier",
        ),
        (
            &["task", "add", "--after", "T-3", "--after", "T-4", "Fix it"][..],
            "--after",
        ),
        (
            &[
                "task",
                "add",
                "--tier",
                "light",
                "--purpose",
                "a",
                "--purpose",
                "b",
                "Fix it",
            ][..],
            "--purpose",
        ),
        (
            &[
                "task", "add", "--tier", "light", "--file", "a.md", "--file", "b.md",
            ][..],
            "--file",
        ),
        (
            &[
                "task",
                "get",
                "T-3",
                "--transcript",
                "--last",
                "2",
                "--last",
                "3",
            ][..],
            "--last",
        ),
        (
            &[
                "task",
                "get",
                "T-3",
                "--transcript",
                "--page",
                "1",
                "--page",
                "2",
            ][..],
            "--page",
        ),
        (
            &[
                "task",
                "get",
                "T-3",
                "--transcript",
                "--find",
                "a",
                "--find",
                "b",
            ][..],
            "--find",
        ),
        (&["history", "--page", "1", "--page", "2"][..], "--page"),
        (&["history", "--find", "a", "--find", "b"][..], "--find"),
        (
            &["tell", "T-3", "--file", "a.md", "--file", "b.md"][..],
            "--file",
        ),
        (
            &["task", "reopen", "T-3", "--file", "a.md", "--file", "b.md"][..],
            "--file",
        ),
        (
            &["task", "resume", "T-3", "--file", "a.md", "--file", "b.md"][..],
            "--file",
        ),
        (
            &["task", "edit", "T-3", "--file", "a.md", "--file", "b.md"][..],
            "--file",
        ),
    ] {
        let ran = cf(words, vec![reply(200, json!({}))]);
        assert_eq!(ran.code.unwrap(), 2, "{words:?}");
        assert_eq!(
            ran.err,
            format!("cf: {flag} takes one value: it was given more than once\n"),
            "{words:?}"
        );
        assert!(ran.out.is_empty(), "{words:?}: {}", ran.out);
        assert!(ran.asked.is_empty(), "{words:?}: {:?}", ran.asked);
    }
}

/// A design task has no tier, so a member of the image designer's says none of it: its line
/// is its name or what it runs on, its role and its harness (designer P2, T-1280). The
/// board says a designer's tier as none (null), and a member with a tier says it.
#[test]
fn staff_says_when_the_chief_works_with_its_own_hands() {
    let ran = cf(
        &["staff"],
        vec![reply(200, json!({"ownHands": true, "members": []}))],
    );
    assert_eq!(ran.code.unwrap(), 0);
    assert!(ran.out.starts_with("The chief works with its own hands; work goes on the board only on the person's word.\n"), "{}", ran.out);
}

#[test]
fn the_staff_says_a_tier_for_a_member_that_has_one_and_none_for_an_image_designer() {
    let members = json!({ "members": [
        { "handle": "claude-opus-5-5-high", "role": "worker", "roles": ["worker"], "tier": "standard",
          "harnessName": "Claude Code", "nameApart": false, "modelName": "claude-opus-5-5", "effort": "high" },
        { "handle": "codex-default", "role": "designer", "roles": ["designer"], "tier": null,
          "harnessName": "Codex", "nameApart": true, "modelName": "Codex default", "effort": null },
    ] });
    let ran = cf(&["staff"], vec![reply(200, members)]);
    assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
    assert_eq!(
        ran.out,
        "@claude-opus-5-5-high · worker · standard · Claude Code\n@codex-default · Codex default · image designer · Codex\n"
    );
}

/// Roles are said by their names, so the frontend designer and the image designer are never
/// one word (spec new-roles, FD-04), and a member of a role with tiers beside one without
/// says its tier.
#[test]
fn the_staff_says_each_role_by_its_name() {
    let members = json!({ "members": [
        { "handle": "claude-opus-5-5-high", "role": "frontend", "roles": ["frontend"], "tier": null,
          "harnessName": "Claude Code", "nameApart": false, "modelName": "claude-opus-5-5", "effort": "high" },
        { "handle": "claude-sonnet-5-5-medium", "role": "worker", "roles": ["worker", "integrator"], "tier": "standard",
          "harnessName": "Claude Code", "nameApart": false, "modelName": "claude-sonnet-5-5", "effort": "medium" },
        { "handle": "gpt-6-astra-high", "role": "architect", "roles": ["architect", "researcher"], "tier": null,
          "harnessName": "Codex", "nameApart": false, "modelName": "gpt-6-astra", "effort": "high" },
        { "handle": "codex-default", "role": "designer", "roles": ["designer"], "tier": null,
          "harnessName": "Codex", "nameApart": true, "modelName": "Codex default", "effort": null },
    ] });
    let ran = cf(&["staff"], vec![reply(200, members)]);
    assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
    assert_eq!(
        ran.out,
        concat!(
            "@claude-opus-5-5-high · frontend designer · Claude Code\n",
            "@claude-sonnet-5-5-medium · worker+integrator · standard · Claude Code\n",
            "@gpt-6-astra-high · architect+researcher · Codex\n",
            "@codex-default · Codex default · image designer · Codex\n",
        )
    );
}

#[test]
fn stc_several_decisions_are_one_request_with_the_words_preserved() {
    for action in ["accept", "cancel"] {
        let make = |number, state| json!({"task": {"number":number,"title":"Work","state":state,"requester":"chief","assignee":null,"pool":"worker","tier":"standard","blockedBy":[]}});
        let state = if action == "accept" {
            "accepted"
        } else {
            "cancelled"
        };
        let ran = cf(
            &["task", action, "T-3", "T-4", "checked both"],
            vec![reply(200, json!({"tasks":[make(3,state), make(4,state)]}))],
        );
        assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
        assert_eq!(asked(&ran), [("POST", "/api/tasks")]);
        assert_eq!(
            ran.asked[0].2,
            Some(json!({"action":action,"tasks":[3,4],"body":"checked both"}))
        );
        assert!(
            ran.out.contains("T-3") && ran.out.contains("T-4"),
            "{}",
            ran.out
        );
    }
}

#[test]
fn stc_a_malformed_task_in_a_decision_is_refused_before_any_request() {
    for action in ["accept", "cancel"] {
        for words in [
            vec!["task", action, "T-3", "T-no"],
            vec!["task", action, "T-3", "T-4", "T-no"],
        ] {
            let ran = cf(&words, vec![reply(200, json!({}))]);
            assert_eq!(ran.code.unwrap(), 2, "{}", ran.err);
            assert!(ran.err.contains("T-no"), "{}", ran.err);
            assert!(ran.asked.is_empty(), "{:?}", ran.asked);
        }
    }
}

#[test]
fn bkv05_staff_says_the_backup_and_each_vacation() {
    let members = json!({ "members": [
        { "handle": "zeus", "role": "worker", "roles": ["worker"], "tier": "standard",
          "harnessName": "Claude Code", "nameApart": false, "modelName": "claude-sonnet-5", "effort": null,
          "vacationSince": "2026-11-13T10:00:00Z" },
        { "handle": "hera", "role": "worker", "roles": ["worker"], "tier": "standard",
          "harnessName": "Codex", "nameApart": true, "modelName": "gpt-6-astra", "effort": null,
          "backupOf": "zeus", "vacationSince": "2026-11-13T10:00:00Z" },
    ] });
    let ran = cf(&["staff"], vec![reply(200, members)]);
    assert_eq!(ran.code.unwrap(), 0, "{}", ran.err);
    assert_eq!(ran.out, "@zeus · worker · standard · Claude Code · on Vacation\n  @hera · backup of @zeus · Codex · gpt-6-astra · on Vacation\n");
}
