//! Live: a command typed at an idle Claude's prompt leaves the window at rest, as the
//! daemon reads it. It is opt-in (`cargo xtask live commands-at-rest`, `npm run
//! live:commands-at-rest`): it spends the machine's quota on a window of Claude Code, and
//! needs it installed and logged in, so it is ignored unless asked for and is no part of
//! `check` or of CI.
//!
//! The Mac's live long run of 2026-10-19 (Claude Code 2.1.303): `/compact` was typed into
//! the idle chief, which compacted and went back to its prompt, and ConsensFlow read it at
//! work for the 80 minutes left: Claude Code writes a command typed at its prompt as user's
//! records (the bare `/compact`, the words before a command, the command) and then what it
//! printed, which held a `PostCompact` hook's words, and runs no model.
//!
//! One window, in a folder of its own, on Claude's cheap model, with no MCP server, no
//! browser, and none of the owner's settings (`--setting-sources local`): its hooks are
//! ConsensFlow's and the run's own, which print words of their own (`PostCompact`, and
//! `SessionStart` as the conversation begins, again after a compaction or a `/clear`). A
//! first message, answered; then each command, typed as a person types it (its keys, then
//! Enter) at the window idle; once Claude's status has read idle for a few seconds, the
//! window must read at rest as the daemon reads it: Claude's status idle, and its record
//! settled. Then a last message, answered, and at rest.
//!
//! - `compact`    `/compact`, which prints the `PostCompact` hook's words
//! - `model`      `/model` of the model the window runs on
//! - `effort`     `/effort medium`
//! - `clear`      `/clear`: the window shows a new conversation, which its status names
//! - `cancelled`  `/compact`, and Escape a second later: a finding (what Claude wrote of
//!   it, and how it reads), never a failure
//!
//! ```text
//! cargo xtask live commands-at-rest                           every command
//! cargo xtask live commands-at-rest --command compact --command clear
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::claude::{self, Entry};
use cf_e2e::live::members::named;
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window};
use cf_e2e::live::{self, agents, ask, Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows: no
/// window of this test runs it (it is opened on the real harness's own path), but a rig
/// has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The folder of the check, below the live folders.
const FOLDER: &str = "commands-at-rest";

/// What the run's hooks print: made-up words, which no one's settings say.
const POST_COMPACT: &str = "probe-post-compact-hook-says-the-conversation-was-compacted";
const SESSION_START: &str = "probe-session-start-hook-says-the-conversation-began";

/// How long a command may take to be done (a compaction is minutes), how long Claude's
/// status must have read idle for it to count as done, and how often it is read.
const DONE: Duration = Duration::from_secs(600);
const HELD: Duration = Duration::from_secs(5);
const LOOK: Duration = Duration::from_millis(250);

/// The gap between a command's keys and its Enter, as a person types, and before the
/// Escape of `cancelled`.
const KEYED: Duration = Duration::from_millis(600);
const CANCEL_AFTER: Duration = Duration::from_secs(1);

/// A command typed at the prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Compact,
    Model,
    Effort,
    Clear,
    Cancelled,
}

impl Command {
    const ALL: [Command; 5] = [
        Command::Compact,
        Command::Model,
        Command::Effort,
        Command::Clear,
        Command::Cancelled,
    ];

    const fn name(self) -> &'static str {
        match self {
            Command::Compact => "compact",
            Command::Model => "model",
            Command::Effort => "effort",
            Command::Clear => "clear",
            Command::Cancelled => "cancelled",
        }
    }

    /// What is typed, for a window that runs on `model`.
    fn typed(self, model: &str) -> String {
        match self {
            Command::Compact | Command::Cancelled => "/compact".to_owned(),
            Command::Model => format!("/model {model}"),
            Command::Effort => "/effort medium".to_owned(),
            Command::Clear => "/clear".to_owned(),
        }
    }

    /// Whether what comes of it is a finding, which fails nothing.
    fn finding(self) -> bool {
        self == Command::Cancelled
    }
}

/// The run's own hooks, which print: `PostCompact`, and `SessionStart`.
fn hooks() -> Value {
    let say = |words: &str| json!([{ "hooks": [{ "type": "command", "command": format!("echo {words}") }] }]);
    json!({ "PostCompact": say(POST_COMPACT), "SessionStart": say(SESSION_START) })
}

