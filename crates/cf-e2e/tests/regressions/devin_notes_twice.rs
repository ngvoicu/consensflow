//! A cancelled task of 2026-10-13, ConsensFlow 3.0.3 on the owner's Windows
//! machine, a Devin chief and Devin staff: **each of the two notes the chief was
//! sent went into its window twice.**
//!
//! What the owner saw: the human cancelled T-17. ConsensFlow sent the chief two
//! notes one right after the other, m-186 ("@human cancelled T-17 (…): @…'s
//! window was stopped") and m-187 ("T-17's window closed with background work
//! still running: …"), and the chief reported that each had been pasted into its
//! window twice in a row (m-186, m-186, m-187, m-187: "two consecutive duplicate
//! deliveries"). Each message is in the ledger once. The owner's trace (the
//! project's `events.jsonl`, read from a photo) holds the daemon's own word for it:
//! m-187's first paste begun at 18:30:50, the chief working at 18:30:54, at rest at
//! 18:31:09, working at 18:31:14, at rest at 18:31:26, and at 18:31:49 `delivery.retried`
//! with the reason "the harness record never showed it", the second paste begun 0.1 s
//! later; m-186's second paste (18:30:45) was confirmed 11 ms after it began, by the
//! header of the first.
//!
//! What should have happened: each note goes into the window once, and the chief
//! reads it once, whatever its record takes to show it.
//!
//! What the daemon did, and what Devin does. A paste is confirmed by the harness's
//! record showing its header, and one the record had not shown by the arrival window (a
//! minute) was taken for lost and pasted again: with nothing failing, it is the one way left for a
//! message the ledger holds once to be pasted twice. A Devin chief in a turn reads at
//! rest in the first seconds of it (the wire shows none of its work until its agent's
//! first word), so the daemon pasted into it; and what Devin is handed in a turn goes
//! into its agent's queue and is written to its store when the agent runs it (a live
//! run of the same day on the Mac, Devin 3000.11.3: a note pasted a second after
//! another showed in the record 63 s later, when the 60 s command of the turn before it
//! ended). The record was late, not lacking, and the paste went again.
//!
//! The stand-in Devin (`fake-devin`) has what that needs: a turn takes the time a
//! model takes, from the message to its agent's first word (`ttftMs` of the wire's own
//! `stats`) and then as long as the work (`FAKE_DEVIN_FIRST_WORD_MS`,
//! `FAKE_DEVIN_WORK_MS`), and a message it is handed in a turn is run when the turn
//! ends, its row written then. The prompt of a turn is on its wire as a request of the
//! TUI, as it is run. The daemon runs on it whole: the Devin adapter reads its store
//! and its wire as it reads Devin's.
//!
//! The case is the shape of the report: the human types to the chief and cancels at
//! once, and two notes come together. Since RS-25 (the brain's decision "Nothing is
//! typed to the chief that it need not decide") the second note of the report, a window
//! closed with work still running, asks the chief nothing and is shown, not typed; the
//! cancel's own note is still typed, as a task called off by another is. So the human
//! cancels two tasks, each a worker's, one right after the other: two cancel notes, both
//! typed, sent together into the chief's turn, as the report's two were. The daemon's
//! arrival window is 15 s
//! here, not the minute it gives a person's ([`ARRIVAL_MS`]), so the notes' turns are
//! 28 s: a note whose record waits for the turn before it shows later than the window.
//! Without the fix the second note is pasted into the first's turn, which has said
//! nothing yet, and its record is some 25 s late: the paste goes again, an arrival window
//! on. With it, the window reads busy from the TUI's prompt, and the second note is
//! pasted once the first's turn has ended; and were one pasted into a turn, only the
//! window's rest counts towards the arrival window. What proves
//! it is the daemon's own word for its pastes (`delivery.begun`, `delivery.retried`):
//! the chief reads a second copy only after the turns before it, later than this looks,
//! and its store is read as it stands by the time both notes are in.

use std::path::PathBuf;

