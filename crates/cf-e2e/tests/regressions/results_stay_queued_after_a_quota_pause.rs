//! Results stayed queued after a quota pause, never typed into the chief.
//!
//! Seen on 2026-10-22 on Gabriel's other Mac, ConsensFlow 3.0.5 (the issue
//! `issues/results-stay-queued-after-a-quota-pause.md` in consensflow-sme): two members on
//! Claude Code ran out of quota mid-task, and ConsensFlow held their tasks until the reset
//! two days on. The note that T-47 waits (m-514) reached the chief; the one that T-46 waits
//! (m-515) ended cancelled. The human switched accounts with `/login`, and both tasks went
//! on and finished, but the note that T-47 goes on (m-518) and both results (m-519, m-520)
//! stayed queued and were never typed into the chief, which was at rest. It learned of
//! them only when the human asked it to look at the board.
//!
//! The cause, forced here: the chief runs on the account its members run on, and ran out
//! with them. Its turn on the note that T-46 waits was refused, so ConsensFlow marked the
//! chief out until the reset and queued the note again (it was cancelled when T-46 went
//! on, which is m-515). Nothing is typed into a window whose participant is out, and only
//! three things bring one back: its reset, a turn of its own window that gets through, or
//! the human's Try again now. A chief at rest takes no turn of its own, so a `/login` that
//! brought the members back, their windows answering again, left the chief out for two
//! days, with everything sent to it queued behind the mark, and the board said nothing of
//! that to the human.
//!
//! What should have been seen: once a window on the chief's harness gets a turn through
//! (the human's new account answering), the chief is back too, as every window of that
//! harness is logged into the one account, and what waits for it is typed in; and while
//! it is out with something waiting for it, the human is told so once on the board (held
//! by the engine's tests, `quota_accounts`).
//!
//! The cases are that shape on the rig: the chief and its one worker share an account
//! (`CF_TEST_QUOTA_SPENT`), spent while the worker is at work. The worker is refused, its
//! task held, and the chief is refused on the note that says so. The human logs into
//! another account and goes on in the worker's window, or presses Try again now on the
//! worker or on the chief; the task finishes, and its result must be typed into the chief.

use std::path::PathBuf;

use cf_e2e::rig::{write_staff, Project, Rig, OPEN};
use cf_e2e::{files, Result};
use regex::Regex;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief};
use crate::{config, secs, Outcome};

/// The account the chief and its worker share: spent while its file is there
/// (`CF_TEST_QUOTA_FILE`).
struct Account {
    folder: TempDir,
}

impl Account {
    /// An account with quota.
    fn new() -> Result<Self> {
        let folder = tempfile::Builder::new()
            .prefix("cf-account-")
            .tempdir()
            .map_err(|source| cf_e2e::Error::File {
                action: "make a folder in",
                path: std::env::temp_dir(),
                source,
            })?;
        Ok(Self { folder })
    }

    fn file(&self) -> PathBuf {
        self.folder.path().join("spent")
    }

    /// Its quota runs out: every window logged into it is refused.
    fn spend(&self) -> Result<()> {
        files::write(&self.file(), "")
    }

    /// The human logs the harness into another account, which has quota.
    fn switch(&self) {
        let _ = std::fs::remove_file(self.file());
    }
}

/// How the human brings the worker back once the account is switched.
#[derive(Clone, Copy)]
enum Back {
    /// Goes on in the worker's window, which answers: its turn gets through.
    InItsWindow,
    /// Presses Try again now on the worker.
    TryAgainNow,
    /// Presses Try again now on the chief.
    TryAgainNowOnChief,
}

#[test]
fn a_chief_out_of_quota_with_its_worker_is_handed_the_result_once_the_workers_window_answers_on_the_new_account(
) -> Outcome {
    handed_the_result_after_the_switch(Back::InItsWindow)
}

#[test]
fn a_chief_out_of_quota_with_its_worker_is_handed_the_result_once_the_human_says_the_worker_is_back(
) -> Outcome {
    handed_the_result_after_the_switch(Back::TryAgainNow)
}

