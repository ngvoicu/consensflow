//! Live: what a Claude Code window shows, and its record holds, while it is stopped right
//! after a paste went in, as a loaded machine stops it (3.0.7 chief-meeting, CMT-24, T-760's
//! F4: m-3093 and m-4273 had no record of their paste for 177 s and 196 s on a loaded Mac,
//! and were pasted again). It is opt-in (`cargo xtask live stopped`): it spends the
//! machine's quota on one Claude Code window, on its cheap model, so it is ignored unless
//! asked for and is no part of `check` or of CI. Unix only: it stops the window's processes
//! through the kit's `descent`, which stops a process only while it is still the one chosen.
//!
//! A note of forty numbered lines is pasted as the daemon pastes one (`pane.write_paste`,
//! its Enter after it); at once every process of the window's group is stopped, for
//! ninety seconds, and the pane host is asked what it sees (`pane.snapshot`: how long the
//! screen printed nothing, its last lines as drawn) as the stop goes on. Then the window
//! goes on, and the turn's answer is waited for. What each look found is said in counts
//! and shapes: whether the screen's last lines show the header, a placeholder of a paste
//! (`[Pasted text #`), and whether the record holds the header yet.
//!
//! The second test is the daemon's (`stopped_chief`): a real Claude Code is the chief of a
//! project on the native daemon, at its own settle and arrival wait, and the stand-in worker's
//! result is handed to it; as its `delivery.begun` is written, every process of the chief's
//! window is stopped for ninety seconds, longer than the minute of rest after which a paste
//! the record never showed went again before 3.0.7. It is held to the paste going once: on
//! its first attempt, no retry, and in the chief's record once.
//!
//! ```text
//! cargo xtask live stopped                   both
//! cargo xtask live stopped -- stopped_chief  the daemon's alone
//! ```

#![cfg(unix)]

use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{
    descending_from, find, member_as, pause_chosen, stand_in_worker, Identity, Machine, Members,
    Pause, Plan, ThisMachine, STAND_IN_WORKER,
};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window, ANSWER};
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
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
    let header = lines
        .iter()
        .filter(|line| line.contains("[ConsensFlow m-9"))
        .count();
    let placeholder = lines
        .iter()
        .filter(|line| line.contains("[Pasted text #"))
        .count();
    let numbered = lines
        .iter()
        .filter(|line| line.contains("A line of the note"))
        .count();
    let prompt = lines
        .iter()
        .filter(|line| line.trim_start().starts_with('>') || line.trim_start().starts_with('❯'))
        .count();
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

/// Stops the window's own tree where it is, or lets it go on: its program, and what
/// descends from it, each only while it is still the process chosen (`descent`).
fn stall(root: Identity, pause: Pause) -> Result<()> {
    let all = ThisMachine.processes()?;
    let mut tree: Vec<_> = all
        .iter()
        .filter(|process| process.identity == root)
        .cloned()
        .collect();
    tree.extend(descending_from(&all, root));
    let paused = pause_chosen(&tree, pause);
    if paused.ended.is_empty() || !paused.problems().is_empty() {
        return Err(Error::Daemon(format!(
            "{pause:?} of the window's tree: {} reached; {:?}",
            paused.ended.len(),
            paused.problems()
        )));
    }
    Ok(())
}

/// The window's program, as the pane host named it.
fn root_of(window: &Window) -> Result<Identity> {
    window
        .process()
        .ok_or_else(|| Error::Daemon("the pane host named no process of the window".into()))
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
    let root = root_of(&window)?;
    stall(root, Pause::Stop)?;
    let stopped = Instant::now();
    log.say("claude: pasted, and stopped");
    let mut seen = Vec::new();
    for at in LOOKS {
        let due = Duration::from_secs(at);
        if let Some(wait) = due.checked_sub(stopped.elapsed()) {
            sleep(wait);
        }
        let snapshot = window.snapshot(20)?;
        let said = format!(
            "{at:>3} s stopped: {} · record: header in {} user item(s)",
            shape(&snapshot),
            in_record(&window)
        );
        log.say(&said);
        if at == LOOKS[0] || at == LOOKS[LOOKS.len() - 1] {
            log.say(drawn(&snapshot));
        }
        seen.push(said);
    }
    if let Some(wait) = STOPPED.checked_sub(stopped.elapsed()) {
        sleep(wait);
    }
    stall(root, Pause::Continue)?;
    log.say("claude: going on");
    let answered = live::until(ANSWER, LOOK, || {
        let record = window.record()?;
        (record.settled && window.screen().contains(SUM)).then_some(())
    });
    let after = window.snapshot(20)?;
    log.say(format!("at rest: {}", shape(&after)));
    log.say(drawn(&after));
    let checks = vec![
        Check::new(
            "claude",
            "the paste went in",
            pasted["ok"] == true,
            pasted.to_string(),
        ),
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
            format!(
                "{} user item(s) with its header; looks: {seen:#?}",
                in_record(&window)
            ),
        ),
    ];
    drop(window);
    rig.close()?;
    report::conclude(&log, "what each check came to", &checks)
}

