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
//! With `CF_TEST_MCP_SERVER` set, every window starts a job as it opens, before
//! any turn: a server the harness started with it. And the program is the job
//! too (`background`): started with `--background-job` first, it is not a
//! window but the work a window left running.
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
//! in the record: quit the app, as a human may at any moment.
//!
//! It is the program of the suites that stands in a window, so it reads the
//! environment a window is given and runs `cf` as an agent does, both through
//! `cf_e2e::process`, the one place a program starts and the environment is read.

// The parts are in `fake_agent/`: cargo looks for the modules of a binary's
// root file beside it, where each would be a binary of its own.
#[path = "fake_agent/args.rs"]
mod args;
#[path = "fake_agent/background.rs"]
mod background;
#[path = "fake_agent/records.rs"]
mod records;
#[path = "fake_agent/reply.rs"]
mod reply;
#[cfg(unix)]
#[path = "fake_agent/signals.rs"]
mod signals;
#[path = "fake_agent/stop.rs"]
mod stop;
#[path = "fake_agent/terminal.rs"]
mod terminal;

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use cf_e2e::pattern;
use cf_e2e::process::own_var;
use regex::Regex;
use serde_json::json;

use background::Jobs;
use records::{note_process, Records};
use reply::{is_session_of, reply_to, Reply, Window};
use stop::Stop;
use terminal::{Event, Input};

/// How long the server a window starts as it opens runs: past any case.
const SERVER_SECONDS: u64 = 600;

/// How long an Escape waits for the rest of a paste marker before it is the
/// Escape key: a TUI's own wait is about as long.
const ESCAPE_WAIT: Duration = Duration::from_millis(50);

/// What Claude writes as the user's turn when it interrupts one it was at work on.
const INTERRUPTED: &str = "[Request interrupted by user]";

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
    // Signals are waited for on a thread of their own, so they are blocked in
    // this one and in every thread started after it.
    let signals = block_signals();
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
fn run(launch: &args::Launch, signals: Signals) -> Result<std::convert::Infallible, String> {
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
    leave_on_signal(signals, status_file.clone());

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
        jobs: Jobs::new(Path::new(&home), pid),
        wake: turns.clone(),
        stop: Arc::clone(&stop),
    };
    // A server the harness starts with its window, before any turn, as an MCP
    // server is.
    if own_var("CF_TEST_MCP_SERVER").is_some_and(|server| !server.is_empty()) {
        window.jobs.start("server", SERVER_SECONDS, false, || {})?;
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
    terminal::raw_mode();
    let (read, incoming) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        let mut stdin = io::stdin().lock();
        while let Ok(read_now @ 1..) = stdin.read(&mut chunk) {
            if read.send(chunk[..read_now].to_vec()).is_err() {
                return;
            }
        }
    });
    let mut input = Input::default();
    let mut bytes = Utf8::default();
    loop {
        let events = match incoming.recv_timeout(ESCAPE_WAIT) {
            Ok(chunk) => input.feed(&bytes.decode(&chunk)),
            Err(RecvTimeoutError::Timeout) => input.quiet(),
            // The terminal is gone.
            Err(RecvTimeoutError::Disconnected) => leave(&status_file, 0),
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

/// One message: the window works on it and is idle again once it is answered
/// or an Escape stopped it.
fn take_turn(records: &mut Records, pid: u32, window: &Window, text: &str) -> Result<(), String> {
    window.stop.begin();
    let done = turn(records, pid, window, text);
    window.stop.end();
    done
}

/// A turn: the window is busy (not in the hooks of its prompt, where Claude's
/// status reads idle), records the message, answers it, and is idle.
fn turn(records: &mut Records, pid: u32, window: &Window, text: &str) -> Result<(), String> {
    let failed = |cause: io::Error| cause.to_string();
    // Only the first message of the window is in the hooks of its prompt: the
    // words that go in later, a brief they carry among them, do not put it back.
    let hooks = hooks_of(text).filter(|_| records.ordinal() == 0);
    if hooks.is_none() {
        records.status(pid, "busy").map_err(failed)?;
    }
    records
        .append(
            pid,
            json!({ "type": "user", "message": { "role": "user", "content": text } }),
        )
        .map_err(failed)?;
    if let Some(hooks) = hooks {
        // Stopped there, the turn leaves nothing of itself: Claude puts the
        // message back in its input box, and its status never left idle.
        if window.stop.work(hooks) {
            return Ok(());
        }
        records.status(pid, "busy").map_err(failed)?;
    }
    let reply = reply_to(text, window)?;
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
        }
    }
    records.status(pid, "idle").map_err(failed)
}