use cf_e2e::devin_store;
use cf_e2e::rig::{Config, Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{devin_config, secs, Outcome};

/// How long the daemon waits for the record of a paste to show it in the cases of a
/// Devin that is late with it: the minute it gives a person's, cut to this by the
/// variable it reads (`CONSENSFLOW_ARRIVAL_TIMEOUT_MS`), so that the turns which outlast
/// it are seconds longer and no minute and a half. What a case asserts is that a record
/// later than the window is no paste lost, whatever the window is; the stand-in's turns
/// are sized against it, and the 30 s the daemon reads a Devin window busy from its prompt
/// stays the 30 s it is.
pub(crate) const ARRIVAL_MS: i64 = 15_000;

/// `config`, with the daemon's arrival window [`ARRIVAL_MS`].
pub(crate) fn arrival(config: Config) -> Config {
    config.var("CONSENSFLOW_ARRIVAL_TIMEOUT_MS", ARRIVAL_MS.to_string())
}

/// How long the stand-in's agent works on a turn after its first word, in
/// milliseconds: longer than the arrival window by more than a slow machine's passes
/// take, which a turn that is late with its record has to be.
pub(crate) const WORK_MS: i64 = ARRIVAL_MS + 10_000;

/// [`WORK_MS`] as the stand-in is given it.
pub(crate) fn work_ms() -> String {
    WORK_MS.to_string()
}

/// How long the stand-in's agent takes to its first word, in milliseconds.
const FIRST_WORD_MS: &str = "3000";

/// The roster of the case: a Devin (the stand-in) is the chief, and the stand-in
/// Claude is each of the two workers.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "devin", "model": "fake" },
            { "id": "worker", "kind": "claude-code", "model": "fake" },
            { "id": "worker2", "kind": "claude-code", "model": "fake" },
        ],
    });
    rig.stand_ins(&roster)
}

/// Where the stand-in Devin keeps its store in the rig's home.
pub(crate) fn store_of(rig: &Rig) -> PathBuf {
    let home = rig
        .var("HOME")
        .or_else(|| rig.var("USERPROFILE"))
        .unwrap_or_default();
    devin_store::location(
        cfg!(windows),
        home,
        rig.var("APPDATA"),
        rig.var("XDG_DATA_HOME"),
    )
}

/// When the window's wire log said back the human's message that holds `words` and none of
/// the daemon's headers, in milliseconds since the epoch: the time Devin 3000.12.43 stamps
/// it with (T-1423). None where no log says it, a message the log does not stamp
/// (3000.11's, replayed ones) included.
pub(crate) fn said_back_at(rig: &Rig, words: &str) -> Option<i64> {
    cf_e2e::live::wire::newest(&rig.home())
        .and_then(|log| std::fs::read_to_string(log).ok())
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|line| line["update"]["sessionUpdate"] == "user_message_chunk")
        .filter(|line| {
            line["update"]["content"]["text"]
                .as_str()
                .is_some_and(|text| text.contains(words) && !text.contains("[ConsensFlow m-"))
        })
        .find_map(|line| {
            line["update"]["_meta"]["cognition.ai/timestamp"]
                .as_str()?
                .parse::<jiff::Timestamp>()
                .ok()
                .map(|time| time.as_millisecond())
        })
}

/// The note of the chief's inbox that says `words`, if there is one.
fn note(project: &Project, words: &str) -> cf_e2e::Result<Option<Value>> {
    Ok(project.inbox("chief")?.into_iter().find(|message| {
        message["kind"] == "note"
            && message["body"]
                .as_str()
                .is_some_and(|body| body.contains(words))
    }))
}

/// Whether the message `id` of the chief's inbox was confirmed.
fn delivered(project: &Project, id: i64) -> cf_e2e::Result<bool> {
    Ok(project
        .inbox("chief")?
        .iter()
        .any(|message| message["id"] == id && message["state"] == "delivered"))
}

#[test]
fn two_notes_that_come_together_after_two_cancels_go_into_a_devin_chief_s_window_once_each(
) -> Outcome {
    once_each(None)
}

/// The same with no settle (3.0.7's pacing turned off, `CONSENSFLOW_SETTLE_MS=0`): the
/// pacing's settle alone keeps a window read at rest in the first seconds of a turn from
/// being given a note, so the rule that waits for Devin's record holds here by itself
/// (T-842's P3: the early-rest plant was caught only with the settle taken out too).
#[test]
fn two_notes_that_come_together_after_two_cancels_go_in_once_each_with_no_settle() -> Outcome {
    once_each(Some("0"))
}

