//! Live: what Claude Code writes of a command it runs in the background and of a
//! Monitor its agent arms on it, and what the daemon's reader counts of them. It is
//! opt-in (`cargo xtask live monitor-wait`, `npm run live:monitor-wait`): it spends the
//! machine's quota on a window of Claude Code, and needs it installed and logged in, so
//! it is ignored unless asked for and is no part of `check` or of CI.
//!
//! poker-lab's T-442 (2026-10-17, Claude Code 2.1.302): a worker started a test run in
//! the background, armed Monitor on its output and ended its turn, and ConsensFlow took
//! that turn's words for the task's result and ended the run. Turn-end (3.0.6) keeps a
//! task working while its window's harness says it waits on a job, a monitor among them
//! (`cf-harness`' `claude::record::jobs`). T-442's own record has the start of its
//! monitor and two of its notifications, but its tool content was reconstructed; this run
//! is where the shapes the reader reads come from, and the fixture
//! `tests/fixtures/claude/monitor-wait.jsonl` is its record (`--fixture <file>` writes it,
//! with the run's paths and ids of the machine taken out).
//!
//! One window, in a folder of its own, on Claude's cheap model, with no MCP server, no
//! browser, none of the owner's settings read (`--setting-sources local`), and none of a
//! Claude session's own variables if the run is started from inside one (a child session
//! keeps no transcript). Its first message has the agent start a command in the
//! background that writes three lines and ends, arm a Monitor on its output that expires
//! before long, and end its turn; then answer each event with a word, the command's end
//! with another, and the monitor's expiry with a third. Its second has it start a command
//! in the background and stop it with TaskStop. Its third has it arm a monitor on a
//! command that writes three lines and exits; its fourth, on a command that writes ten
//! lines a second, which Claude Code stops.
//!
//! - `arming`   at the first look at rest after the monitor was armed, the reader counts
//!   the command and the monitor pending
//! - `events`   a monitor's event wakes a turn, and is no end: the monitor is pending after it
//! - `job-end`  the command's end wakes a turn, and ends it alone: the monitor is pending
//! - `expiry`   the monitor's notice that it expired wakes a turn, and ends it: none pending
//! - `stop`     a command the agent stopped with TaskStop is pending no more, though no
//!   notification says so
//! - `expiry-time` Claude Code's notice of expiry comes no earlier than the monitor's call
//!   plus its time, which the reader takes it for no earlier than; the run says how long
//!   after that, and after its start's record plus its time
//! - `monitor-exit` a monitor whose watched command exits (T-442's own shape) is said over
//!   by its status, and is pending no more
//! - `noisy-stop` a monitor whose command writes more than Claude Code lets through is
//!   stopped by Claude Code after half a minute, with its notice (`[Monitor stopped — too
//!   much output …]`), no status and nothing after: as no status or time tells that notice
//!   from a line of the command's, the monitor stays counted, and its task waits for
//!   Finish (2.1.306)
//!
//! ```text
//! cargo xtask live monitor-wait
//! cargo xtask live monitor-wait --fixture /tmp/monitor-wait.jsonl
//! ```

use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::claude::{self, Entry};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window};
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
use serde_json::{json, Map, Value};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows: no
/// window of this test runs it (it is opened on the real harness's own path), but a rig
/// has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The folder of the check, below the live folders.
const FOLDER: &str = "monitor-wait";

/// The variables a Claude Code session gives its children, which a run started from
/// inside one would hand the window: with them it keeps no transcript.
const SESSION_IDENTITY: [&str; 11] = [
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
];

/// The words the agent is asked to answer each wake-up with.
const ARMED: &str = "WAITING";
const NOTED: &str = "NOTED";
const REPORTED: &str = "REPORT-READY";
const EXPIRED: &str = "ALL-DONE";
const STOPPED: &str = "STOPPED";
const QUIETED: &str = "QUIETED";
const RUN_OVER: &str = "RUN-OVER";

/// What the agent is asked first: T-442's shape, with a monitor that expires within the
/// run.
const ARM: &str = "Do exactly this, using your tools. 1) Use the Bash tool with run_in_background set to true to run: for i in 1 2 3; do echo tick $i; sleep 5; done; echo finished . 2) Use ToolSearch to load the Monitor tool (select:Monitor), then use Monitor to watch that run: its command is tail -f on the background command's output file piped through grep --line-buffered tick, with timeout_ms 40000 and description 'ticks'. 3) End your turn at once, saying only WAITING. Then, each time a notification wakes you: if it is a monitor event with a tick line, reply only NOTED; if it says the background command completed, reply only REPORT-READY; if it says the monitor expired, reply only ALL-DONE.";

