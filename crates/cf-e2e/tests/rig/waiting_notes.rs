//! What the chief is told of a task that waits, end to end through the real pane
//! host (`npm run test:integration`), as the chief's report of 2026-10-08 had it
//! on a daemon of Node's: fake agents in real PTYs, a worker whose provider
//! refuses it, and a chief in the middle of a turn, so that what ConsensFlow
//! writes it waits behind the turn.
//! - m-1668: "T-357 was taken back from @ullr … waits for another standard
//!   worker" reached the chief after another worker had taken T-357 and
//!   started.
//! - m-1670 and its kin: "T-166 waits with @artemis … until 2026-10-13T08:00Z;
//!   it goes on by itself then" reached the chief after the human had logged
//!   the harness into another account and the daemon had resumed the task; and
//!   the chief it had reached believed the work stopped for five days.
//!
//! The note a task's wait left is withdrawn if the chief has not been given it
//! when the task moves on, and the chief that was given the hold note is told
//! the task goes on.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use cf_e2e::rig::{write_staff, Project, Rig, OPEN};
use cf_e2e::{files, Result};
use regex::Regex;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::{config, secs, session_of, Outcome};

/// A brief the fake worker's provider refuses (`QUOTA-OUT`), and answers when it
/// does not.
const REFUSED: &str = "QUOTA-OUT Reply with exactly: WORKER_OK";

/// The account the fake worker's provider refuses while its file is there
/// (`CF_TEST_QUOTA_FILE`): [`Account::switched`] removes it, the human logging
/// the harness into another account, after which the same brief is not refused.
struct Account {
    folder: TempDir,
}

impl Account {
    fn new() -> Result<Self> {
        let folder = tempfile::Builder::new()
            .prefix("cf-account-")
            .tempdir()
            .map_err(|source| cf_e2e::Error::File {
                action: "make a folder in",
                path: std::env::temp_dir(),
                source,
            })?;
        files::write(&folder.path().join("refused"), "")?;
        Ok(Self { folder })
    }

    fn file(&self) -> PathBuf {
        self.folder.path().join("refused")
    }

    fn switched(&self) {
        let _ = std::fs::remove_file(self.file());
    }
}

/// A rig whose worker's provider refuses it for as long as `account` says.
fn refused_by(account: &Account) -> Result<Rig> {
    Rig::start(
        config()
            .var("CF_TEST_QUOTA_OUT", "worker")
            .var("CF_TEST_QUOTA_FILE", account.file().to_string_lossy()),
    )
}

