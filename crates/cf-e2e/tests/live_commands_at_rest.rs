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
//! printed, which held a `PostCompact` hook's words, and runs no model. This suite's first
//! run (2026-10-20, 2.1.304) found a `/compact` with nothing to compact written another way:
//! what it printed as the system's record.
//!
//! One window, in a folder of its own, on Claude's cheap model, with no MCP server, no
//! browser, and none of the owner's settings read (`--setting-sources local`): its hooks
//! are ConsensFlow's and the run's own, which print words of their own (`PostCompact`, and
//! `SessionStart` as the conversation begins, again after a compaction or a `/clear`). A
//! first message, answered; then each command, typed as a person types it (its keys, then
//! Enter) at the window idle. Each must leave fresh evidence that it ran (below) before the
//! window is read; then, once Claude's status has read idle for a few seconds, the window
//! must read at rest as the daemon reads it: Claude's status idle, and its record settled.
//! The owner's Claude settings file must be byte for byte what it was before each command:
//! `--setting-sources local` limits what Claude reads, not what it writes, and `/model` and
//! `/effort` save themselves there as the default for new sessions (the first run changed
//! the owner's model so), which is why neither is typed here (the reader's tests hold their
//! shape). Then a last message, answered, and at rest.
//!
//! - `compact`    `/compact`: the boundary, and the `PostCompact` hook's words in what it
//!   printed
//! - `clear`      `/clear`: the window shows a new conversation, which its status names
//! - `refused`    `/clear`, then `/compact` with nothing to compact: what it printed, as the
//!   system's record
//! - `cancelled`  a message, answered, then `/compact` and Escape a second later: a record of
//!   the command, or Claude's status busy after the keys. A finding, never a failure (left
//!   for 3.0.6): its second run found Claude 2.1.304 writes of the cancel the bare `/compact`
//!   and the words before a command alone, which ConsensFlow reads as a turn at work, and
//!   puts the `/compact` back in its input box, where a message pasted next goes after it
//!   and becomes the compaction's instructions. The case says what Claude wrote and how the
//!   record reads, then sends what the box holds and says whether it was the `/compact`
//!   alone, which leaves the box empty for what comes after
//!
//! ```text
//! cargo xtask live commands-at-rest                           every command
//! cargo xtask live commands-at-rest --command compact --command cancelled
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::claude::{self, Entry};
use cf_e2e::live::members::named;
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window};
use cf_e2e::live::{self, ask, Harness, Log};
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
    Clear,
    Refused,
    Cancelled,
}

impl Command {
    const ALL: [Command; 4] = [
        Command::Compact,
        Command::Clear,
        Command::Refused,
        Command::Cancelled,
    ];

    const fn name(self) -> &'static str {
        match self {
            Command::Compact => "compact",
            Command::Clear => "clear",
            Command::Refused => "refused",
            Command::Cancelled => "cancelled",
        }
    }

    /// What is typed.
    const fn typed(self) -> &'static str {
        match self {
            Command::Clear => "/clear",
            Command::Compact | Command::Refused | Command::Cancelled => "/compact",
        }
    }

    /// Whether what comes of it is a finding, which fails nothing.
    fn finding(self) -> bool {
        self == Command::Cancelled
    }
}

/// What the run saw of a command once its keys were typed: what was written in the
/// conversation the window shows, whether that is a new one, and whether Claude's status
/// read busy.
struct Seen {
    written: Vec<Entry>,
    new_conversation: bool,
    busy: bool,
}

/// Whether what was seen is fresh evidence that `command` ran: a window whose status and
/// record are as they were before the keys says nothing of a command, which may never
/// have been taken.
fn ran(command: Command, seen: &Seen) -> bool {
    let wrote = |kind: &str| seen.written.iter().any(|entry| entry.kind == kind);
    let the_command = || {
        seen.written.iter().any(|entry| {
            entry.kind == "user"
                && (entry.text == "/compact"
                    || entry
                        .text
                        .starts_with("<command-name>/compact</command-name>"))
        })
    };
    match command {
        Command::Compact => {
            wrote("system/compact_boundary")
                && seen
                    .written
                    .iter()
                    .any(|entry| entry.text.contains(POST_COMPACT))
        }
        Command::Clear => seen.new_conversation,
        Command::Refused => wrote("system/local_command"),
        Command::Cancelled => the_command() || seen.busy,
    }
}

