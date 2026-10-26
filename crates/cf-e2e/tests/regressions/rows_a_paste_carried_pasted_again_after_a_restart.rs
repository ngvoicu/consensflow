//! What a paste carried was pasted again after a restart.
//!
//! Found on 2026-10-16 by two reviewers (GPT-6-Astra and GPT-6.1-Sol, at max) reading
//! the ledger on `main`, 3.0.5's branch; not seen by the owner, and no issue in
//! consensflow-sme holds it yet. Several messages can ride in one paste: the words that
//! resume a task carry what was kept for its window while it was paused (the answers to
//! its questions, notes about it), each under a line of its own, and the window's
//! record proves them all by the carrier's marker. A daemon that stopped after such a
//! paste reached the window and before it took the receipt left the carrier
//! `delivering` and its rows `queued` under it. Opening the ledger at the next start let
//! go of them (`release_stranded`, written for the rows the Node daemon left under its
//! carriers); the start then confirmed the carrier from its marker in the window's
//! record, and the rows, no longer under it, went into the window again, each on its
//! own. One that never reached the window went without its carrier.
//!
//! What should have happened: what a paste carried follows its carrier. Where the
//! window's record shows the carrier's marker, the rows were received with it; where it
//! does not, they go again in its paste. Each reaches the window once.
//!
//! A chief is given no such paste: a carrier is a task message for the window that has
//! the task, and the chief's own work is never paused. So the cases are a worker's, of
//! each harness the rig stands in for that the daemon pastes into (Claude, and Devin, as
//! the windows of that machine were), in the shape of T-202 (the ledger's carrying tests):
//! the worker asks the chief two questions in its turn, the chief pauses T-1 and answers both
//! while it is paused, which keeps the answers for the window, and resumes it. The case
//! catches the paste of the words that resume it (`Rig::catch_paste`): into the window,
//! or nowhere, its answer never reaches the daemon, which waits for it still when the
//! app quits, daemon first. The app is started again on the same home, and the human
//! resumes the project.
//!
//! Is this B5 of 2026-10-15 ("a result delivered twice, the second copy glued in front of
//! the next result")? A result is never carried, and nor is anything the chief is given,
//! so not as it was reported: what this pastes twice is what a worker's window was kept.
//! A row let go of so, and then folded into the words of a later resume or reopening, does
//! come again glued in front of them.

use cf_e2e::devin_store;
use cf_e2e::files;
use cf_e2e::rig::{Config, Pasted, Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::devin_notes_twice::store_of;
use crate::{config, secs, Outcome, FAKE_DEVIN};

/// What the chief answers the worker's two questions with.
const ANSWERS: [&str; 2] = ["ANSWER_ONE", "ANSWER_TWO"];

/// What the words that resume T-1 begin with, by which their paste is caught.
const RESUMED: &str = "Resumed: Go on";

/// The harness of the worker's window, and where its record is read.
#[derive(Clone, Copy)]
enum Harness {
    Claude,
    Devin,
}

impl Harness {
    /// The rig's configuration for it.
    fn config(self) -> Config {
        match self {
            Self::Claude => config(),
            Self::Devin => config().devin(FAKE_DEVIN),
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::Claude => "claude-code",
            Self::Devin => "devin",
        }
    }

    /// What the worker's window was given, each message of the user's in its record,
    /// oldest first: Claude's conversation `session`, or Devin's store, which only the
    /// worker writes in these cases.
    fn given(self, rig: &Rig, session: &str) -> cf_e2e::Result<Vec<String>> {
        match self {
            Self::Claude => Ok(rig
                .transcript(session)
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .filter(|record| record["type"] == "user")
                .map(|record| words(&record["message"]["content"]))
                .collect()),
            Self::Devin => {
                let store = store_of(rig);
                if !store.exists() {
                    return Ok(Vec::new());
                }
                Ok(devin_store::messages(&store)?
                    .into_iter()
                    .filter(|message| message.role == "user")
                    .map(|message| message.text)
                    .collect())
            }
        }
    }
}

/// The words of a Claude message's content: a string as it is, or its text parts.
fn words(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// The roster: the stand-in Claude is the chief, and the worker is of `harness`.
fn write_roster(rig: &Rig, harness: Harness) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": harness.kind(), "model": "fake", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// What a case is about: the project, the worker's handle and its conversation.
struct Kept {
    project: i64,
    worker: String,
    session: String,
}

/// `cf` in the window `frame`, which must do what it is asked: what it printed.
fn cf(rig: &Rig, frame: &Value, words: &[&str]) -> cf_e2e::Result<String> {
    let ran = rig.cf_in_window(frame, words)?;
    if ran.code == Some(0) {
        Ok(ran.output())
    } else {
        Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))))
    }
}

/// The message id `m-<n>` that what `cf` printed begins with.
fn message_id(printed: &str) -> cf_e2e::Result<String> {
    printed
        .split_whitespace()
        .find(|word| word.starts_with("m-"))
        .map(str::to_owned)
        .ok_or_else(|| Error::Daemon(format!("no message in: {printed}")))
}

