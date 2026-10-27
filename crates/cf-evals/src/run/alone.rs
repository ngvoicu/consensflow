//! The bare arm: the chief as a bare harness, no ConsensFlow project, the harness
//! alone in a window of the same pane host, started through the same wrappers
//! (model, effort, no MCP), its state read from its own record. The owner types the
//! prompt and the follow-ups, answers what it asks at the end of a turn, and nudges
//! it once or twice when it stops without asking, as in the other arms.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::ask::{self, Item};
use cf_e2e::live::window::session_for;
use cf_e2e::rig::Rig;
use jiff::Timestamp;
use serde_json::{json, Value};

use super::floor::{settled, Floor};
use super::login;
use super::owner::{Owner, Seen, Step};
use super::report::{keep, Bare, Said};
use super::{start, Plan};
use crate::bare::{asking_turn_end, bare_metrics};
use crate::measure::{begun, changed, ended, gone, Metrics, OpenQuestion};
use crate::plan::{harness, last_lines, terminal_answer};
use crate::scenarios::FollowUp;
use crate::{js, Error, Result};

/// How often the owner looks at the window and its record.
const LOOK: Duration = Duration::from_secs(5);

/// How long a bare window's screen and record both hold still before it counts as
/// idle whatever its record says: a working harness keeps drawing (a spinner, a timer)
/// or adding to its record, and without ConsensFlow Devin's and OpenCode's records
/// never say a turn is over.
const SCREEN_IDLE: Duration = Duration::from_secs(30);

/// How long a window is given to take what was typed before Enter is pressed again,
/// and how long to wait after a paste.
const TAKES: Duration = Duration::from_secs(5);
const PASTED: Duration = Duration::from_secs(8);

/// The window of the bare chief: the id its pane is opened by.
const PANE: &str = "bare-chief";

/// The chief's window and what is known of its record.
struct Window<'r> {
    rig: &'r Rig,
    kind: &'static str,
    workspace: PathBuf,
    /// When the run began, in milliseconds since the epoch: its session is the newest
    /// that began after.
    since_ms: i64,
    /// The conversation of the chief: given to Claude and Pi, found in the store of
    /// the others.
    session: Option<String>,
}

impl Window<'_> {
    /// What the chief's own record says so far, if it says anything yet.
    fn record(&mut self) -> Option<ask::Record> {
        if self.session.is_none() {
            self.session = ask::newest_session(
                self.kind,
                &self.workspace,
                self.since_ms,
                self.rig.environment(),
            )
            .ok()
            .flatten();
        }
        let session = self.session.as_deref()?;
        ask::records(self.kind, session, self.rig.environment())
            .ok()
            .flatten()
    }

    fn printed(&self) -> usize {
        self.rig.screen(PANE).len()
    }

    fn press(&self, bytes: &[u8]) -> Result {
        self.rig.host(
            "pane.input",
            json!({ "id": PANE, "generation": 1, "bytes": bytes }),
        )?;
        Ok(())
    }

    /// Types `words` as the owner would, a bracketed paste and Enter, and Enter
    /// again, three times at most, while the record shows nothing was taken:
    /// Devin takes a pasted prompt into its box and waits for an Enter of its own;
    /// Codex on Windows once took the second Enter as a new line too.
    fn types(&mut self, floor: &mut Floor, words: &str) -> Result {
        settled(|| self.printed());
        let before = self.record().map_or(0, |record| record.items.len());
        self.press(format!("\u{1b}[200~{words}\u{1b}[201~\r").as_bytes())?;
        sleep(PASTED);
        for _ in 0..3 {
            if self.record().map_or(0, |record| record.items.len()) != before {
                break;
            }
            self.press(&[13])?;
            floor.note("Enter pressed again: the window had not taken the text")?;
            sleep(TAKES);
        }
        Ok(())
    }
}