#[test]
fn a_chief_out_of_quota_with_its_worker_is_handed_the_result_once_the_human_says_the_chief_is_back(
) -> Outcome {
    handed_the_result_after_the_switch(Back::TryAgainNowOnChief)
}

/// The chief and its worker run out together; the human switches the account and brings
/// the worker back as `back` says; the worker's result must reach the chief.
fn handed_the_result_after_the_switch(back: Back) -> Outcome {
    let account = Account::new()?;
    let rig = Rig::start(
        config()
            .var("CF_TEST_QUOTA_SPENT", "chief,worker")
            .var("CF_TEST_QUOTA_FILE", account.file().to_string_lossy()),
    )?;
    write_staff(&rig)?;
    let p = Project::open_with_staff(&rig, "chief", None, &[("worker", "worker")])?;
    let chief = rig.open_frame(&format!("p{}-chief", p.id()), OPEN)?;
    let session = after(&chief, "--session-id")
        .ok_or("the chief's window has no session")?
        .to_owned();
    let tier = p.tiers()?.remove("worker").unwrap_or_default();
    rig.tell(
        p.id(),
        &format!("DISPATCH --tier {tier} SLEEP 20 Reply with exactly: WORKER_OK"),
    )?;
    rig.wait_for("T-1 to be at work", secs(60), || {
        Ok(p.task(1)?["state"] == "working")
    })?;
    wait_for_the_chief(&rig, &p)?;

    // The account runs out while the worker works: its next request is refused, and the
    // chief's turn on the note that T-1 waits is refused too.
    account.spend()?;
    rig.wait_for("T-1 to be held for the quota", secs(60), || {
        Ok(p.task(1)?["state"] == "paused")
    })?;
    rig.wait_for("the chief to be out of quota", secs(60), || {
        Ok(p.lane("chief")?["activity"]["state"] == "out")
    })?;
    let waits = Regex::new(r"^T-1 waits with ")?;
    assert_eq!(
        notes_to(&p, "chief", &waits)?.len(),
        1,
        "the chief is sent the note that T-1 waits"
    );

    // The human logs into another account and brings the worker back.
    account.switch();
    match back {
        Back::InItsWindow => {
            let pane = p.lane("worker")?["pane"].clone();
            let typed = rig.host(
                "pane.input",
                json!({ "id": pane["id"], "generation": pane["generation"], "bytes": b"go on\r" }),
            )?;
            assert_eq!(typed["ok"], true, "{typed}");
        }
        Back::TryAgainNow | Back::TryAgainNowOnChief => {
            let pressed = if matches!(back, Back::TryAgainNow) {
                "worker"
            } else {
                "chief"
            };
            let said = rig.page(
                "member.back",
                json!({ "project": p.id(), "participant": pressed }),
            )?;
            assert_eq!(said["ok"], true, "{said}");
        }
    }
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(p.task(1)?["state"] == "done")
    })?;
    rig.wait_for(
        "the worker's result to be typed into the chief",
        secs(60),
        || {
            Ok(result(&p)?.is_some_and(|(id, delivered)| {
                delivered
                    && rig
                        .transcript(&session)
                        .contains(&format!("[ConsensFlow m-{id} "))
            }))
        },
    )?;
    rig.close()?;
    Ok(())
}

/// The notes of ConsensFlow's own to `participant` whose words match `pattern`,
/// oldest first.
fn notes_to(project: &Project, participant: &str, pattern: &Regex) -> Result<Vec<Value>> {
    let mut notes: Vec<Value> = project
        .inbox(participant)?
        .into_iter()
        .filter(|m| {
            m["kind"] == "note"
                && m.get("sender") == Some(&Value::Null)
                && pattern.is_match(m["body"].as_str().unwrap_or_default())
        })
        .collect();
    notes.sort_by_key(|m| m["id"].as_i64().unwrap_or_default());
    Ok(notes)
}

/// The chief's result, once it is given one: its id, and whether it is delivered.
fn result(project: &Project) -> Result<Option<(i64, bool)>> {
    Ok(project
        .inbox("chief")?
        .iter()
        .find(|message| message["kind"] == "result")
        .map(|message| {
            (
                message["id"].as_i64().unwrap_or_default(),
                message["state"] == "delivered",
            )
        }))
}
