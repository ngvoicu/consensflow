//! Live: a Muse member's window as the daemon opens one, on the real model. It is opt-in
//! (`cargo xtask live muse-window`): it spends the Muse account's allowance on a worker of
//! Muse's cheap model, `muse-spark-1.3-contributor` at `minimal` (contributor mode off; `--models
//! muse=<model>:<effort>` names another), and needs Muse Code installed and
//! logged in, so it is ignored unless asked for and is no part of `check` or of CI. On a
//! Mac and on Windows (MUS-20, T-792: Muse runs there as the rig's stand-in's child).
//!
//! The rig's cases (`tests/rig/muse_window.rs`) run the same windows on Muse's fake model,
//! which repeats each message after a delay: it never calls a tool, and its timing is no
//! model's. Here each case runs once on the real model (Gabriel, 2026-10-26: echo is never
//! the proof), in a rig of its own: a throwaway project, the windows' Muse settings and
//! sessions in the rig's home, the person's login reached through their keychain
//! ([`cf_e2e::muse::write_live_stand_in`]). One Muse session at a time; a run may use all
//! of the allowance: before each case it is asked (`muse usage --json`), and only on a
//! window or a week used up the run stops with the reset time. A failed case says what of
//! it was spent after the run ("Muse allowance after the run"), so a red an empty
//! allowance caused is told from a bug (T-1123). A case whose session Muse moved to
//! another model on its own (`model_reconfigure`) stops the run too (T-705's contributor
//! switch).
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
//! - `question`   the worker asks the chief with `cf ask` (it has no question tool: MUS-13),
//!   the answer comes back into its next turn, and its role text wins over the person's
//!   rules of another harness that say never to run `cf` (MUS-16)

use std::path::PathBuf;
use std::time::Duration;

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, agents, Harness, Log};
use cf_e2e::muse::members::{
    in_its_turn, last_resumed, message_with, pane_of, project, the_session, worker, WORKER,
};
use cf_e2e::muse::{
    self, deliveries, has, messages, moved_away, question_tools, records, shape, terminals,
};
use cf_e2e::process::own_var;
use cf_e2e::rig::{Config, Project, Rig};
use cf_e2e::{checkout, files, Error, Result};
use serde_json::{json, Value};

/// The stand-in for the chief's window, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The rig's Muse on Windows, which runs the machine's ([`muse::StandIn`]).
const MUSE_STAND_IN: &str = env!("CARGO_BIN_EXE_muse-stand-in");

/// The cases, in the order they run.
const CASES: [&str; 9] = [
    "result", "early", "tell", "steer", "cut", "after", "restart", "rest", "question",
];

/// A brief whose turn runs a shell command a while before it answers: a turn a stop,
/// a tell or a quit can find at work after the model's first output.
const LONG: &str =
    "Run the shell command `sleep 25` and wait for it to finish, then reply with exactly: T1_DONE";

/// The question case's brief: a question only the chief can answer. The stand-in chief
/// answers a question that ends `REPLY <words>` with those words (`fake-agent`).
const QUESTION: &str = "Before you do anything else, ask the chief which colour to use, red or blue, and end your question with the words REPLY blue. Do not pick a colour yourself: wait for the chief's answer. Once you have it, reply with exactly one line: COLOUR=<the answer>";

