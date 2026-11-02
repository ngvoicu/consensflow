//! **Live: Muse Code's background work holds its turn's task, and lets it go when it
//! ends** (MUS-07, T-789; Gabriel, 2026-10-26: echo and fixtures are never the proof).
//!
//!     cargo xtask live muse-background [--keep]
//!
//! One real turn on `muse-spark-1.3` at `minimal` (contributor mode off), in Muse's own
//! TUI in a pty, as a member's window is: the model is asked to start a command that
//! sleeps 40 s in the background and end its turn at once. The case reads Muse's own
//! log of the session as ConsensFlow does (the harness code's own reader, asked through
//! `harness-ask`: `cf_e2e::live::ask`), every half second, and passes when it saw what
//! holds a task and then lets it go:
//! - the turn settled with the command running: `settled`, a job pending;
//! - later, the command's end taken into the run it woke, and that run settled with
//!   nothing pending.
//!
//! No board task is opened: ConsensFlow opens no Muse window yet (MUS-08). What this
//! proves is the reading the engine holds a task by (`Pending`), on a real model's
//! record; the hold itself is the engine's, proven for every harness by its own tests.
//!
//! The rules it keeps: the binary is run directly (`muse-bin-<.muse-version>`, with
//! `MUSE_NO_AUTO_UPDATE=1`), never the launcher; on the owner's login (the keychain,
//! through the real `HOME`) with config, data, cache, runtime and temporary folders of
//! its own (a copy of `auth.json`, which holds no secret, and of the feature gates);
//! personal rules and skills, telemetry, session messaging and reminders off; a
//! throwaway project. It refuses to start while another test runs a Muse (one live
//! session at a time; the owner's own Muse in the home is no test's), or when
//! `muse usage --json` says 90 % of a window is spent, and stops at once on any model
//! but the one it launched (Muse moves a session to a contributor model by itself after
//! a refusal: T-705). The owner's `settings.json`, `trust.json` and install files are
//! fingerprinted before and after and must not change. `--keep` leaves the run's home
//! where it is, for `cargo xtask scrub-muse-session` to make a fixture of its log.

#[path = "live_muse/tui.rs"]
mod tui;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::live::{ask, Log};
use serde_json::json;

use tui::{
    another_model, binary, environment, other_tests_muse, owner_files, session_log, set_up, usage,
    Outcome, Window,
};

/// The command the model starts: one that outlives its turn by far.
const COMMAND: &str = "python3 -c \"import time; time.sleep(40); print('LIVE_BG_DONE')\"";

/// How long the case waits for the whole of it.
const LIMIT: Duration = Duration::from_secs(240);

/// What one look read: the turn's state, the work pending, and how many turns had ended.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    state: String,
    jobs: u64,
    agents: bool,
    ended: usize,
}

impl Seen {
    fn settled(&self) -> bool {
        self.state == "settled"
    }
}

