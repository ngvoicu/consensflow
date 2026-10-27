//! Live: a Muse member's window as the daemon opens one, on the real model. It is opt-in
//! (`cargo xtask live muse-window`): it spends the Muse account's allowance on a worker of
//! `muse-spark-1.3` at `minimal` (contributor mode off), and needs Muse Code installed and
//! logged in, so it is ignored unless asked for and is no part of `check` or of CI. Mac
//! only for now: no Windows machine has Muse yet (MUS-20).
//!
//! The rig's cases (`tests/rig/muse_window.rs`) run the same windows on Muse's fake model,
//! which repeats each message after a delay: it never calls a tool, and its timing is no
//! model's. Here each case runs once on the real model (Gabriel, 2026-10-26: echo is never
//! the proof), in a rig of its own: a throwaway project, the windows' Muse settings and
//! sessions in the rig's home, the person's login reached through their keychain
//! ([`cf_e2e::muse::write_live_stand_in`]). One Muse session at a time; before each case
//! the allowance is asked (`muse usage --json`), and at 90% of its window or its week the
//! run stops with the reset time. A case whose session Muse moved to another model on its
//! own (`model_reconfigure`) stops the run too (T-705's contributor switch).
//!
//! What a case reads is what the rig's do (the board, the worker's inbox, the keys the
//! daemon pressed, the session's log), and each says the shape of its session, so that
//! where the real model writes what echo does not (a tool call, a turn that answers a
//! steer inside itself) it is said. `--case` picks cases by name:
//!
//! - `result`     a task from its brief to its result
//! - `early`      a pause the moment the turn begins: an Escape before the first token,
//!   the brief taken back (`run_retracted`, `took_back`), and the resume pasted alone
//! - `tell`       the chief's tell into a turn running a shell command: a stop after output
//! - `steer`      words that land in a turn at work (`active_run_delivery`)
//! - `cut`        the app quits in a turn; the resume pastes the note once the window is up
//! - `after`      a task put after another, on the same conversation (`muse resume`)
//! - `restart`    the same after the app quit at rest
//! - `rest`       a stop that finds the window at rest (the human's Escape first)

use std::path::PathBuf;
use std::time::Duration;

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, Log};
use cf_e2e::muse::members::{
    in_its_turn, last_resumed, message_with, pane_of, project, the_session, worker, MODEL,
};
use cf_e2e::muse::{self, deliveries, has, messages, moved_away, records, shape, terminals};
use cf_e2e::process::own_var;
use cf_e2e::rig::{Config, Project, Rig};
use cf_e2e::{checkout, Error, Result};
use serde_json::{json, Value};

/// The stand-in for the chief's window, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The cases, in the order they run.
const CASES: [&str; 8] = [
    "result", "early", "tell", "steer", "cut", "after", "restart", "rest",
];

/// The most of its window or week of allowance a run spends: at this, it stops.
const STOP_AT_PERCENT: u64 = 90;

/// A brief whose turn runs a shell command a while before it answers: a turn a stop,
/// a tell or a quit can find at work after the model's first output.
const LONG: &str =
    "Run the shell command `sleep 25` and wait for it to finish, then reply with exactly: T1_DONE";

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// What a live case is run with: the machine's Muse and the person's home.
struct Live {
    install: muse::Installed,
    home: PathBuf,
}

impl Live {
    /// A rig of the case's own, its Muse windows live.
    fn config(&self) -> Config {
        let mut config = Config::new(FAKE_AGENT)
            .judged_by_outcome()
            .muse_live(self.install.clone(), &self.home)
            .var("LANG", "en_US.UTF-8")
            .reports_in(checkout::path("app/src-tauri/target/live-reports"));
        // The user's name, which Muse asks the keychain by.
        for name in ["USER", "LOGNAME"] {
            if let Some(value) = own_var(name) {
                config = config.var(name, value);
            }
        }
        config
    }

