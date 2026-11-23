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
use serde_json::json;
#[cfg(unix)]
use serde_json::Value;

use crate::{Outcome, FAKE_AGENT};

/// The rig's leftover check takes a noted number for a window's only while its process runs
/// the stand-in (T-1236; on Windows CI a reused number held by `sort.exe` was said to be a
/// window left): the chief's window, running, is the one it names, by its number and its
/// command line, so the program it reads is the window's own on each system.
#[test]
fn the_leftover_check_names_a_window_still_running_by_the_stand_in_it_runs() -> Outcome {
    use crate::secs;

    let rig = crate::rig()?;
    let _project = cf_e2e::rig::Project::open(&rig, "chief", serde_json::json!({}))?;
    rig.wait_for("the chief's window to note its number", secs(60), || {
        Ok(!rig.started().is_empty())
    })?;
    let chief = rig.started()[0].pid;
    let named = rig
        .windows_the_close_would_name()
        .ok_or("the check named no window, though the chief's runs")?;
    assert!(named.starts_with(&format!("{chief} (")), "{named}");
    assert!(named.contains("fake-agent"), "{named}");
    rig.close()?;
    Ok(())
}

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
    let opened = rig.open_pane(json!({
        "id": "closed-1",
        "generation": 1,
        "cwd": "/tmp",
        "argv": [FAKE_AGENT, "--session-id", "closed-session"],
        "env": {},
        "size": { "rows": 24, "cols": 80 },
        "backlogBytes": 65536,
    }))?;
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

/// Waits until `file` is there, for up to ten seconds. The cases that ask are Unix's.
#[cfg(unix)]
pub(crate) fn until_there(file: &Path) -> bool {
    let started = Instant::now();
    while !file.exists() && started.elapsed() < Duration::from_secs(10) {
        thread::sleep(Duration::from_millis(10));
    }
    file.exists()
}

/// The process the file `file` holds once it holds a whole one, waited for up to ten
/// seconds: a program that writes its process in place makes the file before it writes
/// into it, and a file that is there is not one that is written. The stand-ins write
/// theirs whole (`cf_e2e::files::say_its_process`), and this waits for a number all the same.
/// Where none comes, it says what the file held. The job's own case asks it on every system.
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
    let folder = cf_e2e::door::made("cf-stand-in-")?;
    let file = folder.path().join("job-1-1");
    let mut job = start_job("1", &file, false)?;

    assert_eq!(
        pid_in(&beside(&file, "pid"))?,
        job.id(),
        "the job says it runs"
    );
    let ended = job
        .wait(Duration::from_secs(10))?
        .ok_or("the job did not end in ten seconds")?;
    assert!(ended.success(), "{ended:?}");
    assert!(beside(&file, "finished").exists());
    assert!(!beside(&file, "released").exists());
    Ok(())
}

