//! A stand-in for the `claude` CLI in the integration tests. It speaks Claude's
//! launch arguments, writes Claude's transcript records and its live
//! `sessions/<pid>.json` status, and reads its terminal raw, the way a real TUI
//! does: a bracketed paste followed by Enter is one message. What it answers a
//! message with is told in `reply`.
//!
//! The window of the participant `CF_TEST_NO_LOGIN` names has no login: it
//! prints that and stays, doing nothing else (`CF_TEST_NO_LOGIN_EXITS`: and
//! ends with the code 3). A key pressed in a turn (the Escape that stops it) is
//! ignored: this window is never stopped.
//!
//! With `CF_TEST_MCP_SERVER` set, every window starts a server of its harness,
//! in the group of its program (`background`): as it opens, before any turn, or,
//! with the word `late`, as it takes its first message, after the daemon has
//! marked where that turn began, as Claude Code starts its servers; with
//! `CF_TEST_MCP_BROWSER` set the server launches a browser of its own, in a session
//! of its own, as `@playwright/mcp` and `chrome-devtools-mcp` do. And the
//! program is the job too: started with `--background-job` first, it is not a
//! window but the work a window left running. With `CF_TEST_STATUS_LINE` set (on Unix)
//! it runs its harness's status line, a script of the user's that shows the branch,
//! for as long as it lives (`status_line`).
//!
//! Escape stops the turn at work, as it stops Claude's: a turn that works (a
//! `SLEEP`) ends there, writing the record Claude writes of a turn the user
//! interrupted, `[Request interrupted by user]`, and no answer; and a turn still
//! in the hooks of its prompt (`HOOKS`) writes nothing, as Claude writes nothing
//! of one stopped before a word of its answer. A window with
//! `CF_TEST_WIND_DOWN_MS` set takes that long to stop, as Claude does when it
//! has a tool to wind down, so that a case can do something while the stop is
//! not yet paid. Control-C once clears what is typed, and twice ends the window.
//! A window with `CF_TEST_ANSWER_MS` set takes that long to write its answer once
//! its work is done, so that a case can do something while the turn is still open
//! in the record: quit the app, as a human may at any moment. A turn that runs a
//! command (`RUN`, in `tool`) works until the command ends, and Escape stops it
//! and, as the line says, the command with it or not. A turn that runs a build
//! (`BUILD`, in `build`) waits for it as for a command, and the build leaves a
//! daemon running, as Gradle's does: the program is the build too (`--build`).
//! `/compact` typed alone compacts the conversation as Claude Code does while idle:
//! its boundary and its summary in the record, and no turn.
//!
//! It is the program of the suites that stands in a window, so it reads the
//! environment a window is given and runs `cf` as an agent does, both through
//! `cf_e2e::process`, the one place a program starts and the environment is read.

// The parts are in `fake_agent/`: cargo looks for the modules of a binary's
// root file beside it, where each would be a binary of its own.
#[path = "fake_agent/agents.rs"]
mod agents;
#[path = "fake_agent/args.rs"]
mod args;
#[path = "fake_agent/background.rs"]
mod background;
#[path = "fake_agent/build.rs"]
mod build;
#[path = "fake_agent/records.rs"]
mod records;
#[path = "fake_agent/reply.rs"]
mod reply;
#[path = "fake_agent/signals.rs"]
mod signals;
#[path = "fake_agent/status_line.rs"]
#[cfg(unix)]
mod status_line;
#[path = "fake_agent/stop.rs"]
mod stop;
#[path = "fake_agent/terminal.rs"]
mod terminal;
#[path = "fake_agent/tool.rs"]
mod tool;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::pattern;
use cf_e2e::process::own_var;
use regex::Regex;
use serde_json::json;

use agents::Agents;
use background::{Jobs, Server};
use records::{note_process, Records, INTERRUPTED};
use reply::{is_session_of, reply_to, Reply, Window};
use stop::Stop;
use terminal::{Event, Keys};

/// What a window with no login says on its screen, as Pi says it on a machine
/// that has none.
const NO_LOGIN: &str =
    "No API key found for the selected model.\r\nUse /login to log into a provider.\r\n";