    /// The allowance now, or the end of the run where it is spent past the stop.
    fn allowance(&self, log: &Log) -> Result<String> {
        let folder = tempfile::tempdir().map_err(|source| Error::File {
            action: "make",
            path: PathBuf::from("a temporary folder"),
            source,
        })?;
        muse::seed_live_home(folder.path(), &self.home)?;
        let usage = muse::usage(&self.install, &self.home, &folder.path().join(".config"))?;
        let said = usage["allowance"].to_string();
        log.say(format!("allowance: {said}"));
        match muse::used_percent(&usage) {
            Some((window, weekly)) if window >= STOP_AT_PERCENT || weekly >= STOP_AT_PERCENT => {
                Err(Error::Daemon(format!(
                    "the Muse allowance is at {window}% of its window and {weekly}% of its week: the run stops (resets: {} and {})",
                    usage["allowance"]["window"]["resetsAtMs"], usage["allowance"]["weekly"]["resetsAtMs"]
                )))
            }
            Some((window, weekly)) => Ok(format!("{window}% of the window, {weekly}% of the week")),
            None => Err(Error::Daemon(format!("Muse says no allowance: {usage}"))),
        }
    }
}

/// Every session of the rig, held to the model the worker was launched on: one Muse moved
/// to another model on its own stops the run.
fn still_on_its_model(rig: &Rig) -> Result {
    for (id, log) in rig.muse_sessions() {
        if let Some(model) = moved_away(&records(&log), MODEL) {
            return Err(Error::Daemon(format!(
                "Muse moved session {id} from {MODEL} to {model} on its own: the run stops (MUS-24)"
            )));
        }
    }
    Ok(())
}

