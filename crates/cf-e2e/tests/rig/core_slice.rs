//! ConsensFlow end to end (TEST-BDC-09 through the real pane host): the daemon,
//! the real Rust headless bridge and PTYs, and fake Claude agents in them. The
//! human gives the chief a task on the board; the chief hands part of it to a
//! worker's tier with `cf task add`; the daemon picks the worker and opens its
//! window with the task, collects the worker's answer as the result and
//! delivers it into the chief's window, where the chief's own transcript shows
//! it arrived.

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::{files, Error};
use regex::Regex;
use serde_json::{json, Value};

use crate::{after, config, rig, secs, session_of, Outcome};

/// The tier the member `added` was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

#[test]
fn a_chief_hands_a_task_to_a_worker_through_the_board_and_the_result_lands_in_its_window() -> Outcome
{
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let chief_frame = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;

    // A member's work runs in a session of its own: its lane is the session's.
    rig.wait_for("the worker's task to be done", secs(30), || {
        Ok(project.lane("worker")?["tasks"][0]["state"] == "done")
    })?;
    let worker_task = project.lane("worker")?["tasks"][0].clone();
    assert_eq!(worker_task["requester"], "chief");
    assert_eq!(worker_task["number"], 1);

    let worker_id = format!(
        "p{}-{}",
        project.id(),
        worker_task["assignee"].as_str().unwrap_or_default()
    );
    let worker_frame = rig
        .open_frames()
        .into_iter()
        .find(|frame| frame["id"] == worker_id.as_str())
        .unwrap_or(Value::Null);
    let argv = worker_frame["argv"].as_array().cloned().unwrap_or_default();
    let brief = argv.last().and_then(Value::as_str).unwrap_or_default();
    assert!(
        Regex::new(
            r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nReply with exactly: WORKER_OK$"
        )?
        .is_match(brief),
        "{brief:?}"
    );
    assert!(
        argv.iter().any(|word| word == "bypassPermissions"),
        "{argv:?}"
    );

    let delivered = |messages: &[Value]| {
        messages
            .iter()
            .any(|m| m["kind"] == "result" && m["state"] == "delivered")
    };
    rig.wait_for("the result to be delivered to the chief", secs(30), || {
        Ok(delivered(&project.inbox("chief")?))
    })?;
    let messages = project.inbox("chief")?;
    let result = messages
        .iter()
        .find(|m| m["kind"] == "result")
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(result["body"], "WORKER_OK");

    // Every move also went to the home's event file as it happened, for
    // whoever watches the daemon from outside.
    let events: Vec<Value> = files::read_string(&rig.home().join("events.jsonl"))?
        .trim()
        .split('\n')
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|source| Error::Json {
            text: "events.jsonl".to_owned(),
            source,
        })?;
    assert!(events
        .iter()
        .any(|e| e["kind"] == "task.opened" && e["data"]["task"] == 1));
    assert!(events
        .iter()
        .any(|e| e["kind"] == "task.state" && e["data"]["to"] == "done"));
    assert!(events
        .iter()
        .any(|e| e["kind"] == "window.activity" && e["participant"] == "chief"));

    // The chief's native session is the `--session-id` its window was launched with.
    let chief_session = session_of(&chief_frame);
    let arrived = Regex::new(&format!(
        r"\[ConsensFlow m-{} · T-1 · result from @worker-[a-z]+-[a-z]+\]",
        result["id"]
    ))?;
    let transcript = rig.transcript(&chief_session);
    assert!(arrived.is_match(&transcript), "{transcript}");
    rig.close()?;
    Ok(())
}

