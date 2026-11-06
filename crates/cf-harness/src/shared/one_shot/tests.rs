use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use serde_json::json;

use super::*;
use crate::testing::{ChildScript, Driver, Ends, ManualTime, ScriptedProcesses};

fn program() -> Program {
    Program {
        executable: PathBuf::from("harness"),
        args: Vec::new(),
        cwd: None,
        env: Env::default(),
    }
}

/// What a conversation of one question, answered by the line whose `id` is 2, came to.
fn asked(
    lines: &[&str],
    ends: Ends,
    processes: &Rc<ScriptedProcesses>,
    time: &Rc<ManualTime>,
) -> Driver<Result<Value, String>> {
    processes.child(
        "harness",
        ChildScript {
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
            ends,
        },
    );
    let mut driver = Driver::default();
    let (processes, clock) = (Rc::clone(processes), Rc::clone(time));
    driver.begin(0, async move {
        conversation(
            &*processes,
            &*clock,
            program(),
            "the harness",
            BOUND_MS,
            async |child| {
                ask(child, "the harness", &json!({ "id": 2 }), |line| {
                    line["id"] == 2
                })
                .await
            },
        )
        .await
    });
    driver
}

#[test]
fn the_answer_is_the_line_taken_for_it_past_what_comes_between_and_the_program_is_ended() {
    let (processes, time) = (
        Rc::new(ScriptedProcesses::default()),
        Rc::new(ManualTime::new(0)),
    );
    let mut driver = asked(
        &[
            "not json",
            r#"{"method":"note"}"#,
            r#"{"id":1}"#,
            r#"{"id":2,"ok":true}"#,
        ],
        Ends::Asked,
        &processes,
        &time,
    );
    assert_eq!(driver.run(), [(0, Ok(json!({ "id": 2, "ok": true })))]);
    assert_eq!(processes.take_written(), [r#"{"id":2}"#]);
}

#[test]
fn a_program_that_ends_before_it_answers_says_so() {
    let (processes, time) = (
        Rc::new(ScriptedProcesses::default()),
        Rc::new(ManualTime::new(0)),
    );
    let mut driver = asked(&[r#"{"id":1}"#], Ends::Itself, &processes, &time);
    assert_eq!(
        driver.run(),
        [(0, Err("the harness ended before it answered".to_owned()))]
    );
}

#[test]
fn a_program_silent_past_the_bound_is_given_up_on_and_ended() {
    let (processes, time) = (
        Rc::new(ScriptedProcesses::default()),
        Rc::new(ManualTime::new(0)),
    );
    let mut driver = asked(&[], Ends::Asked, &processes, &time);
    assert!(driver.run().is_empty(), "it waits for the answer");
    assert!(time.fire_next(i64::MAX), "the bound is armed");
    assert_eq!(time.wall_ms(), BOUND_MS as i64, "given up at the bound");
    assert_eq!(
        driver.run(),
        [(0, Err("the harness did not answer within 30 s".to_owned()))]
    );
}

#[test]
fn a_program_that_does_not_start_says_why() {
    let processes = ScriptedProcesses::default();
    let time = ManualTime::new(0);
    let answer = crate::testing::finished(Box::pin(conversation(
        &processes,
        &time,
        program(),
        "the harness",
        BOUND_MS,
        async |_child| Ok(()),
    )));
    assert_eq!(answer, Err("spawn harness ENOENT".to_owned()));
}

/// What a conversation of one question, started as `streams` says, came to.
fn asked_as(
    streams: Streams,
    ends: Ends,
    processes: &Rc<ScriptedProcesses>,
    time: &Rc<ManualTime>,
) -> Driver<Result<Value, String>> {
    processes.child(
        "harness",
        ChildScript {
            lines: Vec::new(),
            ends,
        },
    );
    let mut driver = Driver::default();
    let (processes, clock) = (Rc::clone(processes), Rc::clone(time));
    driver.begin(0, async move {
        conversation_as(
            &*processes,
            &*clock,
            program(),
            streams,
            "the harness",
            BOUND_MS,
            async |child| {
                ask(child, "the harness", &json!({ "id": 2 }), |line| {
                    line["id"] == 2
                })
                .await
            },
        )
        .await
    });
    driver
}

#[test]
fn a_conversation_that_ended_says_the_code_and_the_last_words_of_the_one_start() {
    for (streams, said) in [
        (
            Streams::Conversation,
            "the harness ended before it answered (exit 2); it said:\nloading\nError: no such \
             thing\nHint: try this",
        ),
        // Any other kind keeps nothing of what its program said, and says as it always did.
        (Streams::Lines, "the harness ended before it answered"),
    ] {
        let (processes, time) = (
            Rc::new(ScriptedProcesses::default()),
            Rc::new(ManualTime::new(0)),
        );
        processes.child_said(
            "harness",
            "loading\n    at frame (file.js:1:2)\nError: no such thing\n\nHint: try this\n",
            Some(2),
        );
        let mut driver = asked_as(streams, Ends::Itself, &processes, &time);
        assert_eq!(driver.run(), [(0, Err(said.to_owned()))], "{streams:?}");
        assert_eq!(processes.take_spawned().len(), 1, "started once");
    }
}

#[test]
fn a_conversation_given_up_on_says_what_the_program_had_said_and_no_code() {
    let (processes, time) = (
        Rc::new(ScriptedProcesses::default()),
        Rc::new(ManualTime::new(0)),
    );
    processes.child_said("harness", "still loading the package\n", None);
    let mut driver = asked_as(Streams::Conversation, Ends::Asked, &processes, &time);
    assert!(driver.run().is_empty());
    assert!(time.fire_next(i64::MAX));
    assert_eq!(
        driver.run(),
        [(
            0,
            Err(
                "the harness did not answer within 30 s; it said:\nstill loading the package"
                    .to_owned()
            )
        )]
    );
}

#[test]
fn what_a_program_said_is_quoted_by_its_last_lines_that_are_no_frame_and_cut_to_a_length() {
    assert_eq!(quoted(""), None);
    assert_eq!(quoted("\n  \n"), None);
    assert_eq!(
        quoted("    at one (a.js:1:1)\n    at two (b.js:2:2)\n"),
        None
    );
    let many = (1..=9)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        quoted(&many).as_deref(),
        Some("line 6\nline 7\nline 8\nline 9")
    );
    let wide = "é".repeat(2_000);
    let cut = quoted(&wide).unwrap();
    assert_eq!(cut.chars().count(), QUOTE_CHARS + 1);
    assert!(cut.ends_with('…'));
}