/// What the chief's folder tells it.
const CHIEF_BRIEF: &str = "This folder is an automated ConsensFlow test. When a ConsensFlow message arrives, reply with exactly one line, SEEN, and run no tools.\n";

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of Claude Code; cargo xtask live stopped"]
fn stopped_chief_is_handed_its_result_once_though_it_shows_nothing_for_ninety_seconds() -> Result {
    let log = Log::begin("live stopped (daemon)")?;
    ask::prepare()?;
    log.found(Harness::Claude)?;
    let plan = Plan::new("stopped-chief", member_as(Harness::Claude, "chief"))
        .with(stand_in_worker())
        .told(CHIEF_BRIEF);
    let mut members = Members::open(&plan, std::path::Path::new(FAKE_AGENT), &log)?;
    members.add(STAND_IN_WORKER, &[])?;
    if !members.chief_is_idle(&log)? {
        let checks = vec![Check::failed(
            "claude chief",
            "goes idle",
            members.chief_window(),
        )];
        members.close()?;
        return report::conclude(&log, "what each check came to", &checks);
    }
    let id = format!("p{}-chief", members.project_id());
    let generation = members
        .rig()
        .open_frames()
        .into_iter()
        .rev()
        .find(|frame| frame["id"] == id.as_str())
        .and_then(|frame| frame["generation"].as_u64())
        .unwrap_or(1);
    let pane = serde_json::json!({ "id": id, "generation": generation });
    // The rig keeps the process it read as the host answered; else the host's own word, read
    // now.
    let root = members
        .rig()
        .window_process(&pane)
        .or_else(|| {
            members
                .rig()
                .open_frames()
                .into_iter()
                .rev()
                .find(|frame| frame["id"] == pane["id"])
                .and_then(|frame| frame["pid"].as_u64())
                .and_then(|pid| u32::try_from(pid).ok())
                .and_then(|pid| ThisMachine.identity(pid).ok().flatten())
        })
        .ok_or_else(|| Error::Daemon("the pane host named no process of the chief".into()))?;
    members.give(STAND_IN_WORKER, "Reply with exactly: ONE done.")?;
    // Stopped as soon as the daemon begins to hand the result over.
    let result = find(
        &log,
        "the result's delivery to begin",
        Duration::from_secs(300),
        Duration::from_millis(100),
        || {
            Ok(members.events().into_iter().find(|event| {
                event["kind"] == "delivery.begun"
                    && members
                        .inbox(Some("chief"))
                        .ok()
                        .into_iter()
                        .flatten()
                        .any(|message| {
                            message["kind"] == "result" && message["id"] == event["data"]["message"]
                        })
            }))
        },
    )?;
    let Some(begun) = result else {
        members.close()?;
        return Err(Error::Daemon("the result's delivery never began".into()));
    };
    stall(root, Pause::Stop)?;
    log.say("claude chief: the result's paste begun, the chief stopped");
    sleep(STOPPED);
    stall(root, Pause::Continue)?;
    log.say("claude chief: going on");
    let id = begun["data"]["message"].as_i64().unwrap_or_default();
    let settled = find(
        &log,
        "the result to settle",
        Duration::from_secs(300),
        Duration::from_secs(1),
        || {
            Ok(members.inbox(Some("chief"))?.into_iter().find(|message| {
                message["id"] == id
                    && (message["state"] == "delivered" || message["state"] == "failed")
            }))
        },
    )?;
    let events = members.events();
    let count = |kind: &str| {
        events
            .iter()
            .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
            .count()
    };
    let session = members
        .rig()
        .open_frames()
        .into_iter()
        .rev()
        .find(|frame| frame["id"] == pane["id"])
        .and_then(|frame| cf_e2e::live::members::session_of(&frame));
    let items = match session {
        Some(session) => ask::records(
            Harness::Claude.kind(),
            &session,
            members.rig().environment(),
        )?
        .map(|record| record.items)
        .unwrap_or_default(),
        None => Vec::new(),
    };
    let marker = format!("[ConsensFlow m-{id} ");
    let held = items
        .iter()
        .filter(|item| item.role == "user" && item.text.contains(&marker))
        .count();
    let checks = vec![
        Check::new(
            "claude chief",
            "the result is delivered on its first attempt",
            settled
                .as_ref()
                .is_some_and(|message| message["state"] == "delivered" && message["attempts"] == 1),
            format!("{settled:?}"),
        ),
        Check::new(
            "claude chief",
            "it is pasted once",
            count("delivery.begun") == 1 && count("delivery.retried") == 0,
            format!(
                "{} begun, {} retried",
                count("delivery.begun"),
                count("delivery.retried")
            ),
        ),
        Check::new(
            "claude chief",
            "the chief's record holds it once",
            held == 1,
            format!("{held} user item(s) hold its header"),
        ),
    ];
    members.close()?;
    report::conclude(&log, "what each check came to", &checks)
}
