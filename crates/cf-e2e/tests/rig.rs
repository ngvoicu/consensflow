//! The rig's black-box suites: the native daemon and the real pane host, run as
//! the app runs them and connected by their production JSON-lines pipes, with a
//! stand-in `claude` (the `fake-agent` binary) in the windows they open. The
//! cases ask the daemon what the page asks and the pane host what the desktop
//! app asks, type to a chief as a human does, and judge what comes of it: the
//! board, the inboxes, the windows' own records, the daemon's log and event
//! file, the processes left behind. They are ported one for one from the
//! JavaScript suites of the integration rig (gone with the port); the
//! module of a case is the JavaScript file it came from, and its name the
//! JavaScript test's. (`stand_in_codex` is new: the stand-in Codex's own transport
//! on Windows, which the supervisor's cases here do not reach, and what a window
//! the daemon opens on it runs on; and `stand_ins`: the stand-in
//! agent's part in how a window closes: it leaves when it is asked to, and so
//! does the job it leaves running. So is `live_support`: what the opt-in live
//! tests stand on, and `live_base`: the rest of it, which the stand-in agent
//! stands in for the harnesses of (what the rig tells of its windows, the
//! records the stand-ins leave in a Claude folder that is the person's, a
//! window opened as the app opens one, the harness code asked as a process).
//! So is `devin_bypass_first`: a resumed Devin worker's first message behind
//! `/bypass`, received once however Devin stores the command.
//! So is `devin_compaction`: a Devin worker's answer from the chief, given once
//! into the same window after Devin's compactor cut the user's message out of the
//! turn's chain (T-655). So is `devin_stamped`: a Devin window that writes its wire log
//! as Devin 3000.11 does is at work for as long as its turn is (T-660).
//! So is `live_member`: a Devin member's window as `live_devin_resume` opens
//! one, whole as the adapter prepares it, cut, and opened again on its
//! conversation with the note in its launch.
//! So is `live_members`: the project of the live checks that play the chief with
//! its own token, with the stand-in agent for the chief and the member, a
//! question judged from what the daemon wrote of it, and the app quit and started
//! again.
//! So is `background_work`: what ConsensFlow does with the work
//! a worker's turn left running (and not with the servers its harness keeps,
//! which the stand-in agent starts after the first message, as Claude Code does,
//! nor with the browser one of them launches),
//! and how a window closes. (The bugs the owner hit in real use are `regressions`,
//! a test of its own, on this rig.) So is `approval`: what
//! the daemon types into a window while the words that were for it wait for the
//! human, where the stand-in agent stops a turn at the Escape it is pressed. So is
//! `pending_agents`: what it does with the agents a turn left running, which no
//! program of the window's tree is. So is `start_suspended`: a start opens
//! nothing, and the human's Resume brings back the chief and the windows at
//! work, the one whose turn the quit cut told so. So is `specs`: the Specmint
//! skill a chief of a project with specs on is given, ConsensFlow's copy kept at
//! one place for every launch and read besides its role, and no worker, and no
//! chief of a project with them off; and `specs_codex`, a chief on the stand-in
//! Codex, whose resumed thread keeps the instructions it began with, told of them
//! by a message once the project is resumed, and of nothing at the Resume after.
//! So is `wake_ups`: the human's
//! wake-up calls, which nudge a chief at rest; its two cases wait out a real
//! minute each, the least a wake-up may wait. So is `described`: a project the
//! human renames and writes a line under, which are in the event file and which no
//! window is told of. So is `stranger`: the rig keeps a process that is none of any
//! window's beside them, which every case that ends a window holds the pane host to
//! leaving as it was, and whose end of its own is no end the pane host gave it. So is
//! `last_exits`: the exits the pane host tells as a rig closes, which the long run of
//! the evals reads after the close. So is `reports`: a rig keeps a report of a case
//! that failed and none of one that passed, and every case here and in the
//! regressions that starts a rig closes it. So is `smoke_stand_in`: the packaged
//! smoke's stand-in harness taking its brief and handing back its result on the
//! checkout's daemon, which the smoke itself proves only in a built app.)
//!
//! The cases take turns ([`cf_e2e::serial`]): each starts a daemon and a pane
//! host of its own, with windows in real terminals, and the waits are timed.
//!
//!     cargo test -p cf-e2e --test rig        (`npm run test:integration`)

