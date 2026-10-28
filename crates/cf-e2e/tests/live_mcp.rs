//! Live: a real Claude Code worker that has an MCP server finishes its task, and
//! the result reaches the chief with nothing of the harness's own ended. It is the
//! proof of what the rig's stand-in can only stand in for (found on 3.0.1: Claude Code
//! starts its MCP servers as it takes its first message, after ConsensFlow marked where
//! the turn began, and the servers were taken for work the turn left running, so that a
//! worker's task was never done; and since the owner's decision of 2026-10-13 what a
//! turn left running was ended, so a server taken for it would be killed; since
//! turn-end's smallest rule of 2026-10-21 a turn's end ends nothing), and it is
//! opt-in (`cargo xtask live mcp`): it spends the machine's Claude quota on one short
//! task on its cheap model, and needs Claude Code installed and logged in, and
//! `python3`, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The native daemon and the real pane host run in a home of the run's own (never
//! `~/.consensflow`). The chief is a stand-in that only notes what it is sent, and
//! the test plays the chief with the chief's own token (`cf task add`); the worker
//! is the machine's Claude Code, started through a shim that adds `--mcp-config`
//! for the one server of the run ([`server`]) to the flags that shut the human's own
//! servers, connectors and browser out (`--strict-mcp-config --no-chrome`): the
//! window has the server and no other. The server answers what Claude asks to list
//! it, offers no tool and writes its process id, so that the run can tell a task
//! that was not held from a window that had no server at all.
//!
//! The task is a sum the worker does in its head, and nothing more: a turn's end ends
//! nothing since turn-end's smallest rule of 2026-10-21, so the run no longer leaves a
//! command for an ending to spare the server beside (and a command Claude Code runs in
//! the background would hold the task, `cf-harness`' `claude::record::jobs`).
//! The worker's window is kept open, as the human keeps one (Show): a window that closes
//! with its task takes the server with it whatever the daemon does, and a server that is
//! gone with its window has lived through nothing. The run waits for the task to be done,
//! up to five minutes, looking once a second at the task and the server, for the result
//! to reach the chief, and then as long as an ending of the turn's work took when there
//! was one ([`SETTLE`]); then it holds what it saw to the brief ([`verdict`]): the task is
//! done, its result is in the chief's inbox and says the sum, the chief was told nothing
//! of work (a note that names the server is the daemon taking a process of the harness's
//! for the turn's), and the server, which was started while the worker worked, is the
//! same process, by its number and when it began, as it was. A run that fails says what
//! the pane host lists as the turn's work, naming the processes.
//!
//! The folder the worker runs in is the live tests' own
//! (`~/.consensflow-candidate/live/mcp`), where Claude Code is asked once to trust
//! it; `--keep` leaves the home of the run where it is.

// The parts are in `live_mcp/`: cargo looks for the modules of a test's root file
// beside it, where each would be a suite of its own.
#[path = "live_mcp/verdict.rs"]
mod verdict;

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::agents as live_agents;
use cf_e2e::live::machine::live_folder;
use cf_e2e::live::members::{delivered_result, notes_of_work};
use cf_e2e::live::tree;
use cf_e2e::live::{self, text, wait, Harness, Log};
use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::{files, Error, Result};
use serde_json::{json, Value};

use cf_e2e::live::mcp_server::{self as server, Runtime, Server};
use verdict::{judge, Seen, ANSWER};

/// The stand-in for the chief's window, built for these tests: the program the
/// shim runs for the agent whose model is `fake-chief`.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the chief gives the worker to do: a sum its answer is, which the task does not
/// hold.
const TASK: &str = "Reply with only the sum of 1234 and 4321, in digits.";

/// What Claude Code reads in the folder (`CLAUDE.md`): the worker does what each
/// message asks and nothing more.
const BRIEF: &str = "This folder is an automated ConsensFlow check. Do exactly what each message \
asks and nothing more, and run no tool a message does not name.\n";

/// The tier the worker is of, which the task asks for.
const TIER: &str = "standard";

