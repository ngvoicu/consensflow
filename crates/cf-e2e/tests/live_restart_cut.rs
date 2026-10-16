//! Live: a window the app's quit cut in a turn, through the product. It is opt-in
//! (`cargo xtask live restart-cut`, `npm run live:restart-cut`): it spends the
//! machine's quota on a chief or a worker of each harness named, and needs each
//! one installed and logged in, so it is ignored unless asked for and is no part of
//! `check` or of CI.
//!
//! The human asks a window to run a long command; the app quits with the window
//! inside it (the daemon dies, then the pane host, which ends every window); the
//! app starts again on the same home. Nothing opens by itself: the project waits,
//! suspended, for the human's Resume, and the board shows its task as it was. The
//! Resume brings the window back on the conversation the quit cut, and the
//! daemon's first word to it is a note that says its turn was cut, once. The
//! window then goes on (a command it starts again is ended, for it is the long one),
//! and what is waiting for it, or its result, goes in as the usual rules say.
//!
//! - A chief: a real harness is the chief of a project on the native daemon, and
//!   its one member is the stand-in (no model). The chief reads idle once it has
//!   gone on, and a task given to the member then has its result delivered to it.
//! - A worker: the chief is the stand-in, and the member is a real harness, given
//!   a task whose brief is the long command. It is told, goes on, and its result
//!   reaches the chief.
//!
//! ```text
//! cargo xtask live restart-cut                     a Claude chief, and a Claude worker
//! cargo xtask live restart-cut --chief pi          Pi is the chief (only Claude and Pi can be)
//! cargo xtask live restart-cut --worker codex      Codex is the worker (any harness can be)
//! cargo xtask live restart-cut claude pi codex     each is the chief where it can be, and the worker
//! ```
//!
//! The run fails when a window opened by itself, a window the quit cut was not
//! told so once, or what waited for it, or its result, did not reach the window it
//! was for.

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{
    end_chosen, find, member, member_as, stand_in_chief, stand_in_worker, Account, Ended,
    LongCommand, Members, Plan, Running, ThisMachine, STAND_IN_WORKER,
};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, ask, seconds, Harness, Log};
use cf_e2e::{Error, Result};
use serde_json::Value;

/// The stand-in for the window of the project's stand-in member or chief.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The command a window is cut inside of.
const LONG: LongCommand = LongCommand::new("cf-restart-cut-long-command");

/// How long a window may take to be at its command.
const WORK: Duration = Duration::from_secs(180);
/// How long the app started again is watched for a window it opens by itself.
const QUIET: Duration = Duration::from_secs(5);
/// How long a window that came back has to be given the note that says its turn
/// was cut, then to go on and read idle, and then to be given a result.
const TOLD: Duration = Duration::from_secs(120);
const IDLE: Duration = Duration::from_secs(120);
const DELIVERED: Duration = Duration::from_secs(180);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// What the daemon tells a chief whose turn the quit cut.
const CHIEF_CUT: &str =
    "ConsensFlow stopped in the middle of your turn, which was cut. Go on where you stopped.";

/// What the daemon tells a worker whose turn the quit cut on the task `number`.
fn worker_cut(number: i64) -> String {
    format!("ConsensFlow quit while you were working on T-{number}. Go on where you stopped.")
}

/// What the human asks the window: to run the long command and wait for it.
fn the_ask() -> String {
    format!(
        "Run this exact command with your shell tool, wait for it to finish, and then reply with exactly one line: FINISHED\n{}",
        LONG.text()
    )
}

/// What a run is asked to do: the chiefs and the workers to run, each once.
#[derive(Debug, PartialEq, Eq)]
struct Cases {
    chiefs: Vec<Harness>,
    workers: Vec<Harness>,
}

/// `harness`, put in `list` if it is not there.
fn once(list: &mut Vec<Harness>, harness: Harness) {
    if !list.contains(&harness) {
        list.push(harness);
    }
}