/// The person's rules of another harness, which Muse reads as theirs: `$CODEX_HOME/AGENTS.md`
/// where there is no `~/.claude/CLAUDE.md` (Muse 1.4.14, T-790's echo probe), loaded as a
/// `rules-file` reminder in every posture. One rule a member can keep, which proves they are
/// read.
///
/// Until T-1126 they also said "Never run the `cf` command, for any reason. When you need to
/// know something, choose the answer yourself.", to pit the person's rules against the role's
/// `cf ask`. On the cheap Muse model (`muse-spark-1.3-contributor`, minimal) the person's rule
/// won 3 runs of 18 over the Mac and zeewin2 (T-954, T-1124): the worker asked in words as its
/// turn's last message and never ran `cf ask`, so its task ended with the question as its
/// result. That is the model's choice between two instructions, not a defect of ConsensFlow's,
/// and a case that loses one run in six proves nothing either way; it is left out.
const FOREIGN_RULES: &str = "# My rules\n\n- End every reply with the word FOREIGN_SEEN.\n";

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// What a live case is run with: the machine's Muse, the person's home, and the worker's
/// agent (its model and effort, as the run names them), which the log says it ran on.
struct Live {
    install: muse::Installed,
    home: PathBuf,
    agent: Value,
    model: String,
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
        // The kit's stop, which every rig that seats Muse asks as it starts too.
        muse::allowance_left(&usage).map_err(Error::Daemon)
    }

    /// What of the allowance is spent now, asked as [`Live::allowance`] asks it (`muse
    /// usage --json` on a copy of the login), for a failed case's report: a run may start
    /// near the end and run out midway, so a red an empty allowance caused is told from a
    /// bug (T-1123). Why it could not be asked, where it could not.
    fn allowance_after(&self) -> String {
        let asked = tempfile::tempdir()
            .map_err(|source| Error::File {
                action: "make",
                path: PathBuf::from("a temporary folder"),
                source,
            })
            .and_then(|folder| {
                muse::seed_live_home(folder.path(), &self.home)?;
                muse::usage(&self.install, &self.home, &folder.path().join(".config"))
            });
        match asked {
            Ok(usage) => match muse::used_percent(&usage) {
                Some((window, week)) => format!("window {window}%, week {week}%"),
                None => format!("no allowance: {usage}"),
            },
            Err(why) => why.to_string(),
        }
    }
}

