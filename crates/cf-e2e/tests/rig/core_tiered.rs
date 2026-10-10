//! Tiered dispatch, reviews and quota, end to end through the real pane host
//! (VERIFY-TD-13): fake Claude agents in real PTYs, the daemon picking the
//! member, a review the chief puts on the board going to a reviewer like any
//! task, and a worker whose provider refuses it mid-task losing the task to
//! another.

use std::thread;
use std::time::Duration;

use cf_e2e::process::is_alive;
use cf_e2e::rig::{write_staff, Project, Rig};
use regex::Regex;
use serde_json::{json, Value};

use crate::{config, rig, secs, Outcome};

/// Whether the lane's `pane` is there and null: the window is closed. A lane
/// that is not there, or has no `pane`, is not.
fn pane_is_null(lane: &Value) -> bool {
    lane.get("pane") == Some(&Value::Null)
}

/// The tier the agent `agent` has on the board.
fn tier<'a>(tiers: &'a std::collections::BTreeMap<String, String>, agent: &str) -> &'a str {
    tiers.get(agent).map_or("", String::as_str)
}

/// A project with four agents' staff as the cases below open one, on a rig.
fn project_of<'a>(
    rig: &'a Rig,
    gate: Option<bool>,
    members: &[(&str, &str)],
) -> cf_e2e::Result<Project<'a>> {
    write_staff(rig)?;
    Project::open_with_staff(rig, "chief", gate, members)
}

#[test]
fn a_review_is_a_task_the_chief_puts_on_the_board_a_reviewer_of_its_tier_takes_it_and_its_findings_come_back_as_the_result(
) -> Outcome {
    let rig = rig()?;
    let p = project_of(&rig, None, &[("worker", "worker"), ("checker", "reviewer")])?;
    let tiers = p.tiers()?;
    rig.tell(
        p.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: WORKER_OK",
            tier(&tiers, "worker")
        ),
    )?;
    rig.wait_for("the worker's result to be delivered", secs(60), || {
        Ok(p.inbox("chief")?
            .iter()
            .any(|m| m["kind"] == "result" && m["taskNumber"] == 1 && m["state"] == "delivered"))
    })?;
    assert_eq!(
        p.task(1)?["state"],
        "done",
        "nothing is reviewed on its own"
    );
    let numbers: Vec<Value> = p.board()?["lanes"]
        .as_array()
        .map(|lanes| {
            lanes
                .iter()
                .flat_map(|lane| lane["tasks"].as_array().cloned().unwrap_or_default())
                .map(|task| task["number"].clone())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(numbers, [json!(1)], "no review task appears by itself");

    rig.tell(
        p.id(),
        &format!(
            "DISPATCH --review --tier {} Review T-1. Reply with exactly: FINDINGS_OK",
            tier(&tiers, "checker")
        ),
    )?;
    rig.wait_for("the review to be done", secs(90), || {
        Ok(p.task(2)?["state"] == "done")
    })?;
    let review = p.task(2)?;
    assert_eq!(
        [review["pool"].clone(), review["tier"].clone()],
        [json!("reviewer"), json!(tier(&tiers, "checker"))]
    );
    let assignee = review["assignee"].as_str().unwrap_or_default();
    assert!(assignee.starts_with("checker-"), "{assignee}");
    let findings = review["messages"]
        .as_array()
        .and_then(|messages| messages.iter().find(|m| m["kind"] == "result"))
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        [findings["recipient"].clone(), findings["body"].clone()],
        [json!("chief"), json!("FINDINGS_OK")]
    );
    assert_eq!(p.task(1)?["state"], "done", "the chief decides the work");
    rig.close()?;
    Ok(())
}