/// The case, its daemon's settle `settle` milliseconds where it names one.
fn once_each(settle: Option<&str>) -> Outcome {
    let mut config = arrival(devin_config())
        .var("FAKE_DEVIN_FIRST_WORD_MS", FIRST_WORD_MS)
        .var("FAKE_DEVIN_WORK_MS", work_ms());
    if let Some(settle) = settle {
        config = config.var("CONSENSFLOW_SETTLE_MS", settle);
    }
    let rig = Rig::start(config)?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    project.add_member("worker2")?;
    let tier = added["member"]["tier"]
        .as_str()
        .ok_or("the worker has no tier")?
        .to_owned();

    // The chief gives T-1 and T-2 to the two workers, as `cf` does in its window, each a
    // turn of two minutes: both are working when the human cancels them.
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), secs(30))?;
    for _ in 0..2 {
        let given = rig.cf_in_window(
            &chief,
            [
                "task",
                "add",
                "--tier",
                tier.as_str(),
                "SLEEP 120 Reply with exactly: WORKER_OK",
            ],
        )?;
        assert_eq!(given.code, Some(0), "{}", given.output());
    }
    rig.wait_for("both workers to take their tasks", secs(60), || {
        Ok(project.task(1)?["state"] == "working" && project.task(2)?["state"] == "working")
    })?;

    // The human types to the chief, a turn of a few seconds, and cancels T-1 and T-2 in its
    // first: the notes of the cancels are the chief's, and it is in a turn.
    rig.tell(project.id(), "SLEEP 2 Reply with exactly: BUSY")?;
    for task in [1, 2] {
        let cancelled = rig.page(
            "task.cancel",
            json!({ "project": project.id(), "task": task }),
        )?;
        assert_eq!(cancelled["ok"], true, "{cancelled}");
    }

    // The two notes of the cancels, each typed (a task called off by another).
    rig.wait_for("both notes to be sent to the chief", secs(60), || {
        Ok(note(&project, "cancelled T-1 (")?.is_some()
            && note(&project, "cancelled T-2 (")?.is_some())
    })?;
    let id_of = |words: &str| -> Result<i64, Error> {
        note(&project, words)?
            .and_then(|message| message["id"].as_i64())
            .ok_or_else(|| Error::Daemon(format!("no note says {words}")))
    };
    let notes = [
        (id_of("cancelled T-1 (")?, "the note of T-1's cancel"),
        (id_of("cancelled T-2 (")?, "the note of T-2's cancel"),
    ];
    rig.wait_for("the chief to be given both notes", secs(300), || {
        Ok(delivered(&project, notes[0].0)? && delivered(&project, notes[1].0)?)
    })?;
    let store = store_of(&rig);

    // What the daemon did: it pasted each note once, and never took a paste of either
    // for lost, though a note's turn took longer than its arrival window. Its own word is the
    // proof: the stand-in runs a message it is handed in a turn when that turn ends, so
    // a second copy of a note is read by the chief only after the turns before it,
    // later than this looks.
    let events = rig.events();
    for (id, what) in notes {
        let of_note = |kind: &str| -> Vec<&Value> {
            events
                .iter()
                .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
                .collect()
        };
        let (pasted, retried) = (of_note("delivery.begun"), of_note("delivery.retried"));
        assert!(
            pasted.len() == 1 && retried.is_empty(),
            "{what} (m-{id}) was pasted {} times; the daemon took it for lost: {retried:?}",
            pasted.len()
        );
    }

    // What the chief had read by then: each note once.
    let given = devin_store::messages(&store)?;
    for (id, what) in notes {
        let marker = format!("[ConsensFlow m-{id} ");
        let times: usize = given
            .iter()
            .filter(|message| message.role == "user")
            .map(|message| message.times(&marker))
            .sum();
        assert_eq!(
            times,
            1,
            "{what} (m-{id}) went into the chief's window {times} times; what it was given: {:?}",
            given
                .iter()
                .filter(|message| message.role == "user")
                .map(|message| message.text.lines().next().unwrap_or_default())
                .collect::<Vec<_>>()
        );
    }
    rig.close()?;
    Ok(())
}

// The same on the wire log of Devin 3000.12.43, the installed Devin (T-1423): the case as
// it is, the stand-in's shape chosen here and not by the environment.

#[test]
fn two_notes_that_come_together_after_two_cancels_go_into_a_devin_chief_s_window_once_each_on_devin_3000_12(
) -> Outcome {
    crate::on_the_installed_devin(
        two_notes_that_come_together_after_two_cancels_go_into_a_devin_chief_s_window_once_each,
    )
}

#[test]
fn two_notes_that_come_together_after_two_cancels_go_in_once_each_with_no_settle_on_devin_3000_12(
) -> Outcome {
    crate::on_the_installed_devin(
        two_notes_that_come_together_after_two_cancels_go_in_once_each_with_no_settle,
    )
}