/// How long the worker has to do the task and have it done on the board, its result to be
/// delivered to the chief once it is, and how long its window has to open for the run to
/// keep it open.
const WORK: Duration = Duration::from_secs(300);
const DELIVERS: Duration = Duration::from_secs(60);
const OPENS: Duration = Duration::from_secs(120);
/// How long an ending of a turn's work took when a turn's end had one (the grace the pane
/// host gave what it ended, and a margin): how long the run looks for one after the result.
const SETTLE: Duration = Duration::from_secs(15);

/// The roster of the run: the chief, which is the stand-in's (its model says so
/// to the shim), and the real Claude Code on its cheap model.
fn roster() -> Vec<Value> {
    vec![
        json!({ "id": "chief", "kind": "claude-code", "model": "fake-chief" }),
        live_agents::row(Harness::Claude, TIER),
    ]
}

/// The pane of the worker's window, once the daemon has opened it.
fn worker_pane(rig: &Rig, project: i64) -> Option<Value> {
    let prefix = format!("p{project}-{}-", live_agents::id(Harness::Claude));
    rig.open_frames().into_iter().find(|frame| {
        frame["id"]
            .as_str()
            .is_some_and(|id| id.starts_with(&prefix))
    })
}

/// One look at the worker: the task's state, and the server once its Claude has
/// started it (it writes its process id as it does), as the process it is. A task that
/// failed or was called off is no use to wait for.
fn look(project: &Project, server: &Server, seen: &mut Seen) -> Result {
    seen.state = text(&project.task(1)?["state"]);
    if seen.server.is_none() {
        seen.server = server.identity();
    }
    if seen.state == "failed" || seen.state == "cancelled" {
        return Err(Error::Daemon(format!("T-1 is {}", seen.state)));
    }
    Ok(())
}

/// Says what the product showed of a task that did not end as it should have.
fn failure(rig: &Rig, project: &Project, log: &Log) {
    log.block("the board", &live::pretty(project.board()));
    if let Some(pane) = worker_pane(rig, project.id()) {
        log.block(
            "what the pane host lists as the work the window left running",
            &tree::show(&tree::read(rig, &pane)),
        );
    }
    log.block(
        "what the worker's window recorded",
        &live::transcript(rig, project.id(), 1),
    );
    log.block("the daemon's log", &live::warnings(&rig.log()).join("\n"));
}

/// Keeps the worker's window open as the human does (Show), once the daemon has opened
/// it: it closes with its task otherwise, and takes the server with it.
fn keep_the_workers_window_open(rig: &Rig, project: &Project, log: &Log) -> Result {
    let agent = live_agents::id(Harness::Claude);
    wait(rig, log, "the worker's window to open", OPENS, || {
        Ok(project.lane(&agent)?["pane"].is_object())
    })?;
    let handle = text(&project.lane(&agent)?["participant"]["handle"]);
    let opened = rig.page(
        "session.open",
        json!({ "project": project.id(), "handle": handle }),
    )?;
    if opened["ok"] != true {
        return Err(Error::Daemon(format!(
            "the worker's window was not kept open: {opened}"
        )));
    }
    log.say(format!("the worker's window ({handle}) is kept open"));
    Ok(())
}