/// The run's own hooks, which print: `PostCompact`, and `SessionStart`.
fn hooks() -> Value {
    let say = |words: &str| json!([{ "hooks": [{ "type": "command", "command": format!("echo {words}") }] }]);
    json!({ "PostCompact": say(POST_COMPACT), "SessionStart": say(SESSION_START) })
}

/// The owner's Claude settings file under the config folder `config`, as its bytes: none
/// where there is none.
fn settings_of(config: &Path) -> Option<Vec<u8>> {
    fs::read(config.join("settings.json")).ok()
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

/// Types `words` into the window, then Enter, as a person types.
fn type_keys(window: &Window, words: &str) -> Result {
    window.press(words.as_bytes())?;
    sleep(KEYED);
    window.press(b"\r")?;
    Ok(())
}

/// After a cancel, the human sends what Claude's input box holds (Enter, nothing typed):
/// says what that was, a `/compact` put back alone, or nothing sent, and waits for Claude
/// to be idle after it, the box empty for what comes after.
fn send_what_the_box_holds(window: &Window, status: &Path, lines: &mut Vec<String>) -> Result {
    let session = window.session().unwrap_or_default();
    let config = window.claude_config();
    let mark = claude::record(&config, &session).len();
    window.press(b"\r")?;
    let sent = live::until(Duration::from_secs(30), LOOK, || {
        claude::record(&config, &session)
            .into_iter()
            .skip(mark)
            .find(|entry| entry.kind == "user")
    });
    lines.push(match &sent {
        Some(entry) if entry.text == "/compact" => {
            "its input box held the `/compact`, put back alone: sent, it wrote `/compact`"
                .to_owned()
        }
        Some(entry) => format!(
            "its input box held something else: sent, it wrote {}",
            shape(entry)
        ),
        None => "its input box held nothing that was sent".to_owned(),
    });
    if !idle_and_held(status) {
        lines.push("Claude never held idle after what the box held was sent".to_owned());
    }
    Ok(())
}

/// What came of one command: the lines that say it, whether the window read at rest, and
/// whether it ran, Claude held idle after it and the owner's settings were kept (all but the
/// daemon's reading of it).
struct Came {
    at_rest: bool,
    ran_and_kept: bool,
    lines: Vec<String>,
}

impl Came {
    /// What came of a command that `done` (it ran, and Claude held idle after it), whose
    /// record the daemon read `settled`, the owner's settings `kept`, said by `lines`.
    fn of(done: bool, settled: bool, kept: bool, lines: Vec<String>) -> Self {
        Self {
            at_rest: done && settled && kept,
            ran_and_kept: done && kept,
            lines,
        }
    }
}

/// What a command needs before it is typed: a conversation just cleared for `refused`,
/// and one with an exchange to compact for `cancelled`. Whether it was had.
fn prepare(window: &Window, status: &Path, command: Command) -> Result<bool> {
    match command {
        Command::Refused => {
            let before = window.session().unwrap_or_default();
            type_keys(window, "/clear")?;
            let cleared = live::until(DONE, LOOK, || {
                read(status)["sessionId"]
                    .as_str()
                    .filter(|session| *session != before)
                    .map(str::to_owned)
            });
            match cleared {
                Some(session) => window.follow(&session),
                None => return Ok(false),
            }
        }
        Command::Cancelled => {
            let sent = window.send(
                &window.given("Reply with exactly one line: AGAIN")?,
                "AGAIN",
            )?;
            if !sent.shown {
                return Ok(false);
            }
        }
        Command::Compact | Command::Clear => {}
    }
    Ok(idle_and_held(status))
}

/// Types `command` into the idle `window` (whose status file is `status`), waits for fresh
/// evidence that it ran and for Claude to be idle after it, and reads the window as the
/// daemon reads it; the owner's settings must be as they were.
fn type_command(window: &Window, status: &Path, command: Command) -> Result<Came> {
    if !prepare(window, status, command)? {
        return Ok(Came::of(
            false,
            false,
            true,
            vec![format!(
                "what `{}` needs first did not come: {}",
                command.name(),
                window.tail()
            )],
        ));
    }
    let config = window.claude_config();
    let settings = settings_of(&config);
    let before = window.session().unwrap_or_default();
    let mark = claude::record(&config, &before).len();
    type_keys(window, command.typed())?;
    if command == Command::Cancelled {
        sleep(CANCEL_AFTER);
        window.press(&[27])?;
    }
    let mut busy = false;
    let look = || {
        // A /clear has the window show a conversation of its own, which its status names.
        let now = read(status);
        let session = now["sessionId"]
            .as_str()
            .map_or(before.clone(), str::to_owned);
        let written: Vec<Entry> = claude::record(&config, &session)
            .into_iter()
            .skip(if session == before { mark } else { 0 })
            .collect();
        (session, written, now["status"] == "busy")
    };
    let seen = live::until(DONE, LOOK, || {
        let (session, written, now_busy) = look();
        busy |= now_busy;
        let seen = Seen {
            written,
            new_conversation: session != before,
            busy,
        };
        ran(command, &seen).then_some((session, seen))
    });
    let done = seen.is_some() && idle_and_held(status);
    let (session, written, _) = look();
    if session != before {
        window.follow(&session);
    }
    let reading = window.record();
    let settled = reading.as_ref().is_some_and(|record| record.settled);
    let kept = settings_of(&config) == settings;
    let mut lines = vec![format!(
        "typed `{}`; {}; Claude's status {}; the record reads {}",
        command.typed(),
        if seen.is_some() {
            "it ran"
        } else {
            "no fresh evidence that it ran"
        },
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
    if !kept {
        lines.push(format!(
            "FAIL Claude changed the owner's settings, {}",
            config.join("settings.json").display()
        ));
    }
    if command == Command::Cancelled {
        send_what_the_box_holds(window, status, &mut lines)?;
    }
    Ok(Came::of(done, settled, kept, lines))
}

/// The check of `command` from what came of it: it holds where the window read at rest
/// after it. For the one whose outcome is a finding (`cancelled`), it holds too where only
/// the daemon's reading is unsettled: the command ran, Claude held idle after it, and the
/// owner's settings were kept. A message before it that never answered, a command that
/// never ran or never left Claude idle, and settings Claude changed fail it, finding or not
/// (Calliope's review of compact-rest round 3).
fn check_of(command: Command, came: &Came) -> Check {
    let finding = command.finding() && came.ran_and_kept && !came.at_rest;
    let said = if came.at_rest {
        "it ran, and the window is at rest after it"
    } else if finding {
        "it ran, Claude held idle after it and the owner's settings were kept, but the window does not read at rest"
    } else {
        "not at rest after it, or it never ran, or the owner's settings changed"
    };
    Check::new(
        "claude",
        command.name(),
        came.at_rest || finding,
        if command.finding() {
            format!("{said} (a finding, left for 3.0.6: its lines above say what Claude wrote and what its box held)")
        } else {
            said.to_owned()
        },
    )
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
        let came = type_command(&window, &status, command)?;
        log.block(&format!("== {}", command.name()), &came.lines.join("\n"));
        checks.push(check_of(command, &came));
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
        // `/model` and `/effort` save themselves in the owner's settings: never typed here.
        for none in ["model", "effort", "cost"] {
            let refused = named(
                &words(&[none]),
                &Command::ALL,
                Command::name,
                "command",
                &Command::ALL,
            );
            assert_eq!(
                refused.err().map(|failed| failed.to_string()),
                Some(format!("no such command: {none}"))
            );
        }
    }

    #[test]
    fn the_cancelled_finding_passes_only_where_it_ran_held_idle_and_kept_the_owners_settings() {
        // Every outcome of a command: whether it was done (it ran, and Claude held idle after
        // it), whether the daemon read it settled, whether the owner's settings were kept.
        for done in [false, true] {
            for settled in [false, true] {
                for kept in [false, true] {
                    let came = Came::of(done, settled, kept, Vec::new());
                    for command in Command::ALL {
                        // At rest holds for any command; the finding (`cancelled`) holds too
                        // where only the daemon's reading is unsettled. A prep message that
                        // never answered, a command never done, or the settings changed
                        // fail it all the same.
                        let holds = (done && settled && kept)
                            || (command == Command::Cancelled && done && kept);
                        let check = check_of(command, &came);
                        assert_eq!(
                            check.ok, holds,
                            "{command:?} done {done} settled {settled} kept {kept}: {}",
                            check.detail
                        );
                    }
                }
            }
        }
        let finding = check_of(Command::Cancelled, &Came::of(true, false, true, Vec::new()));
        assert!(
            finding.detail.starts_with("it ran, Claude held idle"),
            "{}",
            finding.detail
        );
        let failed = check_of(
            Command::Cancelled,
            &Came::of(true, false, false, Vec::new()),
        );
        assert!(
            failed.detail.starts_with("not at rest"),
            "{}",
            failed.detail
        );
    }

    #[test]
    fn each_command_is_typed_as_a_person_types_it() {
        let typed: Vec<&str> = Command::ALL.iter().map(|command| command.typed()).collect();
        assert_eq!(typed, ["/compact", "/clear", "/compact", "/compact"]);
    }

    #[test]
    fn only_the_cancelled_compaction_is_a_finding_which_fails_nothing() {
        // Left for 3.0.6: ConsensFlow reads a compaction cancelled with Escape as a turn at
        // work, and Claude puts the `/compact` back in its input box.
        let findings: Vec<Command> = Command::ALL
            .into_iter()
            .filter(|command| command.finding())
            .collect();
        assert_eq!(findings, [Command::Cancelled]);
    }

    /// A record of the kind `kind`, saying `text`.
    fn entry(kind: &str, text: &str) -> Entry {
        Entry {
            at_ms: None,
            kind: kind.to_owned(),
            text: text.to_owned(),
            model: None,
            raw: String::new(),
        }
    }

    #[test]
    fn a_command_ran_only_where_it_left_fresh_evidence_of_its_own() {
        let seen = |written: Vec<Entry>, new_conversation: bool, busy: bool| Seen {
            written,
            new_conversation,
            busy,
        };
        let nothing = || seen(Vec::new(), false, false);
        // An unchanged window says nothing of any command.
        for command in Command::ALL {
            assert!(!ran(command, &nothing()), "{command:?}");
        }
        let boundary = || entry("system/compact_boundary", "");
        let printed = |words: &str| entry("user", &format!("<local-command-stdout>{words}"));
        assert!(ran(
            Command::Compact,
            &seen(vec![boundary(), printed(POST_COMPACT)], false, false)
        ));
        assert!(!ran(
            Command::Compact,
            &seen(vec![boundary()], false, false)
        ));
        assert!(!ran(
            Command::Compact,
            &seen(vec![printed(POST_COMPACT)], false, false)
        ));
        assert!(ran(Command::Clear, &seen(Vec::new(), true, false)));
        assert!(!ran(Command::Clear, &seen(vec![boundary()], false, true)));
        assert!(ran(
            Command::Refused,
            &seen(vec![entry("system/local_command", "")], false, false)
        ));
        assert!(!ran(
            Command::Refused,
            &seen(vec![printed("x")], false, true)
        ));
        for written in [
            vec![entry("user", "/compact")],
            vec![entry(
                "user",
                "<command-name>/compact</command-name> ⏎ <command-args></command-args>",
            )],
        ] {
            assert!(ran(Command::Cancelled, &seen(written, false, false)));
        }
        assert!(ran(Command::Cancelled, &seen(Vec::new(), false, true)));
        assert!(!ran(
            Command::Cancelled,
            &seen(
                vec![entry("user", "<command-name>/clear</command-name>")],
                false,
                false
            )
        ));
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
    fn the_owners_settings_are_read_as_their_bytes_and_as_none_where_there_are_none() {
        let config = tempfile::tempdir().unwrap();
        assert_eq!(settings_of(config.path()), None);
        fs::write(config.path().join("settings.json"), b"{\"model\":\"x\"}").unwrap();
        assert_eq!(
            settings_of(config.path()),
            Some(b"{\"model\":\"x\"}".to_vec())
        );
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
