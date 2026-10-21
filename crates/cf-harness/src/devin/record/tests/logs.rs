//! What the launches' wire logs say, where the goldens' scenarios never
//! reach: evidence that conflicts, pieces of no text, a log gone, logs written
//! at once, launches that are no folders, and lines of other sessions.

use std::fs::{self, File, FileTimes};
use std::time::{Duration, UNIX_EPOCH};

use super::*;

#[test]
fn conflicting_completion_evidence_fails_the_look_until_the_logs_agree() {
    let staged = Staged::new();
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        reply("a-1", json!("Done.")),
    ]);
    staged.wire(
        "launch-1",
        &[
            chunk(Some(json!("Done.")), "stream-1"),
            complete(),
            chunk(Some(json!("Other.")), "stream-2"),
            complete(),
        ],
    );
    let mut reader = staged.reader();
    assert_eq!(
        show(&look(&mut reader)),
        "unknown: unreadable: conflicting Devin completion evidence"
    );
    staged.wire(
        "launch-1",
        &[chunk(Some(json!("Done.")), "stream-1"), complete()],
    );
    assert_eq!(
        show(&look(&mut reader)),
        format!(
            r#"{{"items":{},"inFlight":false,"asking":false,"failed":false,"settlement":"settled"}}"#,
            asked_and_replied("Done.", true)
        )
    );
}

#[test]
fn a_streamed_piece_with_no_text_streams_undefined_as_node_did() {
    let staged = Staged::new();
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        reply("a-1", json!("undefined")),
    ]);
    staged.wire("launch-1", &[chunk(None, "stream-1"), complete()]);
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":{},"inFlight":false,"asking":false,"failed":false,"settlement":"settled"}}"#,
            asked_and_replied("undefined", true)
        )
    );
}

#[test]
fn a_log_gone_from_its_launch_takes_what_it_said_with_it_once_the_store_changes() {
    let staged = Staged::new();
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        reply("a-1", json!("Done.")),
    ]);
    staged.wire(
        "launch-1",
        &[chunk(Some(json!("Done.")), "stream-1"), complete()],
    );
    let mut reader = staged.reader();
    let first = look(&mut reader);
    assert_eq!(
        show(&first),
        format!(
            r#"{{"items":{},"inFlight":false,"asking":false,"failed":false,"settlement":"settled"}}"#,
            asked_and_replied("Done.", true)
        )
    );
    fs::remove_file(staged.launch("launch-1").join("wire.jsonl")).unwrap();
    // No log read anything new, nor did the store: Node answered the same.
    assert!(Arc::ptr_eq(&first, &look(&mut reader)));
    staged.node(
        Some("n-3"),
        Some("n-2"),
        &json!({ "message_id": "c-1", "role": "system", "content": "ctx" }),
    );
    staged.head("n-3");
    assert_eq!(
        show(&look(&mut reader)),
        format!(
            r#"{{"items":[["u-1","user","Review T-1",true,"{AT}"],["a-1","assistant","Done.",false,"{AT}"],["c-1","custom","ctx",true,"{AT}"]],"inFlight":true,"asking":false,"failed":false,"settlement":"unknown"}}"#
        )
    );
}

#[test]
fn a_log_read_a_moment_after_its_last_write_is_read_to_tell_once_and_then_by_its_time() {
    // A look each second reads a log a moment after Devin wrote it: the look after one
    // that read its bytes to tell takes its time, and answers as the one before.
    // Written over at its size and time then, it is the log read (as git takes a clean
    // entry).
    let staged = Staged::new();
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        reply("a-1", json!("Done.")),
    ]);
    let written = 1_790_000_000_000;
    let wire_at = |said: &str| {
        staged.wire(
            "launch-1",
            &[chunk(Some(json!(said)), "stream-1"), complete()],
        );
        File::options()
            .write(true)
            .open(staged.launch("launch-1").join("wire.jsonl"))
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_millis(written as u64))
            .unwrap();
    };
    wire_at("Done.");
    let mut reader = staged.reader();
    let mut look_at = |at: i64| reader.look(&Options::default(), written + at);
    let first = look_at(500);
    assert!(
        Arc::ptr_eq(&first, &look_at(2_500)),
        "its bytes as they were"
    );
    wire_at("Dune.");
    assert!(
        Arc::ptr_eq(&first, &look_at(3_000)),
        "its time settled: not read"
    );
}

#[test]
fn of_two_logs_written_at_once_the_one_listed_later_says_whether_devin_works() {
    for (busy_first, read) in [
        (
            false,
            format!(
                r#"{{"items":{},"inFlight":true,"asking":false,"failed":false,"settlement":"in-flight"}}"#,
                asked_and_replied("Done.", true)
            ),
        ),
        (
            true,
            format!(
                r#"{{"items":{},"inFlight":false,"asking":false,"failed":false,"settlement":"settled"}}"#,
                asked_and_replied("Done.", true)
            ),
        ),
    ] {
        let staged = Staged::new();
        staged.chain(&[
            user("u-1", Some(json!("request-1"))),
            stopped("a-1", "Done."),
        ]);
        let ended = [chunk(Some(json!("Done.")), "stream-1"), complete()];
        let (first, second) = if busy_first {
            (vec![tool_call()], ended.to_vec())
        } else {
            (ended.to_vec(), vec![tool_call()])
        };
        staged.wire("launch-a", &first);
        staged.wire("launch-b", &second);
        let at = UNIX_EPOCH + Duration::from_millis(1_790_000_000_000);
        for launch in ["launch-a", "launch-b"] {
            File::options()
                .write(true)
                .open(staged.launch(launch).join("wire.jsonl"))
                .unwrap()
                .set_times(FileTimes::new().set_accessed(at).set_modified(at))
                .unwrap();
        }
        assert_eq!(read_once(&staged), read, "busy first: {busy_first}");
    }
}

