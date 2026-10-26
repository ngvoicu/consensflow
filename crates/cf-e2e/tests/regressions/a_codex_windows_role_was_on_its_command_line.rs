//! A Codex window's role was on its command line.
//!
//! Seen on 2026-10-24 with the first messages on ConsensFlow 3.0.5 (T-432, the issue
//! `issues/a-windows-first-message-is-on-its-command-line.md` in consensflow-sme): a
//! window's command line is what every process on the Mac reads (`/bin/ps` is
//! set-uid root), and a kill or wait by a pattern whose words it held matched the
//! window. A Codex window was given its role text, the member's or the chief's whole
//! instructions, as `-c developer_instructions=<JSON>`: on the command line of
//! ConsensFlow's supervisor (`cf codex-session`), which put it on the command lines of
//! Codex's app-server and TUI as well. T-698 is the work.
//!
//! The board's token, which a window's environment holds, was on Codex's server's
//! command line too (`-c shell_environment_policy.set.CONSENSFLOW_TOKEN=…`, so
//! that a user policy of `inherit = "core"` kept it for the thread's commands),
//! found in the review of the role's work (T-764): it goes to each thread in its
//! `config` now.
//!
//! What the owner should have seen: the role is on the command line of none of the
//! window's processes, and a pattern of its words matches none, while the thread the
//! window starts still begins with it, once, as its developer instructions; and a
//! window opened again on that thread has nothing of it on its command line either,
//! and the thread keeps what it began with. The chief's role here is an evaluation's
//! card (`CONSENSFLOW_EVAL_CHIEF_CARD`), which holds a word that is nowhere else.

use std::path::Path;

use cf_e2e::process::Run;
use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::{files, rollout};
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// A word that is in the role and nowhere else on the machine.
fn unique() -> String {
    format!("ROLEWORD{}CODEX", std::process::id())
}

/// What `program` prints, run with `args`, and its exit code.
fn run(program: &str, args: &[&str]) -> cf_e2e::Result<(Option<i32>, String)> {
    let ran = Run::new(Path::new(program))
        .args(args.iter().copied())
        .inheriting_env()
        .run()?;
    Ok((ran.code, ran.stdout.clone()))
}

/// That nothing the system shows of the window `frame` opened holds `word`: the
/// command line the pane host started it with, its process's as `ps` reads it, and
/// every process's as `pgrep -f` matches them (the supervisor's, Codex's server's
/// and its TUI's among them).
fn shown_nowhere(rig: &Rig, frame: &Value, word: &str) -> Outcome {
    assert!(
        !frame["argv"].to_string().contains(word),
        "the window's command line holds its role: {}",
        frame["argv"]
    );
    rig.wait_for("the window's process to be seen", secs(30), || {
        Ok(rig.window_process(frame).is_some())
    })?;
    let process = rig
        .window_process(frame)
        .ok_or("the pane host named no process for the window")?;
    let (_, command) = run(
        "ps",
        &["-ww", "-o", "command=", "-p", &process.pid.to_string()],
    )?;
    assert!(!command.trim().is_empty(), "the window's process is there");
    assert!(
        !command.contains(word),
        "ps shows the role on the window's command line: {command}"
    );
    let (code, found) = run("pgrep", &["-f", word])?;
    assert_eq!(
        code,
        Some(1),
        "a pattern of the role's words matches processes: {found}"
    );
    Ok(())
}

/// That no process on the machine has `secret` on its command line, as `ps` shows
/// every process's to every user: read here rather than matched with `pgrep`,
/// whose pattern a token would be.
fn on_no_process(secret: &str, what: &str) -> Outcome {
    let (_, all) = run("ps", &["-A", "-ww", "-o", "command="])?;
    assert!(!all.trim().is_empty(), "ps listed processes");
    assert!(
        !all.contains(secret),
        "a process has {what} on its command line"
    );
    Ok(())
}

