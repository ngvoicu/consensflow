//! Live: a Claude Code the human starts outside ConsensFlow on a worker's conversation
//! (T-442's three, T-489). It is opt-in (`cargo xtask live outside-claude`, `npm run
//! live:outside-claude`): it spends the machine's quota on a Claude worker's short task and
//! on one turn of a second Claude, and needs Claude Code installed and logged in, so it is
//! ignored unless asked for and is no part of `check` or of CI.
//!
//! The worker is given a task whose turn runs a command for a while. Meanwhile a second Claude
//! is opened on the worker's conversation, as the human opens one in a terminal of their own
//! (`claude --resume <id>`): by the pane host, with no daemon in between, so ConsensFlow knows
//! nothing of it but what Claude Code keeps, on the run's own config folder and in the
//! worker's folder. It is given a message whose turn runs longer than the worker's, so that the
//! worker answers first and the second Claude last.
//!
//! What the run holds the shape to, from what Claude Code kept (only whether each holds is
//! said, never a record's words):
//!
//! - Claude Code lets the second show the conversation beside the worker's window: two live
//!   processes' statuses name it at once;
//! - the second writes its turn into the worker's own transcript, the one file, and its
//!   records name the same conversation and no process: nothing in a record tells the two
//!   writers apart;
//! - the second, resuming while the worker's call runs, closes that call in its own view (a
//!   result and a close of the turn of its own), and the worker's result comes after: two
//!   records are children of the worker's call (Claude Code 2.1.310, 2026-10-26).
//!
//! What ConsensFlow does with it: the record's last answer after the task's message is the
//! second Claude's, and the task is not done by it, nor by the worker's: the chief is told once
//! that another Claude has shown the conversation, and the chief's Finish takes the answer.
//! Before T-489 the task was done with whatever answer the record held last (the second
//! Claude's, in this order).

//! ```text
//! cargo xtask live outside-claude              the one case, on Claude Code's cheap model
//! cargo xtask live outside-claude --keep       leaves the run's home and project where they are
//! ```

use std::fs;
use std::path::Path;
use std::time::Duration;