/// The cases a run is asked for: those `--chief` (Claude or Pi), `--worker` (any
/// harness) and the harnesses named bare (a chief where it can be one, and a
/// worker) say; a Claude chief and a Claude worker when none is said.
fn cases(chiefs: &[String], workers: &[String], named: &[String]) -> Result<Cases> {
    if chiefs.is_empty() && workers.is_empty() && named.is_empty() {
        return Ok(Cases {
            chiefs: vec![Harness::Claude],
            workers: vec![Harness::Claude],
        });
    }
    let mut asked = Cases {
        chiefs: Vec::new(),
        workers: Vec::new(),
    };
    for word in chiefs {
        match Harness::from_name(word) {
            Some(chief @ (Harness::Claude | Harness::Pi)) => once(&mut asked.chiefs, chief),
            _ => return Err(Error::Daemon(format!("no such chief: {word}"))),
        }
    }
    for word in workers {
        match Harness::from_name(word) {
            Some(worker) => once(&mut asked.workers, worker),
            None => return Err(Error::Daemon(format!("no such worker: {word}"))),
        }
    }
    for word in named {
        let Some(harness) = Harness::from_name(word) else {
            return Err(Error::Daemon(format!("no such harness: {word}")));
        };
        if matches!(harness, Harness::Claude | Harness::Pi) {
            once(&mut asked.chiefs, harness);
        }
        once(&mut asked.workers, harness);
    }
    Ok(asked)
}

/// What the check opens for a chief: a project whose chief is the real `chief`,
/// on its harness's cheap model (Pi on its cheap eval model: the bench's own Pi
/// model was one that hangs), and whose one member is the stand-in agent, which
/// has no model.
fn chief_plan(chief: Harness) -> Plan {
    Plan::new("restart-cut", member_as(chief, "chief")).with(stand_in_worker())
}

/// What the check opens for a worker: a project whose chief is the stand-in, which
/// only notes what it is sent, and whose one member is the real `worker`, on its
/// harness's cheap model.
fn worker_plan(worker: Harness) -> Plan {
    Plan::new("restart-cut-worker", stand_in_chief()).with(member(worker))
}

/// The long command's processes in the window of `who` (the chief, or a member's
/// agent), with what they started: what descends from the window's own process and
/// holds the command's mark. None while the window has no process. A process outside
/// the window is none of it, whatever its command line says.
fn in_window(members: &Members, who: &str) -> Result<Vec<Running>> {
    let own = members
        .window_of(who)?
        .and_then(|window| members.window_process(&window));
    match own {
        Some(own) => LONG.chosen_in(&ThisMachine, own),
        None => Ok(Vec::new()),
    }
}

/// Ends what the window of `who` runs of the long command: a command it starts again
/// is the long one. Each process is ended only while it is still the one chosen, and what
/// could not be ended fails the case ([`ended`]).
fn end_in_window(members: &Members, who: &str, case: &mut Account) -> Result<()> {
    ended(case, &end_chosen(&in_window(members, who)?));
    Ok(())
}

/// What could not be ended of the long command, a failure of the case each: a process
/// that may still run is never left unsaid.
fn ended(case: &mut Account, ended: &Ended) {
    for problem in ended.problems() {
        case.fail(format!("the long command: {problem}"));
    }
}

