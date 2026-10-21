//! The stand-in agent itself, where the cases that run it do not reach (the
//! stand-in Codex's are `stand_in_codex`): its part in how a window closes (the
//! cases of `background_work` hold what ConsensFlow does with the work a window
//! left running): the agent leaves when the pane host asks it to end, and the job
//! it leaves running writes its process, ends by itself saying so, says it was
//! asked to end where it is, and a deaf one is only killed. The asking is Unix's:
//! on Windows a window is ended at once, and nothing there is asked to end. And
//! what the regressions stand on: the server the stand-in agent has for its
//! harness, and the browser it launches in a group of its own.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::files;
use cf_e2e::process::{Run, Spawned};
#[cfg(unix)]
use serde_json::json;
#[cfg(unix)]
use serde_json::Value;

use crate::{Outcome, FAKE_AGENT};

/// A window the pane host closes: its stand-in agent is asked to end (TERM and
/// HUP to its group) and leaves by itself, at once and with the code 0, where
/// the host would otherwise wait out its grace and kill it. Every case of the
/// rig closes windows so, and the rig waits five seconds for its programs to
/// end, a grace of ten being the host's.
#[cfg(unix)]
#[test]
fn the_stand_in_agent_a_pane_host_closes_leaves_when_it_is_asked_to_end_and_is_not_killed(
) -> Outcome {
    use crate::secs;

    let rig = crate::rig()?;
    let opened = rig.host(
        "pane.open",
        json!({
            "id": "closed-1",
            "generation": 1,
            "cwd": "/tmp",
            "argv": [FAKE_AGENT, "--session-id", "closed-session"],
            "env": {},
            "size": { "rows": 24, "cols": 80 },
            "backlogBytes": 65536,
        }),
    )?;
    assert_eq!(opened["ok"], true, "{opened}");
    // It says it is idle once it has set itself up to be asked to end.
    let sessions = Path::new(rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default()).join("sessions");
    rig.wait_for("the agent to be idle", secs(10), || {
        Ok(rig
            .started()
            .first()
            .is_some_and(|agent| sessions.join(format!("{}.json", agent.pid)).exists()))
    })?;

    let closing = Instant::now();
    let closed = rig.host("pane.kill", json!({ "id": "closed-1", "generation": 1 }))?;
    assert_eq!(closed["ok"], true, "{closed}");
    assert!(
        closing.elapsed() < secs(5),
        "the agent left when it was asked, and the close did not wait out its grace: {:?}",
        closing.elapsed()
    );
    rig.wait_for("the window's exit to be told", secs(10), || {
        Ok(!rig.exits().is_empty())
    })?;
    let exit = rig.exits().into_iter().next().unwrap_or(Value::Null);
    assert_eq!(exit["exitCode"], 0, "it left by itself: {exit}");
    assert!(exit["signal"].is_null(), "and was not killed: {exit}");
    rig.close()?;
    Ok(())
}

/// The job a window's stand-in agent leaves running, started as the agent starts
/// it: this program, `--background-job <seconds> <file>`, and `--deaf` if it
/// is to ignore the requests to end.
fn start_job(seconds: &str, file: &Path, deaf: bool) -> cf_e2e::Result<Spawned> {
    let mut run = Run::new(FAKE_AGENT)
        .arg("--background-job")
        .arg(seconds)
        .arg(file);
    if deaf {
        run = run.arg("--deaf");
    }
    run.closed_input().spawn()
}

/// The server a window's stand-in agent has for its harness: a job that stays in
/// the group of the program that started it (`--in-group`).
#[cfg(unix)]
fn start_server(seconds: &str, file: &Path) -> cf_e2e::Result<Spawned> {
    Run::new(FAKE_AGENT)
        .args(["--background-job", seconds])
        .arg(file)
        .arg("--in-group")
        .closed_input()
        .spawn()
}

/// The file of the kind `kind` the job `file` writes.
pub(crate) fn beside(file: &Path, kind: &str) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".");
    name.push(kind);
    PathBuf::from(name)
}

/// Waits until `file` is there, for up to ten seconds.
pub(crate) fn until_there(file: &Path) -> bool {
    let started = Instant::now();
    while !file.exists() && started.elapsed() < Duration::from_secs(10) {
        thread::sleep(Duration::from_millis(10));
    }
    file.exists()
}