/// The messages of the chief's inbox that are notes of ConsensFlow's saying
/// `pattern`, oldest first.
fn notes(project: &Project, pattern: &Regex) -> Result<Vec<Value>> {
    let mut notes: Vec<Value> = project
        .inbox("chief")?
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

/// The conversation of the chief's window.
fn chief_session(rig: &Rig, project: &Project) -> Result<String> {
    Ok(session_of(
        &rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?,
    ))
}

/// Waits until the chief's turn ends.
fn until_the_chief_is_idle(rig: &Rig, project: &Project, within: Duration) -> Result<()> {
    rig.wait_for("the chief's turn to end", within, || {
        let board = project.board()?;
        Ok(board["lanes"]
            .as_array()
            .and_then(|lanes| lanes.iter().find(|l| l["participant"]["handle"] == "chief"))
            .is_some_and(|lane| lane["activity"]["state"] == "idle"))
    })
}

/// The tier the worker has on the board.
fn worker_tier(project: &Project) -> Result<String> {
    Ok(project.tiers()?.remove("worker").unwrap_or_default())
}

#[test]
fn the_note_that_a_task_was_taken_back_is_withdrawn_when_another_worker_takes_it_before_the_chief_is_given_the_note(
) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_QUOTA_OUT", "worker"))?;
    write_staff(&rig)?;
    let p = Project::open_with_staff(
        &rig,
        "chief",
        None,
        &[("worker", "worker"), ("worker2", "worker")],
    )?;
    let session = chief_session(&rig, &p)?;
    let taken_back = Regex::new(r"^T-1 was taken back from ")?;
    // The first worker sleeps six seconds on its brief and is then refused; the
    // chief's next turn is far longer than that.
    rig.tell(
        p.id(),
        &format!("DISPATCH --tier {} SLEEP 6 {REFUSED}", worker_tier(&p)?),
    )?;
    rig.tell(p.id(), "SLEEP 40 Reply with exactly: CHIEF_BUSY")?;

    rig.wait_for("the note that T-1 was taken back", secs(60), || {
        Ok(notes(&p, &taken_back)?.len() == 1)
    })?;
    let taken = notes(&p, &taken_back)?[0].clone();
    let body = taken["body"].as_str().unwrap_or_default();
    assert!(
        Regex::new(
            r"^T-1 was taken back from @worker-[a-z]+-[a-z]+ \(ran out of quota after starting\) and waits for another \w+ worker\.$"
        )?
        .is_match(body),
        "{body}"
    );
    // The second worker takes the task while the chief is still in its turn.
    rig.wait_for("the second worker to take T-1", secs(30), || {
        Ok(p.task(1)?["assignee"]
            .as_str()
            .is_some_and(|assignee| assignee.starts_with("worker2-")))
    })?;
    let withdrawn = notes(&p, &taken_back)?[0].clone();
    assert_eq!(
        [
            withdrawn["id"].clone(),
            withdrawn["state"].clone(),
            withdrawn["reason"].clone()
        ],
        [
            taken["id"].clone(),
            json!("cancelled"),
            json!("T-1 was taken by @worker2")
        ]
    );

    // The chief's turn ends and the worker's result reaches it; the note never does.
    rig.wait_for("the worker's result to reach the chief", secs(120), || {
        Ok(p.inbox("chief")?.iter().any(|m| {
            m["kind"] == "result" && m["body"] == "WORKER_OK" && m["state"] == "delivered"
        }))
    })?;
    let transcript = rig.transcript(&session);
    assert!(
        !transcript.contains("was taken back"),
        "the chief was never told the task waits"
    );
    assert!(
        transcript.contains("WORKER_OK"),
        "but it was given the result"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_given_the_note_that_a_task_is_held_is_told_it_goes_on_when_the_account_is_switched(
) -> Outcome {
    let account = Account::new()?;
    let rig = refused_by(&account)?;
    write_staff(&rig)?;
    let p = Project::open_with_staff(&rig, "chief", None, &[("worker", "worker")])?;
    let session = chief_session(&rig, &p)?;
    let waits = Regex::new(r"^T-1 waits with ")?;
    let goes_on = Regex::new(r"^T-1 goes on:")?;
    // The only worker is refused with a reset hours away: its task is held with its window.
    rig.tell(
        p.id(),
        &format!("DISPATCH --tier {} {REFUSED}", worker_tier(&p)?),
    )?;
    rig.wait_for("the note that T-1 is held", secs(60), || {
        Ok(notes(&p, &waits)?.len() == 1)
    })?;
    let hold = notes(&p, &waits)?[0].clone();
    let body = hold["body"].as_str().unwrap_or_default();
    assert!(
        Regex::new(
            r"^T-1 waits with @worker-[a-z]+-[a-z]+: out of quota until \S+, or sooner if its account has quota again; it goes on by itself\.$"
        )?
        .is_match(body),
        "{body}"
    );
    rig.wait_for("the chief to be given the hold note", secs(60), || {
        Ok(notes(&p, &waits)?[0]["state"] == "delivered")
    })?;
    assert_eq!(p.task(1)?["state"], "paused");

    // The human logs the harness into another account and says so.
    account.switched();
    let back = rig.page(
        "member.back",
        json!({ "project": p.id(), "participant": "worker" }),
    )?;
    assert_eq!(back["ok"], true, "{back}");
    rig.wait_for("the note that T-1 goes on", secs(30), || {
        Ok(notes(&p, &goes_on)?.len() == 1)
    })?;
    let goes = notes(&p, &goes_on)?[0].clone();
    assert_eq!(goes["body"], "T-1 goes on: its account has quota again.");
    assert_eq!(goes["taskNumber"], 1);
    until_the_chief_is_idle(&rig, &p, secs(90))?;
    rig.wait_for(
        "the chief to be given the note that T-1 goes on",
        secs(30),
        || Ok(notes(&p, &goes_on)?[0]["state"] == "delivered"),
    )?;
    assert!(rig
        .transcript(&session)
        .contains("T-1 goes on: its account has quota again."));
    let held: Vec<Value> = notes(&p, &waits)?
        .iter()
        .map(|note| note["state"].clone())
        .collect();
    assert_eq!(
        held,
        [json!("delivered")],
        "what the chief was given stays, and the task was held once"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn the_note_that_a_task_is_held_is_withdrawn_when_the_account_is_switched_before_the_chief_is_given_it_and_nothing_is_said_after(
) -> Outcome {
    let account = Account::new()?;
    let rig = refused_by(&account)?;
    write_staff(&rig)?;
    let p = Project::open_with_staff(&rig, "chief", None, &[("worker", "worker")])?;
    let session = chief_session(&rig, &p)?;
    let waits = Regex::new(r"^T-1 waits with ")?;
    let goes_on = Regex::new(r"^T-1 goes on:")?;
    rig.tell(
        p.id(),
        &format!("DISPATCH --tier {} {REFUSED}", worker_tier(&p)?),
    )?;
    rig.tell(p.id(), "SLEEP 40 Reply with exactly: CHIEF_BUSY")?;
    rig.wait_for("the note that T-1 is held", secs(60), || {
        Ok(notes(&p, &waits)?.len() == 1)
    })?;
    let hold = notes(&p, &waits)?[0].clone();
    assert_eq!(hold["state"], "queued", "the chief is in its turn");

    account.switched();
    let back = rig.page(
        "member.back",
        json!({ "project": p.id(), "participant": "worker" }),
    )?;
    assert_eq!(back["ok"], true, "{back}");
    rig.wait_for("T-1 to move on", secs(30), || {
        Ok(p.task(1)?["state"] != "paused")
    })?;
    let withdrawn = notes(&p, &waits)?[0].clone();
    assert_eq!(
        [
            withdrawn["id"].clone(),
            withdrawn["state"].clone(),
            withdrawn["reason"].clone()
        ],
        [hold["id"].clone(), json!("cancelled"), json!("T-1 resumed")]
    );
    assert_eq!(
        notes(&p, &goes_on)?,
        Vec::<Value>::new(),
        "nobody was told it waits"
    );

    until_the_chief_is_idle(&rig, &p, secs(90))?;
    thread::sleep(Duration::from_secs(3));
    let transcript = rig.transcript(&session);
    let inbox: Vec<Value> = p
        .inbox("chief")?
        .iter()
        .map(|m| json!([m["id"], m["kind"], m["state"], m["reason"], m["body"]]))
        .collect();
    assert!(
        !transcript.contains("waits with"),
        "the chief never believed it waited five days: {}",
        Value::from(inbox)
    );
    assert!(!transcript.contains("goes on:"));
    assert_eq!(notes(&p, &waits)?.len(), 1, "the task was held once");
    rig.close()?;
    Ok(())
}
