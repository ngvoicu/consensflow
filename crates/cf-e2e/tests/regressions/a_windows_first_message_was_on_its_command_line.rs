//! A board window's first message was on its command line.
//!
//! Seen on 2026-10-24 on ConsensFlow 3.0.5, the board that builds ConsensFlow: a
//! worker (T-426) sent SIGINT to every process whose `ps` line matched "xtask
//! plants", to stop its own run, and two other workers' Claude windows (T-82's and
//! T-429's) closed, their tools killed. ConsensFlow started a member's Claude window
//! with its task's brief as the last argument of its command line, so `ps` showed the
//! brief to every process on the Mac (`/bin/ps` is set-uid root), and a kill or wait
//! by pattern whose words a brief held matched the window. Codex's and Pi's windows
//! were started the same way. The issue is `issues/a-windows-first-message-is-on-its-command-line.md`
//! in consensflow-sme; T-432 is the work.
//!
//! What the owner should have seen: no window's command line holds its first message,
//! and a pattern made of a brief's words matches no window, while each window is still
//! given its brief, once, as its first message: a worker's window opened on the
//! stand-in Claude, its task under way, read through the pane host's `pane.open` and
//! the system's own `ps` and `pgrep`; and a Codex worker's window, the stand-in Codex
//! under ConsensFlow's supervisor, whose task is done.

#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
use cf_e2e::files;
#[cfg(unix)]
use cf_e2e::process::Run;
use cf_e2e::rig::{Project, Rig, OPEN};
#[cfg(unix)]
use serde_json::json;
use serde_json::Value;

#[cfg(unix)]
use crate::config_with_codex;
use crate::{config, secs, Outcome};

/// A word that is in a brief and nowhere else on the machine.
fn unique(case: &str) -> String {
    format!("BRIEFWORD{}{case}", std::process::id())
}

/// The words of the command line of the window `frame` opened.
fn argv_of(frame: &Value) -> Vec<String> {
    frame["argv"]
        .as_array()
        .map(|words| {
            words
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The chief puts a task for `tier` on the board, its text `task`.
fn put(rig: &Rig, project: &Project, tier: &str, task: &str) -> Outcome {
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let ran = rig.cf_in_window(&chief, ["task", "add", "--tier", tier, task])?;
    assert_eq!(ran.code, Some(0), "{ran}");
    Ok(())
}

/// What `program` prints, run with `args`, and its exit code.
#[cfg(unix)]
fn run(program: &str, args: &[&str]) -> cf_e2e::Result<(Option<i32>, String)> {
    let ran = Run::new(Path::new(program))
        .args(args.iter().copied())
        .inheriting_env()
        .run()?;
    Ok((ran.code, ran.stdout.clone()))
}

#[test]
fn a_workers_claude_window_holds_no_brief_on_its_command_line_and_no_pattern_of_its_words_matches_it(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open_with_staff(&rig, "chief", None, &[("worker", "worker")])?;
    let word = unique("claude");
    let tiers = project.tiers()?;
    let tier = tiers.get("worker").map_or("standard", String::as_str);
    // A turn that works for a while, so the window is there to be looked at.
    put(
        &rig,
        &project,
        tier,
        &format!("SLEEP 30 Reply with exactly: {word}"),
    )?;
    let id = format!("p{}-worker", project.id());
    rig.wait_for("the worker's window to open", secs(30), || {
        Ok(rig
            .open_frames()
            .iter()
            .any(|frame| frame["id"].as_str().is_some_and(|at| at.starts_with(&id))))
    })?;
    rig.wait_for("T-1 to be at work", secs(30), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    // Its brief went in: the window's record has it as the first thing it was told.
    rig.wait_for("the brief to be delivered", secs(30), || {
        let thread = project.task(1)?;
        Ok(thread["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|message| {
                message["kind"] == "task"
                    && message["state"] == "delivered"
                    && message["body"]
                        .as_str()
                        .is_some_and(|body| body.contains(&word))
            }))
    })?;
    let frames: Vec<Value> = rig
        .open_frames()
        .into_iter()
        .filter(|frame| frame["id"].as_str().is_some_and(|at| at.starts_with(&id)))
        .collect();
    assert_eq!(frames.len(), 1, "one window: {frames:?}");
    let argv = argv_of(&frames[0]);
    assert!(
        argv.iter().all(|arg| !arg.contains(&word)),
        "the window's command line holds its brief: {argv:?}"
    );
    // The system's own view of the window's process, what every process on the Mac reads,
    // and a pattern of the brief's words: `ps` and `pgrep` are Unix's.
    #[cfg(unix)]
    seen_by_the_system(&rig, &frames[0], &word)?;
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.close()?;
    Ok(())
}

/// What the system shows of the window `frame` opened: its process's command line holds
/// no `word`, and `pgrep -f` on it finds no process at all.
#[cfg(unix)]
fn seen_by_the_system(rig: &Rig, frame: &Value, word: &str) -> Outcome {
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
        "ps shows the brief on the window's command line: {command}"
    );
    // A wait or kill by a pattern of the brief's words finds no process at all.
    let (code, found) = run("pgrep", &["-f", word])?;
    assert_eq!(
        (code, found.trim()),
        (Some(1), ""),
        "a pattern of the brief's words matches a process"
    );
    Ok(())
}

/// The stand-in Codex is a script, so Unix's.
#[cfg(unix)]
#[test]
fn a_workers_codex_window_holds_no_brief_on_its_command_line_and_its_brief_reaches_it() -> Outcome {
    let rig = Rig::start(config_with_codex())?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "coder", "kind": "codex", "model": "fake" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))?;
    let project = Project::open_with_staff(&rig, "chief", None, &[("coder", "worker")])?;
    let word = unique("codex");
    let tiers = project.tiers()?;
    let tier = tiers.get("coder").map_or("standard", String::as_str);
    put(&rig, &project, tier, &format!("Reply with exactly: {word}"))?;
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let id = format!("p{}-coder", project.id());
    let frames: Vec<Value> = rig
        .open_frames()
        .into_iter()
        .filter(|frame| frame["id"].as_str().is_some_and(|at| at.starts_with(&id)))
        .collect();
    assert!(!frames.is_empty(), "the worker's window opened");
    for frame in &frames {
        let argv = argv_of(frame);
        assert!(
            argv.iter().any(|arg| arg == "codex-session"),
            "a Codex window under ConsensFlow's supervisor: {argv:?}"
        );
        assert!(
            argv.iter().all(|arg| !arg.contains(&word)),
            "the window's command line holds its brief: {argv:?}"
        );
    }
    let result = project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "result")
        .ok_or("no result reached the chief")?;
    assert!(
        result["body"]
            .as_str()
            .is_some_and(|body| body.contains(&word)),
        "the worker answered its brief: {result}"
    );
    rig.close()?;
    Ok(())
}
