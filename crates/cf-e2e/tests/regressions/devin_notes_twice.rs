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
//! What the daemon did. A paste is confirmed by the harness's record showing its
//! header, and one the record had not shown a minute on was taken for lost and
//! pasted again: with nothing failing, it is the one way left for a message the
//! ledger holds once to be pasted twice. A Devin chief in a turn reads at rest in
//! the first seconds of it (its message is in the store, and its agent has not said
//! a word, which the wire shows as none of its work), so the daemon pastes into it,
//! and the record of that paste was late for a minute. The paste went again, and the
//! chief had both.
//!
//! The stand-in Devin (`fake-devin`) has what that needs: a turn takes the time a
//! model takes, from the message to the agent's first word (`ttftMs` of the wire's
//! own `stats`) and then as long as the work (`FAKE_DEVIN_FIRST_WORD_MS`,
//! `FAKE_DEVIN_WORK_MS`), and a message it is given in a turn waits and is written to
//! its store when its turn begins, so its record is late. The daemon runs on it
//! whole: the Devin adapter reads its store and its wire as it reads Devin's. What
//! this proves is the daemon's side (a message handed over to a window that went into
//! a turn after it is waited for, and only a paste into a window that stayed at rest
//! and that the record never shows is pasted again). Why a real Devin's record shows
//! a paste late (it holds the message in its turn, writes its row where the store's
//! main chain has not reached, or keeps the paste unsent in its input) no recording
//! shows: `cargo xtask live notes devin` shows it with a real one, and the rule holds
//! for each.
//!
//! The case is the shape of the report: the chief is in a turn (the human has just
//! typed to it) when the cancel comes, and the two notes are those of a cancel: the
//! cancel's own, and the closing of the worker's window with a job still running.
//! The daemon's arrival window is 12 s here (`CONSENSFLOW_ARRIVAL_TIMEOUT_MS`) and a
//! turn 20 s, so that what a minute and a model's turns bring takes a case, not
//! several. The daemon pastes in the first 3 s of a turn (the window reads at rest);
//! on the old rule the paste is taken for lost 12 s after the hand-over, some 7 s
//! before the turn that holds it ends, and on the rule never, for the window is in
//! that turn at the next look.
//!
//! On the old rule the chief read both notes twice (m-2, m-2, m-3, m-3). The first
//! word's time is what makes the second note go in twice: without it (tried, with
//! the rule put back) the cancel's note is read twice all the same, for it goes
//! into the turn the human's prompt has just begun, and the second once, for the
//! window reads busy from the first's header on and the second waits for its rest.
//! With the time, the second is pasted as the first's header shows, into a turn that
//! has said nothing yet and so reads at rest.

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::devin_store;
use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome, FAKE_DEVIN};

/// How long the daemon gives a window at rest to show a message pasted into it.
const ARRIVAL_MS: &str = "12000";

/// How long the stand-in's agent takes to its first word, and how long it works on
/// a turn after it, in milliseconds: a turn of 20 s, which is longer than the
/// arrival window by more than a slow machine's passes take.
const FIRST_WORD_MS: &str = "3000";
const WORK_MS: &str = "17000";

/// How long the chief's store is to hold still before its turns are taken for over:
/// longer than a turn (a message given twice would be another one).
const STILL: Duration = Duration::from_secs(30);

/// The roster of the case: a Devin (the stand-in) is the chief, and the stand-in
/// Claude is the worker.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "devin", "model": "fake" },
            { "id": "worker", "kind": "claude-code", "model": "fake" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// Where the stand-in Devin keeps its store in the rig's home.
fn store_of(rig: &Rig) -> PathBuf {
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

/// Waits until the store has held as many messages for `still` as it holds now
/// (the turns the chief was handed are all taken), for up to three minutes. A look
/// the store does not answer (its writer has it a moment) is no change.
fn until_still(store: &Path, still: Duration) -> cf_e2e::Result<()> {
    let (mut held, mut since) = (0, Instant::now());
    let begun = Instant::now();
    loop {
        let now = devin_store::messages(store).map_or(held, |found| found.len());
        if now != held {
            (held, since) = (now, Instant::now());
        }
        if since.elapsed() >= still {
            return Ok(());
        }
        if begun.elapsed() >= secs(180) {
            return Err(Error::Timeout(format!(
                "the chief's turns went on for three minutes, holding {held} messages"
            )));
        }
        sleep(Duration::from_millis(500));
    }
}

#[test]
fn two_notes_that_come_together_after_a_cancel_go_into_a_devin_chief_s_window_once_each() -> Outcome
{
    let rig = Rig::start(
        config()
            .devin(FAKE_DEVIN)
            .var("CONSENSFLOW_ARRIVAL_TIMEOUT_MS", ARRIVAL_MS)
            .var("FAKE_DEVIN_FIRST_WORD_MS", FIRST_WORD_MS)
            .var("FAKE_DEVIN_WORK_MS", WORK_MS),
    )?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .ok_or("the worker has no tier")?
        .to_owned();

    // The chief gives T-1 to the worker, as `cf` does in its window, and the worker
    // starts a job and ends its turn: it is working still, with a job running.
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), secs(30))?;
    let given = rig.cf_in_window(
        &chief,
        [
            "task",
            "add",
            "--tier",
            tier.as_str(),
            "BACKGROUND 120 started the job",
        ],
    )?;
    assert_eq!(given.code, Some(0), "{}", given.output());
    rig.wait_for("the worker's job to run", secs(60), || {
        Ok(rig.jobs("job").len() == 1)
    })?;
    rig.wait_for("the worker's turn to end", secs(60), || {
        Ok(project.lane("worker")?["activity"]["reason"].is_string())
    })?;

    // The human types to the chief, which begins a turn, and cancels T-1 in its first seconds.
    rig.tell(project.id(), "Reply with exactly: BUSY")?;
    let cancelled = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(cancelled["ok"], true, "{cancelled}");

    // The two notes of the cancel: the cancel's own, and the closing of the window.
    rig.wait_for("both notes to be sent to the chief", secs(60), || {
        Ok(note(&project, "cancelled T-1")?.is_some()
            && note(&project, "window closed with background work")?.is_some())
    })?;
    let id_of = |words: &str| -> Result<i64, Error> {
        note(&project, words)?
            .and_then(|message| message["id"].as_i64())
            .ok_or_else(|| Error::Daemon(format!("no note says {words}")))
    };
    let notes = [
        (id_of("cancelled T-1")?, "the cancel's note"),
        (
            id_of("window closed with background work")?,
            "the closed window's note",
        ),
    ];
    rig.wait_for("the chief to be given both notes", secs(180), || {
        Ok(delivered(&project, notes[0].0)? && delivered(&project, notes[1].0)?)
    })?;
    let store = store_of(&rig);
    until_still(&store, STILL)?;

    // What the chief read: each note once.
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
    // And the daemon never took a paste of either for lost: its window was in a turn.
    let retried: Vec<Value> = rig
        .events()
        .into_iter()
        .filter(|event| {
            event["kind"] == "delivery.retried"
                && notes.iter().any(|(id, _)| event["data"]["message"] == *id)
        })
        .collect();
    assert!(
        retried.is_empty(),
        "the daemon pasted a note again: {retried:?}"
    );
    rig.close()?;
    Ok(())
}