/// The process a job writes is whole from the moment its file is there, so that a case
/// that waits for the file and reads it reads a number: the file is looked for as often as
/// a case can, and read at once when it is found, as the Codex stand-in's is
/// (`fake_codex/servers.rs`).
#[test]
fn the_stand_in_agents_job_writes_its_process_whole_or_not_at_all() -> Outcome {
    let folder = cf_e2e::door::made("cf-stand-in-")?;
    let file = folder.path().join("job-1-1");
    let _job = start_job("1", &file, false)?;
    let started = Instant::now();
    let held = loop {
        if let Ok(held) = std::fs::read_to_string(beside(&file, "pid")) {
            break held;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the job never wrote its process"
        );
        thread::yield_now();
    };
    assert!(
        held.trim().parse::<u32>().is_ok(),
        "the job's file was there holding {held:?}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn the_stand_in_agents_job_asked_to_end_says_so_and_goes_and_a_deaf_one_is_only_killed() -> Outcome
{
    use cf_e2e::process::Signal;

    let folder = cf_e2e::door::made("cf-stand-in-")?;
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
    let folder = cf_e2e::door::made("cf-stand-in-")?;
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

    let folder = cf_e2e::door::made("cf-stand-in-")?;
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
    let browser = pid_in(&beside(&browser_file, "pid"))?;
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

/// K3 (T-2000 finding 1, T-1998's rig case): a window asked with another home, or with `HOME`
/// taken away, is not opened by the rig's own request; the window opened next writes its real
/// environment down, and it is the rig's: every home below the rig's, and no person's home
/// (`CF_KIT_PROJECT_HOME` is gone).
#[cfg(unix)]
#[test]
fn a_window_asked_with_another_home_or_with_a_home_taken_away_is_not_opened_and_the_next_has_the_rigs_homes(
) -> Outcome {
    let rig = crate::rig()?;
    // A folder of no run's: the temp folder's parent's, never made.
    let elsewhere = std::env::temp_dir()
        .parent()
        .map(|parent| parent.join(format!("not-the-runs-{}", std::process::id())))
        .ok_or("the temp folder has no parent")?;
    let window = |id: &str, extra: serde_json::Value| {
        let mut body = json!({
            "id": id,
            "generation": 1,
            "cwd": rig.workspace(),
            "argv": ["/bin/sh", "-c", "sleep 30"],
            "env": {},
            "size": { "rows": 24, "cols": 80 },
            "backlogBytes": 65536,
        });
        for (key, value) in extra.as_object().into_iter().flatten() {
            body[key] = value.clone();
        }
        body
    };
    let refused = [
        window("other-home", json!({ "env": { "APPDATA": elsewhere } })),
        window("no-home", json!({ "dropEnv": ["HOME"] })),
    ];
    for body in refused {
        let id = body["id"].clone();
        assert!(rig.open_pane(body).is_err(), "{id} was opened");
    }
    assert_eq!(rig.refused_windows().len(), 2);
    let said = rig.workspace().join("window-env.txt");
    let opened = rig.open_pane(window(
        "the-next",
        json!({ "argv": ["/bin/sh", "-c", format!("env > '{}'; sleep 30", said.display())] }),
    ))?;
    assert_eq!(opened["ok"], true, "{opened}");
    rig.wait_for("the window's environment", crate::secs(10), || {
        Ok(std::fs::read_to_string(&said)
            .is_ok_and(|text| text.contains("\nHOME=") || text.starts_with("HOME=")))
    })?;
    let env = std::fs::read_to_string(&said)?;
    let root = rig
        .home()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    for name in cf_e2e::door::spawn::HOME_VARS {
        let line = env
            .lines()
            .find(|line| line.starts_with(&format!("{name}=")))
            .ok_or_else(|| format!("{name} is not in the window's environment"))?;
        let value = &line[name.len() + 1..];
        let real = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        assert!(
            real(Path::new(value)).starts_with(real(&root)) || Path::new(value).starts_with(&root),
            "{name} is {value}, not below the rig's {}",
            root.display()
        );
    }
    assert!(!env.contains("CF_KIT_PROJECT_HOME"), "{env}");
    let _ = rig.close_pane(json!({ "id": "the-next", "generation": 1 }));
    rig.close()?;
    Ok(())
}

/// T-2086 B1: on Windows the pane host's builder puts every variable the account's registry stores
/// over the host's own environment, which the seal never saw (`setx`'s `CODEX_HOME`, and always
/// `TEMP`, `TMP` and `Path` from `HKCU\Environment`). A real window writes its environment down:
/// each name the registry holds has the value the window was judged on, or is not there where the
/// judged environment has none. Nothing is written to the registry: it is read for its names only.
#[cfg(windows)]
#[test]
fn a_windows_window_has_the_environment_it_was_judged_on_and_nothing_the_account_stores() -> Outcome
{
    let rig = crate::rig()?;
    let system = std::env::var_os("SystemRoot").ok_or("no SystemRoot")?;
    let shell = Path::new(&system).join("System32").join("cmd.exe");
    let body = json!({
        "id": "the-window",
        "generation": 1,
        "cwd": rig.workspace(),
        "argv": [shell, "/d", "/c", "set > window-env.txt & ping -n 30 127.0.0.1 > nul"],
        "env": {},
        "size": { "rows": 24, "cols": 80 },
        "backlogBytes": 65536,
    });
    let judged = rig.judged_environment(&body)?;
    let opened = rig.open_pane(body)?;
    assert_eq!(opened["ok"], true, "{opened}");
    let said = rig.workspace().join("window-env.txt");
    rig.wait_for("the window's environment", crate::secs(20), || {
        Ok(std::fs::read_to_string(&said)
            .is_ok_and(|text| text.to_ascii_uppercase().contains("\nPATH=")))
    })?;
    let text = std::fs::read_to_string(&said)?;
    let window: Vec<(String, String)> = text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    let stored = cf_e2e::door::stored_names().expect("the account's registry can be read");
    assert!(
        !stored.is_empty(),
        "the account's registry holds no variable (HKCU\\Environment has TEMP)"
    );
    let mut wrong = Vec::new();
    for name in &stored {
        let name = name.to_string_lossy();
        let had = window
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(&name))
            .map(|(_, value)| value.as_str());
        let judged = judged
            .iter()
            .find(|(held, _)| held.to_string_lossy().eq_ignore_ascii_case(&name))
            .map(|(_, value)| value.to_string_lossy().into_owned());
        if had != judged.as_deref() {
            // Which names differ, never their values: they are the account's.
            wrong.push(format!(
                "{name} (the window {} it, the judged environment {} it)",
                if had.is_some() { "has" } else { "has not" },
                if judged.is_some() { "has" } else { "has not" }
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "the window has what the account stores: {}",
        wrong.join("; ")
    );
    let _ = rig.close_pane(json!({ "id": "the-window", "generation": 1 }));
    rig.close()?;
    Ok(())
}