/// What the harness code's reader reads of the session `id` now, in the run's folders.
fn look(id: &str, env: &BTreeMap<String, String>, ended: usize) -> Result<Option<Seen>, String> {
    let answer =
        ask::question(&json!({ "op": "records", "kind": "muse", "session": id, "env": env }))
            .map_err(|failed| failed.to_string())?;
    let reading = &answer["reading"];
    if reading.get("unknown").is_some() {
        return Ok(None);
    }
    Ok(Some(Seen {
        state: reading["settlement"]["state"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        jobs: reading["jobsPending"].as_u64().unwrap_or(0),
        agents: reading["agentsPending"] == true,
        ended,
    }))
}

#[test]
#[ignore = "live: one real Muse turn on the owner's login (cargo xtask live muse-background)"]
fn a_command_a_muse_turn_leaves_running_holds_the_task_until_it_ends() -> Outcome {
    // Its one window runs the model the owner chose for Muse's live runs, never the
    // shared models of `--models`, which no Muse window is asked for yet.
    let log = Log::own_agents(
        "live muse-background",
        "it runs Muse on muse-spark-1.3 at minimal",
    )
    .map_err(|failed| failed.to_string())?;
    let binary = binary()?;
    // The harness code is built before anything is spent.
    ask::prepare().map_err(|failed| failed.to_string())?;
    let others = other_tests_muse();
    if !others.is_empty() {
        return Err(format!(
            "another test runs Muse now (pids {others:?}): one live session at a time"
        ));
    }
    let root = tempfile::Builder::new()
        .prefix("muse-rec-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let root_path = root.path().to_path_buf();
    set_up(&root_path)?;
    let env = environment(&root_path);
    let spent = usage(&binary, &env, &root_path, &log)?;
    if spent.is_empty() {
        return Err(
            "muse usage --json said nothing spent: the 90 % stop cannot be kept".to_owned(),
        );
    }
    if spent.iter().any(|spent| *spent >= 90.0) {
        return Err(format!("90 % of a window is spent ({spent:?}): stopped"));
    }
    let owner_before = owner_files();
    let mut window = Window::open(&binary, &env, &root_path, &["--yolo"])?;
    let outcome = watch(&mut window, &root_path, &env, &log);
    let tail = window.tail();
    window.close();
    let after = usage(&binary, &env, &root_path, &log);
    let owner_after = owner_files();
    if cf_e2e::live::keep_requested() {
        let kept = root.keep();
        log.say(format!("kept the run's home: {}", kept.display()));
    }
    if owner_before != owner_after {
        return Err(format!(
            "the owner's Muse files changed: {owner_before:?} → {owner_after:?}"
        ));
    }
    log.say(format!("usage after: {after:?}"));
    outcome.map_err(|failed| format!("{failed}\nthe window's last bytes:\n{tail}"))
}

/// Pastes the turn's words and reads the record until what holds the task and lets it
/// go has been seen, or the limit passes, or the record names another model.
fn watch(window: &mut Window, root: &Path, env: &BTreeMap<String, String>, log: &Log) -> Outcome {
    let started = Instant::now();
    // The window is up once its session's log says so.
    let id = loop {
        if started.elapsed() > Duration::from_secs(40) {
            return Err("the window wrote no session log in 40 s".to_owned());
        }
        if let Some((id, file)) = session_log(root) {
            if fs::read_to_string(&file).is_ok_and(|text| text.contains("\"session.started\"")) {
                break id;
            }
        }
        thread::sleep(Duration::from_millis(250));
    };
    thread::sleep(Duration::from_secs(2));
    window.paste(&format!(
        "Use the bash tool once to start this exact command so that it keeps running in the \
         background, and do not wait for it: {COMMAND} . Then reply with the single word \
         STARTED and end your turn at once. Do not use bash_input, do not poll it and do not \
         sleep. When you are told the command finished, reply with the single word FINISHED."
    ));
    let mut seen: Vec<(f64, Seen)> = Vec::new();
    let mut held = false;
    while started.elapsed() < LIMIT {
        thread::sleep(Duration::from_millis(500));
        let Some((_, file)) = session_log(root) else {
            continue;
        };
        let text = fs::read_to_string(&file).unwrap_or_default();
        if let Some(model) = text.lines().find_map(another_model) {
            return Err(format!("the record names another model, {model}: stopped"));
        }
        let ended = text.matches("\"kind\":\"terminal\",\"terminal\"").count();
        let Some(now) = look(&id, env, ended)? else {
            continue;
        };
        if seen.last().is_none_or(|(_, last)| *last != now) {
            let at = started.elapsed().as_secs_f64();
            log.say(format!("{at:6.1}s {now:?}"));
            seen.push((at, now.clone()));
        }
        if now.settled() && now.jobs > 0 {
            held = true;
        }
        if held && now.settled() && now.jobs == 0 && !now.agents && now.ended >= 2 {
            log.say(format!("held for {:.1}s, then let go", held_for(&seen)));
            return Ok(());
        }
    }
    if !held {
        return Err(format!(
            "no reading of a settled turn with its command pending in {LIMIT:?}: the model may \
             have waited on it within its turn, which proves nothing; seen: {seen:?}"
        ));
    }
    Err(format!(
        "the command's end never let the task go in {LIMIT:?}; seen: {seen:?}"
    ))
}

/// How long the readings held: from the first settled one with a job pending to the next
/// with none.
fn held_for(seen: &[(f64, Seen)]) -> f64 {
    let Some((first, _)) = seen
        .iter()
        .find(|(_, seen)| seen.settled() && seen.jobs > 0)
    else {
        return 0.0;
    };
    let after = seen
        .iter()
        .find(|(at, seen)| at > first && seen.jobs == 0)
        .map_or(*first, |(at, _)| *at);
    after - first
}