/// The whole run: the task from the chief, the wait, and the judgement.
fn attempt(rig: &Rig, project: &Project, server: &Server, seen: &mut Seen, log: &Log) -> Result {
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let ran = rig.cf_in_window(&chief, ["task", "add", "--tier", TIER, TASK])?;
    if ran.code != Some(0) {
        return Err(Error::Daemon(format!("cf task add was refused: {ran}")));
    }
    log.say("the chief gave the worker its task: the sum of 1234 and 4321");
    keep_the_workers_window_open(rig, project, log)?;
    wait(rig, log, "the worker's task to be done", WORK, || {
        look(project, server, seen)?;
        Ok(seen.state == "done")
    })?;
    log.say(format!(
        "T-1 is done; the server (process {:?}) {}",
        server.pid(),
        if server.runs() {
            "runs"
        } else {
            "has ended with the window"
        }
    ));
    wait(
        rig,
        log,
        "the result to be delivered to the chief",
        DELIVERS,
        || {
            seen.result = delivered_result(&project.inbox("chief")?);
            Ok(seen.result.is_some())
        },
    )?;
    // A turn's end ends nothing, and the window the human keeps does not close: the
    // server lives through the result, and nothing is said of work. The run waits as long
    // as an ending of the turn's work took when there was one, so that one that came
    // after the result would be seen.
    log.say("the result is the chief's; waiting to see that nothing is ended or said");
    std::thread::sleep(SETTLE);
    seen.notes = notes_of_work(&project.inbox("chief")?, 1);
    seen.server_after = server.identity();
    log.say(format!(
        "the server was {:?} and is {:?}; the chief was told: {}",
        seen.server,
        seen.server_after,
        seen.notes.join(" | ")
    ));
    judge(seen).map_err(|problems| {
        Error::Daemon(format!(
            "the worker's task is not what it should be:\n  - {}",
            problems.join("\n  - ")
        ))
    })
}

/// The folder of the run's project: a git repository with a README and the brief
/// where Claude Code reads it.
fn folder() -> Result<PathBuf> {
    let folder = live_folder("mcp")?;
    live::git_repository(&folder)?;
    files::write(
        &folder.join("README.md"),
        "# MCP check\n\nA demo project for the live check of a worker that has an MCP server.\n",
    )?;
    files::write(&folder.join("CLAUDE.md"), BRIEF)?;
    Ok(folder)
}

#[test]
#[ignore = "opt-in: spends the machine's Claude quota on one short task; cargo xtask live mcp (npm run live:mcp)"]
fn a_real_claude_worker_with_an_mcp_server_finishes_its_task_and_the_chief_gets_the_result(
) -> Result {
    let log = Log::begin("live mcp")?;
    let claude = log.found(Harness::Claude)?;
    let runtime = Runtime::of_machine()?;
    let made = Path::new(env!("CARGO_TARGET_TMPDIR")).join("live-mcp");
    files::make_dir(&made)?;
    let server = Server::write(&made, &runtime)?;
    let shim = live::shimmed_claude_with_servers(
        &made,
        claude.command(),
        Path::new(FAKE_AGENT),
        server.config(),
    )?;
    log.say(format!(
        "the worker's Claude Code is started through {}, with the one server {} ({})",
        shim.display(),
        server::NAME,
        server.config().display()
    ));
    let mut rig = Rig::start(claude.rig(&shim))?;
    if live::keep_requested() {
        log.say(format!(
            "{} is set: the home stays at {}",
            live::KEEP,
            rig.keep_home().display()
        ));
    }
    live_agents::write_roster(&rig, &roster(), &log)?;
    let folder = folder()?;
    let trust = live::window::trust_claude(&rig, &[shim.to_string_lossy().into_owned()], &folder)?;
    log.say(format!("Claude Code and the folder: {trust}"));
    // The window that trusted the folder had the server too, and said so: what it
    // wrote is not the worker's.
    server.forget();
    let project = Project::open(
        &rig,
        "chief",
        json!({
            "directory": folder,
            "staff": [{ "agent": live_agents::id(Harness::Claude), "roles": ["worker"] }],
        }),
    )?;
    log.say(format!(
        "project {} is open in {}, a stand-in chief and the worker {}",
        project.id(),
        folder.display(),
        live_agents::id(Harness::Claude)
    ));

    let mut seen = Seen::default();
    let outcome = attempt(&rig, &project, &server, &mut seen, &log);
    if outcome.is_err() {
        failure(&rig, &project, &log);
    }
    rig.close()?;
    outcome?;
    log.say(format!(
        "ok: T-1 is done and its result ({}) reached the chief; the chief was told nothing of work, \
         and the server {}, which was started while the worker worked, runs on",
        ANSWER,
        server::NAME
    ));
    Ok(())
}
