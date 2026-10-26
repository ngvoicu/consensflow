//! A stand-in for the `claude` CLI in the integration tests. It speaks Claude's
//! launch arguments, writes Claude's transcript records and its live
//! `sessions/<pid>.json` status, and reads its terminal raw, the way a real TUI
//! does: a bracketed paste followed by Enter is one message. What it answers a
//! message with is told in `reply`.
//!
//! A paste that has lines that are the paths of images it can read is made into
//! attachments, as Claude's input box makes them: a `[Image #N]` for each at the head of
//! the message, the lines gone, and a record of the paths after it (`images`).
//!
//! The window of the participant `CF_TEST_NO_LOGIN` names has no login: it
//! prints that and stays, doing nothing else (`CF_TEST_NO_LOGIN_EXITS`: and
//! ends with the code 3). A key pressed in a turn (the Escape that stops it) is
//! ignored: this window is never stopped. The windows of the member
//! `CF_TEST_SLOW_START` names (`worker 8000`) come up that many milliseconds
//! late, as a harness does on a loaded machine: nothing of one shows, its
//! status or the message it was launched with, until then.
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
//! interrupted, `[Request interrupted by user]`, and no answer (a hung one, a
//! `HANG`, takes no Escape at all); and a turn still
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
//! A message with a line `TORN <words>` tears its record as it works, as a short
//! write does: the first half of the record of a tool it runs, a moment, and the
//! tool's result appended to it, one line that is not JSON for good; then it
//! answers the message as it would have.
//! `/compact` typed alone compacts the conversation as Claude Code 2.1.303 does
//! while idle: the bare command as it is typed, its boundary and its summary, then
//! the command's own records and what it printed, in the record, and no turn; with
//! nothing said since the conversation began or was last compacted it compacts
//! nothing, as 2.1.304 does, and says so as the system's record.
//!
//! A window refused for quota with `CF_TEST_AUTO_CONTINUE_MS` set (milliseconds,
//! or `<member> <milliseconds>` for that member's windows alone) continues by
//! itself, as Claude Code 2.1.278 and later do: the refusal is followed by the
//! notice that it continues automatically, and once its account has quota again
//! (`CF_TEST_QUOTA_FILE` is gone) and that many milliseconds have passed, it says
//! the limit was reset and takes a turn on the meta prompt Claude sends
//! (`AUTO_CONTINUE`), queued behind any turn it is in, as Claude's timer is.
//!
//! The window of a participant `CF_TEST_TRUST` names, opened in a git repository it
//! was never told to trust, asks whether to trust it first, as Claude Code does
//! (`trust`); with `CF_TEST_SAYS_FIRST` it prints those words before its first line,
//! and with `CF_TEST_PLAIN_PROMPT` its first line does not say it is in
//! full-permission mode, as a prompt drawn otherwise. The window of a participant
//! `CF_TEST_QUOTES_BEFORE_PROMPT` names draws a conversation that quotes the
//! question's choices for 1.5 s before its first line, as a resumed window does,
//! the `❯` on the first with `CF_TEST_QUOTES_THE_CURSOR` (the question of a trusted
//! folder's grants with `CF_TEST_QUOTES_THE_GRANTS`), and its input box above that line. With `CF_TEST_TRUST_IDLE_WHILE_ASKING` a window says it is idle while it
//! asks, as a Claude Code that writes its status before its question would.
//!
//! The window of a participant `CF_TEST_INPUT_BOX` names draws Claude Code's input
//! box, and opens its model picker for `/model` (`input_box`); one `CF_TEST_SUGGESTION`
//! names shows a suggestion in its empty box, and one `CF_TEST_SLOW_DRAW` names draws
//! its box seconds late.
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
#[path = "fake_agent/images.rs"]
mod images;
#[path = "fake_agent/input_box.rs"]
mod input_box;
#[path = "fake_agent/records.rs"]
mod records;
#[path = "fake_agent/reply.rs"]
mod reply;
#[path = "fake_agent/signals.rs"]
mod signals;
#[path = "fake_agent/status_line.rs"]
#[cfg(unix)]
mod status_line;
#[path = "fake_agent/steps.rs"]
mod steps;
#[path = "fake_agent/stop.rs"]
mod stop;
#[path = "fake_agent/terminal.rs"]
mod terminal;
#[path = "fake_agent/tool.rs"]
mod tool;
#[path = "fake_agent/trust.rs"]
mod trust;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::pattern;
use cf_e2e::process::own_var;
use regex::Regex;
use serde_json::json;