/// What it is asked after.
const STOP: &str = "Do exactly this, using your tools. 1) Use the Bash tool with run_in_background set to true to run: sleep 300 . 2) Use ToolSearch to load the TaskStop tool (select:TaskStop), then use TaskStop to stop that background command. 3) End your turn saying only STOPPED.";

/// What it is asked third: T-442's own shape, a monitor that watches the run itself and is
/// over when the run's command exits, which Claude Code says by the monitor's status.
const EXITS: &str = "Do exactly this, using your tools. 1) Use Monitor to watch the command: for i in 1 2 3; do echo tick $i; sleep 2; done , with timeout_ms 120000 and description 'run'. 2) End your turn at once, saying only WATCHING. Then, each time a notification wakes you: if it says the monitor or its command completed or ended, reply only RUN-OVER; otherwise reply only NOTED.";

/// What it is asked last: a monitor on a command that writes more than Claude Code lets
/// through (2.1.306: ten events, then one each two seconds), which it stops after half a
/// minute of holding events back, with its own notice and no status (`FIn` of its bundle).
const NOISY: &str = "Do exactly this, using your tools. 1) Use Monitor to watch the command: while true; do echo tick; sleep 0.1; done , with timeout_ms 300000 and description 'noisy'. 2) End your turn at once, saying only WATCHING. Then, each time a notification wakes you: if it says the monitor stopped, reply only QUIETED; otherwise reply only NOTED.";

/// How long the agent has for the whole of the first message's work (the command runs
/// for 15 seconds and the monitor expires after 40), and for the second's; how often the
/// window is looked at.
const WORK: Duration = Duration::from_secs(240);
const LOOK: Duration = Duration::from_millis(500);

/// One look at the window: its record as the daemon reads it, the last words of its
/// agent, and Claude's status.
#[derive(Debug, Clone, PartialEq)]
struct Look {
    settled: bool,
    jobs: u64,
    last: String,
    status: String,
}

impl Look {
    fn line(&self) -> String {
        format!(
            "{} jobs pending {} · last words {:?} · Claude's status {}",
            if self.settled { "settled" } else { "in flight" },
            self.jobs,
            self.last,
            self.status
        )
    }
}

/// The window now.
fn look(window: &Window) -> Option<Look> {
    let record = window.record()?;
    let last = record
        .items
        .iter()
        .rev()
        .find(|item| item.role == "assistant" && item.complete)
        .map(|item| item.text.trim().to_owned())
        .unwrap_or_default();
    let session = window.session().unwrap_or_default();
    let status = claude::status_of(&window.claude_config(), &session)
        .and_then(|status| status["status"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "unsaid".to_owned());
    Some(Look {
        settled: record.settled,
        jobs: record.jobs_pending,
        last,
        status,
    })
}

/// The looks at the window until it is settled with its agent's last words `until`, or
/// [`WORK`] has passed: each that differs from the one before.
fn watch(window: &Window, until: &str) -> Vec<Look> {
    let (mut seen, started) = (Vec::<Look>::new(), Instant::now());
    while started.elapsed() < WORK && !window.closed() {
        if let Some(now) = look(window) {
            if seen.last() != Some(&now) {
                seen.push(now.clone());
            }
            if now.settled && now.last == until {
                break;
            }
        }
        sleep(LOOK);
    }
    seen
}

/// The first look at rest after the agent's first words: the turn that armed the monitor
/// is seldom seen at rest, as the monitor's first event is queued before it ends and
/// begins the next turn at once (2.1.305), and the reader's tests hold that turn's end
/// on the fixture this run writes.
fn first_settled(looks: &[Look]) -> Option<&Look> {
    looks
        .iter()
        .skip_while(|look| look.last.is_empty())
        .find(|look| look.settled)
}

/// When `record` was written, in milliseconds.
fn written_at(record: &Value) -> Option<i64> {
    record["timestamp"]
        .as_str()?
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|at| at.as_millisecond())
}