/// A command a turn left running may go on reporting while it runs (a ping says a line a
/// second), and a shell an Escape left running ends after its turn did: neither is new
/// work, and a window that read them as work read as working for good, its task never
/// done: the symptom of a worker's ping left running, on zeewin, 2026-10-14.
#[test]
fn what_a_tool_goes_on_saying_once_its_turn_has_ended_is_no_new_work_whatever_its_status() {
    for status in [
        None,
        Some("in_progress"),
        Some("completed"),
        Some("failed"),
        Some("cancelled"),
    ] {
        let staged = Staged::new();
        requested_done(&staged);
        staged.wire(
            "launch-1",
            &[
                tool_call(),
                chunk(Some(json!("Done.")), "stream-1"),
                complete(),
                tool_update(status),
                tool_update(status),
            ],
        );
        assert_eq!(read_once(&staged), settled_done(), "its status: {status:?}");
    }
}

#[test]
fn a_call_begun_after_a_turns_end_is_work_again_and_what_it_says_is_no_more_of_it() {
    let staged = Staged::new();
    requested_done(&staged);
    staged.wire(
        "launch-1",
        &[
            chunk(Some(json!("Done.")), "stream-1"),
            complete(),
            tool_call(),
            tool_update(Some("in_progress")),
        ],
    );
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":{},"inFlight":true,"asking":false,"failed":false,"settlement":"in-flight"}}"#,
            asked_and_replied("Done.", true)
        )
    );
}

#[test]
fn a_launches_folder_that_is_a_file_fails_the_look() {
    let staged = Staged::new();
    staged.chain(&[user("u-1", None)]);
    fs::create_dir_all(staged.root.path().join("home/integrations")).unwrap();
    fs::write(staged.root.path().join("home/integrations/devin"), "").unwrap();
    let read = read_once(&staged);
    // Node: `ENOTDIR: not a directory, scandir '…'`, the platform's words.
    assert!(read.starts_with("unknown: unreadable: "), "{read}");
    assert!(read.contains("integrations"), "{read}");
}

#[test]
#[cfg(unix)]
fn a_link_to_a_folder_is_no_launch() {
    let staged = Staged::new();
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        stopped("a-1", "Done."),
    ]);
    let elsewhere = staged.root.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("wire.jsonl"), format!("{}\n", tool_call())).unwrap();
    fs::create_dir_all(staged.root.path().join("home/integrations/devin")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, staged.launch("launch-1")).unwrap();
    // Read as a launch, its log would say Devin is at work.
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":{},"inFlight":false,"asking":false,"failed":false,"settlement":"settled"}}"#,
            asked_and_replied("Done.", true)
        )
    );
}

#[test]
fn a_log_s_lines_of_other_sessions_are_never_parsed() {
    let staged = Staged::new();
    requested_done(&staged);
    let folder = staged.launch("launch-1");
    fs::create_dir_all(&folder).unwrap();
    let lines = format!(
        "{{\"sessionId\":\"quiet-lake\", broken\n{}\n{}\n",
        chunk(Some(json!("Done.")), "stream-1"),
        complete()
    );
    fs::write(folder.join("wire.jsonl"), lines).unwrap();
    assert_eq!(read_once(&staged), settled_done());
}

#[test]
fn a_reply_streamed_for_a_request_of_empty_text_is_no_outcome() {
    let staged = Staged::new();
    staged.chain(&[user("u-1", Some(json!(""))), reply("a-1", json!("Done."))]);
    let mut piece = chunk(Some(json!("Done.")), "stream-1");
    piece["turnClientMessageId"] = json!("");
    let mut ended = complete();
    ended["turnClientMessageId"] = json!("");
    staged.wire("launch-1", &[piece, ended]);
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":{},"inFlight":true,"asking":false,"failed":false,"settlement":"unknown"}}"#,
            asked_and_replied("Done.", false)
        )
    );
}

#[test]
fn a_streamed_piece_that_is_no_text_is_no_part_of_the_reply() {
    let staged = Staged::new();
    requested_done(&staged);
    let mut image = chunk(Some(json!("x")), "stream-1");
    image["update"]["content"]["type"] = json!("image");
    staged.wire(
        "launch-1",
        &[image, chunk(Some(json!("Done.")), "stream-1"), complete()],
    );
    assert_eq!(read_once(&staged), settled_done());
}

#[test]
fn a_second_stream_in_a_turn_starts_its_reply_anew() {
    let staged = Staged::new();
    requested_done(&staged);
    staged.wire(
        "launch-1",
        &[
            chunk(Some(json!("Half")), "stream-1"),
            chunk(Some(json!("Done.")), "stream-2"),
            complete(),
        ],
    );
    assert_eq!(read_once(&staged), settled_done());
}
