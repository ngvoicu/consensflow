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