/// How long after the time it expires the monitor described as `description` was said by
/// Claude Code to have expired, in milliseconds, counted from the call that armed it and
/// from the record of its start: none where the records do not say.
fn expiry_margins(raw: &[Value], description: &str) -> Option<(i64, i64)> {
    // Each call described so, and its time: the agent may call again where a call failed.
    let calls: Vec<(String, i64)> = raw
        .iter()
        .flat_map(|record| {
            let at = written_at(record);
            record["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|part| {
                    part["type"] == "tool_use"
                        && part["name"] == "Monitor"
                        && part["input"]["description"] == description
                })
                .filter_map(move |part| Some((part["id"].as_str()?.to_owned(), at?)))
        })
        .collect();
    // The start a call's result says: the call that armed the monitor.
    let (called, start) = calls.iter().find_map(|(id, called)| {
        raw.iter()
            .find(|record| {
                record["toolUseResult"]["taskId"].is_string()
                    && record["message"]["content"]
                        .as_array()
                        .is_some_and(|parts| {
                            parts.iter().any(|part| {
                                part["type"] == "tool_result" && part["tool_use_id"] == id.as_str()
                            })
                        })
            })
            .map(|start| (*called, start))
    })?;
    let timeout = start["toolUseResult"]["timeoutMs"].as_i64()?;
    let task = start["toolUseResult"]["taskId"].as_str()?.to_owned();
    let started = written_at(start)?;
    let expired = raw
        .iter()
        .filter(|record| {
            let text = record.to_string();
            text.contains(&format!("<task-id>{task}</task-id>"))
                && text.contains("[Monitor expired after ")
        })
        .filter_map(written_at)
        .min()?;
    Some((expired - called - timeout, expired - started - timeout))
}

/// Whether some look was settled at the agent's words `words`, with `jobs` pending.
fn settled_at(looks: &[Look], words: &str, jobs: u64) -> bool {
    looks
        .iter()
        .any(|look| look.settled && look.last == words && look.jobs == jobs)
}

/// The notifications Claude Code began a turn with (the user's records from the system,
/// for a notification), as their texts.
fn notifications(raw: &[Value]) -> Vec<String> {
    raw.iter()
        .filter(|record| {
            record["type"] == "user"
                && record["promptSource"] == "system"
                && record["turnOrigin"] == "task_notification"
        })
        .map(|record| {
            let content = &record["message"]["content"];
            content.as_str().map_or_else(
                || {
                    content
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|part| part["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                },
                str::to_owned,
            )
        })
        .collect()
}

/// Whether a notification's head has the field `tag` (on a line of its own).
fn has(text: &str, tag: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with(&format!("<{tag}>")))
}

/// The records of the window's conversation, as Claude wrote them.
fn raw_records(window: &Window) -> Vec<Value> {
    let session = window.session().unwrap_or_default();
    claude::transcript_file(&window.claude_config(), &session)
        .and_then(|file| fs::read_to_string(file).ok())
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The record types the fixture keeps: what the reader reads, and none of what says
/// where the machine keeps things or what its owner's setup is.
const KEPT_TYPES: [&str; 5] = [
    "user",
    "assistant",
    "system",
    "queue-operation",
    "attachment",
];

/// The fields of a record the fixture keeps.
const KEPT_FIELDS: [&str; 22] = [
    "type",
    "subtype",
    "uuid",
    "parentUuid",
    "isSidechain",
    "sessionId",
    "timestamp",
    "version",
    "message",
    "toolUseResult",
    "operation",
    "content",
    "promptSource",
    "turnOrigin",
    "origin",
    "isMeta",
    "level",
    "durationMs",
    "messageCount",
    "pendingBackgroundAgentCount",
    "pendingWorkflowCount",
    "preventedContinuation",
];

/// What of a tool's result the fixture keeps: the fields that start, arm and stop jobs.
const KEPT_RESULT: [&str; 8] = [
    "backgroundTaskId",
    "taskId",
    "timeoutMs",
    "persistent",
    "task_id",
    "task_type",
    "message",
    "matches",
];

/// The records of the run as the fixture keeps them (see [`KEPT_FIELDS`]): Claude's
/// attachments but those that bring a notification into a turn are left out (they say
/// the machine's setup), and every string has the run's session id, its folders and the
/// owner's home put as `$SESSION`, `$WORK` and `$HOME`.
fn fixture(raw: &[Value], session: &str, folders: &[(String, &str)]) -> Vec<String> {
    raw.iter()
        .filter(|record| {
            record["type"]
                .as_str()
                .is_some_and(|kind| KEPT_TYPES.contains(&kind))
                && (record["type"] != "attachment"
                    || record["attachment"]["type"] == "queued_command")
        })
        .map(|record| {
            let mut kept = Map::new();
            for name in KEPT_FIELDS {
                if let Some(value) = record.get(name) {
                    let value = if name == "toolUseResult" {
                        only(value, &KEPT_RESULT)
                    } else {
                        value.clone()
                    };
                    kept.insert(name.to_owned(), value);
                }
            }
            if record["type"] == "attachment" {
                kept.insert(
                    "attachment".to_owned(),
                    only(&record["attachment"], &["type", "commandMode", "prompt"]),
                );
            }
            let mut text = Value::Object(kept).to_string();
            text = text.replace(session, "$SESSION");
            for (folder, put) in folders {
                text = text.replace(folder.as_str(), put);
            }
            text
        })
        .collect()
}

/// The fields `names` of `value`, where it is an object; else it as it is.
fn only(value: &Value, names: &[&str]) -> Value {
    match value.as_object() {
        Some(fields) => Value::Object(
            fields
                .iter()
                .filter(|(name, _)| names.contains(&name.as_str()))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        ),
        None => value.clone(),
    }
}

/// The folders a string of the run may name, longest first, as the fixture puts them:
/// Claude's own folder of the conversation's temporary files (`/private/tmp/claude-…/
/// <project>/`) and the window's folder are `$WORK`, the owner's home `$HOME`.
fn folders(window: &Window) -> Vec<(String, &'static str)> {
    let workspace = window.workspace().to_string_lossy().into_owned();
    let mut folders = vec![(workspace.clone(), "$WORK")];
    let slug: String = workspace
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    for root in ["/private/tmp", "/tmp"] {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("claude-") {
                    folders.push((format!("{root}/{name}/{slug}"), "$WORK"));
                }
            }
        }
    }
    if let Ok(home) = live::machine::home() {
        folders.push((home.to_string_lossy().into_owned(), "$HOME"));
    }
    folders.sort_by_key(|(folder, _)| std::cmp::Reverse(folder.len()));
    folders
}

