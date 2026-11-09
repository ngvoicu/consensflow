use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use serde_json::json;

use super::*;
use crate::testing::{ChildScript, Driver, Ends, ManualTime, ScriptedProcesses};
use cf_proto::models::{IT_SAID, WORDS_WITHHELD};

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
        .map_err(String::from)
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
    // The words are Node's; what a note says is the kind and the code (T-1328).
    let said = answer.unwrap_err();
    assert_eq!(said.words, "spawn harness ENOENT");
    assert_eq!(
        said.failure,
        Some(
            cf_base::startup::StartFailure::of(cf_base::startup::StartKind::NotFound)
                .with_errno("ENOENT")
        )
    );
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
        .map_err(String::from)
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

/// What a note may say of a conversation that failed is the kind of failure and the status the
/// program ended with, beside the words for the page (T-1328): none of what the program wrote.
#[test]
fn a_conversation_that_failed_carries_its_kind_and_its_status_beside_its_words() {
    use cf_base::startup::{StartFailure, StartKind};
    let (processes, time) = (
        Rc::new(ScriptedProcesses::default()),
        Rc::new(ManualTime::new(0)),
    );
    processes.child_said("harness", "password: hunter22\n", Some(2));
    processes.child(
        "harness",
        ChildScript {
            lines: Vec::new(),
            ends: Ends::Itself,
        },
    );
    let mut driver: Driver<Result<Value, Said>> = Driver::default();
    let (shared, clock) = (Rc::clone(&processes), Rc::clone(&time));
    driver.begin(0, async move {
        conversation_as(
            &*shared,
            &*clock,
            program(),
            Streams::Conversation,
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
    let (_, ended) = driver.run().remove(0);
    let said = ended.unwrap_err();
    assert!(said.words.contains("hunter22"), "the page has the words");
    let failure = said.failure.expect("a kind");
    assert_eq!(failure, StartFailure::of(StartKind::Failed).with_exit(2));
    assert_eq!(
        failure.words("Pi"),
        "Pi's program ended with an error (exit code 2)"
    );

    // One that waited in vain says so, and no status.
    let (processes, time) = (
        Rc::new(ScriptedProcesses::default()),
        Rc::new(ManualTime::new(0)),
    );
    processes.child_said("harness", "password: hunter22\n", None);
    let mut driver = asked_as(Streams::Conversation, Ends::Asked, &processes, &time);
    assert!(driver.run().is_empty());
    assert!(time.fire_next(i64::MAX));
    let [(_, Err(words))] = &driver.run()[..] else {
        panic!("the conversation was given up on");
    };
    assert!(words.contains("did not answer within 30 s"), "{words}");
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

/// T-1276, `label-before-4KiB`, `long-password-continuation`: cf-process keeps the last 4 KiB of
/// a program's error stream, and where the front was let go, a label cut off it leaves the words
/// of its value in what is kept. None of what the program said is handed on then, and the
/// sentence stands after the mark in its place: ConsensFlow's description and the exit code stay.
#[test]
fn where_the_front_of_what_a_program_said_was_let_go_none_of_it_is_handed_on() {
    for (ends, said, code, failure) in [
        (
            Ends::Itself,
            "horse\nbattery\nstaple\n",
            Some(1),
            "the harness ended before it answered (exit 1)",
        ),
        (
            Ends::Itself,
            "to\nred\nbird\ncedar\nflower\norchard\n",
            Some(2),
            "the harness ended before it answered (exit 2)",
        ),
        (
            Ends::Asked,
            "violet\nwillow\n",
            None,
            "the harness did not answer within 30 s",
        ),
    ] {
        let (processes, time) = (
            Rc::new(ScriptedProcesses::default()),
            Rc::new(ManualTime::new(0)),
        );
        processes.child_said_cut("harness", said, code);
        let mut driver = asked_as(Streams::Conversation, ends, &processes, &time);
        if ends == Ends::Asked {
            assert!(driver.run().is_empty());
            assert!(time.fire_next(i64::MAX));
        }
        assert_eq!(
            driver.run(),
            [(0, Err(format!("{failure}{IT_SAID}{WORDS_WITHHELD}")))],
            "{said:?}"
        );
    }
    // What was cut and held nothing but blanks has no words to withhold.
    let (processes, time) = (
        Rc::new(ScriptedProcesses::default()),
        Rc::new(ManualTime::new(0)),
    );
    processes.child_said_cut("harness", "  \n", Some(3));
    let mut driver = asked_as(Streams::Conversation, Ends::Itself, &processes, &time);
    assert_eq!(
        driver.run(),
        [(
            0,
            Err("the harness ended before it answered (exit 3)".to_owned())
        )]
    );
}