#[test]
fn one_member_runs_two_tasks_at_once_each_in_a_session_and_window_of_its_own() -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    for word in ["ONE", "TWO"] {
        rig.tell(
            project.id(),
            &format!(
                "DISPATCH --tier {} Reply with exactly: {word}",
                tier(&added)
            ),
        )?;
    }
    let dispatched = || -> cf_e2e::Result<Vec<Value>> {
        Ok(project.board()?["lanes"]
            .as_array()
            .map(|lanes| {
                lanes
                    .iter()
                    .filter(|lane| lane["participant"]["member"] == "worker")
                    .flat_map(|lane| lane["tasks"].as_array().cloned().unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default())
    };
    rig.wait_for("both tasks to be done", secs(60), || {
        Ok(dispatched()?
            .iter()
            .filter(|t| t["state"] == "done")
            .count()
            == 2)
    })?;
    let sessions: Vec<String> = dispatched()?
        .iter()
        .map(|t| t["assignee"].as_str().unwrap_or_default().to_owned())
        .collect();
    let distinct: std::collections::BTreeSet<&String> = sessions.iter().collect();
    assert_eq!(distinct.len(), 2, "two sessions: {sessions:?}");
    let named = Regex::new(r"^worker-[a-z]+-[a-z]+$")?;
    for handle in &sessions {
        assert!(named.is_match(handle), "{handle}");
    }
    let prefix = format!("p{}-worker-", project.id());
    let mut windows: Vec<String> = rig
        .open_frames()
        .iter()
        .filter_map(|frame| frame["id"].as_str())
        .filter(|id| id.starts_with(&prefix))
        .map(str::to_owned)
        .collect();
    windows.sort();
    let mut expected: Vec<String> = sessions
        .iter()
        .map(|handle| format!("p{}-{handle}", project.id()))
        .collect();
    expected.sort();
    assert_eq!(windows, expected, "each session had a window of its own");
    rig.close()?;
    Ok(())
}

#[test]
fn switching_the_chief_opens_a_fresh_window_that_gets_the_handoff_and_reads_what_the_old_one_was_told(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let chiefs = || -> Vec<Value> {
        rig.open_frames()
            .into_iter()
            .filter(|frame| frame["id"] == format!("p{id}-chief").as_str())
            .collect()
    };
    let first = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    assert_eq!(
        after(&first, "--model"),
        "fake-chief",
        "the first chief runs on its agent's model"
    );
    rig.wait_for("the first chief to be idle", secs(10), || {
        let board = project.board()?;
        let chief = board["lanes"]
            .as_array()
            .and_then(|lanes| lanes.iter().find(|l| l["participant"]["handle"] == "chief"))
            .cloned()
            .unwrap_or(Value::Null);
        Ok(chief["activity"]["state"] == "idle")
    })?;
    // The human tells the first chief something only it knows.
    let typed = rig.host(
        "pane.input",
        json!({
            "id": first["id"],
            "generation": first["generation"],
            "bytes": "Reply with exactly: TERN-7314\r".as_bytes(),
        }),
    )?;
    assert_eq!(typed["ok"], true, "{typed}");
    rig.wait_for("the first chief to answer", secs(10), || {
        Ok(rig
            .transcript(&session_of(&first))
            .contains("\"text\":\"TERN-7314\""))
    })?;

    let switched = rig.page("chief.switch", json!({ "project": id, "agent": "worker" }))?;
    assert_eq!(switched["ok"], true, "{switched}");
    rig.wait_for("a second chief window to be opened", secs(10), || {
        Ok(chiefs().len() == 2)
    })?;
    let second = chiefs()[1].clone();
    assert_ne!(
        session_of(&second),
        session_of(&first),
        "every switch starts fresh"
    );
    assert_eq!(after(&second, "--model"), "fake", "the agent's model");
    rig.wait_for("the new chief to be told it is the chief", secs(30), || {
        Ok(rig
            .transcript(&session_of(&second))
            .contains("You are the chief now"))
    })?;
    let board = project.board()?;
    assert_eq!(board["project"]["state"], "open", "a switch is not a close");

    // The new chief reads, with its own token, what the human told the old one.
    let history = rig.cf_in_window(&second, ["history"])?;
    assert_eq!(history.code, Some(0), "{history}");
    assert!(
        Regex::new(r"Human: Reply with exactly: TERN-7314")?.is_match(&history.stdout),
        "{history}"
    );
    assert!(
        Regex::new(r"Claude Code chief: TERN-7314")?.is_match(&history.stdout),
        "{history}"
    );
    assert!(
        !history.stdout.contains("[ConsensFlow m-"),
        "no page can prove a delivery arrived"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_restart_brings_the_project_back_on_its_chiefs_own_conversation() -> Outcome {
    let mut rig = rig()?;
    // The human's own agent, not the rig's: the restart must keep the roster.
    let roster = json!({
        "schemaVersion": 1,
        "agents": [{ "id": "my-chief", "kind": "claude-code", "model": "fake-chief" }],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))?;
    let opened = rig.page(
        "project.open",
        json!({ "directory": rig.workspace(), "agent": "my-chief" }),
    )?;
    let project = opened["project"]["id"].as_i64().unwrap_or_default();
    let first = rig.open_frame(&format!("p{project}-chief"), OPEN)?;
    let session = session_of(&first);
    rig.tell(project, "Reply with exactly: BEFORE-RESTART")?;
    rig.wait_for("the chief to answer", secs(10), || {
        Ok(rig
            .transcript(&session)
            .contains("\"text\":\"BEFORE-RESTART\""))
    })?;

    // The app's quit order: the daemon dies first, then the pane host.
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(config().existing_root(root))?;
    let again = rig.open_frame(&format!("p{project}-chief"), secs(30))?;
    assert_eq!(after(&again, "--resume"), session, "{:?}", again["argv"]);
    let board = Project::new(&rig, project).board()?;
    assert_eq!(board["project"]["state"], "open");
    rig.close()?;
    Ok(())
}