/// What a window the provider refuses says, in the record Claude writes for it.
const LIMIT: &str = "You've hit your limit. Resets in 2 hours.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The work a window left running is this program, with a word first that no
    // window is launched with: it holds its signals as it needs, not as a window.
    if args.first().map(String::as_str) == Some(background::JOB) {
        return background::run(&args[1..]);
    }
    // The build a turn's command runs, which leaves a daemon of its own.
    if args.first().map(String::as_str) == Some(build::BUILD) {
        return build::run(&args[1..]);
    }
    // The shell of a command a harness detaches, which a session of its own is made for.
    #[cfg(unix)]
    if args.first().map(String::as_str) == Some(background::SESSION) {
        return background::run_in_session(&args[1..]);
    }
    // Signals are waited for on a thread of their own, so they are blocked in
    // this one and in every thread started after it.
    let signals = signals::block();
    let own = std::env::current_exe().ok();
    let launch = match args::parse(&args, own.as_deref()) {
        Ok(launch) => launch,
        Err(failed) => return fail(&failed),
    };
    match run(&launch, signals) {
        Ok(never) => match never {},
        Err(failed) => fail(&failed),
    }
}

/// Says why the window could not run, on its error output.
fn fail(failed: &str) -> ExitCode {
    let _ = writeln!(io::stderr(), "fake-agent: {failed}");
    ExitCode::FAILURE
}

/// Ends the window with `code`, after taking its status file away.
fn leave(status_file: &Path, code: i32) -> ! {
    let _ = std::fs::remove_file(status_file);
    std::process::exit(code)
}

/// The window, from its start to its end, which it does not return from.
fn run(
    launch: &args::Launch,
    signals: signals::Signals,
) -> Result<std::convert::Infallible, String> {
    let config = own_var("CLAUDE_CONFIG_DIR").ok_or("CLAUDE_CONFIG_DIR is not set")?;
    let home = own_var("CONSENSFLOW_HOME").ok_or("CONSENSFLOW_HOME is not set")?;
    let participant = own_var("CONSENSFLOW_PARTICIPANT").unwrap_or_default();
    let pid = std::process::id();
    let mut records = Records::new(Path::new(&config), &launch.session, pid)
        .map_err(|failed| failed.to_string())?;
    let status_file: PathBuf = records.status_file().to_path_buf();
    // The ids of the processes started, for the test beside the daemon's home.
    let beside = Path::new(&home).parent().unwrap_or(Path::new("."));
    note_process(beside, pid, &launch.session).map_err(|failed| failed.to_string())?;
    // Ended when its terminal hangs up or it is asked to: the status file goes,
    // and the code is 0, as the Node stand-in's was.
    let leaving = status_file.clone();
    signals::leave_on(signals, move || leave(&leaving, 0));

    // A harness with no login, as Pi is on a machine that has none: it says so
    // on its screen and writes no record of a conversation or a status, so the
    // message it was launched with never shows.
    let named = |variable: &str| {
        own_var(variable)
            .is_some_and(|wanted| !wanted.is_empty() && is_session_of(&participant, &wanted))
    };
    if named("CF_TEST_NO_LOGIN") || named("CF_TEST_NO_LOGIN_EXITS") {
        let mut out = io::stdout().lock();
        out.write_all(NO_LOGIN.as_bytes())
            .and_then(|()| out.flush())
            .map_err(|failed| failed.to_string())?;
        if named("CF_TEST_NO_LOGIN_EXITS") {
            leave(&status_file, 3);
        }
        // Stays: reads its terminal and does nothing with it, until it is closed.
        let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
        leave(&status_file, 0);
    }

    records
        .status(pid, "idle")
        .map_err(|failed| failed.to_string())?;
    let said = if launch.resuming {
        "resumed"
    } else {
        "started"
    };
    say(&format!("fake agent {said} on {}\n", launch.session))
        .map_err(|failed| failed.to_string())?;

    let (turns, queued) = mpsc::channel::<String>();
    let stop = Arc::new(Stop::default());
    let window = Window {
        participant,
        settings: launch.settings.clone(),
        session: launch.session.clone(),
        jobs: Arc::new(
            Jobs::new(Path::new(&home), pid)
                .launching_a_browser(own_var("CF_TEST_MCP_BROWSER").as_deref()),
        ),
        server: Server::of(own_var("CF_TEST_MCP_SERVER").as_deref()),
        wake: turns.clone(),
        stop: Arc::clone(&stop),
        agents: Agents::default(),
    };
    // A server the harness starts with its window, before any turn, as an MCP
    // server may be (one that is late begins with the first message instead).
    window.server.as_the_window_opens(&window.jobs)?;
    // The harness's status line, which the user's script is run for, while it lives.
    #[cfg(unix)]
    if let Some(status_line) =
        status_line::StatusLine::of(own_var("CF_TEST_STATUS_LINE").as_deref())
    {
        status_line.go(Path::new(&config), &window.jobs)?;
    }
    {
        let status_file = status_file.clone();
        thread::spawn(move || {
            // One turn at a time, in the order the messages came.
            for text in queued {
                if let Err(failed) = take_turn(&mut records, pid, &window, &text) {
                    let _ = writeln!(io::stderr(), "fake-agent: {failed}");
                    leave(&status_file, 1);
                }
            }
        });
    }
    if let Some(seed) = &launch.seed {
        let _ = turns.send(seed.clone());
    }

    // Raw input, like a real TUI: a bracketed paste is text, Enter outside one
    // submits, and an Escape that nothing follows is the key.
    let mut keys = Keys::start();
    loop {
        // The terminal is gone.
        let Some(events) = keys.read() else {
            leave(&status_file, 0)
        };
        for event in events {
            match event {
                Event::Submit(text) => {
                    let _ = turns.send(text);
                }
                // A real TUI draws what was pasted, and a paste's Enter waits for that.
                Event::Pasted(characters) => {
                    let _ = say(&format!("[pasted, {characters} characters]\n"));
                }
                Event::Escape => stop.press(),
                Event::Interrupt => leave(&status_file, 0),
            }
        }
    }
}

