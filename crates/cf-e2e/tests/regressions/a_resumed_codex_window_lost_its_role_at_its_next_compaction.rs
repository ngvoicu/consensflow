//! A resumed Codex window lost ConsensFlow's role at its thread's next compaction.
//!
//! Found on 2026-11-10 by a worker's probe (T-1388), on main `c51e5ee3d` (3.0.7-alpha.4)
//! with Codex 0.167.0, not by a real board; the report was the issue
//! `issues/a-resumed-codex-window-loses-its-role-at-its-next-compaction.md` in
//! consensflow-sme. The product's rig ran a real Codex member on the kit's `CODEX_HOME`
//! with `model_auto_compact_token_limit = 1000`: two one-reply tasks, then their `--after`
//! follow-ups, which asked the member its role and the command it asks the chief with. Both
//! resumed windows compacted at their turn's start, both `compacted` records' replacement
//! histories had no "Your ConsensFlow role is worker" developer message, and both answers had
//! no `cf ask`: "No command is specified for asking the chief a question." Every `--after`
//! window, reopened task and resumed chief on Codex was open to it, at Codex's own
//! compaction too.
//!
//! The cause: the window's broker gave the role (`developerInstructions`) to a main
//! `thread/start` alone, since a resumed thread keeps the developer message it began with.
//! It does, until it compacts: Codex rebuilds a compacted history from the instructions of
//! the session that compacts it (`build_compaction_replacement_history`, codex-rs 0.167.0),
//! and a resumed session had none of ConsensFlow's. The stand-in Codex compacts as Codex does
//! (`fake_codex/agent.rs`: `COMPACT <seconds>` at a turn's start, its replacement history
//! rebuilt from the session's instructions), and a line `Say your role` is answered from what
//! the thread holds then.
//!
//! What should have happened: a resumed window keeps its role through a compaction, as a
//! first window does; its resume is given the role, and the role is said to the thread no
//! second time (no note of it, and once in the rebuilt history). The chief resumed with its
//! project is the same.
//!
//! But only a thread Codex says is top-level: the window's resume names its thread by an id
//! the ledger saved, which can be a sub-agent's the user selected, and a sub-agent's thread
//! given the role acts on the board as the member (T-1402, T-1412). The broker asks Codex
//! (`thread/read`) before it lets a resume go (T-1414); the stand-in reads every thread as a
//! sub-agent's with `CF_TEST_CODEX_LINEAGE=sub-agent`.

use std::path::{Path, PathBuf};

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::rollout;
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// What a message that tells a window its role again says, which no other note does.
const REPLACES: &str = "replace the ones you were given then";

/// The brief that compacts the thread at its turn's start and asks the agent its role.
const ASKS_ROLE: &str = "COMPACT 1 Say your role";

/// What the stand-in says of a role it holds: its sentence, which names the member's role.
fn names_role(said: &str, role: &str) -> bool {
    said.contains(&format!("Your ConsensFlow role is {role}."))
}

/// What the stand-in says of the command its role asks the chief with (none says "No command
/// is specified for asking the chief a question").
fn names_ask(said: &str) -> bool {
    said.contains("Ask the chief with: ")
}

/// The results the chief was sent, in order: their bodies.
fn results(project: &Project) -> cf_e2e::Result<Vec<String>> {
    let mut results: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect();
    results.sort_by_key(|message| message["id"].as_i64());
    Ok(results
        .iter()
        .map(|message| message["body"].as_str().unwrap_or_default().to_owned())
        .collect())
}