use agents::Agents;
use background::{Jobs, Server, THINKING};
use records::{note_process, Records, INTERRUPTED};
use reply::{is_session_of, reply_to, Reply, Window};
use steps::Tasks;
use stop::Stop;
use terminal::{Event, Keys};

/// What a window with no login says on its screen, as Pi says it on a machine
/// that has none.
const NO_LOGIN: &str =
    "No API key found for the selected model.\r\nUse /login to log into a provider.\r\n";

/// What a window the provider refuses says, in the record Claude writes for it.
const LIMIT: &str = "You've hit your limit. Resets in 2 hours.";

/// The meta prompt Claude Code sends by itself once a refused account has quota
/// again (2.1.278 and later; this Mac's transcripts).
const AUTO_CONTINUE: &str =
    "Your claude.ai usage limit has reset. Continue the task you were working on.";

/// What the window's own timer queues as a turn when it continues by itself: no
/// message anyone types.
const AUTO_CONTINUE_MARK: &str = "\u{1}auto-continue";

/// Whether the window's timer is set to continue by itself: once at a time, as
/// Claude has one.
static CONTINUING: AtomicBool = AtomicBool::new(false);

/// A conversation that quotes the question's two choices, each a line of its own, with
/// no input box below it yet, as a list or as the dialog draws them, and how long it
/// stands before the window's prompt.
const QUOTES_THE_CHOICES: &str =
    " ⏺ The dialog offers:\r\n   - No, exit\r\n   - Yes, I trust this folder\r\n";
const QUOTES_THE_DIALOG: &str =
    " ⏺ The dialog reads:\r\n ❯ No, exit\r\n   Yes, I trust this folder\r\n";
const QUOTES_THE_GRANTS: &str = " ⏺ The dialog reads:\r\n ❯ No, continue without these permissions\r\n   Yes, I trust this folder\r\n";
const QUOTED_BEFORE_PROMPT: std::time::Duration = std::time::Duration::from_millis(1_500);