/// The shape of the rig's session, as a case says it.
fn shape_of(rig: &Rig) -> String {
    rig.muse_sessions()
        .iter()
        .map(|(id, log)| format!("{id}: {}", shape(&records(log)).join(" ")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `condition` holds, or the error that says `what` did not.
fn held(condition: bool, what: impl Into<String>) -> Result {
    if condition {
        Ok(())
    } else {
        Err(Error::Daemon(what.into()))
    }
}

fn dispatch(rig: &Rig, project: &Project<'_>, brief: &str) -> Result {
    rig.tell(project.id(), &format!("DISPATCH --tier standard {brief}"))
}

fn done(rig: &Rig, project: &Project<'_>, number: i64, within: Duration) -> Result {
    rig.wait_for(&format!("T-{number} to be answered"), within, || {
        Ok(project.task(number)?["state"] == "done")
    })
}

fn result_case(live: &Live) -> Result<String> {
    let rig = Rig::start(live.config())?;
    let project = project(&rig)?;
    dispatch(&rig, &project, "Reply with exactly: T1_DONE")?;
    done(&rig, &project, 1, secs(240))?;
    let task = project.task(1)?;
    held(
        task.to_string().contains("T1_DONE"),
        format!("T-1's result: {task}"),
    )?;
    let (_, log) = the_session(&rig);
    held(
        terminals(&log) == ["completed"],
        format!("{:?}", terminals(&log)),
    )?;
    still_on_its_model(&rig)?;
    let said = shape_of(&rig);
    rig.close()?;
    Ok(said)
}

fn early_case(live: &Live) -> Result<String> {
    let rig = Rig::start(live.config())?;
    let project = project(&rig)?;
    dispatch(&rig, &project, "Reply with exactly: T1_DONE")?;
    // The pause goes in the moment the log has the turn begun, before its first token.
    rig.wait_for("the turn to begin", secs(240), || {
        Ok(rig
            .muse_sessions()
            .first()
            .is_some_and(|(_, log)| has(&records(log), "run/started")))
    })?;
    let paused = rig.page("task.pause", json!({ "project": project.id(), "task": 1 }))?;
    held(paused["ok"] == true, format!("{paused}"))?;
    rig.wait_for("T-1 to be paused", secs(60), || {
        Ok(project.task(1)?["state"] == "paused")
    })?;
    rig.wait_for("the stopped turn's end", secs(60), || {
        Ok(!terminals(&the_session(&rig).1).is_empty())
    })?;
    let (_, log) = the_session(&rig);
    let retracted = has(&log, "run/run_retracted");
    let first = terminals(&log);
    // The daemon reads the window at rest, taken back by its own stop, and puts the brief
    // back in its queue for the resume.
    let worker = worker(&project)?;
    let mut brief = Value::Null;
    rig.wait_for("the brief to be taken back", secs(60), || {
        brief = message_with(&project, &worker, "T1_DONE")?.unwrap_or_default();
        Ok(brief["state"] == "queued")
    })?;
    let resumed = rig.page("task.resume", json!({ "project": project.id(), "task": 1 }))?;
    held(resumed["ok"] == true, format!("{resumed}"))?;
    done(&rig, &project, 1, secs(240))?;
    let (_, log) = the_session(&rig);
    let words = messages(&log);
    let alone = words
        .last()
        .is_some_and(|last| last.matches("Reply with exactly: T1_DONE").count() == 1);
    still_on_its_model(&rig)?;
    let id = pane_of(&project)?["id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let said = format!(
        "first turn ended {first:?}, run_retracted {retracted}, the brief {} after the pause ({}), Escapes {}, Control-Cs {}, the resumed brief alone {alone}\n{}",
        brief["state"], brief["reason"], rig.keys_pressed(&id, &[27]), rig.keys_pressed(&id, &[3]), shape_of(&rig)
    );
    rig.close()?;
    held(retracted, format!("no retract on the real model: {said}"))?;
    held(
        alone,
        format!("the resumed brief went in beside the one put back: {words:?}"),
    )?;
    Ok(said)
}

fn tell_case(live: &Live) -> Result<String> {
    let rig = Rig::start(live.config())?;
    let project = project(&rig)?;
    dispatch(&rig, &project, LONG)?;
    in_its_turn(&rig, &project, 1, secs(240))?;
    // Into the shell command: the stop comes after the model's first output.
    rig.wait_for("the shell command to run", secs(120), || {
        Ok(the_session(&rig)
            .1
            .iter()
            .any(|record| muse::kind(record) == "run/assistant_tool_calls_committed"))
    })?;
    let worker = worker(&project)?;
    rig.tell(project.id(), "CF tell T-1 :: Also say HANDED_OVER.")?;
    let mut told = None;
    rig.wait_for("the tell to be received", secs(180), || {
        told = message_with(&project, &worker, "HANDED_OVER")?
            .filter(|message| message["state"] == "delivered");
        Ok(told.is_some())
    })?;
    let told = told.unwrap_or_default();
    rig.wait_for("the question's turn to end", secs(240), || {
        Ok(terminals(&the_session(&rig).1).len() >= 2)
    })?;
    let (_, log) = the_session(&rig);
    let words = messages(&log);
    let alone = words
        .last()
        .is_some_and(|last| last.contains("HANDED_OVER") && !last.contains("sleep 25"));
    still_on_its_model(&rig)?;
    let said = format!(
        "the tell given {} time(s), turns {:?}, retracted {}, deliveries {:?}, the question alone {alone}\n{}",
        told["attempts"], terminals(&log), has(&log, "run/run_retracted"), deliveries(&log), shape_of(&rig)
    );
    rig.close()?;
    held(told["attempts"] == 1, format!("the tell: {told}"))?;
    held(
        alone,
        format!("the question went in beside other words: {words:?}"),
    )?;
    Ok(said)
}

fn steer_case(live: &Live) -> Result<String> {
    let rig = Rig::start(live.config())?;
    let project = project(&rig)?;
    dispatch(&rig, &project, LONG)?;
    in_its_turn(&rig, &project, 1, secs(240))?;
    rig.wait_for("the shell command to run", secs(120), || {
        Ok(the_session(&rig)
            .1
            .iter()
            .any(|record| muse::kind(record) == "run/assistant_tool_calls_committed"))
    })?;
    let pane = pane_of(&project)?;
    // A paste, then its Enter a moment later, as the pane host sends a paste of the daemon's.
    for bytes in [
        "\u{1b}[200~Also put the word MID_TURN in your reply.\u{1b}[201~".as_bytes(),
        b"\r",
    ] {
        let answer = rig.host(
            "pane.input",
            json!({ "id": pane["id"], "generation": pane["generation"], "bytes": bytes }),
        )?;
        held(answer["ok"] == true, format!("{answer}"))?;
        std::thread::sleep(Duration::from_millis(700));
    }
    let id = pane["id"].as_str().unwrap_or_default().to_owned();
    let taken = rig.wait_for("Muse to take the words in", secs(60), || {
        Ok(deliveries(&the_session(&rig).1).len() >= 2)
    });
    if let Err(failed) = taken {
        let said = format!(
            "{failed}\nthe window showed:\n{}\n{}",
            rig.screen(&id),
            shape_of(&rig)
        );
        rig.close()?;
        return Err(Error::Daemon(said));
    }
    let at_once = deliveries(&the_session(&rig).1);
    done(&rig, &project, 1, secs(300))?;
    let (_, log) = the_session(&rig);
    still_on_its_model(&rig)?;
    let said = format!(
        "deliveries {at_once:?}, turns {:?}, the result {}\n{}",
        terminals(&log),
        project.task(1)?["result"],
        shape_of(&rig)
    );
    rig.close()?;
    held(
        at_once
            .get(1)
            .is_some_and(|second| second == "active_run_delivery"),
        format!("not a hand-over to the turn at work: {said}"),
    )?;
    Ok(said)
}

fn cut_case(live: &Live) -> Result<String> {
    let config = live.config();
    let rig = Rig::start(config.clone())?;
    let project = project(&rig)?;
    let id = project.id();
    dispatch(&rig, &project, LONG)?;
    in_its_turn(&rig, &project, 1, secs(240))?;
    rig.wait_for("the shell command to run", secs(120), || {
        Ok(the_session(&rig)
            .1
            .iter()
            .any(|record| muse::kind(record) == "run/assistant_tool_calls_committed"))
    })?;
    let worker = worker(&project)?;
    let (session, _) = the_session(&rig);
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(config.existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    held(resumed["ok"] == true, format!("{resumed}"))?;
    let project = Project::new(&rig, id);
    let mut received = None;
    rig.wait_for("the worker's note to be received", secs(240), || {
        received = message_with(&project, &worker, "Go on where you stopped.")?
            .filter(|message| message["state"] == "delivered");
        Ok(received.is_some())
    })?;
    done(&rig, &project, 1, secs(300))?;
    let (same, log) = the_session(&rig);
    still_on_its_model(&rig)?;
    let said = format!(
        "the note given {} time(s), resumed on the session {}, turns {:?}\n{}",
        received.unwrap_or_default()["attempts"],
        last_resumed(&rig, &session),
        terminals(&log),
        shape_of(&rig)
    );
    rig.close()?;
    held(
        same == session && said.contains("given 1 time"),
        said.clone(),
    )?;
    Ok(said)
}

fn after_case(live: &Live, restart: bool) -> Result<String> {
    let config = live.config();
    let rig = Rig::start(config.clone())?;
    let project = project(&rig)?;
    let id = project.id();
    dispatch(&rig, &project, "Reply with exactly: T1_DONE")?;
    done(&rig, &project, 1, secs(240))?;
    let (session, _) = the_session(&rig);
    let rig = if restart {
        let handle = worker(&project)?;
        rig.wait_for("the worker's window to close", secs(60), || {
            Ok(project.lane(&handle)?["pane"].is_null())
        })?;
        let root = rig.close_keeping_the_root()?;
        let rig = Rig::start(config.existing_root(root))?;
        let resumed = rig.page("project.resume", json!({ "project": id }))?;
        held(resumed["ok"] == true, format!("{resumed}"))?;
        rig
    } else {
        rig
    };
    let project = Project::new(&rig, id);
    rig.tell(
        id,
        "CF task accept T-1\nDISPATCH --tier standard --after T-1 Reply with exactly: T2_DONE",
    )?;
    done(&rig, &project, 2, secs(240))?;
    let (same, log) = the_session(&rig);
    let words = messages(&log);
    still_on_its_model(&rig)?;
    let said = format!(
        "resumed on the session {}, messages {}, turns {:?}, T-2's result {}\n{}",
        last_resumed(&rig, &session),
        words.len(),
        terminals(&log),
        project.task(2)?["result"],
        shape_of(&rig)
    );
    rig.close()?;
    held(
        same == session && words.last().is_some_and(|last| last.contains("T2_DONE")),
        said.clone(),
    )?;
    Ok(said)
}

fn rest_case(live: &Live) -> Result<String> {
    let rig = Rig::start(live.config())?;
    let project = project(&rig)?;
    dispatch(&rig, &project, "Reply with exactly: T1_DONE")?;
    rig.wait_for("the turn to begin", secs(240), || {
        Ok(rig
            .muse_sessions()
            .first()
            .is_some_and(|(_, log)| has(&records(log), "run/started")))
    })?;
    let pane = pane_of(&project)?;
    rig.types(&pane, &[27])?;
    rig.wait_for("the stopped turn's end", secs(60), || {
        Ok(!terminals(&the_session(&rig).1).is_empty())
    })?;
    std::thread::sleep(secs(3));
    let state = project.task(1)?["state"].clone();
    let paused = rig.page("task.pause", json!({ "project": project.id(), "task": 1 }))?;
    std::thread::sleep(secs(8));
    let id = pane["id"].as_str().unwrap_or_default().to_owned();
    let (_, log) = the_session(&rig);
    still_on_its_model(&rig)?;
    let said = format!(
        "T-1 {state} after the human's Escape, its pause {}, the daemon's Escapes {}, turns {:?}, retracted {}\n{}",
        paused["ok"], rig.keys_pressed(&id, &[27]), terminals(&log), has(&log, "run/run_retracted"), shape_of(&rig)
    );
    rig.close()?;
    held(said.contains("the daemon's Escapes 0"), said.clone())?;
    Ok(said)
}

#[test]
#[ignore = "opt-in: spends the Muse account's allowance; cargo xtask live muse-window"]
fn each_case_of_a_muse_members_window_holds_on_the_real_model() -> Result {
    let log = Log::own_agents(
        "live muse-window",
        "its Muse worker runs on muse-spark-1.3 at minimal, contributor mode off",
    )?;
    let install = muse::installed().map_err(Error::Daemon)?;
    let home = PathBuf::from(own_var("HOME").ok_or_else(|| Error::Daemon("no home".to_owned()))?);
    let live = Live { install, home };
    let asked = live::list("case");
    let mut checks = Vec::new();
    for case in CASES
        .into_iter()
        .filter(|case| asked.is_empty() || asked.iter().any(|named| named == case))
    {
        let before = match live.allowance(&log) {
            Ok(before) => before,
            Err(stop) => {
                checks.push(Check::failed("muse", case, stop.to_string()));
                break;
            }
        };
        log.say(format!("== {case} (allowance before: {before})"));
        let ran = match case {
            "result" => result_case(&live),
            "early" => early_case(&live),
            "tell" => tell_case(&live),
            "steer" => steer_case(&live),
            "cut" => cut_case(&live),
            "after" => after_case(&live, false),
            "restart" => after_case(&live, true),
            _ => rest_case(&live),
        };
        let stop = ran
            .as_ref()
            .err()
            .is_some_and(|failed| failed.to_string().contains("the run stops"));
        match ran {
            Ok(said) => {
                log.block(&format!("{case}: held (allowance before: {before})"), &said);
                checks.push(Check::passed("muse", case, before));
            }
            Err(failed) => {
                log.block(
                    &format!("{case}: FAILED (allowance before: {before})"),
                    &failed.to_string(),
                );
                checks.push(Check::failed("muse", case, failed.to_string()));
            }
        }
        if stop {
            break;
        }
    }
    let after = live.allowance(&log).unwrap_or_else(|stop| stop.to_string());
    log.say(format!("allowance after the run: {after}"));
    report::conclude(&log, "what each case came to", &checks)
}