/// The worker holds T-1 in a long turn and asks the chief two questions in it, as its
/// agent runs `cf ask`; the chief pauses T-1 and answers both while it is paused, which
/// keeps them for the window. The chief's window, and what the case is about.
fn two_answers_kept_for_a_paused_worker(
    rig: &Rig,
    harness: Harness,
) -> cf_e2e::Result<(Value, Kept)> {
    write_roster(rig, harness)?;
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or("standard")
        .to_owned();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), secs(30))?;
    cf(
        rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            &tier,
            "SLEEP 600 Reply with exactly: WORKED",
        ],
    )?;
    rig.wait_for("the worker to be at work on T-1", secs(60), || {
        Ok(project.task(1)?["state"] == "working"
            && project.lane("worker")?["activity"]["state"] == "working")
    })?;
    let worker = project.lane("worker")?["participant"]["handle"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let window = rig.open_frame(&format!("p{}-{worker}", project.id()), secs(30))?;
    let session = session_of(&window);
    let questions = [
        message_id(&cf(rig, &window, &["ask", "Which format?"])?)?,
        message_id(&cf(rig, &window, &["ask", "Which tests?"])?)?,
    ];

    cf(rig, &chief, &["task", "pause", "T-1"])?;
    rig.wait_for("T-1 to be paused and its window at rest", secs(60), || {
        Ok(project.task(1)?["state"] == "paused"
            && project.lane("worker")?["activity"]["state"] == "idle")
    })?;
    for (question, answer) in questions.iter().zip(ANSWERS) {
        cf(rig, &chief, &["answer", question, answer])?;
    }
    for answer in ANSWERS {
        let kept = answer_to(&project, &worker, answer)?;
        assert_eq!(kept["state"], "queued", "kept for the window: {kept}");
    }
    Ok((
        chief,
        Kept {
            project: project.id(),
            worker,
            session,
        },
    ))
}

/// The conversation a window of the stand-in Claude was opened on (`--session-id`, or
/// `--resume`); none for Devin's, whose store the case reads whole.
fn session_of(frame: &Value) -> String {
    let argv: Vec<&str> = frame["argv"]
        .as_array()
        .map(|argv| argv.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    argv.windows(2)
        .find(|pair| pair[0] == "--session-id" || pair[0] == "--resume")
        .map(|pair| pair[1].to_owned())
        .unwrap_or_default()
}

/// The message to `worker` that says `words`.
fn answer_to(project: &Project, worker: &str, words: &str) -> cf_e2e::Result<Value> {
    project
        .inbox(worker)?
        .into_iter()
        .find(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains(words))
        })
        .ok_or_else(|| Error::Daemon(format!("@{worker} has no message that says {words}")))
}

/// The chief resumes T-1, its words carrying both answers in one paste, which the case
/// catches as `pasted` says. The carrier's id.
fn resumed_with_its_paste_caught(
    rig: &Rig,
    chief: &Value,
    kept: &Kept,
    pasted: Pasted,
) -> cf_e2e::Result<i64> {
    rig.catch_paste(RESUMED, pasted);
    cf(
        rig,
        chief,
        &[
            "task",
            "resume",
            "T-1",
            "Go on\nSLEEP 600 Reply with exactly: RESUMED",
        ],
    )?;
    rig.wait_for("the paste of the words that resume T-1", secs(60), || {
        Ok(rig.paste_caught())
    })?;
    let carrier = answer_to(&Project::new(rig, kept.project), &kept.worker, RESUMED)?;
    carrier["id"]
        .as_i64()
        .ok_or_else(|| Error::Daemon(format!("the words have no id: {carrier}")))
}

