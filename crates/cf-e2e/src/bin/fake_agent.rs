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
//! It is the program of the suites that stands in a window, so it reads the
//! environment a window is given and runs `cf` as an agent does, both through
//! `cf_e2e::process`, the one place a program starts and the environment is read.

// The parts are in `fake_agent/`: cargo looks for the modules of a binary's
// root file beside it, where each would be a binary of its own.
#[path = "fake_agent/args.rs"]
mod args;
#[path = "fake_agent/records.rs"]
mod records;
#[path = "fake_agent/reply.rs"]
mod reply;
#[path = "fake_agent/terminal.rs"]
mod terminal;

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;

use cf_e2e::process::own_var;
use serde_json::json;

use records::{note_process, Records};
use reply::{is_session_of, reply_to, Reply, Window};
use terminal::{Event, Input};

/// What a window with no login says on its screen, as Pi says it on a machine
/// that has none.
const NO_LOGIN: &str =
    "No API key found for the selected model.\r\nUse /login to log into a provider.\r\n";

/// What a window the provider refuses says, in the record Claude writes for it.
const LIMIT: &str = "You've hit your limit. Resets in 2 hours.";

fn main() -> ExitCode {
    // Signals are waited for on a thread of their own, so they are blocked in
    // this one and in every thread started after it.
    let signals = block_signals();
    let args: Vec<String> = std::env::args().skip(1).collect();
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

    let window = Window {
        participant,
        settings: launch.settings.clone(),
        session: launch.session.clone(),
    };
    let (turns, queued) = mpsc::channel::<String>();
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

    // Raw input, like a real TUI: a bracketed paste is text, Enter outside one submits.
    terminal::raw_mode();
    let mut input = Input::default();
    let mut bytes = Utf8::default();
    let mut chunk = [0_u8; 4096];
    let mut stdin = io::stdin().lock();
    loop {
        let read = match stdin.read(&mut chunk) {
            Ok(0) | Err(_) => leave(&status_file, 0),
            Ok(read) => read,
        };
        for event in input.feed(&bytes.decode(&chunk[..read])) {
            match event {
                Event::Submit(text) => {
                    let _ = turns.send(text);
                }
                // A real TUI draws what was pasted, and a paste's Enter waits for that.
                Event::Pasted(characters) => {
                    let _ = say(&format!("[pasted, {characters} characters]\n"));
                }
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

/// One message: the window is busy, records the message, answers it, and is idle.
fn take_turn(records: &mut Records, pid: u32, window: &Window, text: &str) -> Result<(), String> {
    let failed = |cause: io::Error| cause.to_string();
    records.status(pid, "busy").map_err(failed)?;
    records
        .append(
            pid,
            json!({ "type": "user", "message": { "role": "user", "content": text } }),
        )
        .map_err(failed)?;
    let reply = reply_to(text, window)?;
    // The message id numbers the records so far, this one not yet among them.
    let id = format!("{}-message-{pid}-{}", window.session, records.ordinal());
    match reply {
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
        if signals.wait().is_ok() {
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
    fn bytes_that_are_no_character_are_the_replacement_character() {
        let mut bytes = Utf8::default();
        assert_eq!(bytes.decode(b"a\xffb"), "a\u{fffd}b");
    }
}