/// How long a torn record's half stays unfinished before the next record is
/// appended to it: two of the daemon's looks.
const SHORT_WRITE: Duration = Duration::from_secs(2);

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
    thread::sleep(slow_start(
        own_var("CF_TEST_SLOW_START").as_deref(),
        &participant,
    ));

    // Claude Code's question about a folder it was never told to trust comes first.
    let asking = named("CF_TEST_TRUST");
    let draws_box = named("CF_TEST_INPUT_BOX");
    let suggests = named("CF_TEST_SUGGESTION");
    let draws_slowly = named("CF_TEST_SLOW_DRAW");
    if asking {
        trust::bring_the_record_back(records.transcript());
        // A Claude Code that writes its status before its question reads at rest.
        if own_var("CF_TEST_TRUST_IDLE_WHILE_ASKING").is_some() {
            records
                .status(pid, "idle")
                .map_err(|failed| failed.to_string())?;
        }
    }
    let grows = asking && own_var("CF_TEST_RECORD_GROWS_ON_NO").is_some();
    // Its terminal is read from the start, raw, as a TUI reads it: the question
    // before the prompt takes its keys from the same reader.
    let mut keys = Keys::start();
    let answer = trust::ask(Path::new(&config), asking, &mut keys, || {
        if !grows {
            return Ok(());
        }
        // A process before this window's, which the host could not end, writes still.
        let written = json!({
            "type": "assistant",
            "message": {
                "id": format!("{}-before-{pid}", launch.session),
                "role": "assistant",
                "stop_reason": "end_turn",
                "content": [{ "type": "text", "text": "written by the process before" }],
            },
        });
        records
            .append(pid, written)
            .map_err(|failed| failed.to_string())
    })?;
    if let trust::Answer::Exit = answer {
        leave(&status_file, 0);
    }
    // Then, in a trusted folder, the dialog of a server of its `.mcp.json`.
    trust::ask_server(Path::new(&config), asking, &mut keys)?;
    if asking {
        if let Some(said) = own_var("CF_TEST_SAYS_FIRST") {
            say(&format!("{said}\n")).map_err(|failed| failed.to_string())?;
        }
    }
    // A resumed window draws its conversation before its prompt: here one that quotes
    // the question's two choices, each a line of its own, for as long as Claude Code
    // 2.1.307 took on a chief's conversation (`cargo xtask live folder-trust`).
    let quotes = named("CF_TEST_QUOTES_BEFORE_PROMPT");
    if quotes {
        let quote = if own_var("CF_TEST_QUOTES_THE_GRANTS").is_some() {
            QUOTES_THE_GRANTS
        } else if own_var("CF_TEST_QUOTES_THE_CURSOR").is_some() {
            QUOTES_THE_DIALOG
        } else {
            QUOTES_THE_CHOICES
        };
        say(quote).map_err(|failed| failed.to_string())?;
        std::thread::sleep(QUOTED_BEFORE_PROMPT);
        say(" ❯ \r\n").map_err(|failed| failed.to_string())?;
    }
    records
        .status(pid, "idle")
        .map_err(|failed| failed.to_string())?;
    let said = if launch.resuming {
        "resumed"
    } else {
        "started"
    };
    // Claude's prompt says so under its input box, in full-permission mode, unless
    // the window is one whose prompt says it otherwise.
    let mode = if asking && own_var("CF_TEST_PLAIN_PROMPT").is_some() {
        ""
    } else {
        " · bypass permissions on"
    };
    say(&format!("fake agent {said} on {}{mode}\n", launch.session))
        .map_err(|failed| failed.to_string())?;
    if let (trust::Answer::Yes, Some(after)) = (&answer, trust::leaves_after()) {
        thread::sleep(after);
        leave(&status_file, 0);
    }

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
        seeded: launch.seed.is_some(),
        tasks: Tasks::default(),
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

    // Claude Code's input box and its model picker, drawn as it draws them.
    let mut input_box = draws_box.then(|| input_box::InputBox::new(suggests, draws_slowly));

    // Raw input, like a real TUI: a bracketed paste is text, Enter outside one
    // submits, and an Escape that nothing follows is the key.
    loop {
        // The terminal is gone.
        let Some(events) = keys.read() else {
            leave(&status_file, 0)
        };
        for event in events {
            match event {
                Event::Submit(text) => {
                    if !input_box.as_mut().is_some_and(|shown| shown.takes(&text)) {
                        let _ = turns.send(text);
                    }
                }
                // A real TUI draws what was pasted, and a paste's Enter waits for that.
                Event::Pasted(characters) => {
                    let below = input_box.as_mut().map_or("", input_box::InputBox::below);
                    let _ = say(&format!("{below}[pasted, {characters} characters]\n"));
                }
                Event::Escape => {
                    if !input_box
                        .as_mut()
                        .is_some_and(|shown| shown.escape(&mut keys))
                    {
                        stop.press();
                    }
                }
                Event::Interrupt => leave(&status_file, 0),
            }
        }
        if let Some(shown) = input_box.as_mut() {
            let _ = shown.draw(keys.typed(), say);
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

/// How long after its account has quota again the window of `participant`
/// continues by itself, as `said` tells: its milliseconds for every window, or
/// for the windows of each member it names (`<member> <milliseconds>`, a comma
/// between them) alone, and none for any other, which waits for ConsensFlow to
/// resume it (an account whose windows' timers fire apart).
fn continues_after(said: &str, participant: &str) -> Option<Duration> {
    if !said.contains(' ') {
        return Some(wait_of(Some(said)));
    }
    said.split(',')
        .filter_map(|pair| pair.trim().split_once(' '))
        .find(|(member, _)| is_session_of(participant, member))
        .map(|(_, wait)| wait_of(Some(wait)))
}

/// How late the window of `participant` comes up: the milliseconds `said`
/// gives its member's windows (`<member> <milliseconds>`), and no time for any
/// other's.
fn slow_start(said: Option<&str>, participant: &str) -> Duration {
    said.and_then(|said| said.split_once(' '))
        .filter(|(member, _)| is_session_of(participant, member))
        .map_or(Duration::ZERO, |(_, wait)| wait_of(Some(wait)))
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

/// What a `/compact` that found nothing to compact prints.
const NOTHING_TO_COMPACT: &str = "There is nothing in the conversation to compact yet.";

/// What Claude Code writes before a command typed at its prompt, as a meta record.
const CAVEAT: &str = "<local-command-caveat>The messages below were made by a command the user ran; do not answer them.</local-command-caveat>";

/// One message: the window works on it and is idle again once it is answered
/// or an Escape stopped it. `/compact` typed alone is no message: the window
/// compacts its conversation as Claude Code does while idle (`compact`), and runs
/// no turn (the long run's compaction, which its watch finds in the record).
fn take_turn(records: &mut Records, pid: u32, window: &Window, text: &str) -> Result<(), String> {
    if text.trim() == COMPACT {
        return compact(records, pid);
    }
    if text == AUTO_CONTINUE_MARK {
        records
            .append(pid, notice("Usage limit reset · continuing automatically"))
            .map_err(|cause| cause.to_string())?;
    }
    // Written to the transcript alone, a notification is no turn: the window stays as it is.
    if let Some(notification) = steps::for_the_transcript_alone(text) {
        return steps::write_for_the_transcript_alone(records, pid, notification)
            .map_err(|cause| cause.to_string());
    }
    window.stop.begin();
    let done = turn(records, pid, window, text);
    window.stop.end();
    done
}

/// The conversation compacted while idle, as Claude Code 2.1.303 writes it (the
/// long run of 2026-10-19, whose chief read at work after it for 80 minutes): busy
/// while it compacts, with the bare `/compact` written as it is typed; then the
/// boundary, whose chain starts afresh, the summary, the words before the command,
/// the command and what it printed, each the child of the one before it, and the
/// hook that ran as the conversation began again; and idle. A conversation with
/// nothing said since it began or was last compacted is not compacted
/// (`nothing_to_compact`).
fn compact(records: &mut Records, pid: u32) -> Result<(), String> {
    if !records.said() {
        return nothing_to_compact(records, pid);
    }
    let failed = |cause: io::Error| cause.to_string();
    records.status(pid, "busy").map_err(failed)?;
    records
        .append(pid, user(COMPACT, json!({})))
        .map_err(failed)?;
    let boundary = json!({
        "type": "system", "subtype": "compact_boundary", "isSidechain": false,
        "parentUuid": null, "logicalParentUuid": records.last_uuid(pid),
        "content": "Conversation compacted", "level": "info",
        "compactMetadata": { "trigger": "manual" },
    });
    records.append(pid, boundary).map_err(failed)?;
    let command = compact_command();
    let hook = json!({
        "type": "attachment", "isSidechain": false,
        "attachment": { "type": "hook_success", "hookEvent": "SessionStart", "hookName": "SessionStart:compact" },
    });
    for mut record in [
        user(
            COMPACT_SUMMARY,
            json!({ "isCompactSummary": true, "isVisibleInTranscriptOnly": true }),
        ),
        user(CAVEAT, json!({ "isMeta": true })),
        user(&command, json!({})),
        user(
            "<local-command-stdout>\u{1b}[2mCompacted \u{1b}[22m</local-command-stdout>",
            json!({ "repeatsCompactSummary": true }),
        ),
        hook,
    ] {
        record["parentUuid"] = json!(records.last_uuid(pid));
        records.append(pid, record).map_err(failed)?;
    }
    records.compacted();
    records.status(pid, "idle").map_err(failed)
}

/// A `/compact` that found nothing to compact, as Claude Code 2.1.304 writes it
/// (the live run of 2026-10-20, a `/compact` typed into a conversation just
/// cleared): no bare command and no compaction; the words before the command, the
/// command, and what it printed as the system's record (`local_command`), each the
/// child of the one before it.
fn nothing_to_compact(records: &mut Records, pid: u32) -> Result<(), String> {
    let failed = |cause: io::Error| cause.to_string();
    records.status(pid, "busy").map_err(failed)?;
    let printed = json!({
        "type": "system", "subtype": "local_command", "isSidechain": false,
        "isMeta": false, "level": "info",
        "content": format!("<local-command-stdout>{NOTHING_TO_COMPACT}</local-command-stdout>"),
    });
    for mut record in [
        user(CAVEAT, json!({ "isMeta": true })),
        user(&compact_command(), json!({})),
        printed,
    ] {
        record["parentUuid"] = json!(records.last_uuid(pid));
        records.append(pid, record).map_err(failed)?;
    }
    records.compacted();
    records.status(pid, "idle").map_err(failed)
}

/// What the user's record of a `/compact` says.
fn compact_command() -> String {
    format!(
        "<command-name>{COMPACT}</command-name>\n            <command-message>compact</command-message>\n            <command-args></command-args>"
    )
}

/// A user's record of the main conversation saying `content`, with `fields` besides.
fn user(content: &str, fields: serde_json::Value) -> serde_json::Value {
    let mut record = json!({
        "type": "user", "isSidechain": false,
        "message": { "role": "user", "content": content },
    });
    if let (Some(record), serde_json::Value::Object(fields)) = (record.as_object_mut(), fields) {
        record.extend(fields);
    }
    record
}

/// The records a message `text` is written as. What wakes the window when a job it
/// started in the background ends is queued by Claude Code and taken as the turn it
/// begins, as its own: no paste, so no path in it is made an attachment. Any other message
/// is as Claude's input box keeps it: a paste that names images is attached to it, with a
/// record of their paths after it, unless it is the first message of a launch
/// (`launched`: no paste, Claude Code 2.1.308 keeps its paths as they are). The prompt
/// Claude Code sends by itself once a refused account has quota again (`meta`) is its own,
/// and says so (`isMeta`).
fn record_message(
    records: &mut Records,
    pid: u32,
    launched: bool,
    meta: bool,
    text: &str,
) -> io::Result<()> {
    if steps::is_notification(text) {
        return steps::write_notification(records, pid, text);
    }
    let written = if launched {
        vec![images::unattached(text)]
    } else {
        images::user_records(text, || records.next_image())
    };
    for mut record in written {
        if meta {
            record["isMeta"] = json!(true);
        }
        records.append(pid, record)?;
    }
    Ok(())
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
    let continuing = text == AUTO_CONTINUE_MARK;
    let text = if continuing { AUTO_CONTINUE } else { text };
    record_message(records, pid, first && window.seeded, continuing, text).map_err(failed)?;
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
    if text.lines().any(|line| line.starts_with("TORN ")) {
        tear(records, pid, window).map_err(failed)?;
    }
    // What it did before it answers: the steps a worker's progress is, among them the call
    // that runs a job in the background or arms a monitor, and what it answers.
    let (worked, reply) = match tool::reply(text, window).or_else(|| build::reply(text, window)) {
        Some(reply) => (Vec::new(), reply?),
        None => match steps::reply(text, window) {
            Some(worked) => worked?,
            None => (Vec::new(), reply_to(text, window)?),
        },
    };
    steps::write(records, pid, &window.session, &worked).map_err(failed)?;
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
            continue_later(records, pid, window).map_err(failed)?;
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
    // Claude Code says `shell` for a window at its prompt that has a job or a monitor
    // running in the background (2.1.305, the live run of 2026-10-21), and `busy` for as
    // long as an agent runs.
    let status = if window.agents.running() > 0 {
        "busy"
    } else if window.tasks.running() > 0 {
        "shell"
    } else {
        "idle"
    };
    records.status(pid, status).map_err(failed)
}

/// A notice of Claude Code's own, as the system's record.
fn notice(content: &str) -> serde_json::Value {
    json!({
        "type": "system", "subtype": "informational", "level": "notice",
        "isSidechain": false, "isMeta": false, "content": content,
    })
}

/// After a refusal, a window told to (`CF_TEST_AUTO_CONTINUE_MS`) says it will
/// continue by itself, and its timer queues the turn that does once the account
/// has quota again and the milliseconds have passed.
fn continue_later(records: &mut Records, pid: u32, window: &Window) -> io::Result<()> {
    let Some(after) = own_var("CF_TEST_AUTO_CONTINUE_MS")
        .and_then(|said| continues_after(&said, &window.participant))
    else {
        return Ok(());
    };
    records.append(
        pid,
        notice("Usage limit reached · continuing automatically at the reset · esc to cancel"),
    )?;
    if CONTINUING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let wake = window.wake.clone();
    thread::spawn(move || {
        let account = own_var("CF_TEST_QUOTA_FILE");
        while account
            .as_deref()
            .is_some_and(|file| Path::new(file).exists())
        {
            thread::sleep(Duration::from_millis(200));
        }
        thread::sleep(after);
        CONTINUING.store(false, Ordering::SeqCst);
        let _ = wake.send(AUTO_CONTINUE_MARK.to_owned());
    });
    Ok(())
}

/// A record torn as a short write tears it: the first half of the assistant's
/// call of a tool, and once a moment has passed the tool's result appended to it.
fn tear(records: &mut Records, pid: u32, window: &Window) -> io::Result<()> {
    thread::sleep(THINKING);
    let tool = format!("{}-torn-{pid}", window.session);
    records.cut_short(
        pid,
        json!({
            "type": "assistant",
            "message": {
                "id": format!("{tool}-message"),
                "role": "assistant",
                "content": [{ "type": "tool_use", "id": tool, "name": "Bash", "input": { "command": "true" } }],
            },
        }),
    )?;
    thread::sleep(SHORT_WRITE);
    records.append(
        pid,
        json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": tool, "content": "" }] },
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The records `text` is written as, in a conversation of its own.
    fn recorded(text: &str, launched: bool) -> Vec<serde_json::Value> {
        let config = tempfile::tempdir().unwrap();
        let mut records = Records::new(config.path(), "s", 7).unwrap();
        record_message(&mut records, 7, launched, false, text).unwrap();
        written_from(config.path(), 0)
    }

    #[test]
    fn a_notification_that_names_a_picture_is_written_as_a_notification_and_no_paste_attaches_it() {
        // Alpha.3 merged the notification's records (turn-end) with the images a paste is
        // made into (T-215): a notification is a turn Claude Code begins by itself, so
        // the path in its report is no attachment, where the same words pasted are one.
        let folder = tempfile::tempdir().unwrap();
        let picture = folder.path().join("hero.png");
        std::fs::write(&picture, cf_e2e::png::SMALL).unwrap();
        let report = format!("Saved:\n{}\nDone", picture.display());
        let notified = steps::notification("b1", &report);
        let written = recorded(&notified, false);
        assert_eq!(
            written
                .iter()
                .map(|record| record["operation"].as_str().or(record["type"].as_str()))
                .collect::<Vec<_>>(),
            [Some("enqueue"), Some("user")]
        );
        assert_eq!(written[1]["promptSource"], "system");
        assert_eq!(written[1]["message"]["content"], notified.as_str());
        assert!(
            !written
                .iter()
                .any(|record| record.to_string().contains("[Image")),
            "{written:?}"
        );
        // The same words pasted are attached, so that the notification's is no accident
        // of there being no picture.
        let pasted = recorded(&format!("{report}\nlook"), false);
        assert_eq!(pasted.len(), 2, "{pasted:?}");
        assert!(pasted[0].to_string().contains("[Image #1]"), "{pasted:?}");
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

    /// The records of the conversation `s` in the config folder `config`, from the
    /// `from`th on.
    fn written_from(config: &Path, from: usize) -> Vec<serde_json::Value> {
        std::fs::read_to_string(config.join("projects/integration/s.jsonl"))
            .unwrap()
            .lines()
            .skip(from)
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// A record as its subtype or type, and how its words open, to its first `>`.
    fn shape(record: &serde_json::Value) -> String {
        let content = record["message"]["content"]
            .as_str()
            .or(record["content"].as_str())
            .unwrap_or_default();
        let words = content.split('>').next().unwrap_or_default().to_owned();
        format!(
            "{}{}",
            record["subtype"]
                .as_str()
                .unwrap_or(record["type"].as_str().unwrap()),
            if words.is_empty() {
                String::new()
            } else {
                format!(" {words}")
            }
        )
    }

    /// A user's message, as a turn records it.
    fn told(records: &mut Records) {
        records
            .append(
                7,
                json!({ "type": "user", "message": { "role": "user", "content": "Hello" } }),
            )
            .unwrap();
    }

    #[test]
    fn a_compaction_writes_claude_2_1_303s_records_each_the_child_of_the_one_before_and_no_turn() {
        let config = tempfile::tempdir().unwrap();
        let mut records = Records::new(config.path(), "s", 7).unwrap();
        told(&mut records);
        compact(&mut records, 7).unwrap();
        let written = written_from(config.path(), 1);
        assert_eq!(
            written.iter().map(shape).collect::<Vec<_>>(),
            [
                "user /compact",
                "compact_boundary Conversation compacted",
                "user This session is being continued from a previous conversation that ran out of context.",
                "user <local-command-caveat",
                "user <command-name",
                "user <local-command-stdout",
                "attachment",
            ]
        );
        assert!(written.iter().all(|record| record["isSidechain"] == false));
        // The boundary starts the chain afresh, after the bare command; each record after
        // it is the child of the one before.
        assert_eq!(written[1]["parentUuid"], serde_json::Value::Null);
        assert_eq!(written[1]["logicalParentUuid"], written[0]["uuid"]);
        for pair in written[1..].windows(2) {
            assert_eq!(pair[1]["parentUuid"], pair[0]["uuid"]);
        }
        assert_eq!(written[2]["isCompactSummary"], true);
        assert_eq!(written[3]["isMeta"], true);
        assert!(written[4]["message"]["content"]
            .as_str()
            .unwrap()
            .starts_with("<command-name>/compact</command-name>"));
        assert!(written.iter().all(|record| record["type"] != "assistant"));
        let status: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(records.status_file()).unwrap()).unwrap();
        assert_eq!(status["status"], "idle");
    }

    #[test]
    fn a_compaction_with_nothing_said_since_the_last_writes_what_claude_2_1_304_does_and_no_turn() {
        let config = tempfile::tempdir().unwrap();
        let mut records = Records::new(config.path(), "s", 7).unwrap();
        let refused = [
            "user <local-command-caveat",
            "user <command-name",
            "local_command <local-command-stdout",
        ];
        // A conversation with nothing said in it.
        compact(&mut records, 7).unwrap();
        let written = written_from(config.path(), 0);
        assert_eq!(written.iter().map(shape).collect::<Vec<_>>(), refused);
        assert!(written.iter().all(|record| record["isSidechain"] == false));
        for pair in written.windows(2) {
            assert_eq!(pair[1]["parentUuid"], pair[0]["uuid"]);
        }
        assert_eq!(
            (&written[2]["isMeta"], &written[2]["level"]),
            (&json!(false), &json!("info"))
        );
        assert_eq!(
            written[2]["content"],
            format!("<local-command-stdout>{NOTHING_TO_COMPACT}</local-command-stdout>")
        );
        let status: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(records.status_file()).unwrap()).unwrap();
        assert_eq!(status["status"], "idle");
        // Something said is compacted; a /compact right after that finds nothing again.
        told(&mut records);
        compact(&mut records, 7).unwrap();
        let compacted = written_from(config.path(), 4);
        assert_eq!(shape(&compacted[0]), "user /compact");
        assert_eq!(compacted.len(), 7);
        compact(&mut records, 7).unwrap();
        let again = written_from(config.path(), 11);
        assert_eq!(again.iter().map(shape).collect::<Vec<_>>(), refused);
        // A conversation the window is resumed on has what was said in it.
        let resumed = Records::new(config.path(), "s", 8).unwrap();
        assert!(resumed.said());
    }

    #[test]
    fn a_window_continues_by_itself_after_its_milliseconds_or_its_members_alone() {
        assert_eq!(
            continues_after("1000", "chief"),
            Some(Duration::from_millis(1000))
        );
        let said = "worker 1000";
        assert_eq!(
            continues_after(said, "worker-amber-pine"),
            Some(Duration::from_millis(1000))
        );
        for participant in ["chief", "workers-amber", "reviewer-amber-pine"] {
            assert_eq!(continues_after(said, participant), None, "{participant}");
        }
        // Each member its own timer.
        let said = "chief 500,worker 8000";
        assert_eq!(
            continues_after(said, "chief"),
            Some(Duration::from_millis(500))
        );
        assert_eq!(
            continues_after(said, "worker-amber-pine"),
            Some(Duration::from_millis(8000))
        );
        assert_eq!(continues_after(said, "reviewer-amber-pine"), None);
    }

    #[test]
    fn a_slow_start_is_for_the_windows_of_the_member_it_names_alone() {
        let said = Some("worker 8000");
        assert_eq!(
            slow_start(said, "worker-amber-pine"),
            Duration::from_millis(8000)
        );
        assert_eq!(slow_start(said, "worker"), Duration::from_millis(8000));
        for participant in ["chief", "workers-amber", "reviewer-amber-pine"] {
            assert_eq!(
                slow_start(said, participant),
                Duration::ZERO,
                "{participant}"
            );
        }
        for said in [None, Some(""), Some("worker"), Some("worker soon")] {
            assert_eq!(
                slow_start(said, "worker-amber-pine"),
                Duration::ZERO,
                "{said:?}"
            );
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
}