/// The app quits, daemon first, before the daemon took the receipt of the paste, and is
/// started again on the same home; the human resumes the project. The rig it runs in.
fn the_app_starts_again(mut rig: Rig, harness: Harness, project: i64) -> cf_e2e::Result<Rig> {
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(harness.config().existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": project }))?;
    if resumed["ok"] != true {
        return Err(Error::Daemon(format!("project.resume: {resumed}")));
    }
    Ok(rig)
}

/// Waits until the carrier and the answers are delivered and the worker's window is open
/// again and its record holds more than the `before` messages it held before the restart
/// (the note that its turn was cut, or the words that resume T-1): what it was given, and
/// the daemon's word that no answer was pasted on its own. The ledger may say all is
/// delivered as the daemon starts, before the window is back.
///
/// The record's silence is no proof that nothing more comes: an answer pasted again goes
/// in as a message of its own, and the daemon says so (`delivery.begun` of its row) when it
/// pastes it, before the window can show it. An answer the ledger holds delivered, with no
/// paste of its own begun, has nothing left to be pasted: it rode in its carrier's.
fn until_all_is_given(
    rig: &Rig,
    harness: Harness,
    kept: &Kept,
    before: usize,
) -> cf_e2e::Result<Vec<String>> {
    let project = Project::new(rig, kept.project);
    rig.wait_for(
        "the worker's window to be open again and given what waits for it",
        secs(180),
        || {
            Ok(project.lane(&kept.worker)?["pane"].is_object()
                && harness.given(rig, &kept.session)?.len() > before)
        },
    )?;
    rig.wait_for(
        "the words and the answers to be delivered",
        secs(180),
        || {
            let inbox = project.inbox(&kept.worker)?;
            let delivered = |words: &str| {
                inbox.iter().any(|message| {
                    message["state"] == "delivered"
                        && message["body"]
                            .as_str()
                            .is_some_and(|body| body.contains(words))
                })
            };
            Ok(delivered(RESUMED) && ANSWERS.iter().all(|answer| delivered(answer)))
        },
    )?;
    let events = rig.events();
    for answer in ANSWERS {
        let id = answer_to(&project, &kept.worker, answer)?["id"].clone();
        let begun = events
            .iter()
            .filter(|event| event["kind"] == "delivery.begun" && event["data"]["message"] == id)
            .count();
        assert_eq!(
            begun, 0,
            "{answer} (m-{id}) was pasted on its own {begun} times, though it rode in its carrier's paste"
        );
    }
    harness.given(rig, &kept.session)
}

/// What the owner should have seen: each answer reached the worker's window once, in
/// the paste of the words that resume T-1 (the message with the carrier's marker).
fn each_answer_went_in_once_with_the_words(given: &[String], carrier: i64) {
    let marker = format!("[ConsensFlow m-{carrier} ");
    let firsts: Vec<&str> = given
        .iter()
        .map(|text| text.lines().next().unwrap_or_default())
        .collect();
    for answer in ANSWERS {
        let times: usize = given.iter().map(|text| text.matches(answer).count()).sum();
        assert_eq!(
            times, 1,
            "{answer} went into the worker's window {times} times; what it was given: {firsts:?}"
        );
        assert!(
            given
                .iter()
                .any(|text| text.starts_with(&marker) && text.contains(answer)),
            "{answer} did not go in with the words that resume T-1 (m-{carrier}); what it was given: {firsts:?}"
        );
    }
}

/// The paste reached the window, whose record shows the carrier, and the daemon stopped
/// before it took the receipt.
fn the_paste_reached_the_window_before_the_restart(harness: Harness) -> Outcome {
    let rig = Rig::start(harness.config())?;
    let (chief, kept) = two_answers_kept_for_a_paused_worker(&rig, harness)?;
    let carrier = resumed_with_its_paste_caught(&rig, &chief, &kept, Pasted::IntoTheWindow)?;
    let marker = format!("[ConsensFlow m-{carrier} ");
    rig.wait_for("the worker's record to show the words", secs(60), || {
        Ok(harness.given(&rig, &kept.session)?.iter().any(|text| {
            text.starts_with(&marker) && ANSWERS.iter().all(|answer| text.contains(answer))
        }))
    })?;
    let before = harness.given(&rig, &kept.session)?.len();
    let rig = the_app_starts_again(rig, harness, kept.project)?;
    let given = until_all_is_given(&rig, harness, &kept, before)?;
    each_answer_went_in_once_with_the_words(&given, carrier);
    rig.close()?;
    Ok(())
}

/// The paste never reached the window, and the daemon stopped waiting for it.
fn the_paste_never_reached_the_window_before_the_restart(harness: Harness) -> Outcome {
    let rig = Rig::start(harness.config())?;
    let (chief, kept) = two_answers_kept_for_a_paused_worker(&rig, harness)?;
    let carrier = resumed_with_its_paste_caught(&rig, &chief, &kept, Pasted::Nowhere)?;
    let before = harness.given(&rig, &kept.session)?.len();
    let rig = the_app_starts_again(rig, harness, kept.project)?;
    let given = until_all_is_given(&rig, harness, &kept, before)?;
    each_answer_went_in_once_with_the_words(&given, carrier);
    rig.close()?;
    Ok(())
}

#[test]
fn answers_a_paste_carried_into_a_claude_workers_window_are_not_pasted_again_after_a_restart(
) -> Outcome {
    the_paste_reached_the_window_before_the_restart(Harness::Claude)
}

#[test]
fn answers_a_paste_carried_into_a_devin_workers_window_are_not_pasted_again_after_a_restart(
) -> Outcome {
    the_paste_reached_the_window_before_the_restart(Harness::Devin)
}

#[test]
fn answers_a_paste_never_carried_into_a_claude_workers_window_go_again_in_it_after_a_restart(
) -> Outcome {
    the_paste_never_reached_the_window_before_the_restart(Harness::Claude)
}

#[test]
fn answers_a_paste_never_carried_into_a_devin_workers_window_go_again_in_it_after_a_restart(
) -> Outcome {
    the_paste_never_reached_the_window_before_the_restart(Harness::Devin)
}