/// The states a lane was read in, each with when it was first read, as a line:
/// `idle from +0.0 s → working from +2.5 s`.
fn reads(samples: &[(String, Instant)], from: Instant) -> String {
    samples
        .iter()
        .map(|(state, at)| {
            format!(
                "{state} from +{} s",
                seconds(i64::try_from(at.duration_since(from).as_millis()).unwrap_or(i64::MAX))
            )
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

/// The state of a lane, or `no lane`.
fn state_of(lane: &Value) -> String {
    lane["activity"]["state"]
        .as_str()
        .unwrap_or("no lane")
        .to_owned()
}

/// Whether `message` is a note from ConsensFlow itself that says `words`.
fn says(message: &Value, words: &str) -> bool {
    message["kind"] == "note" && message["sender"].is_null() && message["body"] == words
}

/// The notes in the inbox of `participant` that say `words`, oldest first.
fn notes_saying(members: &Members, participant: &str, words: &str) -> Result<Vec<Value>> {
    let mut notes: Vec<Value> = members
        .inbox(Some(participant))?
        .into_iter()
        .filter(|message| says(message, words))
        .collect();
    notes.sort_by_key(|message| message["id"].as_i64().unwrap_or_default());
    Ok(notes)
}

/// The result for the task `number` in the chief's inbox.
fn result_for(members: &Members, number: i64) -> Result<Option<Value>> {
    Ok(members
        .inbox(Some("chief"))?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["taskNumber"] == number))
}

/// The app started again, and nothing came of it by itself: no window opened, and
/// the project waits, suspended, for the human.
fn waits_for_the_human(case: &mut Account, members: &Members) -> Result<()> {
    sleep(QUIET);
    let opened = members.rig().open_frames();
    let open = members.is_open()?;
    case.check(
        "nothing opens by itself",
        opened.is_empty() && !open,
        format!(
            "{} windows opened in {} s, and the project is {}",
            opened.len(),
            QUIET.as_secs(),
            if open { "open" } else { "suspended" }
        ),
    );
    Ok(())
}

/// The chief is cut in a command, the app is quit and started again, and the human's
/// Resume brings the chief back, told so once; it goes on and reads idle, and what
/// waits for it goes in.
fn drive_chief(mut members: Members, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    case.say(format!("trust: {}", members.trust().unwrap_or("none")));
    members.add(STAND_IN_WORKER, &[])?;

    // The human asks the chief to run a long command.
    let pane = members.lane("chief")?["pane"].clone();
    members.paste(&pane, &ask::window_text(&the_ask())?)?;
    let at_command = find(
        log,
        "the chief inside its command",
        WORK,
        Duration::from_millis(200),
        || Ok((!in_window(&members, "chief")?.is_empty()).then_some(())),
    )?;
    if at_command.is_none() {
        case.fail(format!(
            "the chief never ran its command: {}",
            members.tail(pane["id"].as_str().unwrap_or_default())
        ));
        case.closed(members.close());
        return Ok(case);
    }
    sleep(Duration::from_secs(2));
    case.say(format!(
        "before the quit: the chief's lane reads {}, its command runs",
        state_of(&members.lane("chief")?)
    ));

    // The app's quit, and the app started again: it waits for the human. What the
    // window ran of the command is chosen while it is the window's, and what the quit
    // left of it is ended after.
    let quit_at = Instant::now();
    let left = in_window(&members, "chief")?;
    let restarted = members.restart(log);
    ended(&mut case, &end_chosen(&left));
    let members = restarted?;
    case.say(format!(
        "quit and started again in {} s",
        seconds(i64::try_from(quit_at.elapsed().as_millis()).unwrap_or(i64::MAX))
    ));
    waits_for_the_human(&mut case, &members)?;

    // The human's Resume: the chief is told its turn was cut, and goes on.
    members.resume()?;
    let told = find(
        log,
        "the chief to be told its turn was cut",
        TOLD,
        LOOK,
        || {
            // A command it starts again is the long one: it is ended.
            end_in_window(&members, "chief", &mut case)?;
            Ok(notes_saying(&members, "chief", CHIEF_CUT)?
                .into_iter()
                .find(|note| note["state"] == "delivered"))
        },
    )?;
    case.check(
        "the chief is told its turn was cut",
        told.is_some(),
        told.as_ref().map_or_else(
            || "never delivered".to_owned(),
            |note| format!("m-{}", note["id"]),
        ),
    );

    // The resumed chief: the lane, sampled until it reads idle or the wait is out.
    let mut samples: Vec<(String, Instant)> = Vec::new();
    let started_at = Instant::now();
    let idle = find(
        log,
        "the resumed chief's lane to read idle",
        IDLE,
        LOOK,
        || {
            end_in_window(&members, "chief", &mut case)?;
            let state = state_of(&members.lane("chief")?);
            if samples.last().is_none_or(|(last, _)| *last != state) {
                samples.push((state.clone(), Instant::now()));
            }
            Ok((state == "idle").then_some(()))
        },
    )?;
    case.check(
        "the chief goes on and reads idle",
        idle.is_some(),
        format!(
            "after the Resume its lane reads: {}",
            reads(&samples, started_at)
        ),
    );

    // A result for it: the member's task, given as the chief would give it.
    let number = members.give(STAND_IN_WORKER, "Reply with exactly: DONE")?;
    let given = Instant::now();
    let delivered = find(
        log,
        "the result to be delivered to the chief",
        DELIVERED,
        LOOK,
        || Ok(result_for(&members, number)?.filter(|result| result["state"] == "delivered")),
    )?;
    case.check(
        "what waits for the chief goes in",
        delivered.is_some(),
        match &delivered {
            Some(_) => format!(
                "T-{number}'s result was delivered {} s after the task was given",
                seconds(i64::try_from(given.elapsed().as_millis()).unwrap_or(i64::MAX))
            ),
            None => format!(
                "T-{number}'s result was never delivered in {} s: its state is {}; the chief's lane reads {}",
                DELIVERED.as_secs(),
                result_for(&members, number)?
                    .and_then(|result| result["state"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| "none".to_owned()),
                members.lane("chief")?["activity"]
            ),
        },
    );
    let told_once = notes_saying(&members, "chief", CHIEF_CUT)?.len() == 1;
    case.check("told once", told_once, "one note, not more");
    let lane = members.lane("chief")?;
    case.say(format!(
        "the chief's screen: {}",
        members.tail(lane["pane"]["id"].as_str().unwrap_or_default())
    ));
    end_in_window(&members, "chief", &mut case)?;
    case.closed(members.close());
    Ok(case)
}

/// The worker is cut in a command, the app is quit and started again, and the human's
/// Resume brings the worker back, told so once; it goes on, and its result reaches the
/// chief.
fn drive_worker(mut members: Members, agent: &str, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    case.say(format!("trust: {}", members.trust().unwrap_or("none")));
    members.add(agent, &[])?;

    // The worker is given a task whose brief is the long command.
    let number = members.give(agent, &the_ask())?;
    let at_command = find(
        log,
        "the worker inside its command",
        WORK,
        Duration::from_millis(200),
        || Ok((!in_window(&members, agent)?.is_empty()).then_some(())),
    )?;
    if at_command.is_none() {
        case.fail(format!(
            "the worker never ran its command: {}",
            members.lane(agent)?["activity"]
        ));
        case.closed(members.close());
        return Ok(case);
    }
    sleep(Duration::from_secs(2));
    let before = members.lane(agent)?;
    let handle = before["participant"]["handle"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    case.say(format!(
        "before the quit: @{handle}'s lane reads {}, its command runs",
        state_of(&before)
    ));

    // The app's quit, and the app started again: it waits for the human, and the task is as it was.
    // What the window ran of the command is chosen while it is the window's, and what the
    // quit left of it is ended after.
    let quit_at = Instant::now();
    let left = in_window(&members, agent)?;
    let restarted = members.restart(log);
    ended(&mut case, &end_chosen(&left));
    let members = restarted?;
    case.say(format!(
        "quit and started again in {} s",
        seconds(i64::try_from(quit_at.elapsed().as_millis()).unwrap_or(i64::MAX))
    ));
    waits_for_the_human(&mut case, &members)?;
    let state = members.task(number)?["state"]
        .as_str()
        .unwrap_or("none")
        .to_owned();
    case.check(
        "the task is as it was",
        state == "working",
        format!("T-{number} is {state}"),
    );

    // The human's Resume: the worker is told its turn was cut, and goes on.
    members.resume()?;
    let words = worker_cut(number);
    let told = find(
        log,
        "the worker to be told its turn was cut",
        TOLD,
        LOOK,
        || {
            // A command it starts again is the long one: it is ended.
            end_in_window(&members, agent, &mut case)?;
            Ok(notes_saying(&members, &handle, &words)?
                .into_iter()
                .find(|note| note["state"] == "delivered"))
        },
    )?;
    case.check(
        "the worker is told its turn was cut",
        told.is_some(),
        told.as_ref().map_or_else(
            || "never delivered".to_owned(),
            |note| format!("m-{}", note["id"]),
        ),
    );

    // It finishes, and its result reaches the chief.
    let given = Instant::now();
    let delivered = find(
        log,
        "the result to be delivered to the chief",
        DELIVERED,
        LOOK,
        || {
            end_in_window(&members, agent, &mut case)?;
            Ok(result_for(&members, number)?.filter(|result| result["state"] == "delivered"))
        },
    )?;
    case.check(
        "the worker finishes and its result reaches the chief",
        delivered.is_some(),
        match &delivered {
            Some(_) => format!(
                "T-{number}'s result was delivered {} s after the Resume",
                seconds(i64::try_from(given.elapsed().as_millis()).unwrap_or(i64::MAX))
            ),
            None => format!(
                "T-{number}'s result was never delivered in {} s: the task is {}; @{handle}'s lane reads {}",
                DELIVERED.as_secs(),
                members.task(number)?["state"],
                members.lane(agent)?["activity"]
            ),
        },
    );
    let told_once = notes_saying(&members, &handle, &words)?.len() == 1;
    case.check("told once", told_once, "one note, not more");
    end_in_window(&members, agent, &mut case)?;
    case.closed(members.close());
    Ok(case)
}

/// Runs the check on `plan`, `drive` its case: what the case saw, which holds when
/// every check of it did. A case ends what its window runs of the command, in the
/// window's own tree, before it closes the project; one that fails on the way leaves it
/// to end by itself, in five minutes.
fn run(plan: &Plan, log: &Log, drive: impl FnOnce(Members) -> Result<Account>) -> Account {
    let opened = Members::open(plan, Path::new(FAKE_AGENT), log).and_then(drive);
    opened.unwrap_or_else(|failed| {
        let mut case = Account::new();
        case.fail(failed);
        case
    })
}

/// The check of one case, as a row of the table at the end of the run.
fn checked(
    harness: Harness,
    who: &str,
    held: &str,
    failed: &str,
    case: &Account,
    log: &Log,
) -> Check {
    let version = live::version_of(harness);
    log.block(
        &format!("== {} {who}: {version}", harness.name()),
        &case.text(),
    );
    Check::new(
        &format!("{} {who}", harness.name()),
        if case.held() { held } else { failed },
        case.held(),
        version,
    )
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief or a worker of each harness named; cargo xtask live restart-cut (npm run live:restart-cut)"]
fn a_window_the_quit_cut_in_a_turn_comes_back_on_the_humans_resume_told_so_once_and_goes_on(
) -> Result {
    let log = Log::new("live restart-cut");
    ask::prepare()?;
    let asked = cases(
        &live::list("chief"),
        &live::list("worker"),
        &live::list("harness"),
    )?;
    let mut checks = Vec::new();
    for chief in asked.chiefs {
        let scoped = log.scoped(format!("{} chief", chief.name()));
        let case = run(&chief_plan(chief), &scoped, |members| {
            drive_chief(members, &scoped)
        });
        checks.push(checked(
            chief,
            "chief",
            "a chief the quit cut is told so once, reads at rest and is given what waits for it",
            "the chief the quit cut was not brought back as it should",
            &case,
            &log,
        ));
    }
    for worker in asked.workers {
        let plan = worker_plan(worker);
        let agent = plan.roster[1]["id"].as_str().unwrap_or_default().to_owned();
        let scoped = log.scoped(format!("{} worker", worker.name()));
        let case = run(&plan, &scoped, |members| {
            drive_worker(members, &agent, &scoped)
        });
        checks.push(checked(
            worker,
            "worker",
            "a worker the quit cut comes back on Resume, told so once, and its result reaches the chief",
            "the worker the quit cut was not brought back as it should",
            &case,
            &log,
        ));
    }
    report::conclude(&log, "what each case came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn the_human_asks_the_window_to_run_the_long_command_and_to_wait_for_it() {
        let ask = the_ask();
        assert!(ask.starts_with("Run this exact command with your shell tool, wait for it to finish, and then reply with exactly one line: FINISHED\n"));
        assert!(
            ask.ends_with("node -e \"setTimeout(() => {}, 300000)\" cf-restart-cut-long-command")
        );
    }

    #[test]
    fn a_claude_chief_and_a_claude_worker_run_unless_told_and_only_claude_or_pi_can_be_a_chief() {
        assert_eq!(
            cases(&[], &[], &[]).unwrap(),
            Cases {
                chiefs: vec![Harness::Claude],
                workers: vec![Harness::Claude]
            }
        );
        // A flag runs the cases it names alone, each once.
        assert_eq!(
            cases(&words(&["pi", "claude", "pi"]), &[], &[]).unwrap(),
            Cases {
                chiefs: vec![Harness::Pi, Harness::Claude],
                workers: vec![]
            }
        );
        assert_eq!(
            cases(&[], &words(&["codex", "devin", "codex"]), &[]).unwrap(),
            Cases {
                chiefs: vec![],
                workers: vec![Harness::Codex, Harness::Devin]
            }
        );
        // A harness named bare is a chief where it can be, and a worker.
        assert_eq!(
            cases(&[], &[], &words(&["codex", "pi", "claude"])).unwrap(),
            Cases {
                chiefs: vec![Harness::Pi, Harness::Claude],
                workers: vec![Harness::Codex, Harness::Pi, Harness::Claude]
            }
        );
        for wrong in ["codex", "opencode", "devin", "nobody"] {
            let failed = cases(&words(&[wrong]), &[], &[]).unwrap_err().to_string();
            assert_eq!(failed, format!("no such chief: {wrong}"));
        }
        let failed = cases(&[], &words(&["nobody"]), &[])
            .unwrap_err()
            .to_string();
        assert_eq!(failed, "no such worker: nobody");
        let failed = cases(&[], &[], &words(&["nobody"]))
            .unwrap_err()
            .to_string();
        assert_eq!(failed, "no such harness: nobody");
    }

    #[test]
    fn the_chief_is_a_real_harness_on_its_cheap_model_and_the_one_member_is_the_stand_in() {
        for chief in [Harness::Claude, Harness::Pi] {
            let plan = chief_plan(chief);
            assert_eq!(plan.folder, "restart-cut");
            assert_eq!(plan.chief, "chief");
            let (cheap, _) = live::agents::cheap(chief);
            assert_eq!(plan.roster[0]["id"], "chief");
            assert_eq!(plan.roster[0]["model"], cheap, "{chief:?}");
            assert_eq!(plan.roster[0]["kind"], chief.kind());
            // The member has no model: it only notes what it is sent.
            assert_eq!(plan.roster[1]["model"], "fake");
            // Only the chief's harness is the machine's: the stand-in is the member's Claude.
            assert_eq!(plan.harnesses(), [chief]);
        }
        assert_eq!(
            live::agents::cheap(Harness::Pi).0,
            "openrouter/meta/muse-spark-1.3"
        );
    }

    #[test]
    fn the_worker_is_a_real_harness_on_its_cheap_model_and_the_chief_is_the_stand_in() {
        for worker in Harness::ALL {
            let plan = worker_plan(worker);
            assert_eq!(plan.folder, "restart-cut-worker");
            assert_eq!(plan.chief, "chief");
            // The chief has no model: nothing answers a question or takes a task but the check.
            assert_eq!(plan.roster[0]["model"], "fake-chief");
            let (cheap, _) = live::agents::cheap(worker);
            assert_eq!(plan.roster[1]["model"], cheap, "{worker:?}");
            assert_eq!(plan.roster[1]["kind"], worker.kind());
            assert_eq!(plan.harnesses(), [worker]);
        }
    }

    #[test]
    fn what_a_window_is_told_when_its_turn_was_cut_is_what_the_daemon_writes() {
        assert_eq!(
            worker_cut(3),
            "ConsensFlow quit while you were working on T-3. Go on where you stopped."
        );
        let note = serde_json::json!({
            "kind": "note", "sender": null, "state": "delivered", "body": worker_cut(3)
        });
        assert!(says(&note, &worker_cut(3)));
        assert!(!says(&note, &worker_cut(4)));
        // A note of a participant's is no word of the daemon's.
        let theirs = serde_json::json!({
            "kind": "note", "sender": "chief", "state": "delivered", "body": worker_cut(3)
        });
        assert!(!says(&theirs, &worker_cut(3)));
        let result = serde_json::json!({ "kind": "result", "sender": null, "body": worker_cut(3) });
        assert!(!says(&result, &worker_cut(3)));
    }

    #[test]
    fn what_a_lane_read_is_said_as_each_state_with_when_it_was_first_read() {
        let from = Instant::now();
        let samples = vec![
            ("closed".to_owned(), from),
            ("idle".to_owned(), from + Duration::from_millis(2_500)),
        ];
        assert_eq!(
            reads(&samples, from),
            "closed from +0.0 s → idle from +2.5 s"
        );
        assert_eq!(reads(&[], from), "");
        assert_eq!(
            state_of(&serde_json::json!({ "activity": { "state": "idle" } })),
            "idle"
        );
        assert_eq!(state_of(&Value::Null), "no lane");
    }
}