// The cases are in `tests/rig/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "rig/approval.rs"]
mod approval;
#[path = "rig/background_close.rs"]
mod background_close;
#[path = "rig/background_work.rs"]
mod background_work;
#[path = "rig/cf_native.rs"]
mod cf_native;
#[path = "rig/codex_session.rs"]
mod codex_session;
#[path = "rig/core_board.rs"]
mod core_board;
#[path = "rig/core_questions.rs"]
mod core_questions;
#[path = "rig/core_slice.rs"]
mod core_slice;
#[path = "rig/core_tells.rs"]
mod core_tells;
#[path = "rig/core_tiered.rs"]
mod core_tiered;
#[path = "rig/daemon_seam.rs"]
mod daemon_seam;
#[path = "rig/decided_result.rs"]
mod decided_result;
#[path = "rig/described.rs"]
mod described;
#[path = "rig/devin_bypass_first.rs"]
mod devin_bypass_first;
#[path = "rig/devin_compaction.rs"]
mod devin_compaction;
#[path = "rig/devin_stamped.rs"]
mod devin_stamped;
#[path = "rig/harness_homes.rs"]
#[cfg(unix)]
mod harness_homes;
#[path = "rig/help.rs"]
mod help;
#[path = "rig/homes.rs"]
mod homes;
#[path = "rig/kept_config.rs"]
#[cfg(unix)]
mod kept_config;
#[path = "rig/last_exits.rs"]
mod last_exits;
#[path = "rig/live_base.rs"]
mod live_base;
#[path = "rig/live_member.rs"]
mod live_member;
#[path = "rig/live_members.rs"]
mod live_members;
#[path = "rig/live_support.rs"]
mod live_support;
#[path = "rig/muse_window.rs"]
mod muse_window;
#[path = "rig/pacing.rs"]
mod pacing;
#[path = "rig/pending_agents.rs"]
mod pending_agents;
#[path = "rig/printed_output.rs"]
mod printed_output;
#[path = "rig/reports.rs"]
mod reports;
#[path = "rig/specs.rs"]
mod specs;
// The packaged smoke's stand-in is a shell script.
#[path = "rig/smoke_stand_in.rs"]
#[cfg(unix)]
mod smoke_stand_in;
#[path = "rig/specs_codex.rs"]
#[cfg(unix)]
mod specs_codex;
#[path = "rig/stand_in_codex.rs"]
mod stand_in_codex;
#[path = "rig/stand_ins.rs"]
mod stand_ins;
#[path = "rig/start_suspended.rs"]
mod start_suspended;
#[path = "rig/stranger.rs"]
mod stranger;
#[path = "rig/waiting_notes.rs"]
mod waiting_notes;
#[path = "rig/wake_ups.rs"]
mod wake_ups;
#[path = "rig/window_says_why.rs"]
mod window_says_why;

use std::time::Duration;

use cf_e2e::rig::{Config, Rig};

/// What a case ends with: nothing, or why it could not be run. A check that
/// does not hold is a panic, which is the case's failure. The error a case returns
/// is how the rigs it started learn that it failed.
type Outcome = cf_e2e::Outcome;

/// The stand-in for a window's harness program, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The stand-in for an OpenCode 1.x, which the daemon launches a window on as it
/// launches one on OpenCode, built for these tests.
const FAKE_OPENCODE: &str = env!("CARGO_BIN_EXE_fake-opencode-1");

/// A rig whose windows run the stand-in agent.
fn rig() -> cf_e2e::Result<Rig> {
    Rig::start(config())
}

/// The configuration of a rig whose windows run the stand-in agent, to add
/// variables to. Its case is judged by what it returns, so a rig it drops unclosed
/// says nothing of a case that passed.
fn config() -> Config {
    Config::new(FAKE_AGENT).judged_by_outcome()
}

/// `n` seconds.
fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// What the window of the conversation `session` was told first, as the stand-in
/// recorded it: the text of the first user message of its record, waited for up to
/// 30 s. A window is given its first message once it is up, on no word of its command
/// line (T-432).
fn first_told(rig: &cf_e2e::rig::Rig, session: &str) -> String {
    let read = || {
        rig.transcript(session)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|row| row["type"] == "user")
            .and_then(|row| row["message"]["content"].as_str().map(str::to_owned))
    };
    let _ = rig.wait_for(
        "the window's record to hold its first message",
        secs(30),
        || Ok(read().is_some()),
    );
    read().unwrap_or_default()
}

/// Whether a word of the command line of the window `frame` opened holds `text`.
fn on_command_line(frame: &serde_json::Value, text: &str) -> bool {
    frame["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .any(|word| word.contains(text))
}

/// The conversation a window was launched with: the word after `--session-id`
/// in its command line.
fn session_of(frame: &serde_json::Value) -> String {
    after(frame, "--session-id")
}

/// The word that follows `flag` in the command line of the window `frame`
/// opens.
fn after(frame: &serde_json::Value, flag: &str) -> String {
    let argv = frame["argv"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    argv.iter()
        .position(|word| word == flag)
        .and_then(|at| argv.get(at + 1))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}