/// The lines of the stand-in Codex's log.
fn log(rig: &Rig) -> Vec<Value> {
    let log = rig.var("CF_TEST_CODEX_LOG").unwrap_or_default();
    files::read_string(Path::new(log))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The `thread/resume` requests the stand-in's servers were sent, as the broker sent them.
fn resumes(rig: &Rig) -> Vec<Value> {
    log(rig)
        .into_iter()
        .filter(|line| line["run"] == "server" && line["request"].is_object())
        .map(|line| line["request"].clone())
        .collect()
}

/// The records of every rollout in the rig's Codex home.
fn rollouts(rig: &Rig) -> cf_e2e::Result<Vec<Vec<Value>>> {
    let home = PathBuf::from(rig.var("CODEX_HOME").unwrap_or_default());
    rollout::rollouts(&home)
        .iter()
        .map(|found| Ok(rollout::records(&rollout::read(found)?)))
        .collect()
}

/// The developer messages of each `compacted` record's replacement history, in `records`.
fn rebuilt(records: &[Value]) -> Vec<Vec<String>> {
    records
        .iter()
        .filter(|record| record["type"] == "compacted")
        .map(|record| {
            record["payload"]["replacement_history"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|item| item["role"] == "developer")
                .map(|item| {
                    item["content"][0]["text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect()
        })
        .collect()
}

/// What the agent said last in `records`.
fn last_answer(records: &[Value]) -> String {
    records
        .iter()
        .rev()
        .find(|record| {
            record["type"] == "response_item" && record["payload"]["role"] == "assistant"
        })
        .and_then(|record| record["payload"]["content"][0]["text"].as_str())
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn a_codex_members_follow_up_window_keeps_its_role_and_ask_command_through_a_compaction() -> Outcome
{
    let rig = Rig::start(config_with_codex())?;
    rig.stand_ins(&json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "codex", "model": "fake", "workTier": "standard" },
        ],
    }))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or("standard")
        .to_owned();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // T-1: a first window, its thread begun with the role, compacts and is asked its role.
    let ran = rig.cf_in_window(&chief, ["task", "add", "--tier", &tier, ASKS_ROLE])?;
    assert_eq!(ran.code, Some(0), "{ran}");
    rig.wait_for("T-1's result", secs(120), || {
        Ok(!results(&project)?.is_empty())
    })?;
    let first = results(&project)?.remove(0);
    assert!(
        names_role(&first, "worker"),
        "a first window keeps it: {first}"
    );
    assert!(names_ask(&first), "and its ask command: {first}");
    let assignee = project.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    rig.wait_for("the worker's first window to close", secs(60), || {
        Ok(project.lane(&assignee)?["activity"]["state"] == "closed")
    })?;

    // T-2, its follow-up, resumes the thread in a window of its own, which compacts at its
    // turn's start and is asked its role, as the low-limit run of T-1388 did.
    let ran = rig.cf_in_window(&chief, ["task", "accept", "T-1"])?;
    assert_eq!(ran.code, Some(0), "{ran}");
    let ran = rig.cf_in_window(&chief, ["task", "add", "--after", "T-1", ASKS_ROLE])?;
    assert_eq!(ran.code, Some(0), "{ran}");
    rig.wait_for("T-2's result", secs(120), || {
        Ok(results(&project)?.len() >= 2)
    })?;
    assert_eq!(
        project.task(2)?["assignee"],
        assignee.as_str(),
        "the same session"
    );

    let second = results(&project)?.remove(1);
    assert!(
        names_role(&second, "worker"),
        "the resumed window keeps its role through the compaction: {second}"
    );
    assert!(names_ask(&second), "and its ask command: {second}");
    let resumed = resumes(&rig);
    assert_eq!(
        resumed.len(),
        1,
        "T-2's window resumed T-1's thread: {resumed:?}"
    );
    let given = resumed[0]["developerInstructions"]
        .as_str()
        .unwrap_or_default();
    assert!(
        names_role(given, "worker"),
        "the resume was given the window's role: {:?}",
        resumed[0]["developerInstructions"]
    );

    // Said to the thread once: the rebuilt history holds it once, and no note says it again.
    let records: Vec<Value> = rollouts(&rig)?.into_iter().flatten().collect();
    let last = rebuilt(&records).pop().unwrap_or_default();
    assert_eq!(
        last.iter()
            .filter(|said| names_role(said, "worker"))
            .count(),
        1,
        "the role once in the history T-2's compaction rebuilt: {last:?}"
    );
    let notes: Vec<_> = project
        .inbox(&assignee)?
        .into_iter()
        .filter(|message| message["body"].to_string().contains(REPLACES))
        .collect();
    assert!(
        notes.is_empty(),
        "no note of a role that did not change: {notes:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_codex_members_follow_up_window_on_a_thread_codex_calls_a_sub_agents_is_not_given_the_role(
) -> Outcome {
    let rig = Rig::start(config_with_codex().var("CF_TEST_CODEX_LINEAGE", "sub-agent"))?;
    rig.stand_ins(&json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "codex", "model": "fake", "workTier": "standard" },
        ],
    }))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or("standard")
        .to_owned();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let ran = rig.cf_in_window(
        &chief,
        ["task", "add", "--tier", &tier, "Reply with exactly: OK"],
    )?;
    assert_eq!(ran.code, Some(0), "{ran}");
    rig.wait_for("T-1's result", secs(120), || {
        Ok(!results(&project)?.is_empty())
    })?;
    let assignee = project.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    rig.wait_for("the worker's first window to close", secs(60), || {
        Ok(project.lane(&assignee)?["activity"]["state"] == "closed")
    })?;
    let ran = rig.cf_in_window(&chief, ["task", "accept", "T-1"])?;
    assert_eq!(ran.code, Some(0), "{ran}");
    let ran = rig.cf_in_window(&chief, ["task", "add", "--after", "T-1", ASKS_ROLE])?;
    assert_eq!(ran.code, Some(0), "{ran}");
    rig.wait_for("T-2's result", secs(120), || {
        Ok(results(&project)?.len() >= 2)
    })?;
    let resumed = resumes(&rig);
    assert_eq!(
        resumed.len(),
        1,
        "T-2's window resumed the thread: {resumed:?}"
    );
    assert!(
        !names_role(
            resumed[0]["developerInstructions"]
                .as_str()
                .unwrap_or_default(),
            "worker"
        ),
        "a thread Codex calls a sub-agent's is not given the member's role: {:?}",
        resumed[0]["developerInstructions"]
    );
    let second = results(&project)?.remove(1);
    assert!(
        !names_role(&second, "worker"),
        "and its compaction rebuilt no role into it: {second}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_codex_chief_resumed_with_its_project_keeps_its_role_through_a_compaction() -> Outcome {
    let rig = Rig::start(config_with_codex())?;
    let project = Project::open_with_staff(&rig, "maia", None, &[("worker", "worker")])?;
    let id = project.id();
    let chief = format!("p{id}-chief");
    let window = rig.open_frame(&chief, OPEN)?;
    let tier = project.tiers()?.get("worker").cloned().unwrap_or_default();

    // The chief's thread has a turn (the worker's result), which is what lets Codex resume it.
    let ran = rig.cf_in_window(
        &window,
        [
            "task",
            "add",
            "--tier",
            &tier,
            "Reply with exactly: WORKER_OK",
        ],
    )?;
    assert_eq!(ran.code, Some(0), "{ran}");
    rig.wait_for(
        "the worker's result in the chief's thread",
        secs(90),
        || {
            let arrived = rollouts(&rig)?.iter().flatten().any(|record| {
                record["payload"]["role"] == "user" && record.to_string().contains("WORKER_OK")
            });
            let delivered = project
                .inbox("chief")?
                .iter()
                .any(|message| message["kind"] == "result" && message["state"] == "delivered");
            Ok(arrived && delivered)
        },
    )?;
    rig.wait_for("the chief to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })?;

    // The human closes the project and resumes it; the chief's window opens on its thread.
    let closed = rig.page("project.close", json!({ "project": id }))?;
    assert_eq!(closed["ok"], true, "project.close: {closed}");
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "project.resume: {resumed}");
    rig.wait_for("the chief's next window", secs(60), || {
        Ok(rig
            .open_frames()
            .iter()
            .filter(|frame| frame["id"] == chief.as_str())
            .count()
            >= 2)
    })?;

    // Its thread compacts at the start of the human's next message, which asks its role.
    rig.tell(id, ASKS_ROLE)?;
    let mut said = String::new();
    rig.wait_for("the chief's answer", secs(90), || {
        said = rollouts(&rig)?
            .iter()
            .filter(|records| records.iter().any(|record| record["type"] == "compacted"))
            .map(|records| last_answer(records))
            .find(|answer| answer.contains("ConsensFlow role"))
            .unwrap_or_default();
        Ok(!said.is_empty())
    })?;
    assert!(
        names_role(&said, "chief"),
        "the chief keeps its role: {said}"
    );
    let given = resumes(&rig);
    assert!(
        given.iter().any(|request| names_role(
            request["developerInstructions"]
                .as_str()
                .unwrap_or_default(),
            "chief"
        )),
        "the chief's resume was given its role: {given:?}"
    );
    let notes = project
        .inbox("chief")?
        .iter()
        .filter(|message| message["body"].to_string().contains(REPLACES))
        .count();
    assert_eq!(notes, 0, "no note of a role that did not change");
    rig.close()?;
    Ok(())
}