/// Every session of the rig, held to the model `launched` the worker was launched on:
/// one Muse moved to another model on its own stops the run.
fn still_on(rig: &Rig, launched: &str) -> Result {
    for (id, log) in rig.muse_sessions() {
        if let Some(model) = moved_away(&records(&log), launched) {
            return Err(Error::Daemon(format!(
                "Muse moved session {id} from {launched} to {model} on its own: the run stops (MUS-24)"
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
    let project = project(&rig, &live.agent)?;
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
    still_on(&rig, &live.model)?;
    let said = shape_of(&rig);
    rig.close()?;
    Ok(said)
}

fn early_case(live: &Live) -> Result<String> {
    let rig = Rig::start(live.config())?;
    let project = project(&rig, &live.agent)?;
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
    // The daemon reads the window at rest, taken back by its own stop, and puts the brief
    // back in its queue for the resume.
    let worker = worker(&project)?;
    let mut brief = Value::Null;
    rig.wait_for("the brief to be taken back", secs(60), || {
        brief = message_with(&project, &worker, "T1_DONE")?.unwrap_or_default();
        Ok(brief["state"] == "queued")
    })?;
    // Read once the daemon took the brief back: Muse writes the run's end a moment before
    // its `run_retracted` (T-931's live run read the log between the two).
    let (_, log) = the_session(&rig);
    let retracted = has(&log, "run/run_retracted");
    let first = terminals(&log);
    let resumed = rig.page("task.resume", json!({ "project": project.id(), "task": 1 }))?;
    held(resumed["ok"] == true, format!("{resumed}"))?;
    done(&rig, &project, 1, secs(240))?;
    let (_, log) = the_session(&rig);
    let words = messages(&log);
    let alone = words
        .last()
        .is_some_and(|last| last.matches("Reply with exactly: T1_DONE").count() == 1);
    still_on(&rig, &live.model)?;
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
    let project = project(&rig, &live.agent)?;
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
    rig.tell(project.id(), "CF task pause T-1 :: Also say HANDED_OVER.")?;
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
    still_on(&rig, &live.model)?;
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
    let project = project(&rig, &live.agent)?;
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
    still_on(&rig, &live.model)?;
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
    let project = project(&rig, &live.agent)?;
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
    still_on(&rig, &live.model)?;
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
    let project = project(&rig, &live.agent)?;
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
    // The chief accepts T-1 once it has its result, and a follow-up of T-1 waits for that
    // acceptance (T-1118). Its stand-in runs a message's DISPATCH lines and then nothing
    // else: the accept is a message of its own.
    rig.wait_for("the chief to be given T-1's result", secs(120), || {
        Ok(project
            .inbox("chief")?
            .iter()
            .any(|message| message["kind"] == "result" && message["state"] == "delivered"))
    })?;
    rig.tell(id, "CF task accept T-1")?;
    rig.tell(
        id,
        "DISPATCH --tier standard --after T-1 Reply with exactly: T2_DONE",
    )?;
    done(&rig, &project, 2, secs(240))?;
    let (same, log) = the_session(&rig);
    let words = messages(&log);
    still_on(&rig, &live.model)?;
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
    let project = project(&rig, &live.agent)?;
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
    still_on(&rig, &live.model)?;
    let said = format!(
        "T-1 {state} after the human's Escape, its pause {}, the daemon's Escapes {}, turns {:?}, retracted {}\n{}",
        paused["ok"], rig.keys_pressed(&id, &[27]), terminals(&log), has(&log, "run/run_retracted"), shape_of(&rig)
    );
    rig.close()?;
    held(said.contains("the daemon's Escapes 0"), said.clone())?;
    Ok(said)
}

fn question_case(live: &Live) -> Result<String> {
    let foreign = tempfile::tempdir().map_err(|source| Error::File {
        action: "make",
        path: PathBuf::from("a temporary folder"),
        source,
    })?;
    files::write(&foreign.path().join("AGENTS.md"), FOREIGN_RULES)?;
    let config = live
        .config()
        .var("CODEX_HOME", foreign.path().display().to_string());
    let rig = Rig::start(config)?;
    let project = project(&rig, &live.agent)?;
    dispatch(&rig, &project, QUESTION)?;
    let chiefs = |project: &Project<'_>, kind: &str| -> Result<Value> {
        Ok(project
            .inbox("chief")?
            .into_iter()
            .find(|message| message["kind"] == kind)
            .unwrap_or_default())
    };
    let asked = rig.wait_for(
        "the worker's question to reach the chief",
        secs(300),
        || Ok(!chiefs(&project, "question")?.is_null()),
    );
    if let Err(waited) = asked {
        // What the worker did instead, for the red to be read (T-954: one run of nine timed out
        // here and said nothing more; its rerun held).
        let did = what_the_worker_did(&rig, &project);
        rig.close()?;
        return Err(Error::Daemon(format!("{waited}\n{did}")));
    }
    done(&rig, &project, 1, secs(300))?;
    let question = chiefs(&project, "question")?;
    let worker = worker(&project)?;
    let answer = project
        .inbox(&worker)?
        .into_iter()
        .find(|message| message["kind"] == "answer")
        .unwrap_or_default();
    let result = chiefs(&project, "result")?["body"].to_string();
    let (_, log) = the_session(&rig);
    let offered = question_tools(&log);
    let dialog = has(&log, "run/user_input_prompt_requested");
    // The rules were read: Muse put them in the request's context.
    let read = log.iter().any(|record| {
        muse::kind(record) == "run/model_request_configured"
            && record.to_string().contains("FOREIGN_SEEN")
    });
    still_on(&rig, &live.model)?;
    // Muse reads the person's `~/.claude/CLAUDE.md` before `$CODEX_HOME/AGENTS.md`, and a
    // live window's home is the person's (T-985 P3-1 on T-790): where they have one, the
    // rules this case gives are never read, and that check is skipped, saying why.
    let owners = owners_claude_rules(&live.home);
    let skipped = owners.as_ref().map_or(String::new(), |path| {
        format!(
            "; the foreign rules' check is skipped: the person has {}, which Muse reads before $CODEX_HOME/AGENTS.md",
            path.display()
        )
    });
    let said = format!(
        "the question from {} ({:?}), the answer {} ({}), the result {result}, question tools offered {offered:?}, a dialog {dialog}, the foreign rules read {read}{skipped}, turns {:?}\n{}",
        question["sender"], question["body"], answer["state"], answer["body"], terminals(&log), shape_of(&rig)
    );
    rig.close()?;
    held(
        question["sender"]
            .as_str()
            .is_some_and(|sender| sender == worker || sender.starts_with(WORKER))
            && question["taskNumber"] == 1,
        format!("no board question from the worker: {said}"),
    )?;
    held(
        answer["state"] == "delivered",
        format!("the answer did not reach the worker: {said}"),
    )?;
    held(
        result.contains("COLOUR=blue"),
        format!("the result: {said}"),
    )?;
    held(
        !offered.is_empty() && offered.iter().all(Vec::is_empty) && !dialog,
        format!("Muse offered a question tool: {said}"),
    )?;
    if owners.is_none() {
        held(read, format!("the foreign rules were not read: {said}"))?;
    }
    Ok(said)
}

/// What the worker did, as a red case says it: its task as the board has it, the last lines its
/// window drew, and the shape of its session.
fn what_the_worker_did(rig: &Rig, project: &Project<'_>) -> String {
    let task = project.task(1).map_or_else(
        |failed| failed.to_string(),
        |task| {
            let messages: Vec<String> = task["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|message| {
                    format!(
                        "{} {} from {}: {}",
                        message["kind"],
                        message["state"],
                        message["sender"],
                        live::cut(message["body"].as_str().unwrap_or_default(), 300)
                    )
                })
                .collect();
            format!("T-1 {}; {}", task["state"], messages.join(" | "))
        },
    );
    let read = rig.muse_sessions().iter().any(|(_, log)| {
        records(log).iter().any(|record| {
            muse::kind(record) == "run/model_request_configured"
                && record.to_string().contains("FOREIGN_SEEN")
        })
    });
    let screen = pane_of(project)
        .ok()
        .and_then(|pane| pane["id"].as_str().map(|id| rig.screen(id)))
        .map_or_else(String::new, |screen| live::last_lines(&screen));
    format!(
        "the task: {task}\nthe foreign rules were read: {read}\nits window last drew:\n{screen}\nits session: {}",
        shape_of(rig)
    )
}

/// The person's own Claude Code rules in the home `home`, which Muse reads as theirs before
/// any other harness's: none where they have none.
fn owners_claude_rules(home: &std::path::Path) -> Option<PathBuf> {
    Some(home.join(".claude").join("CLAUDE.md")).filter(|path| path.is_file())
}

#[test]
#[ignore = "opt-in: spends the Muse account's allowance; cargo xtask live muse-window"]
fn each_case_of_a_muse_members_window_holds_on_the_real_model() -> Result {
    let log = Log::begin("live muse-window")?;
    let install = muse::installed()
        .map_err(Error::Daemon)?
        .through(MUSE_STAND_IN);
    let home = muse::real_home().map_err(Error::Daemon)?;
    let agent = agents::agent(Harness::Muse);
    log.launched(&agents::row(Harness::Muse, "standard"));
    let model = agents::model(Harness::Muse).name;
    let live = Live {
        install,
        home,
        agent,
        model,
    };
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
            "question" => question_case(&live),
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
                let after = format!("Muse allowance after the run: {}", live.allowance_after());
                log.block(
                    &format!("{case}: FAILED (allowance before: {before})"),
                    &format!("{failed}\n{after}"),
                );
                checks.push(Check::failed("muse", case, format!("{after}\n{failed}")));
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

#[cfg(test)]
mod tests {
    use super::FOREIGN_RULES;

    /// The person's rules the question case gives Muse prove they are read (a word a member
    /// keeps) and pit nothing against the role's `cf ask`: a rule that forbids `cf` lost the
    /// role 3 runs of 18 on the cheap model (T-1124), the model's choice and no defect of
    /// ConsensFlow's.
    #[test]
    fn the_foreign_rules_ask_for_a_word_a_member_keeps_and_forbid_no_cf() {
        assert!(FOREIGN_RULES.contains("FOREIGN_SEEN"));
        assert!(!FOREIGN_RULES.contains("`cf`"), "{FOREIGN_RULES}");
        assert!(
            !FOREIGN_RULES.to_lowercase().contains("never run"),
            "{FOREIGN_RULES}"
        );
    }
}