/// Bytes read in pieces, decoded to text as they come: a character cut in two
/// by the pieces is held until the rest arrives.
#[derive(Default)]
struct Utf8 {
    held: Vec<u8>,
}

impl Utf8 {
    fn decode(&mut self, chunk: &[u8]) -> String {
        self.held.extend_from_slice(chunk);
        let mut text = String::new();
        loop {
            match std::str::from_utf8(&self.held) {
                Ok(valid) => {
                    text.push_str(valid);
                    self.held.clear();
                    return text;
                }
                Err(cut) => {
                    let (valid, rest) = self.held.split_at(cut.valid_up_to());
                    text.push_str(&String::from_utf8_lossy(valid));
                    match cut.error_len() {
                        // Not a character at all: the replacement character, and on.
                        Some(length) => {
                            text.push('\u{fffd}');
                            self.held = rest[length..].to_vec();
                        }
                        // A character cut short at the end: kept for the next piece.
                        None => {
                            self.held = rest.to_vec();
                            return text;
                        }
                    }
                }
            }
        }
    }
}

/// The signals a window ends on: SIGHUP when its terminal hangs up and SIGTERM
/// when it is asked to end. Windows has neither.
#[cfg(unix)]
type Signals = nix::sys::signal::SigSet;
#[cfg(not(unix))]
struct Signals;

/// Blocks SIGHUP and SIGTERM in this thread (and so in every thread it starts),
/// for [`leave_on_signal`] to wait for.
#[cfg(unix)]
fn block_signals() -> Signals {
    use nix::sys::signal::{SigSet, Signal};

    let mut signals = SigSet::empty();
    signals.add(Signal::SIGHUP);
    signals.add(Signal::SIGTERM);
    let _ = signals.thread_block();
    signals
}

#[cfg(not(unix))]
fn block_signals() -> Signals {
    Signals
}

/// Ends the window when its terminal hangs up or it is asked to end: the status
/// file goes, and the code is 0, as the Node stand-in's was.
#[cfg(unix)]
fn leave_on_signal(signals: Signals, status_file: PathBuf) {
    thread::spawn(move || {
        if signals::wait(&signals) {
            leave(&status_file, 0);
        }
    });
}

#[cfg(not(unix))]
fn leave_on_signal(_: Signals, _: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_read_in_pieces_is_whole_and_a_cut_character_waits_for_its_rest() {
        let mut bytes = Utf8::default();
        let text = "a · b";
        let (first, second) = text.as_bytes().split_at(3);
        // The middle dot is two bytes; the first piece ends in the middle of it.
        assert_eq!(bytes.decode(first), "a ");
        assert_eq!(bytes.decode(second), "· b");
        assert_eq!(bytes.decode(b""), "");
    }

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
    fn a_wait_is_the_milliseconds_a_variable_says_and_none_where_it_says_none() {
        assert_eq!(wait_of(Some("1500")), Duration::from_millis(1500));
        assert_eq!(wait_of(Some("0")), Duration::ZERO);
        for text in [None, Some(""), Some("soon"), Some("-5"), Some("1.5")] {
            assert_eq!(wait_of(text), Duration::ZERO, "{text:?}");
        }
    }

    #[test]
    fn bytes_that_are_no_character_are_the_replacement_character() {
        let mut bytes = Utf8::default();
        assert_eq!(bytes.decode(b"a\xffb"), "a\u{fffd}b");
    }
}
