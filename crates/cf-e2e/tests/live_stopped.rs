//! Live: what a Claude Code window shows, and its record holds, while it is stopped right
//! after a paste went in, as a loaded machine stops it (3.0.7 chief-meeting, CMT-24, T-760's
//! F4: m-3093 and m-4273 had no record of their paste for 177 s and 196 s on a loaded Mac,
//! and were pasted again). It is opt-in (`cargo xtask live stopped`): it spends the
//! machine's quota on one Claude Code window, on its cheap model, so it is ignored unless
//! asked for and is no part of `check` or of CI. Unix only: it stops the window's processes
//! with `SIGSTOP`.
//!
//! A note of forty numbered lines is pasted as the daemon pastes one (`pane.write_paste`,
//! its Enter after it); at once every process of the window's group is stopped, for
//! ninety seconds, and the pane host is asked what it sees (`pane.snapshot`: how long the
//! screen printed nothing, its last lines as drawn) as the stop goes on. Then the window
//! goes on, and the turn's answer is waited for. What each look found is said in counts
//! and shapes: whether the screen's last lines show the header, a placeholder of a paste
//! (`[Pasted text #`), and whether the record holds the header yet.
//!
//! ```text
//! cargo xtask live stopped
//! ```

#![cfg(unix)]

use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window, ANSWER};
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::{getpgid, Pid};
use serde_json::Value;

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The folder the window works in, and its id.
const FOLDER: &str = "stopped";

/// The note's header.
const HEADER: &str = "[ConsensFlow m-9 · note from @human]";

/// The sum the note asks for.
const SUM: &str = "3825";

/// How long the window stays stopped, and when within it the pane host is asked.
const STOPPED: Duration = Duration::from_secs(90);
const LOOKS: [u64; 6] = [1, 5, 15, 30, 60, 88];

/// How often the record is read.
const LOOK: Duration = Duration::from_millis(500);

/// The note: a header, forty numbered lines, and a sum it does not hold.
fn note() -> String {
    let lines: Vec<String> = (1..=40)
        .map(|n| format!("{n}. A line of the note, to make the paste long."))
        .collect();
    format!(
        "{HEADER}\n{}\nWhen you have read it, reply with the sum of 2468 and 1357, in digits, and nothing else.",
        lines.join("\n")
    )
}

/// What a snapshot's screen shows, in counts and shapes: never its words.
fn shape(snapshot: &Value) -> String {
    let lines: Vec<&str> = snapshot["bottom"]
        .as_array()
        .map(|lines| lines.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let header = lines.iter().filter(|line| line.contains("[ConsensFlow m-9")).count();
    let placeholder = lines.iter().filter(|line| line.contains("[Pasted text #")).count();
    let numbered = lines
        .iter()
        .filter(|line| line.contains("A line of the note"))
        .count();
    let prompt = lines.iter().filter(|line| line.trim_start().starts_with('>') || line.trim_start().starts_with('❯')).count();
    format!(
        "outputQuietMs {} pasteQuietMs {} pasteInFlight {} · bottom {} lines: header {header}, placeholder {placeholder}, numbered {numbered}, prompt-marked {prompt}",
        snapshot["outputQuietMs"],
        snapshot["pasteQuietMs"],
        snapshot["pasteInFlight"],
        lines.len(),
    )
}

/// The lines a snapshot drew, each with what marks it: for the probe's log only.
fn drawn(snapshot: &Value) -> String {
    snapshot["bottom"]
        .as_array()
        .map(|lines| {
            lines
                .iter()
                .filter_map(Value::as_str)
                .map(|line| format!("    |{line}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// How many of the record's user items hold the header.
fn in_record(window: &Window) -> usize {
    window
        .recorded()
        .iter()
        .filter(|item| item.role == "user" && item.text.contains(HEADER))
        .count()
}

/// Signals every process of the window's group.
fn signal(window: &Window, signal: Signal) -> Result<()> {
    let pid = window
        .process()
        .ok_or_else(|| Error::Daemon("the pane host named no process of the window".into()))?
        .pid;
    let group = getpgid(Some(Pid::from_raw(pid as i32)))
        .map_err(|error| Error::Daemon(format!("no group of {pid}: {error}")))?;
    killpg(group, signal).map_err(|error| Error::Daemon(format!("{signal} {group}: {error}")))
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of Claude Code; cargo xtask live stopped"]
fn a_claude_code_window_stopped_after_a_paste_shows_and_records_it_as_it_shows() -> Result {
    let log = Log::begin("live stopped")?;
    ask::prepare()?;
    let claude = log.found(Harness::Claude)?;
    let rig = Rig::start(live::rig(std::slice::from_ref(&claude), FAKE_AGENT))?;
    let window = Open::new(&rig, &claude, FOLDER, FOLDER).open_telling(&log, "claude")?;
    window
        .ready()
        .map_err(|screen| Error::Daemon(format!("the window did not open: {screen}")))?;
    let pasted = window.paste(&window.given(&note())?)?;
    signal(&window, Signal::SIGSTOP)?;
    let stopped = Instant::now();
    log.say("claude: pasted, and stopped");
    let mut seen = Vec::new();
    for at in LOOKS {
        let due = Duration::from_secs(at);
        if let Some(wait) = due.checked_sub(stopped.elapsed()) {
            sleep(wait);
        }
        let snapshot = window.snapshot(20)?;
        let said = format!("{at:>3} s stopped: {} · record: header in {} user item(s)", shape(&snapshot), in_record(&window));
        log.say(&said);
        if at == LOOKS[0] || at == LOOKS[LOOKS.len() - 1] {
            log.say(drawn(&snapshot));
        }
        seen.push(said);
    }
    if let Some(wait) = STOPPED.checked_sub(stopped.elapsed()) {
        sleep(wait);
    }
    signal(&window, Signal::SIGCONT)?;
    log.say("claude: going on");
    let answered = live::until(ANSWER, LOOK, || {
        let record = window.record()?;
        (record.settled && window.screen().contains(SUM)).then_some(())
    });
    let after = window.snapshot(20)?;
    log.say(format!("at rest: {}", shape(&after)));
    log.say(drawn(&after));
    let checks = vec![
        Check::new("claude", "the paste went in", pasted["ok"] == true, pasted.to_string()),
        Check::new(
            "claude",
            "the turn answered the note once it went on",
            answered.is_some(),
            window.tail(),
        ),
        Check::new(
            "claude",
            "the record holds the note once",
            in_record(&window) == 1,
            format!("{} user item(s) with its header; looks: {seen:#?}", in_record(&window)),
        ),
    ];
    drop(window);
    rig.close()?;
    report::conclude(&log, "what each check came to", &checks)
}