#[test]
fn each_task_runs_in_its_own_worker_session_the_window_closes_with_the_task_the_next_opens_a_new_one(
) -> Outcome {
    let rig = rig()?;
    let p = project_of(&rig, None, &[("worker", "worker")])?;
    let tiers = p.tiers()?;
    let sessions = || {
        rig.started()
            .into_iter()
            .map(|entry| entry.session)
            .collect::<std::collections::BTreeSet<_>>()
    };
    for (word, expected) in [("ONE", 2), ("TWO", 3)] {
        rig.tell(
            p.id(),
            &format!(
                "DISPATCH --tier {} Reply with exactly: {word}",
                tier(&tiers, "worker")
            ),
        )?;
        rig.wait_for(&format!("the task {word} to be done"), secs(60), || {
            Ok(p.lane("worker")?["tasks"].as_array().is_some_and(|tasks| {
                tasks.iter().any(|t| {
                    t["state"] == "done" && t["title"].as_str().is_some_and(|t| t.contains(word))
                })
            }))
        })?;
        rig.wait_for("the worker's window to close", secs(30), || {
            Ok(pane_is_null(&p.lane("worker")?))
        })?;
        assert_eq!(p.lane("worker")?["activity"]["state"], "closed");
        assert_eq!(
            sessions().len(),
            expected,
            "one native session per task, plus the chief"
        );
    }
    let started = rig.started();
    let chief = started.first().map(|entry| entry.pid);
    let worker_pids: Vec<u32> = started
        .iter()
        .map(|entry| entry.pid)
        .filter(|pid| Some(*pid) != chief)
        .collect();
    rig.wait_for("the workers' processes to be gone", secs(30), || {
        Ok(worker_pids.iter().all(|pid| !is_alive(*pid)))
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_worker_refused_by_its_provider_mid_task_loses_the_task_to_the_other_worker_of_its_tier(
) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_QUOTA_OUT", "worker"))?;
    let p = project_of(&rig, None, &[("worker", "worker"), ("worker2", "worker")])?;
    let tiers = p.tiers()?;
    rig.tell(
        p.id(),
        &format!(
            "DISPATCH --tier {} QUOTA-OUT Reply with exactly: WORKER_OK",
            tier(&tiers, "worker")
        ),
    )?;
    rig.wait_for("the task to be done", secs(90), || {
        Ok(p.task(1)?["state"] == "done")
    })?;
    let done = p.task(1)?;
    let assignee = done["assignee"].as_str().unwrap_or_default();
    assert!(
        assignee.starts_with("worker2-"),
        "a session of the other worker: {assignee}"
    );
    let body = done["body"].as_str().unwrap_or_default();
    assert!(
        Regex::new(r"Reassigned from @worker-[a-z]+-[a-z]+ \(ran out of quota after starting\)")?
            .is_match(body),
        "{body}"
    );
    // Quota is the member's: the member row says it is out, not a session.
    let board = p.board()?;
    let out = board["lanes"]
        .as_array()
        .and_then(|lanes| {
            lanes
                .iter()
                .find(|l| l["participant"]["handle"] == "worker")
        })
        .map_or(Value::Null, |lane| lane["participant"].clone());
    let until = out["outUntil"]
        .as_str()
        .and_then(|at| at.parse::<jiff::Timestamp>().ok());
    assert!(
        until.is_some_and(|until| until > jiff::Timestamp::now()),
        "the first worker is out: {}",
        out["outUntil"]
    );
    let taken_back = Regex::new(
        r"^T-1 was taken back from @worker-[a-z]+-[a-z]+ \(ran out of quota after starting\)",
    )?;
    assert!(
        p.inbox("chief")?.iter().any(|m| {
            m["kind"] == "note" && taken_back.is_match(m["body"].as_str().unwrap_or_default())
        }),
        "the requester was told"
    );
    let second = p.lane("worker2")?["tasks"][0].clone();
    assert_eq!(second["number"], 1);
    rig.wait_for("the result to be delivered", secs(60), || {
        Ok(p.inbox("chief")?.iter().any(|m| {
            m["kind"] == "result" && m["body"] == "WORKER_OK" && m["state"] == "delivered"
        }))
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn with_human_approval_required_the_brief_and_the_result_each_wait_for_the_human_before_they_move(
) -> Outcome {
    let rig = rig()?;
    let p = project_of(&rig, Some(true), &[("worker", "worker")])?;
    let tiers = p.tiers()?;
    assert_eq!(p.board()?["project"]["gate"], true);
    rig.tell(
        p.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: WORKER_OK",
            tier(&tiers, "worker")
        ),
    )?;

    // The chief's brief is assigned, then held: no worker window opens for it.
    let gated = || -> cf_e2e::Result<Vec<Value>> {
        Ok(p.board()?["gated"].as_array().cloned().unwrap_or_default())
    };
    rig.wait_for("the brief to be held", secs(60), || Ok(gated()?.len() == 1))?;
    let brief = gated()?[0].clone();
    assert_eq!(
        [
            brief["kind"].clone(),
            brief["sender"].clone(),
            brief["taskNumber"].clone()
        ],
        [json!("task"), json!("chief"), json!(1)]
    );
    assert_eq!(p.task(1)?["state"], "queued");
    thread::sleep(Duration::from_millis(1500));
    let window = format!(
        "p{}-{}",
        p.id(),
        brief["recipient"].as_str().unwrap_or_default()
    );
    assert!(
        !rig.open_frames()
            .iter()
            .any(|frame| frame["id"] == window.as_str()),
        "nothing opened while the human had not approved"
    );
    let approved = rig.page("message.approve", json!({ "message": brief["id"] }))?;
    assert_eq!(approved["ok"], true, "{approved}");
    rig.wait_for("the task to be done", secs(60), || {
        Ok(p.task(1)?["state"] == "done")
    })?;

    // The result waits the same way; the chief's window gets nothing until it is passed on.
    rig.wait_for(
        "the result to be held",
        secs(60),
        || Ok(gated()?.len() == 1),
    )?;
    let result = gated()?[0].clone();
    assert_eq!(
        [
            result["kind"].clone(),
            result["recipient"].clone(),
            result["body"].clone()
        ],
        [json!("result"), json!("chief"), json!("WORKER_OK")]
    );
    let results: Vec<Value> = p
        .inbox("chief")?
        .into_iter()
        .filter(|m| m["kind"] == "result")
        .collect();
    assert_eq!(results, Vec::<Value>::new(), "not in the chief's inbox yet");
    rig.page("message.approve", json!({ "message": result["id"] }))?;
    rig.wait_for("the result to be delivered", secs(60), || {
        Ok(p.inbox("chief")?
            .iter()
            .any(|m| m["id"] == result["id"] && m["state"] == "delivered"))
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn the_human_opens_a_finished_sessions_window_on_its_own_conversation_and_it_stays_until_the_session_ends(
) -> Outcome {
    let rig = rig()?;
    let p = project_of(&rig, None, &[("worker", "worker")])?;
    let tiers = p.tiers()?;
    rig.tell(
        p.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: ONE",
            tier(&tiers, "worker")
        ),
    )?;
    rig.wait_for("the task to be done", secs(60), || {
        Ok(p.task(1)?["state"] == "done")
    })?;
    let handle = p.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let session = || -> cf_e2e::Result<Value> {
        Ok(p.board()?["lanes"]
            .as_array()
            .and_then(|lanes| {
                lanes
                    .iter()
                    .find(|lane| lane["participant"]["handle"] == handle.as_str())
            })
            .cloned()
            .unwrap_or(Value::Null))
    };
    rig.wait_for("the session's window to close", secs(30), || {
        Ok(pane_is_null(&session()?))
    })?;
    // The fake agent records its pid and native session: the chief's comes first.
    let started = rig.started();
    let chief = started.first().map(|entry| entry.session.clone());
    let first = started
        .iter()
        .find(|entry| Some(&entry.session) != chief.as_ref())
        .cloned()
        .ok_or("the worker's window was never started")?;
    rig.wait_for("the worker's process to be gone", secs(30), || {
        Ok(!is_alive(first.pid))
    })?;

    let opened = rig.page(
        "session.open",
        json!({ "project": p.id(), "handle": handle }),
    )?;
    assert_eq!(opened["ok"], true, "{opened}");
    rig.wait_for("the session's window to open", secs(30), || {
        Ok(!pane_is_null(&session()?))
    })?;
    // A fresh process on the same conversation.
    rig.wait_for("a fresh process on the conversation", secs(30), || {
        Ok(rig
            .started()
            .iter()
            .any(|entry| entry.session == first.session && entry.pid != first.pid))
    })?;
    let again = rig
        .started()
        .into_iter()
        .rfind(|entry| entry.session == first.session)
        .ok_or("the conversation has no process")?;
    thread::sleep(Duration::from_secs(2));
    let lane = session()?;
    assert!(
        !lane.is_null() && !pane_is_null(&lane),
        "it stays open with nothing to do"
    );
    assert!(is_alive(again.pid));

    // Ending the session is what closes a window the human opened.
    let ended = rig.page(
        "session.end",
        json!({ "project": p.id(), "handle": handle }),
    )?;
    assert_eq!(ended["ok"], true, "{ended}");
    rig.wait_for("the window to close", secs(30), || Ok(!is_alive(again.pid)))?;
    assert_eq!(session()?, Value::Null, "its lane folds into its member");
    rig.close()?;
    Ok(())
}