/// Writes `text` to the window's screen.
fn say(text: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// How long a variable's text says to wait: its milliseconds, none unless it is
/// a number of them.
fn wait_of(text: Option<&str>) -> Duration {
    Duration::from_millis(text.and_then(|text| text.parse().ok()).unwrap_or(0))
}

/// Waits as long as the variable `name` says.
fn wait_as_told(name: &str) {
    thread::sleep(wait_of(own_var(name).as_deref()));
}

/// How long the message `text` stays in the hooks of its prompt: a line
/// `HOOKS <seconds> <words>`.
fn hooks_of(text: &str) -> Option<Duration> {
    static HOOKS: OnceLock<Regex> = OnceLock::new();
    pattern::once(&HOOKS, r"(?m)^HOOKS (\d+) ")
        .captures(text)
        .and_then(|hooks| hooks[1].parse().ok())
        .map(Duration::from_secs)
}

/// What the user types to have the conversation compacted.
const COMPACT: &str = "/compact";

/// What a compaction writes as the summary of the conversation before it.
const COMPACT_SUMMARY: &str =
    "This session is being continued from a previous conversation that ran out of context.";

/// One message: the window works on it and is idle again once it is answered
/// or an Escape stopped it. `/compact` typed alone is no message: the window
/// compacts its conversation as Claude Code does while idle, writing the
/// `compact_boundary` and the summary (`isCompactSummary`), and no turn (the long
/// run's compaction, which its watch finds in the record).
fn take_turn(records: &mut Records, pid: u32, window: &Window, text: &str) -> Result<(), String> {
    if text.trim() == COMPACT {
        return compact(records, pid);
    }
    window.stop.begin();
    let done = turn(records, pid, window, text);
    window.stop.end();
    done
}

/// The conversation compacted while idle: busy while it compacts, then the two
/// records Claude Code writes of it, and idle again.
fn compact(records: &mut Records, pid: u32) -> Result<(), String> {
    let failed = |cause: io::Error| cause.to_string();
    records.status(pid, "busy").map_err(failed)?;
    for record in compaction_records() {
        records.append(pid, record).map_err(failed)?;
    }
    records.status(pid, "idle").map_err(failed)
}

/// What Claude Code writes of a compaction while idle, as the product's reader
/// is tested with (`cf-harness`'s `record/tests/compaction.rs`).
fn compaction_records() -> [serde_json::Value; 2] {
    [
        json!({
            "type": "system", "subtype": "compact_boundary", "parentUuid": null,
            "content": "Conversation compacted", "isMeta": false, "level": "info",
            "compactMetadata": { "trigger": "manual" },
        }),
        json!({
            "type": "user", "isCompactSummary": true, "isVisibleInTranscriptOnly": true,
            "message": { "role": "user", "content": COMPACT_SUMMARY },
        }),
    ]
}

/// A turn: the window is busy (not in the hooks of its prompt, where Claude's
/// status reads idle), records the message, answers it, and is idle, or busy
/// still while the background agents it started run, as Claude Code is.
fn turn(records: &mut Records, pid: u32, window: &Window, text: &str) -> Result<(), String> {
    let failed = |cause: io::Error| cause.to_string();
    let began = Instant::now();
    // Only the first message of the window is in the hooks of its prompt: the
    // words that go in later, a brief they carry among them, do not put it back.
    let first = records.ordinal() == 0;
    let hooks = hooks_of(text).filter(|_| first);
    if hooks.is_none() {
        records.status(pid, "busy").map_err(failed)?;
    }
    records
        .append(
            pid,
            json!({ "type": "user", "message": { "role": "user", "content": text } }),
        )
        .map_err(failed)?;
    // The harness starts its servers as it takes the first message, which now shows.
    if first {
        window.server.as_the_first_message_shows(&window.jobs);
    }
    if let Some(hooks) = hooks {
        // Stopped there, the turn leaves nothing of itself: Claude puts the
        // message back in its input box, and its status never left idle.
        if window.stop.work(hooks) {
            return Ok(());
        }
        records.status(pid, "busy").map_err(failed)?;
    }
    let reply = match tool::reply(text, window).or_else(|| build::reply(text, window)) {
        Some(reply) => reply?,
        None => reply_to(text, window)?,
    };
    // The message id numbers the records so far, this one not yet among them.
    let id = format!("{}-message-{pid}-{}", window.session, records.ordinal());
    match reply {
        Reply::Stopped => {
            // The turn takes that long to end once it is stopped.
            wait_as_told("CF_TEST_WIND_DOWN_MS");
            // What Claude writes of a turn the user interrupted: the marker, as
            // a turn of the user's, and no answer after it.
            records
                .append(
                    pid,
                    json!({
                        "type": "user",
                        "message": { "role": "user", "content": [{ "type": "text", "text": INTERRUPTED }] },
                    }),
                )
                .map_err(failed)?;
        }
        Reply::Refused => {
            // The provider refused: Claude writes the refusal as an assistant record.
            records
                .append(
                    pid,
                    json!({
                        "type": "assistant",
                        "isApiErrorMessage": true,
                        "apiErrorStatus": 429,
                        "error": "rate_limit",
                        "message": {
                            "id": id,
                            "role": "assistant",
                            "content": [{ "type": "text", "text": LIMIT }],
                        },
                    }),
                )
                .map_err(failed)?;
        }
        Reply::Text(reply) => {
            // The work is done (a `cf` it ran has answered) and the answer is
            // not yet written: the turn is open in the record that long.
            wait_as_told("CF_TEST_ANSWER_MS");
            records
                .append(
                    pid,
                    json!({
                        "type": "assistant",
                        "message": {
                            "id": id,
                            "role": "assistant",
                            "content": [{ "type": "text", "text": reply }],
                            "stop_reason": "end_turn",
                        },
                    }),
                )
                .map_err(failed)?;
            records
                .append(
                    pid,
                    json!({
                        "type": "system",
                        "subtype": "stop_hook_summary",
                        "preventedContinuation": false,
                        "hookCount": 1,
                    }),
                )
                .map_err(failed)?;
            // A window that started agents writes what Claude Code writes after
            // the stop hooks: the turn's duration, and how many agents it left
            // running (the count is left out when none).
            if window.agents.used() {
                let mut duration = json!({
                    "type": "system",
                    "subtype": "turn_duration",
                    "isSidechain": false,
                    "durationMs": u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX),
                    "messageCount": records.ordinal(),
                });
                let running = window.agents.running();
                if running > 0 {
                    duration["pendingBackgroundAgentCount"] = json!(running);
                }
                records.append(pid, duration).map_err(failed)?;
            }
        }
    }
    let status = if window.agents.running() > 0 {
        "busy"
    } else {
        "idle"
    };
    records.status(pid, status).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_stays_in_the_hooks_of_its_prompt_as_long_as_its_line_says() {
        let brief = "[ConsensFlow m-1 · T-1 · task from @chief]\nHOOKS 20 Reply with exactly: X";
        assert_eq!(hooks_of(brief), Some(Duration::from_secs(20)));
        for text in [
            "Reply with exactly: X",
            "HOOKS soon X",
            "say HOOKS 5 x",
            "HOOKS 5",
        ] {
            assert_eq!(hooks_of(text), None, "{text}");
        }
    }

    #[test]
    fn a_compaction_writes_its_boundary_and_the_summary_in_claudes_shape_and_no_turn() {
        let config = tempfile::tempdir().unwrap();
        let mut records = Records::new(config.path(), "s", 7).unwrap();
        compact(&mut records, 7).unwrap();
        let written: Vec<serde_json::Value> =
            std::fs::read_to_string(config.path().join("projects/integration/s.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        assert_eq!(written.len(), 2);
        assert_eq!(
            (&written[0]["type"], &written[0]["subtype"]),
            (&json!("system"), &json!("compact_boundary"))
        );
        assert_eq!(written[1]["isCompactSummary"], true);
        assert!(written.iter().all(|record| record["type"] != "assistant"));
        let status: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(records.status_file()).unwrap()).unwrap();
        assert_eq!(status["status"], "idle");
    }

    #[test]
    fn a_wait_is_the_milliseconds_a_variable_says_and_none_where_it_says_none() {
        assert_eq!(wait_of(Some("1500")), Duration::from_millis(1500));
        assert_eq!(wait_of(Some("0")), Duration::ZERO);
        for text in [None, Some(""), Some("soon"), Some("-5"), Some("1.5")] {
            assert_eq!(wait_of(text), Duration::ZERO, "{text:?}");
        }
    }
}
