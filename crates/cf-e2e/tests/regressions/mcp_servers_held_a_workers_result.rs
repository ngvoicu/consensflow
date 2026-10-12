//! MCP servers held a worker's result as background work.
//!
//! Seen by the owner on 2026-10-12, on 3.0.1. Workers whose windows had MCP
//! servers (the owner's own: `@playwright/mcp`, `chrome-devtools-mcp`, and
//! Codex's plugins) never finished their tasks: the board said `background work
//! running` for as long as the window lived, the chief was given no result, it
//! could not accept the task, and the tasks that waited on it (`--after`) never
//! went. The servers began after the daemon had marked where the turn began, and
//! the pane host took everything that began after the mark for what the turn
//! left running. The report is the issue
//! `issues/mcp-servers-held-a-workers-result-as-background-work.md` in
//! consensflow-sme.
//!
//! What the owner should have seen: the worker's turn ends, the task is done, the
//! chief is given the result and can accept it, and the tasks that waited for it
//! (`--after`) go.

use cf_e2e::process::{group_of, is_alive};
use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, config_with_codex, secs, Outcome};

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// The results the chief has been given or is to be given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    let mut results: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect();
    results.sort_by_key(|message| message["id"].as_i64().unwrap_or_default());
    Ok(results)
}

/// The chief accepts the task `task` from its window, as it does with `cf task
/// accept`.
fn accepts(rig: &Rig, project: &Project, task: &str) -> cf_e2e::Result<()> {
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let ran = rig.cf_in_window(&chief, ["task", "accept", task])?;
    if ran.code == Some(0) {
        Ok(())
    } else {
        Err(Error::Daemon(format!("cf task accept {task}: {ran}")))
    }
}

/// A project with a worker, given T-1 through the chief: a turn of a few seconds,
/// in which the harness starts its servers.
fn a_worker_given_a_task(rig: &Rig) -> cf_e2e::Result<Project<'_>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 15 Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    Ok(project)
}

/// What the owner should have seen of T-1: it is done, the chief has the result and
/// accepts it.
fn the_chief_gets_the_result_and_accepts_it(rig: &Rig, project: &Project) -> Outcome {
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(project)?.is_empty())
    })?;
    assert_eq!(results(project)?[0]["body"], "WORKER_OK");
    assert_eq!(
        project.lane("worker")?["activity"]["reason"],
        Value::Null,
        "the board says no work is running"
    );
    accepts(rig, project, "T-1")?;
    assert_eq!(project.task(1)?["state"], "accepted");
    Ok(())
}

/// A Claude worker, whose harness starts its MCP servers as it takes its first
/// message: after the daemon marked where the turn began, in the group of the
/// window's program.
#[test]
fn a_claude_worker_with_mcp_servers_gives_the_chief_its_result_and_the_chief_accepts_it() -> Outcome
{
    let rig = Rig::start(config().var("CF_TEST_MCP_SERVER", "late"))?;
    let project = a_worker_given_a_task(&rig)?;
    // The servers run while the turn is at work, as the owner's did.
    rig.wait_for("the servers to run", secs(60), || {
        let servers = rig.jobs("server");
        Ok(servers.len() >= 2 && servers.iter().all(|server| is_alive(server.pid())))
    })?;
    assert_eq!(project.task(1)?["state"], "working");
    the_chief_gets_the_result_and_accepts_it(&rig, &project)?;
    rig.close()?;
    Ok(())
}