/// Plays the scenario once as the bare chief, run number `index`: whether it held.
pub fn play(plan: &Plan, index: u32, out: &mut dyn Write) -> Result<bool> {
    let (options, scenario) = (plan.options, &plan.options.scenario);
    let since_ms = Timestamp::now().as_millisecond();
    let mut floor = Floor::new(out);
    let workspace = super::workspace::fresh(plan.machine, scenario)?;
    // A bare chief has no board, and so no worker's result to read the files at.
    let begun = begun(&workspace, &scenario.reads())?;
    let rig = start(plan, &mut floor, &workspace)?;
    let kind = harness(&options.chief)?.kind;
    let mut window = Window {
        rig: &rig,
        kind,
        workspace: workspace.clone(),
        since_ms,
        // Claude and Pi open on an id they are given; the others name theirs in their stores.
        session: session_for(kind),
    };
    let mut said = Said::default();
    let mut items: Vec<Item> = Vec::new();
    let mut ends: HashSet<String> = HashSet::new();
    let conducted = conduct(
        plan,
        &mut window,
        &mut floor,
        &mut said,
        (&mut items, &mut ends),
    );
    let screen = last_lines(&rig.screen(PANE), 40);
    let session = window.session.clone();
    let closed = rig.close_keeping_the_root();
    conducted?;
    closed?;

    let fixture = super::workspace::fixture(scenario.fixture);
    let metrics = Metrics {
        files_gone: if scenario.counts_gone_files() {
            Some(gone(&fixture, &workspace)?)
        } else {
            None
        },
        files_read: ended(&workspace, &begun)?,
        ..bare_metrics(&items, changed(&fixture, &workspace)?, &ends)
    };
    let mut checks = scenario.verdict(&metrics);
    checks.extend(login::left_as_it_was(plan.machine, &mut floor)?);
    let kept = Bare {
        options,
        scenario,
        chief: plan.chief,
        session,
        index,
        seconds: floor.seconds(),
        metrics,
        checks,
        screen,
        said,
        log: floor.log(),
        path: PathBuf::new(),
    };
    let name = super::report::file_name(Timestamp::now(), options, index);
    let path = keep(&options.reports, &name, &kept.value())?;
    let kept = Bare { path, ..kept };
    kept.say(&mut floor)?;
    Ok(kept.held())
}

