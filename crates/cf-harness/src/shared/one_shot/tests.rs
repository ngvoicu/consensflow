use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use serde_json::json;

use super::*;
use crate::testing::{ChildScript, Driver, Ends, ManualTime, ScriptedProcesses};
use cf_proto::models::IT_SAID;

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
fn a_conversation_that_ended_says_the_code_and_what_the_one_start_said_whole() {
    for (streams, said) in [
        (
            Streams::Conversation,
            "the harness ended before it answered (exit 2); it said:\nloading\n    at frame \
             (file.js:1:2)\nError: no such thing\n\nHint: try this",
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

/// The third reading of T-1270 (F2, "cut before filter"): the words of the program are handed on
/// whole, after the mark, and are not cut to their last lines here. Whoever keeps or tells the
/// failure screens the block before it cuts it, and a cut made first takes the line that names a
/// credential and leaves the lines of its value.
#[test]
fn what_a_program_said_is_handed_on_whole_after_the_mark_and_cut_by_no_one_here() {
    let (processes, time) = (
        Rc::new(ScriptedProcesses::default()),
        Rc::new(ManualTime::new(0)),
    );
    let said: Vec<String> = std::iter::once("password: correct".to_owned())
        .chain((1..=9).map(|n| format!("line {n}")))
        .collect();
    processes.child_said("harness", &format!("{}\n", said.join("\n")), Some(1));
    let mut driver = asked_as(Streams::Conversation, Ends::Itself, &processes, &time);
    let failed = driver.run().remove(0).1.unwrap_err();
    let (own, program) = failed.split_once(IT_SAID).expect("the mark");
    assert_eq!(own, "the harness ended before it answered (exit 1)");
    assert_eq!(program, said.join("\n"), "all ten lines, the label first");
    // Nothing the program wrote stands before the mark, where ConsensFlow's own words are.
    assert!(!own.contains("password") && !own.contains("line "), "{own}");
}

#[test]
fn a_program_that_said_nothing_has_no_mark() {
    for said in ["", "\n  \n"] {
        let (processes, time) = (
            Rc::new(ScriptedProcesses::default()),
            Rc::new(ManualTime::new(0)),
        );
        processes.child_said("harness", said, Some(3));
        let mut driver = asked_as(Streams::Conversation, Ends::Itself, &processes, &time);
        assert_eq!(
            driver.run(),
            [(
                0,
                Err("the harness ended before it answered (exit 3)".to_owned())
            )],
            "{said:?}"
        );
    }
}