/// The same worker whose servers launch a browser when a tool is used, in a group
/// of its own, as `@playwright/mcp` and `chrome-devtools-mcp` do.
#[test]
fn a_claude_worker_whose_server_launches_a_browser_gives_the_chief_its_result_and_the_chief_accepts_it(
) -> Outcome {
    let rig = Rig::start(
        config()
            .var("CF_TEST_MCP_SERVER", "late")
            .var("CF_TEST_MCP_BROWSER", "1"),
    )?;
    let project = a_worker_given_a_task(&rig)?;
    rig.wait_for("the servers and their browsers to run", secs(60), || {
        let (servers, browsers) = (rig.jobs("server"), rig.jobs("browser"));
        Ok(servers.len() >= 2
            && browsers.len() >= 2
            && servers
                .iter()
                .chain(&browsers)
                .all(|job| is_alive(job.pid())))
    })?;
    // The shapes this case stands on: each server is in its window's group, and the
    // browser it launched leads a group of its own.
    for server in rig.jobs("server") {
        assert_ne!(
            group_of(server.pid()),
            Some(server.pid()),
            "it is in its window's"
        );
    }
    for browser in rig.jobs("browser") {
        assert_eq!(group_of(browser.pid()), Some(browser.pid()), "it leads one");
    }
    assert_eq!(project.task(1)?["state"], "working");
    the_chief_gets_the_result_and_accepts_it(&rig, &project)?;
    rig.close()?;
    Ok(())
}

/// The owner's Codex designers (T-32 and T-34, which waited for it with `--after`):
/// Codex opens its plugins' servers with the window, each in a group of its own, and
/// one of them starts two children as the designer's first turn calls its tools.
#[test]
fn a_codex_designer_whose_plugin_server_starts_children_gives_the_chief_its_result_and_the_task_that_waits_for_it_goes(
) -> Outcome {
    let rig = Rig::start(
        config_with_codex()
            .var("CF_TEST_CODEX_SERVERS", "2")
            .var("CF_TEST_CODEX_WORKERS", "2"),
    )?;
    let project = Project::open(
        &rig,
        "chief",
        json!({ "staff": [{ "agent": "pygmalion", "roles": ["designer"] }] }),
    )?;
    rig.tell(
        project.id(),
        "DISPATCH --design SLEEP 15 Reply with exactly: DESIGN_ONE",
    )?;
    // The task that waits on the board for T-1 to be accepted. (`--after T-1`, the
    // follow-up for the designer's window, is refused while the window is on T-1.)
    rig.tell(
        project.id(),
        "DISPATCH --design --needs T-1 Reply with exactly: DESIGN_TWO",
    )?;
    // The plugins' servers are there with the window, and the children one of them
    // starts as the turn calls its tools run while it is at work.
    rig.wait_for("the plugins' servers and their children", secs(60), || {
        let (servers, children) = (rig.jobs("plugin"), rig.jobs("worker"));
        Ok(servers.len() >= 2
            && children.len() >= 2
            && servers
                .iter()
                .chain(&children)
                .all(|job| is_alive(job.pid())))
    })?;
    // The shapes this case stands on: each server leads a group of its own, and the
    // children of one are in it.
    for server in rig.jobs("plugin") {
        assert_eq!(group_of(server.pid()), Some(server.pid()), "it leads one");
    }
    let first = rig.jobs("plugin").into_iter().next().map(|job| job.pid());
    for child in rig.jobs("worker") {
        assert_eq!(group_of(child.pid()), first, "it is in its server's");
    }
    assert_eq!(project.task(1)?["state"], "working");
    assert_ne!(
        project.task(2)?["state"],
        "working",
        "T-2 waits for T-1 to be accepted"
    );

    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "DESIGN_ONE");
    accepts(&rig, &project, "T-1")?;
    assert_eq!(project.task(1)?["state"], "accepted");
    rig.wait_for("T-2, which waited for T-1, to go", secs(60), || {
        Ok(project.task(2)?["state"] == "working")
    })?;
    rig.wait_for("T-2 to be done", secs(60), || {
        Ok(project.task(2)?["state"] == "done")
    })?;
    rig.wait_for("T-2's result to reach the chief", secs(30), || {
        Ok(results(&project)?.len() == 2)
    })?;
    assert_eq!(results(&project)?[1]["body"], "DESIGN_TWO");
    rig.close()?;
    Ok(())
}