/// What the stand-in Codex wrote down of its runs: each one's command line, and the
/// starts its server was asked for.
fn codex_runs(rig: &Rig) -> cf_e2e::Result<Vec<Value>> {
    let log = rig.var("CF_TEST_CODEX_LOG").unwrap_or_default();
    Ok(files::read_string(Path::new(log))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// The windows of the chief of `project`, in the order they were opened.
fn chief_windows(rig: &Rig, project: &Project) -> Vec<Value> {
    let chief = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == chief.as_str())
        .collect()
}

#[test]
fn a_codex_chiefs_role_is_on_no_command_line_fresh_or_resumed_and_its_thread_begins_with_it_once(
) -> Outcome {
    let word = unique();
    let card = tempfile::tempdir()?;
    let file = card.path().join("chief.md");
    files::write(
        &file,
        format!("# The chief\n\nYour code word is {word}. Put tasks on the board with cf.\n"),
    )?;
    let rig = Rig::start(
        config_with_codex().daemon_var("CONSENSFLOW_EVAL_CHIEF_CARD", file.to_string_lossy()),
    )?;
    let project = Project::open_with_staff(&rig, "maia", None, &[("worker", "worker")])?;
    let first = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // The chief has a turn, which begins its thread's record: a worker's result.
    let tiers = project.tiers()?;
    let tier = tiers.get("worker").map_or("standard", String::as_str);
    let ran = rig.cf_in_window(
        &first,
        [
            "task",
            "add",
            "--tier",
            tier,
            "Reply with exactly: WORKER_OK",
        ],
    )?;
    assert_eq!(ran.code, Some(0), "{ran}");
    let home = Path::new(rig.var("CODEX_HOME").unwrap_or_default()).to_path_buf();
    let chief_record = || -> cf_e2e::Result<String> {
        let rollouts = rollout::rollouts(&home);
        let chiefs: Vec<String> = rollouts
            .iter()
            .filter_map(|path| rollout::read(path).ok())
            .filter(|text| text.contains(&word))
            .collect();
        Ok(chiefs.into_iter().next().unwrap_or_default())
    };
    rig.wait_for("the worker's result to reach the chief", secs(60), || {
        Ok(rollout::said_by(&chief_record()?, "user")
            .iter()
            .any(|said| said.contains("WORKER_OK")))
    })?;
    // The window's three programs are up, its thread begun.
    shown_nowhere(&rig, &first, &word)?;
    // Nor is the board's token, which the window's environment holds.
    let token = first["env"]["CONSENSFLOW_TOKEN"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or("the window has no board token")?
        .to_owned();
    on_no_process(&token, "the window's board token")?;
    let began_with = rollout::said_by(&chief_record()?, "developer");
    assert_eq!(
        began_with.len(),
        1,
        "one set of instructions: {began_with:?}"
    );
    assert!(
        began_with[0].contains(&format!("Your code word is {word}")),
        "the thread begins with the role: {began_with:?}"
    );
    // The broker gave it to the start, and nothing gave it to a command line.
    let runs = codex_runs(&rig)?;
    let starts: Vec<&Value> = runs
        .iter()
        .filter(|run| run.get("started").is_some())
        .collect();
    assert!(
        starts
            .iter()
            .any(|run| run["started"]["developerInstructions"]
                .as_str()
                .is_some_and(|given| given.contains(&word))),
        "no start was given the role: {starts:?}"
    );
    let programs: Vec<&Value> = runs
        .iter()
        .filter(|run| run.get("args").is_some())
        .collect();
    assert!(
        programs.len() >= 2,
        "Codex's server and TUI ran: {programs:?}"
    );
    // The thread was given ConsensFlow's variables for its commands, the token too.
    assert!(
        starts.iter().any(|run| run["started"]["config"]
            ["shell_environment_policy.set.CONSENSFLOW_TOKEN"]
            == token.as_str()),
        "no start was given the board's token for its commands"
    );
    for run in programs {
        assert!(
            !run["args"].to_string().contains(&token),
            "Codex's {} was started with the board's token on its command line",
            run["run"]
        );
        assert!(
            !run["args"].to_string().contains(&word),
            "Codex's {} was started with its role: {}",
            run["run"],
            run["args"]
        );
        assert_eq!(
            run["roleFile"],
            Value::Null,
            "Codex's {} was told where the role's file is",
            run["run"]
        );
    }

    // Opened again on its thread: nothing of the role on any command line, and the
    // thread keeps what it began with.
    rig.wait_for("the chief to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })?;
    let id = project.id();
    for op in ["project.close", "project.resume"] {
        let done = rig.page(op, json!({ "project": id }))?;
        assert_eq!(done["ok"], true, "{op}: {done}");
    }
    rig.wait_for("the chief's next window", secs(60), || {
        Ok(chief_windows(&rig, &project).len() >= 2)
    })?;
    let again = chief_windows(&rig, &project).remove(1);
    assert!(
        again["argv"]
            .as_array()
            .is_some_and(|argv| argv.iter().any(|word| word == "resume")),
        "a window on the thread: {}",
        again["argv"]
    );
    rig.wait_for("the window to name its thread", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })?;
    shown_nowhere(&rig, &again, &word)?;
    let token_again = again["env"]["CONSENSFLOW_TOKEN"]
        .as_str()
        .unwrap_or_default();
    on_no_process(token_again, "the window's board token")?;
    let runs = codex_runs(&rig)?;
    for run in runs.iter().filter(|run| run.get("args").is_some()) {
        assert!(!run["args"].to_string().contains(&word), "{run}");
        assert!(
            !run["args"].to_string().contains(token_again),
            "Codex's {} was started with the board's token on its command line",
            run["run"]
        );
    }
    // The resumed thread was given ConsensFlow's variables for its commands.
    assert!(
        runs.iter().any(|run| run["resumed"]["config"]
            ["shell_environment_policy.set.CONSENSFLOW_TOKEN"]
            == token_again),
        "the resume was not given the board's token for its commands"
    );
    assert_eq!(
        rollout::said_by(&chief_record()?, "developer"),
        began_with,
        "the thread keeps the instructions it began with"
    );
    rig.close()?;
    Ok(())
}