/// The run: the harness opened in its window, the prompt typed, and the owner looking
/// at the window and its record every few seconds until it is over.
fn conduct(
    plan: &Plan,
    window: &mut Window,
    floor: &mut Floor,
    said: &mut Said,
    (items, ends): (&mut Vec<Item>, &mut HashSet<String>),
) -> Result {
    let (options, scenario) = (plan.options, &plan.options.scenario);
    open(plan, window)?;
    settled(|| window.printed());
    floor.note(format!(
        "chief ({}, bare) ready; typing the prompt",
        options.chief
    ))?;
    window.types(floor, &scenario.prompt)?;

    let mut owner = Owner::new(scenario);
    let mut last_change = Instant::now();
    let mut signature = String::new();
    let (mut screen_at, mut record_at) = (
        (usize::MAX, Instant::now()),
        (String::new(), Instant::now()),
    );
    loop {
        sleep(LOOK);
        let state = window.record();
        if let Some(state) = &state {
            *items = state.items.clone();
        }
        let printed = window.printed();
        if printed != screen_at.0 {
            screen_at = (printed, Instant::now());
        }
        let newest = items.last();
        let grown = json!([
            items.len(),
            newest.map(|item| item.id.clone()),
            newest.map(|item| js::length(&item.text)),
        ])
        .to_string();
        if grown != record_at.0 {
            record_at = (grown.clone(), Instant::now());
        }
        let moving = screen_at.1.max(record_at.1).elapsed() < SCREEN_IDLE;
        let busy = state.as_ref().is_none_or(|state| !state.settled) && moving;
        // At rest, its newest message ended the turn.
        if let (false, Some(newest)) = (busy, items.iter().rfind(|item| item.role == "assistant")) {
            ends.insert(newest.id.clone());
        }
        let now = json!([
            items.len(),
            newest.map(|item| item.id.clone()),
            newest.map(|item| js::length(&item.text)),
            busy
        ])
        .to_string();
        if now != signature {
            signature = now;
            last_change = Instant::now();
            floor.note(format!(
                "chief {}; {} items in its record",
                if busy { "working" } else { "idle" },
                items.len()
            ))?;
        }
        let seen = Seen {
            elapsed: floor.elapsed(),
            quiet: last_change.elapsed(),
            busy,
            dialog: false,
            board_empty: true,
        };
        let step = owner.step(scenario, &seen, options.timeout, &mut || {
            Ok(asking_turn_end(items, ends).map(|end| OpenQuestion {
                id: end.id.clone(),
                text: end.text.clone(),
            }))
        })?;
        *said = Said {
            nudges: owner.nudges,
            replies: owner.replies,
            dialogs: 0,
            follow_ups: owner.follow_ups_sent,
        };
        match step {
            Step::Wait => {}
            Step::Dialog => {}
            Step::Reply(end) => {
                let reply = terminal_answer(scenario, &end.text);
                floor.note(format!(
                    "the chief asked in its terminal; the owner answered there: {reply}"
                ))?;
                window.types(floor, &reply)?;
                last_change = Instant::now();
            }
            Step::Nudge(nudge) => {
                floor.note(format!(
                    "the chief stopped in its terminal; the owner typed there: {nudge}"
                ))?;
                window.types(floor, &nudge)?;
                last_change = Instant::now();
            }
            // The bare chief has no card to be switched from: a scenario that switches
            // the chief is refused before a run begins on this arm.
            Step::Next(FollowUp::Switch) => {
                return Err(Error::Refused(
                    "a bare chief has no card to be switched from".to_owned(),
                ));
            }
            Step::Next(FollowUp::Say(next)) => {
                floor.note(format!(
                    "the owner's next message ({}): {}",
                    owner.follow_ups_sent,
                    js::first(&next, 80)
                ))?;
                window.types(floor, &next)?;
                last_change = Instant::now();
            }
            Step::Over => {
                floor.note("quiet: the run is over")?;
                break;
            }
            Step::TimeUp => {
                floor.note("time is up")?;
                break;
            }
        }
    }
    Ok(())
}

/// Opens the window of the harness on a new conversation, as the app opens one,
/// through the wrapper of the run where the harness has one.
fn open(plan: &Plan, window: &mut Window) -> Result {
    let started = ask::start(
        &json!({ "kind": window.kind }),
        window.session.as_deref(),
        None,
    )?
    .ok_or_else(|| {
        Error::Refused(format!(
            "{} has no window of its own to open on a new conversation",
            plan.options.chief
        ))
    })?;
    let executable = match plan.wrappers.of(&started.command) {
        Some(wrapper) => wrapper.to_path_buf(),
        None => plan
            .machine
            .install(&started.command)?
            .command()
            .to_path_buf(),
    };
    let mut words = vec![executable.to_string_lossy().into_owned()];
    words.extend(started.args);
    let argv = ask::program(&words, window.rig.environment())?;
    let opened = window.rig.host(
        "pane.open",
        pane_open(&window.workspace, &argv, &started.drop_env),
    )?;
    if opened["ok"] != true {
        return Err(Error::Refused(format!("pane.open: {opened}")));
    }
    Ok(())
}

/// What the pane host is asked to open the bare chief's window with: the fields it
/// takes and no other, since it refuses a request that has one it does not know.
fn pane_open(cwd: &Path, argv: &[String], drop_env: &[String]) -> Value {
    json!({
        "id": PANE, "generation": 1, "cwd": cwd, "argv": argv, "env": {}, "dropEnv": drop_env,
    })
}

#[cfg(test)]
mod tests;