/// The status file of the running Claude whose status names `session`, under the
/// config folder `config`: the window's, which names the conversation it shows.
fn status_file(config: &Path, session: &str) -> Option<PathBuf> {
    fs::read_dir(config.join("sessions"))
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|file| {
            file.extension()
                .is_some_and(|extension| extension == "json")
        })
        .find(|file| {
            let status = read(file);
            status["sessionId"] == session
                && status["pid"]
                    .as_u64()
                    .and_then(|pid| u32::try_from(pid).ok())
                    .is_some_and(cf_e2e::process::is_alive)
        })
}

/// What a status file says: nothing where it cannot be read.
fn read(file: &Path) -> Value {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// Until Claude's status has read idle for [`HELD`], within [`DONE`]: whether it did.
fn idle_and_held(status: &Path) -> bool {
    let (mut since, started) = (None::<Instant>, Instant::now());
    while started.elapsed() < DONE {
        if read(status)["status"] == "idle" {
            let since = *since.get_or_insert_with(Instant::now);
            if since.elapsed() >= HELD {
                return true;
            }
        } else {
            since = None;
        }
        sleep(LOOK);
    }
    false
}

/// A record as a line of the report: what it is, and how its words open.
fn shape(entry: &Entry) -> String {
    let opening: String = entry.text.replace('\n', " ").chars().take(60).collect();
    format!("{} {opening}", entry.kind)
}

/// What came of one command: the lines that say it, and whether the window read at rest.
struct Came {
    at_rest: bool,
    lines: Vec<String>,
}

/// Types `command` into the idle `window` (whose status file is `status`), waits for it
/// to be done, and reads the window as the daemon reads it.
fn type_command(window: &Window, status: &Path, command: Command, model: &str) -> Result<Came> {
    let config = window.claude_config();
    let before = window.session().unwrap_or_default();
    let mark = claude::record(&config, &before).len();
    window.press(command.typed(model).as_bytes())?;
    sleep(KEYED);
    window.press(b"\r")?;
    if command == Command::Cancelled {
        sleep(CANCEL_AFTER);
        window.press(&[27])?;
    }
    sleep(KEYED);
    let done = idle_and_held(status);
    // A /clear has the window show a conversation of its own, which its status names.
    let session = read(status)["sessionId"]
        .as_str()
        .map_or(before.clone(), str::to_owned);
    if session != before {
        window.follow(&session);
    }
    let written = if session == before {
        claude::record(&config, &session)
            .into_iter()
            .skip(mark)
            .collect()
    } else {
        claude::record(&config, &session)
    };
    let reading = window.record();
    let settled = reading.as_ref().is_some_and(|record| record.settled);
    let mut lines = vec![format!(
        "typed `{}`; Claude's status {}; the record reads {}",
        command.typed(model),
        if done {
            "read idle after it"
        } else {
            "never held idle"
        },
        reading
            .as_ref()
            .map_or("nothing", |record| record.settlement.as_str())
    )];
    if session != before {
        lines.push(format!("the window shows a new conversation: {session}"));
    }
    lines.extend(written.iter().map(|entry| format!("  {}", shape(entry))));
    if command == Command::Compact {
        let printed = written
            .iter()
            .any(|entry| entry.text.contains(POST_COMPACT));
        lines.push(format!(
            "the PostCompact hook's words are {}in what it wrote",
            if printed { "" } else { "not " }
        ));
    }
    Ok(Came {
        at_rest: done && settled,
        lines,
    })
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of Claude Code; cargo xtask live commands-at-rest (npm run live:commands-at-rest)"]
fn each_command_typed_at_an_idle_claudes_prompt_leaves_the_window_at_rest() -> Result {
    let log = Log::begin("live commands-at-rest")?;
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let commands = named(
        &live::list("command"),
        &Command::ALL,
        Command::name,
        "command",
        &Command::ALL,
    )?;
    let claude = log.found(Harness::Claude)?;
    let model = agents::agent(Harness::Claude)["model"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let rig = Rig::start(live::rig(std::slice::from_ref(&claude), FAKE_AGENT))?;
    let window = Open::new(&rig, &claude, FOLDER, FOLDER)
        .claude_alone_with_hooks(hooks())
        .open_telling(&log, "claude")?;
    window
        .ready()
        .map_err(|screen| Error::Daemon(format!("the window did not open: {screen}")))?;
    let first = window.send(
        &window.given("Reply with exactly one line: READY")?,
        "READY",
    )?;
    if !first.shown {
        return Err(Error::Daemon(format!(
            "the window did not answer its first message: {}",
            window.tail()
        )));
    }
    let session = window.session().unwrap_or_default();
    let status = live::until(Duration::from_secs(30), LOOK, || {
        status_file(&window.claude_config(), &session)
    })
    .ok_or_else(|| Error::Daemon("Claude wrote no status of the window".to_owned()))?;
    let mut checks = Vec::new();
    for command in commands {
        if !idle_and_held(&status) {
            checks.push(Check::failed(
                "claude",
                command.name(),
                "the window was never idle to type it into",
            ));
            continue;
        }
        let came = type_command(&window, &status, command, &model)?;
        log.block(&format!("== {}", command.name()), &came.lines.join("\n"));
        let said = if came.at_rest {
            "at rest after it"
        } else {
            "not at rest after it"
        };
        checks.push(Check::new(
            "claude",
            command.name(),
            came.at_rest || command.finding(),
            if command.finding() {
                format!("{said} (a finding)")
            } else {
                said.to_owned()
            },
        ));
    }
    // And the window takes a message after them all, and is at rest after its answer.
    let last = window.send(
        &window.given("Reply with exactly one line: AFTER")?,
        "AFTER",
    )?;
    let rested = idle_and_held(&status) && window.record().is_some_and(|record| record.settled);
    checks.push(Check::new(
        "claude",
        "after",
        last.shown && rested,
        format!(
            "a message after the commands was {}answered, and the window is {}at rest",
            if last.shown { "" } else { "not " },
            if rested { "" } else { "not " }
        ),
    ));
    drop(window);
    rig.close()?;
    report::conclude(&log, "what each command came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn every_command_is_typed_unless_some_are_named_and_one_that_is_none_is_refused() {
        let all = named(&[], &Command::ALL, Command::name, "command", &Command::ALL).unwrap();
        assert_eq!(all, Command::ALL);
        let some = named(
            &words(&["clear", "compact"]),
            &Command::ALL,
            Command::name,
            "command",
            &Command::ALL,
        )
        .unwrap();
        assert_eq!(some, [Command::Clear, Command::Compact]);
        let refused = named(
            &words(&["cost"]),
            &Command::ALL,
            Command::name,
            "command",
            &Command::ALL,
        );
        assert_eq!(
            refused.err().map(|failed| failed.to_string()).as_deref(),
            Some("no such command: cost")
        );
    }

    #[test]
    fn each_command_is_typed_as_a_person_types_it_and_only_the_cancelled_one_is_a_finding() {
        let typed: Vec<String> = Command::ALL
            .iter()
            .map(|command| command.typed("claude-sonnet-5-5"))
            .collect();
        assert_eq!(
            typed,
            [
                "/compact",
                "/model claude-sonnet-5-5",
                "/effort medium",
                "/clear",
                "/compact"
            ]
        );
        assert_eq!(
            Command::ALL
                .iter()
                .filter(|command| command.finding())
                .count(),
            1
        );
        assert!(Command::Cancelled.finding());
    }

    #[test]
    fn the_runs_hooks_print_words_of_their_own_after_a_compaction_and_as_a_conversation_begins() {
        let hooks = hooks();
        let command = |event: &str| hooks[event][0]["hooks"][0]["command"].clone();
        assert_eq!(
            command("PostCompact"),
            json!(format!("echo {POST_COMPACT}"))
        );
        assert_eq!(
            command("SessionStart"),
            json!(format!("echo {SESSION_START}"))
        );
        assert_eq!(hooks.as_object().map(serde_json::Map::len), Some(2));
    }

    #[test]
    fn a_status_file_is_the_running_claudes_that_names_the_conversation() {
        let config = tempfile::tempdir().unwrap();
        let sessions = config.path().join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let mine = std::process::id();
        let write = |name: &str, status: Value| {
            fs::write(sessions.join(name), status.to_string()).unwrap();
        };
        write(
            "1.json",
            json!({ "pid": u32::MAX - 1, "sessionId": "s", "status": "idle" }),
        );
        write(
            &format!("{mine}.json"),
            json!({ "pid": mine, "sessionId": "s", "status": "idle" }),
        );
        write("2.txt", json!({ "pid": mine, "sessionId": "s" }));
        assert_eq!(
            status_file(config.path(), "s"),
            Some(sessions.join(format!("{mine}.json")))
        );
        assert_eq!(status_file(config.path(), "other"), None);
        assert_eq!(read(&sessions.join("none.json")), Value::Null);
    }
}