/// The process the file `file` holds once it holds a whole one, waited for up to ten
/// seconds: a program that writes its process makes the file before it writes into it,
/// and a file that is there is not one that is written. Where none comes, it says what
/// the file held.
#[cfg(unix)]
pub(crate) fn pid_in(file: &Path) -> Result<u32, String> {
    let started = Instant::now();
    loop {
        let held = files::read_string(file).unwrap_or_default();
        if let Ok(pid) = held.trim().parse() {
            return Ok(pid);
        }
        if started.elapsed() > Duration::from_secs(10) {
            return Err(format!(
                "no process in {} after ten seconds: it held {held:?}",
                file.display()
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_stand_in_agents_job_writes_its_process_runs_its_seconds_and_says_it_finished() -> Outcome {
    let folder = tempfile::tempdir()?;
    let file = folder.path().join("job-1-1");
    let mut job = start_job("1", &file, false)?;

    assert!(until_there(&beside(&file, "pid")), "the job says it runs");
    assert_eq!(
        files::read_string(&beside(&file, "pid"))?,
        job.id().to_string()
    );
    let ended = job
        .wait(Duration::from_secs(10))?
        .ok_or("the job did not end in ten seconds")?;
    assert!(ended.success(), "{ended:?}");
    assert!(beside(&file, "finished").exists());
    assert!(!beside(&file, "released").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn the_stand_in_agents_job_asked_to_end_says_so_and_goes_and_a_deaf_one_is_only_killed() -> Outcome
{
    use cf_e2e::process::Signal;

    let folder = tempfile::tempdir()?;
    // Asked to end: it writes that it was, and goes with the code 0.
    let file = folder.path().join("job-1-1");
    let mut job = start_job("60", &file, false)?;
    assert!(until_there(&beside(&file, "pid")), "the job says it runs");
    job.signal(Signal::Terminate);
    let ended = job
        .wait(Duration::from_secs(10))?
        .ok_or("the job did not end when it was asked to")?;
    assert!(ended.success(), "{ended:?}");
    assert!(beside(&file, "released").exists());
    assert!(!beside(&file, "finished").exists());

    // A deaf one ignores the request, and the interrupt, and runs on until it is killed.
    let file = folder.path().join("job-1-2");
    let mut deaf = start_job("60", &file, true)?;
    assert!(until_there(&beside(&file, "pid")), "the job says it runs");
    deaf.signal(Signal::Terminate);
    deaf.signal(Signal::Interrupt);
    thread::sleep(Duration::from_secs(1));
    assert!(!deaf.has_exited(), "it ignored what asked it to end");
    deaf.kill();
    assert!(deaf.has_exited());
    assert!(!beside(&file, "released").exists());
    Ok(())
}

/// What the cases of `background_work` that run a harness's server beside a job
/// stand on: the job leaves the group of the program that started it, as a command
/// the agent's tool runs does, and the server stays in it, as an MCP server does.
/// The pane host tells them apart by that group alone.
#[cfg(unix)]
#[test]
fn the_stand_in_agents_job_leaves_the_group_of_its_window_and_its_server_stays_in_it() -> Outcome {
    use nix::unistd::{getpgid, getpgrp, Pid};

    let group_of = |program: &Spawned| -> Result<Pid, Box<dyn std::error::Error>> {
        Ok(getpgid(Some(Pid::from_raw(i32::try_from(program.id())?)))?)
    };
    let folder = tempfile::tempdir()?;
    let (job_file, server_file) = (
        folder.path().join("job-1-1"),
        folder.path().join("server-1-1"),
    );
    let mut job = start_job("60", &job_file, false)?;
    let mut server = start_server("60", &server_file)?;
    // Each writes its process once it has set itself up, its group chosen.
    assert!(
        until_there(&beside(&job_file, "pid")),
        "the job says it runs"
    );
    assert!(
        until_there(&beside(&server_file, "pid")),
        "the server says it runs"
    );

    // The program that started them is this test's: its group is the window's here.
    let window = getpgrp();
    let job_group = group_of(&job)?;
    assert_eq!(
        job_group.as_raw(),
        i32::try_from(job.id())?,
        "it leads a group of its own"
    );
    assert_ne!(job_group, window);
    assert_eq!(
        group_of(&server)?,
        window,
        "the server stays in its window's group"
    );
    job.kill();
    server.kill();
    Ok(())
}

/// The server a window's stand-in agent has for its harness, which launches a
/// browser of its own (`--browser <file>`): a job that is a child of the server and
/// leads a group of its own, as the browser of `@playwright/mcp` does.
#[cfg(unix)]
#[test]
fn the_stand_in_agents_server_launches_a_browser_that_is_its_child_in_a_group_of_its_own() -> Outcome
{
    use cf_e2e::process::group_of;

    let folder = tempfile::tempdir()?;
    let (server_file, browser_file) = (
        folder.path().join("server-1-1"),
        folder.path().join("browser-1-1"),
    );
    let mut server = Run::new(FAKE_AGENT)
        .args(["--background-job", "60"])
        .arg(&server_file)
        .arg("--in-group")
        .arg("--browser")
        .arg(&browser_file)
        .closed_input()
        .spawn()?;
    assert!(until_there(&beside(&server_file, "pid")), "the server runs");
    assert!(
        until_there(&beside(&browser_file, "pid")),
        "so does its browser"
    );

    let browser: u32 = files::read_string(&beside(&browser_file, "pid"))?
        .trim()
        .parse()?;
    assert_eq!(
        group_of(browser),
        Some(browser),
        "the browser leads a group of its own"
    );
    assert_ne!(
        group_of(server.id()),
        Some(server.id()),
        "the server does not"
    );
    // A server that is killed outright leaves its browser running, as a window's
    // close finds and ends it (by the marker of its environment): here nothing does.
    server.kill();
    cf_e2e::process::signal(browser, cf_e2e::process::Signal::Kill)?;
    Ok(())
}