use cf_e2e::live::members::{find, member, stand_in_chief, Account, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::Open;
use cf_e2e::live::{self, agents, ask, claude, Harness, Log};
use cf_e2e::Result;
use serde_json::Value;

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The folder of the run's project, which the second Claude is opened in too: Claude Code
/// keeps a conversation's transcript under the folder it runs in.
const FOLDER: &str = "outside-claude";

/// What the worker's turn waits on, and what it answers once it has.
const WAIT_SECONDS: u32 = 40;
const WORKER_SAID: &str = "WINDOW-DONE-T489";

/// What the second Claude is asked, and answers: words its question does not hold, which a
/// screen that shows the question would show too. Its turn runs past the worker's, so that
/// it ends after the worker's answer: the order in which the last answer of the record is the
/// second's.
const OUTSIDE_ASKED: &str = "Run this exact command with your shell tool, in the foreground, and wait for it to finish: python3 -c 'import time; time.sleep(60)'\nThen tell me what 2111 plus 2111 is: reply with exactly one line, the word OUTSIDE, a hyphen, and the sum, with no spaces.";
const OUTSIDE_SAID: &str = "OUTSIDE-4222";

/// How long the worker's window may take to be at work on its task, and what the board and the
/// record have to say what they make of the second Claude's answer once it is on its screen.
const AT_WORK: Duration = Duration::from_secs(180);
const SETTLE: Duration = Duration::from_secs(30);

/// What the chief is told when another Claude has shown the worker's conversation.
const TOLD: &str = "another Claude than @";

/// The task's brief: a command that keeps the turn at work while the second Claude takes its
/// own, then the answer.
fn brief() -> String {
    format!(
        "Run this exact command with your shell tool, in the foreground, and wait for it to finish: python3 -c 'import time; time.sleep({WAIT_SECONDS})'\nThen reply with exactly one line: {WORKER_SAID}"
    )
}

/// The live Claude processes whose statuses name `session` in the config folder `config`.
fn holders(config: &Path, session: &str) -> Vec<u32> {
    let Ok(listed) = fs::read_dir(config.join("sessions")) else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = listed
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(|status| status["sessionId"] == session)
        .filter_map(|status| {
            status["pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
        })
        .filter(|pid| cf_process::alive(*pid))
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// The lines of the worker's transcript that hold `said`, and how many of them name a
/// conversation other than `session` or say which process wrote them.
fn lines_saying(config: &Path, session: &str, said: &str) -> (usize, usize) {
    let Some(file) = claude::transcript_file(config, session) else {
        return (0, 0);
    };
    let text = fs::read_to_string(file).unwrap_or_default();
    let holding: Vec<Value> = text
        .lines()
        .filter(|line| line.contains(said))
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let telling = holding
        .iter()
        .filter(|record| {
            record["sessionId"] != session
                || record.get("pid").is_some()
                || record.get("processId").is_some()
        })
        .count();
    (holding.len(), telling)
}

/// Where Claude Code kept what the run looks at, said by names and counts alone: each status
/// file of the config folder (its name, whether its process runs, whether it names `session`),
/// and each transcript under it that holds `said` (its name, and whether it is `session`'s).
fn shape(config: &Path, session: &str, said: &str) -> String {
    let mut lines = vec![format!("the worker's conversation: {session}")];
    for entry in fs::read_dir(config.join("sessions"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let status: Value = fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let pid = status["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok());
        lines.push(format!(
            "status {}: pid runs {}, names it {}, kind {}",
            entry.file_name().to_string_lossy(),
            pid.is_some_and(cf_process::alive),
            status["sessionId"] == session,
            status["kind"],
        ));
    }
    let mut stack = vec![config.join("projects")];
    while let Some(folder) = stack.pop() {
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if fs::read_to_string(&path).is_ok_and(|text| text.contains(said)) {
                lines.push(format!(
                    "a transcript holds the second's answer: {}",
                    path.strip_prefix(config).unwrap_or(&path).display()
                ));
            }
        }
    }
    lines.join("\n")
}

/// Whether an assistant's message of the worker's conversation holds `said`.
fn answered_by(config: &Path, session: &str, said: &str) -> bool {
    claude::record(config, session)
        .iter()
        .any(|entry| entry.kind == "assistant" && entry.text.contains(said))
}

/// How many records of the worker's conversation are children of the worker's call of its
/// command: one, its result, where the worker alone wrote it.
fn branches_from_the_call(config: &Path, session: &str) -> usize {
    let Some(file) = claude::transcript_file(config, session) else {
        return 0;
    };
    let text = fs::read_to_string(file).unwrap_or_default();
    let records: Vec<Value> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let call = records.iter().find(|record| {
        record["type"] == "assistant"
            && record["message"]["content"]
                .to_string()
                .contains("time.sleep(")
            && record["message"]["content"]
                .to_string()
                .contains(&format!("time.sleep({WAIT_SECONDS})"))
    });
    let Some(call) = call.and_then(|call| call["uuid"].as_str()) else {
        return 0;
    };
    records
        .iter()
        .filter(|record| record["parentUuid"] == call)
        .count()
}

/// The notes in the chief's inbox that say another Claude has shown the conversation.
fn told(members: &Members) -> Result<Vec<String>> {
    Ok(members
        .inbox(Some("chief"))?
        .into_iter()
        .filter(|message| message["kind"] == "note")
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.contains(TOLD))
        .collect())
}

/// The case: the worker at work, the second Claude's turn beside it, and what the board makes
/// of the worker's answer.
fn drive(members: &mut Members, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    let claude_install = log.found(Harness::Claude)?;
    let agent = agents::id(Harness::Claude);
    let config = members.claude_config();
    members.add(&agent, &[])?;
    let number = members.give(&agent, &brief())?;
    let session = find(
        log,
        "the worker's window at work",
        AT_WORK,
        Duration::from_millis(500),
        || {
            let Some(window) = members.window_of(&agent)? else {
                return Ok(None);
            };
            let Some(session) = window.session() else {
                return Ok(None);
            };
            let at_work = claude::record(&config, &session)
                .iter()
                .any(|entry| entry.text.contains(&format!("time.sleep({WAIT_SECONDS})")));
            Ok(at_work.then_some(session))
        },
    )?;
    let Some(session) = session else {
        case.fail("the worker never began its command");
        return Ok(case);
    };

    // The human's own Claude on the worker's conversation, in the worker's folder.
    let outside = Open::new(members.rig(), &claude_install, FOLDER, "outside")
        .resume(&session)
        .open(log)?;
    if let Err(screen) = outside.ready() {
        log.block("the second Claude never drew its prompt", &screen);
    }
    let beside = find(
        log,
        "two Claudes to show the conversation",
        SETTLE,
        Duration::from_millis(500),
        || {
            let beside = holders(&config, &session).len();
            Ok((beside >= 2).then_some(beside))
        },
    )?
    .unwrap_or_else(|| holders(&config, &session).len());
    let sent = outside.send(OUTSIDE_ASKED, OUTSIDE_SAID);
    log.block("the second Claude's screen, asked", &outside.screen());
    case.check(
        "two Claudes show the conversation at once",
        beside >= 2,
        format!("{beside} live statuses name it"),
    );
    case.check(
        "the second Claude answered after the worker",
        sent.as_ref().is_ok_and(|sent| sent.shown) && answered_by(&config, &session, WORKER_SAID),
        sent.as_ref().map_or_else(ToString::to_string, |sent| {
            format!("seen {}, after {} s", sent.shown, sent.seconds)
        }),
    );
    case.say(shape(&config, &session, OUTSIDE_SAID));
    let written = find(
        log,
        "the second Claude's answer in the record",
        SETTLE,
        Duration::from_millis(500),
        || Ok(answered_by(&config, &session, OUTSIDE_SAID).then_some(())),
    )?;
    case.check(
        "its turn is in the worker's own transcript",
        written.is_some(),
        "an answer of the worker's conversation is the second Claude's",
    );
    let (_, telling) = lines_saying(&config, &session, OUTSIDE_SAID);
    case.check(
        "no record tells its writer",
        telling == 0,
        format!("{telling} of the records that hold it name another conversation or a process"),
    );
    let branches = branches_from_the_call(&config, &session);
    case.check(
        "the two branch the conversation at the worker's call",
        branches >= 2,
        format!("{branches} records are children of the worker's call: the second closed it in its view, and the worker's result came after"),
    );

    // The worker's answer came first, the second Claude's last: the board makes nothing of
    // either but the note.
    std::thread::sleep(SETTLE);
    let state = members.task(number)?["state"]
        .as_str()
        .unwrap_or("none")
        .to_owned();
    case.check(
        "the task is not done by the record",
        state == "working",
        format!("T-{number} is {state}"),
    );
    let result = members
        .inbox(Some("chief"))?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["taskNumber"] == number);
    case.check(
        "no result reached the chief",
        result.is_none(),
        result.map_or_else(
            || "none".to_owned(),
            |result| {
                let body = result["body"].as_str().unwrap_or_default();
                format!(
                    "one, holding the worker's answer {} and the second Claude's {}",
                    body.contains(WORKER_SAID),
                    body.contains(OUTSIDE_SAID)
                )
            },
        ),
    );
    let notes = told(members)?;
    case.check(
        "the chief is told once",
        notes.len() == 1,
        format!("{} notes", notes.len()),
    );
    drop(outside);
    Ok(case)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a Claude worker's short task and a second Claude's turn; cargo xtask live outside-claude"]
fn a_claude_the_human_starts_on_a_workers_conversation_has_none_of_its_turns_taken_for_the_tasks(
) -> Result {
    let log = Log::begin("live outside-claude")?;
    ask::prepare()?;
    let version = live::version_of(Harness::Claude);
    let plan = Plan::new(FOLDER, stand_in_chief()).with(member(Harness::Claude));
    let case = match Members::open(&plan, Path::new(FAKE_AGENT), &log) {
        Ok(mut members) => {
            let mut case = drive(&mut members, &log).unwrap_or_else(|failed| {
                let mut case = Account::new();
                case.fail(failed);
                case
            });
            case.closed(members.close());
            case
        }
        Err(failed) => {
            let mut case = Account::new();
            case.fail(failed);
            case
        }
    };
    log.block(&format!("== claude {version}"), &case.text());
    let checks = [Check::new(
        "claude",
        "a Claude outside ConsensFlow on a worker's conversation",
        case.held(),
        version,
    )];
    log.report("what the case came to", &report::table(&checks));
    report::verdict(&checks)
}