/// The lines of a look's report: each look, and the last records, as their kinds and
/// how their words open.
fn report_lines(looks: &[Look], entries: &[Entry]) -> String {
    let mut lines: Vec<String> = looks.iter().map(Look::line).collect();
    lines.push("records:".to_owned());
    lines.extend(entries.iter().map(|entry| {
        let opening: String = entry.text.replace('\n', " ").chars().take(80).collect();
        format!("  {} {opening}", entry.kind)
    }));
    lines.join("\n")
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of Claude Code; cargo xtask live monitor-wait (npm run live:monitor-wait)"]
fn a_background_command_and_a_monitor_on_it_are_pending_until_claude_code_says_each_is_over(
) -> Result {
    let log = Log::begin("live monitor-wait")?;
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let claude = log.found(Harness::Claude)?;
    let rig = Rig::start(live::rig(std::slice::from_ref(&claude), FAKE_AGENT))?;
    let mut open = Open::new(&rig, &claude, FOLDER, FOLDER).claude_alone_with_hooks(json!({}));
    for name in SESSION_IDENTITY {
        open = open.without(name);
    }
    let window = open.open_telling(&log, "claude")?;
    window
        .ready()
        .map_err(|screen| Error::Daemon(format!("the window did not open: {screen}")))?;
    window.paste(&window.given(ARM)?)?;
    let looks = watch(&window, EXPIRED);
    let raw = raw_records(&window);
    let told = notifications(&raw);
    let config = window.claude_config();
    let session = window.session().unwrap_or_default();
    log.block(
        "the first message",
        &report_lines(&looks, &claude::record(&config, &session)),
    );
    let event = told
        .iter()
        .any(|text| has(text, "event") && !has(text, "status"));
    let expiry = told
        .iter()
        .any(|text| text.contains("<event>[Monitor expired after ") && !has(text, "status"));
    let ended = told
        .iter()
        .any(|text| has(text, "status") && text.contains("<status>completed</status>"));
    let mut checks = vec![
        Check::new(
            "claude",
            "arming",
            first_settled(&looks).is_some_and(|look| look.jobs == 2),
            "the first look at rest after the monitor was armed counted the command and the monitor pending",
        ),
        Check::new(
            "claude",
            "events",
            event && looks.iter().any(|look| look.settled && look.last == NOTED && look.jobs >= 1),
            "an event of the monitor, with no status, woke a turn, and the monitor was pending after it",
        ),
        Check::new(
            "claude",
            "job-end",
            ended && settled_at(&looks, REPORTED, 1),
            "the command's end woke a turn, after which the monitor alone was pending",
        ),
        Check::new(
            "claude",
            "expiry",
            expiry && settled_at(&looks, EXPIRED, 0),
            "the monitor's own notice of its expiry woke a turn, after which none was pending",
        ),
    ];
    window.paste(&window.given(STOP)?)?;
    let stopping = watch(&window, STOPPED);
    let raw = raw_records(&window);
    let stopped = raw
        .iter()
        .any(|record| record["toolUseResult"]["task_id"].is_string());
    log.block(
        "the second message",
        &stopping
            .iter()
            .map(Look::line)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    checks.push(Check::new(
        "claude",
        "stop",
        stopped && settled_at(&stopping, STOPPED, 0),
        "a command the agent stopped with TaskStop was pending no more",
    ));
    // How a real expiry falls against the time the reader counts it from: never before
    // the call that armed it plus its time, which the reader takes it for no earlier than.
    let margins = expiry_margins(&raw, "ticks");
    if margins.is_none() {
        let calls: Vec<String> = raw
            .iter()
            .filter_map(|record| record["message"]["content"].as_array())
            .flatten()
            .filter(|part| part["type"] == "tool_use" && part["name"] == "Monitor")
            .map(|part| part["input"]["description"].to_string())
            .collect();
        let starts = raw
            .iter()
            .filter(|record| record["toolUseResult"]["taskId"].is_string())
            .count();
        let expiries = raw
            .iter()
            .filter(|record| record.to_string().contains("[Monitor expired after "))
            .count();
        log.say(format!(
            "the expiry's margins could not be read: Monitor calls described {calls:?}, {starts} starts, {expiries} records of an expiry"
        ));
    }
    log.say(format!(
        "the monitor's expiry, against the call that armed it and against its start's record (ms after each plus its time): {margins:?}"
    ));
    checks.push(Check::new(
        "claude",
        "expiry-time",
        margins.is_some_and(|(after_the_call, _)| after_the_call >= 0),
        "Claude Code's notice of the monitor's expiry came no earlier than the call that armed it plus its time",
    ));
    window.paste(&window.given(EXITS)?)?;
    let exits = watch(&window, RUN_OVER);
    let raw = raw_records(&window);
    let told = notifications(&raw);
    let ended_by_status =
        told.iter().rev().take(4).any(|text| {
            text.contains("Monitor") && has(text, "status") && !text.contains("<event>")
        });
    log.block(
        "the third message",
        &exits.iter().map(Look::line).collect::<Vec<_>>().join("\n"),
    );
    checks.push(Check::new(
        "claude",
        "monitor-exit",
        ended_by_status && settled_at(&exits, RUN_OVER, 0),
        "a monitor whose watched command exits is said over by its status, and is pending no more",
    ));
    window.paste(&window.given(NOISY)?)?;
    let noisy = watch(&window, QUIETED);
    let raw = raw_records(&window);
    let told = notifications(&raw);
    let stop_notice = told
        .iter()
        .position(|text| text.contains("<event>[Monitor stopped") && !has(text, "status"));
    // What 2.1.306 wrote of the monitor after its notice: a word of its end, or nothing.
    let after: Vec<String> = stop_notice
        .map(|at| {
            told[at + 1..]
                .iter()
                .filter(|text| has(text, "status"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    log.block(
        "the fourth message",
        &format!(
            "{}\nthe notice that stopped the monitor: {}\nnotifications with a status after it: {}",
            noisy.iter().map(Look::line).collect::<Vec<_>>().join("\n"),
            stop_notice.map_or("none".to_owned(), |at| told[at].replace('\n', " ")),
            after.len()
        ),
    );
    checks.push(Check::new(
        "claude",
        "noisy-stop",
        stop_notice.is_some() && after.is_empty() && settled_at(&noisy, QUIETED, 1),
        "a monitor Claude Code stopped for too much output, with a notice no status or time tells from its command's output, stays counted: its task waits for Finish",
    ));
    if let Some(file) = live::option("fixture") {
        let lines = fixture(&raw, &session, &folders(&window));
        fs::write(Path::new(&file), format!("{}\n", lines.join("\n")))
            .map_err(|error| Error::Daemon(format!("the fixture was not written: {error}")))?;
        log.say(format!("the fixture, {} records: {file}", lines.len()));
    }
    drop(window);
    rig.close()?;
    report::conclude(
        &log,
        "what Claude Code wrote, and what the reader counted",
        &checks,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(settled: bool, jobs: u64, last: &str) -> Look {
        Look {
            settled,
            jobs,
            last: last.to_owned(),
            status: "idle".to_owned(),
        }
    }

    #[test]
    fn a_step_is_seen_only_at_a_settled_look_with_its_words_and_its_count() {
        let looks = [
            look(false, 0, ""),
            look(true, 2, ARMED),
            look(true, 2, NOTED),
            look(true, 1, REPORTED),
        ];
        assert!(settled_at(&looks, ARMED, 2));
        assert!(!settled_at(&looks, ARMED, 1));
        assert!(settled_at(&looks, REPORTED, 1));
        assert!(!settled_at(&looks, EXPIRED, 0));
        assert!(!settled_at(&[look(false, 2, ARMED)], ARMED, 2));
        assert_eq!(first_settled(&looks), Some(&looks[1]));
        let busy_then_noted = [
            look(true, 0, ""),
            look(false, 0, ARMED),
            look(true, 2, NOTED),
        ];
        assert_eq!(first_settled(&busy_then_noted), Some(&busy_then_noted[2]));
    }

    #[test]
    fn a_notification_is_claude_codes_own_turn_and_its_fields_are_lines_of_its_head() {
        let said = "<task-notification>\n<task-id>b1</task-id>\n<summary>Monitor event: \"ticks\"</summary>\n<event>tick 1</event>\n</task-notification>";
        let raw = [
            json!({ "type": "user", "promptSource": "system", "turnOrigin": "task_notification", "message": { "content": said } }),
            json!({ "type": "user", "promptSource": "typed", "turnOrigin": "human", "message": { "content": said } }),
            json!({ "type": "user", "promptSource": "system", "turnOrigin": "task_notification", "message": { "content": [{ "type": "text", "text": said }] } }),
        ];
        let told = notifications(&raw);
        assert_eq!(told.len(), 2);
        assert!(has(&told[0], "event") && !has(&told[0], "status"));
        assert!(!has("say <event>x</event>", "event"));
    }

    #[test]
    fn the_fixture_keeps_what_the_reader_reads_and_nothing_of_the_machine() {
        let raw = [
            json!({ "type": "user", "sessionId": "s-1", "cwd": "/Users/me/work", "uuid": "u1", "timestamp": "t", "message": { "role": "user", "content": "/Users/me/work/x" }, "toolUseResult": { "taskId": "b1", "timeoutMs": 40000, "persistent": false, "stdout": "secret" } }),
            json!({ "type": "attachment", "attachment": { "type": "instructions", "content": "the owner's setup" } }),
            json!({ "type": "attachment", "attachment": { "type": "queued_command", "commandMode": "task-notification", "prompt": "<task-notification>", "extra": 1 } }),
            json!({ "type": "api-request", "body": "x" }),
        ];
        let lines = fixture(&raw, "s-1", &[("/Users/me/work".to_owned(), "$WORK")]);
        assert_eq!(lines.len(), 2);
        let first: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(first["sessionId"], "$SESSION");
        assert_eq!(first["message"]["content"], "$WORK/x");
        assert_eq!(first.get("cwd"), None);
        assert_eq!(
            first["toolUseResult"],
            json!({ "taskId": "b1", "timeoutMs": 40000, "persistent": false })
        );
        let second: Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(
            second["attachment"],
            json!({ "type": "queued_command", "commandMode": "task-notification", "prompt": "<task-notification>" })
        );
    }
}

#[test]
fn the_margins_of_the_fixtures_expiry_are_read_from_its_records() {
    // The fixture of the live run of 2026-10-21: the call at 23:02:30.287, its start's
    // record at 30.317, a time of 40 s, and the notice queued at 23:03:10.317.
    let raw: Vec<Value> = include_str!("../../cf-harness/tests/fixtures/claude/monitor-wait.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(expiry_margins(&raw, "ticks"), Some((30, 0)));
}
