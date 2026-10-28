//! Devin's windows. Each launch runs on a config of its own (the owner's, plus
//! a hook that gives a session its role instructions), in full-permission mode,
//! with the first message in a prompt file. Devin names its session itself; its
//! own wire log for this launch says which one the window opened, and which one
//! it shows after a /new or /resume, so the window is followed to it. A message
//! is pasted, behind whatever the input box holds, and only while Devin still
//! shows the conversation we know.
//!
//! What Devin is handed while its agent is in a turn goes into the agent's own queue,
//! and Devin writes its message row to its store when the turn takes it in (a live run,
//! Devin 3000.11.3, 2026-10-13: a note pasted a second after another showed in the
//! record 63 s later, when the 60 s command of the turn before it ended; the input box
//! says "Guide Devin while it works"). Its wire log says nothing of what the TUI puts to
//! its agent (T-102: Devin 3000.6.14 to 3000.11.92, probed live), and its record reads a
//! message of the user's at rest until the agent's first word: a note was pasted into a
//! turn that had begun, and the engine, which gave the record a minute from the
//! hand-over, pasted it again (the report of the same day: a chief read each of two
//! notes twice). Three things stand against that. The window reads such a message as a
//! turn begun for a model's time to its first word ([`TAKEN_IN_MS`]), so nothing is
//! pasted into a turn that has just begun, and no longer, so that it holds back no
//! message for good. Devin 3000.11.92 writes a prompt's history row at its Enter, a
//! paste queued behind a turn at work too (`history`): the window says the TUI took the
//! paste (`took`), which the engine then waits for until its record shows it, however
//! long the turn ahead of it works; a prompt the human submits after the engine's look
//! holds the window from a paste (`ready`). And the engine waits out a
//! window's turns before it takes the record for lacking (`cf-engine`'s `arrival`). An
//! older Devin writes the row only as the turn takes the paste in: its window says
//! nothing of it, and the engine's rest-based wait alone stands.
//!
//! A window works in full permission, and Devin does not always put a resumed
//! one there: its `--resume` of a session the app's quit cut in its first turn
//! drops `--permission-mode dangerous` and comes up in its normal mode, where a
//! prompt file's every call is refused ("User skipped this tool call", the field
//! report B1 of 2026-10-15) and a pasted turn's opens a dialog nobody answers
//! (Devin 3000.11.92, `cargo xtask live devin-resume`). So a resumed window's
//! first message goes behind `/bypass`, Devin's own command, which switches it
//! before the message runs and changes nothing in a window in full permission
//! already; and no window is ready for a delivery while its wire says it is in
//! the normal mode: `/bypass` is pasted into it once it has drawn (Enter again
//! before it is pasted again), and the window is ready when the wire says, after
//! the look at its screen, that the mode changed. One that has not changed in
//! [`BYPASS_WITHIN_MS`] is unfit for work, and its launch is given up, as one
//! that did not come up.
//!
//! Kept from Node on purpose: a conversation of an empty id, which no
//! ledger holds, is refused in a sentence of this module's own, where V8
//! threw its `TypeError` for the window it made none of.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::js;
use cf_base::text::{console_text, window_text};
use cf_proto::agents::Harness;
use cf_proto::specs::Variant;
use futures_util::future::join;
use serde_json::Value;

use super::channel;
use super::history::{self, History};
use super::install;
use super::paths;

use super::wire::selected_session;
use super::wire_log::{Said, WireLog};
use crate::contract::{
    Adapter, Admission, FirstMessage, Held, Input, Interrupt, Launch, Observed, Over, Pane,
    PaneHost, Prepared, Readiness, Records, Skill, Window, Work,
};
use crate::detect::executable;
use crate::records::{Options, Reading, Role};
use crate::seams::{Services, Time};
use crate::shared::admission::admission;
use crate::shared::input_box::{self, Unsent};
use crate::shared::left_open::LeftOpen;
use crate::shared::record_state::{dialog_waiting, observed_from, switched_to, unnamed};
use crate::shared::role::write_role;
use crate::shared::{pane, specmint, window_args};

/// How long after Devin's store has a message of the user's its agent has not answered
/// the window is read as in the turn that message begins: what an agent takes to its
/// first word, and well past it. Devin's record reads such a message complete, at rest,
/// and its wire says nothing of the turn before the agent's first word (T-102). A turn
/// that says nothing for longer is read as the record reads it, so that it holds back
/// no message for good.
///
/// It counts from the message's own time, which Devin 3000.11.92 writes at the Enter: a
/// message queued behind a turn longer than this reads at rest from its take-in until its
/// agent's first word, and a note pasted then queues behind it (its history row says the
/// TUI took it, and it runs once). The row's own time, the take-in, would not do: Devin
/// 3000.11.3 writes every row of a turn in one second, so a message would read as begun
/// for as long after its turn as the row was written late (the review of 6c8fc595).
const TAKEN_IN_MS: i64 = 30_000;

/// What a window that has not said yet which conversation it shows is held for.
const HOLD: &str = "Devin has not said yet which conversation its window shows";

/// Devin's own command that switches a window to full permission ("Switch to
/// Bypass Permissions mode, or run a prompt under it", `/bypass [prompt]`).
pub(crate) const BYPASS: &str = "/bypass";

/// The mode Devin's wire says a window that asks for every permission is in.
const NORMAL: &str = "normal";

/// What a window being switched to full permission is held for.
const SWITCHING: &str =
    "Devin's window is in its normal permission mode, and /bypass was sent to switch it to full permission";

/// How long a window's output holds still before `/bypass` is pasted into it:
/// what the engine takes for a screen that is drawn. A host that cannot tell is
/// taken as drawn, as the engine takes it.
const DRAWN_QUIET_MS: f64 = 1_500.0;

/// How long after it was pasted `/bypass` is pasted again while the wire still
/// says the normal mode, and how long a window has to leave that mode from the
/// look that first found it there.
const BYPASS_AGAIN_MS: i64 = 10_000;
pub(crate) const BYPASS_WITHIN_MS: i64 = 30_000;

/// What a window that did not leave the normal mode in time is given up for.
fn stayed_in_normal() -> String {
    format!(
        "its Devin window stayed in the normal permission mode, which {BYPASS} did not change within {} s (every tool call of its would wait for a permission nobody gives)",
        BYPASS_WITHIN_MS / 1_000
    )
}

/// Whether a window whose wire says it is in `mode` is given `/bypass` before it is
/// ready for work: only where the wire says the normal mode. One that has said no mode
/// yet is taken as it was before the mode was read.
pub(crate) fn wants_bypass(mode: Option<&str>) -> bool {
    mode == Some(NORMAL)
}

/// What the readiness of a window reads of its wire log: the conversation it shows,
/// the mode that conversation is in, and the command the window is given before it is
/// ready, where it is given one.
#[cfg(any(test, feature = "test-support"))]
pub(crate) struct WireReadiness {
    pub(crate) shown: Option<String>,
    pub(crate) mode: Option<String>,
    pub(crate) command: Option<&'static str>,
}

/// What the readiness of a window reads of the wire log `wire` now. For the live tests
/// (`tooling`), which open Devin's windows themselves and ask whether the daemon would
/// have one switched.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn wire_readiness(wire: &std::path::Path) -> Result<WireReadiness, String> {
    let said = WireLog::new(&wire.to_string_lossy(), jiff::tz::TimeZone::UTC).read(0)?;
    let command = wants_bypass(said.mode.as_deref()).then_some(BYPASS);
    Ok(WireReadiness {
        shown: said.shown,
        mode: said.mode,
        command,
    })
}

/// What Devin's store of `env` says of `pasted`, the text a window of `session` was
/// given as it was pasted, handed over at `at_ms`: whether its TUI took it (a history
/// row of it, written since), and how many history rows the session has. What the
/// engine asks a window after a paste (`Window::took`). For the live tests
/// (`tooling`), which paste into Devin's windows themselves.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn store_took(env: &Env, session: &str, pasted: &str, at_ms: i64) -> (bool, u64) {
    let history = History::default();
    history.expect(pasted, at_ms);
    let seen = history.look(env, session);
    (seen.took, seen.prompts)
}

/// The window of a Devin launch that is open, its wire log `wire`, on the conversation
/// `session`, as the adapter makes one for the daemon (`prepare`). For the live tests
/// (`tooling`), which open Devin's windows themselves and drive the window's readiness
/// as the engine does.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn window_on(services: &Services, wire: &str, session: &str) -> Rc<dyn Window> {
    Rc::new(DevinWindow::new(
        services,
        wire,
        Some(session),
        DISCOVER_FOR_MS,
        true,
        None,
    ))
}

/// The first message a window opens with, as its prompt file has it: as a window
/// can take the text, and behind `/bypass` for a window resumed on its
/// conversation, which Devin may bring back in its normal mode. A message that is
/// no text is none.
fn first_message(message: Option<&str>, resumed: bool) -> Option<String> {
    let text = window_text(message?);
    if text.is_empty() {
        return None;
    }
    Some(if resumed {
        format!("{BYPASS} {text}")
    } else {
        text
    })
}

/// Where a window is in its switch to full permission: since when the wire has
/// said it is in the normal mode, when `/bypass` was last pasted into it, and when
/// Enter was pressed again for that paste.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Switch {
    since: Option<i64>,
    pasted: Option<i64>,
    entered: Option<i64>,
}

/// The key that sends what the input box holds.
const ENTER: u8 = 13;

/// How often the wire log is asked which conversation Devin opened, and the
/// most it is waited on before it is given up on: a minute, or less where the
/// launch has less time ([`Launch::first_message_ms`]).
const DISCOVER_EVERY: Duration = Duration::from_millis(250);
const DISCOVER_FOR_MS: i64 = 60_000;

/// Devin's shell on Windows is Git Bash, which names folders /c/Users/…, and
/// its file tools write such a path to C:\c\Users\…: a Devin worker's file
/// landed there and its task's work was lost (2026-10-03).
const WINDOWS_PATHS: &str = "This machine runs Windows and your shell is Git Bash: give file tools Windows paths (C:\\Users\\…) or paths relative to the project folder, never /c/… paths, which they write under C:\\c\\.";

/// The role text a Devin window gets: on Windows, with how to name a file
/// there. A window given none gets none, and is refused as any is.
fn role_text(instructions: &str, env: &Env) -> String {
    if env.on_windows() && !instructions.is_empty() {
        format!("{instructions}\n\n{WINDOWS_PATHS}\n")
    } else {
        instructions.to_owned()
    }
}

/// Devin, as the engine launches its windows.
pub struct DevinAdapter {
    services: Services,
}

impl DevinAdapter {
    /// The adapter of the windows the engine's `services` serve.
    pub fn new(services: &Services) -> Self {
        Self {
            services: services.clone(),
        }
    }
}

/// The keys that empty Devin's input box of a paste whose Enter was withheld:
/// Control-C, once. It empties the whole box and does no harm to an empty one at
/// rest, where Control-U empties the cursor's line only (the message's header
/// stays) and one Escape leaves the box as it is (`cargo xtask live unsent-box`,
/// Devin 3000.11.92 on macOS and on Windows, 2026-10-21).
const CLEAR_INPUT: [u8; 1] = [0x03];

impl Adapter for DevinAdapter {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let env = &self.services.env;
            let executable = executable(Harness::Devin, env)?;
            let integration = install::integration(
                &self.services,
                launch.id.as_str(),
                &executable,
                launch.role != "chief",
            )
            .await?;
            let role = write_role(
                Harness::Devin,
                launch.role,
                env,
                launch.id,
                &role_text(launch.instructions, env),
            )?;
            // A chief given specs reads its Specmint skill by the path its role
            // text names, which the session-start hook gives it: where Devin
            // has none of its own, ConsensFlow's is written where it is kept
            // for every launch.
            specmint::write_own(Harness::Devin, env, launch)?;
            let agent = launch.agent.unwrap_or_default();
            let message = first_message(launch.message, launch.resume.is_some());
            let open = if launch.resume.is_none() {
                window_args::start
            } else {
                window_args::resume
            };
            // None only for a conversation of an empty id, which no ledger holds.
            let invocation = open(Harness::Devin, agent, launch.resume, message.as_deref())
                .ok_or_else(|| "a Devin window opens on a session id".to_owned())?;
            let invocation = install::prepare_prompt(invocation, &integration)?;
            // What the record holds before this window exists is the old process's: a
            // turn it left open, which a window with no message to begin one is told
            // apart from by what the record holds later. A window given a message
            // begins its own turn.
            let left_open = match launch.resume {
                Some(session) if launch.message.is_none() => {
                    let before = self
                        .services
                        .records
                        .look(Harness::Devin, session, &Options::default())
                        .await;
                    LeftOpen::held(&before)
                }
                _ => None,
            };
            let mut argv = vec![executable];
            argv.extend(integration.args);
            argv.extend(invocation.args);
            let mut vars = integration.env;
            vars.push(("CF_DEVIN_ROLE_FILE".to_owned(), role.file));
            Ok(Prepared {
                argv,
                env: vars,
                drop_env: invocation
                    .drop_env
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect(),
                native_session: launch.resume.map(str::to_owned),
                window: Rc::new(DevinWindow::new(
                    &self.services,
                    &integration.wire,
                    launch.resume,
                    DISCOVER_FOR_MS.min(launch.first_message_ms),
                    history::at_enter(&integration.version),
                    left_open,
                )),
            })
        })
    }

    /// Devin keeps a session's messages in its store, which is read with no window
    /// open as with one: a session with a message of the user's in it had a turn
    /// and is gone on with, and one the store holds with none, or holds no row of
    /// (Devin writes the row with the first message), has nothing to go on with. A
    /// store that cannot be read tells nothing, and the engine goes on with the
    /// session as it did before it asked. Devin resumed on a session it has no
    /// row of opens idle and drops what is typed to it (live, 3000.11.3,
    /// 2026-10-14).
    fn resumable<'a>(&'a self, session: &'a str) -> Work<'a, Option<bool>> {
        Box::pin(async move {
            let looked = self
                .services
                .records
                .look(Harness::Devin, session, &Options::default())
                .await;
            match &*looked {
                Reading::Known(record) => {
                    Some(record.items.iter().any(|item| item.role == Role::User))
                }
                Reading::Missing(_) => Some(false),
                Reading::Unknown(_) => None,
            }
        })
    }

    /// Devin's own status line says it: "esc twice to interrupt". The same
    /// two at a Devin already idle open its rewind ("Revert"), and the next
    /// Enter confirms it, cutting the conversation back: a third Escape a
    /// second later closes it, and changes nothing after a stopped turn or at
    /// an idle prompt (probed 2026-10-03).
    fn interrupt(&self) -> Interrupt {
        Interrupt {
            presses: 2,
            close_after: Some(Duration::from_secs(1)),
        }
    }

    /// In a file of the launch's own, which `--prompt-file` names and Devin reads
    /// as it opens (`install::prepare_prompt`).
    fn first_message(&self) -> FirstMessage {
        FirstMessage::Opened
    }

    /// Pasted, and its box cleared with [`CLEAR_INPUT`].
    fn input(&self) -> Input {
        Input::Pasted {
            clear: &CLEAR_INPUT,
        }
    }

    fn specmint(&self, variant: Variant) -> Result<Skill, String> {
        let env = &self.services.env;
        specmint::place(
            Harness::Devin,
            env,
            variant,
            paths::global_specmint(env, variant),
        )
    }
}

/// A Devin window, and what the engine told it of itself.
struct DevinWindow {
    env: Env,
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    /// The conversation the window shows: its launch's, the one it named
    /// itself, or the one it was followed to.
    session: RefCell<Option<String>>,
    wire: WireLog,
    /// How long the wire log has to name the conversation of a window that has
    /// none yet, in milliseconds.
    discover_for_ms: i64,
    /// Where it is in its switch to full permission.
    switch: Cell<Switch>,
    /// Its Devin writes a prompt's history row at its Enter (`history`): its store
    /// says whether it took the paste last handed over, and holds it queued.
    at_enter: bool,
    /// The paste last handed over, and what the store said of it.
    history: History,
    /// How many history rows its conversation had at the engine's last look at the
    /// window (`observe`), which found it at rest or not.
    looked: Cell<Option<u64>>,
    /// The human's keys left something unsent by the pane host's count, and the
    /// latest readiness read the input box empty on the screen, at this count of
    /// their keys: the delivery's paste says so ([`Unsent::ReadEmpty`]).
    read_empty: Cell<Option<u64>>,
    /// What its record held when the window was opened again with no message to begin
    /// a turn. Devin resumed waits at its prompt and works on nothing, so no process is
    /// in the turn the record leaves, whatever it says: a final answer its store marks
    /// no end of (the GPT models' own, whose end only the old launch's wire log said), a
    /// turn the quit cut after a step whose call has no result, a message of the user's
    /// the cut left unanswered (T-722).
    left_open: RefCell<Option<LeftOpen>>,
}

impl DevinWindow {
    /// The window of a launch whose wire log is `wire`, on `session` where it is
    /// known, as `services` serve it.
    fn new(
        services: &Services,
        wire: &str,
        session: Option<&str>,
        discover_for_ms: i64,
        at_enter: bool,
        left_open: Option<LeftOpen>,
    ) -> Self {
        Self {
            env: services.env.clone(),
            records: Rc::clone(&services.records),
            time: Rc::clone(&services.time),
            session: RefCell::new(session.map(str::to_owned)),
            wire: WireLog::new(wire, services.zone.clone()),
            discover_for_ms,
            switch: Cell::default(),
            at_enter,
            history: History::default(),
            looked: Cell::default(),
            read_empty: Cell::default(),
            left_open: RefCell::new(left_open),
        }
    }

    /// Whether `shown`, a conversation Devin says its window shows, is the
    /// one the window is known to be on.
    fn known(&self, shown: &str) -> bool {
        self.session.borrow().as_deref() == Some(shown)
    }

    /// Whether the record's last item is a message of the user's, written less than
    /// [`TAKEN_IN_MS`] ago, that its agent has not answered: a turn begun, at work though
    /// the record reads it at rest. One with no time is none.
    fn begun_on_the_store(&self, observed: &Observed) -> bool {
        let last = observed
            .items()
            .iter()
            .rev()
            .find(|item| item.role != Role::Custom);
        let at = last
            .filter(|item| item.role == Role::User)
            .and_then(|item| item.at.as_ref())
            .and_then(|at| match at {
                Value::String(text) => cf_base::time::parse(text),
                Value::Number(number) => number.as_f64().map(|seconds| (seconds * 1_000.0) as i64),
                _ => None,
            });
        at.is_some_and(|at| self.time.wall_ms() - at < TAKEN_IN_MS)
    }

    /// What its store says now of the paste last handed over, in the conversation it
    /// shows: none where its Devin writes no history row at the Enter, or it shows
    /// none yet.
    fn history(&self) -> Option<history::Seen> {
        let session = self.session.borrow().clone()?;
        self.at_enter
            .then(|| self.history.look(&self.env, &session))
    }

    /// A look at a window whose wire says it is in the normal mode, which is no
    /// mode to work in: `/bypass` goes into it once its screen is drawn (`snapshot`,
    /// the pane host's word on it), and the window waits for the wire to say it
    /// changed. While the wire says the same, one more Enter goes in after
    /// [`BYPASS_AGAIN_MS`], and only after as long again `/bypass` once more: an
    /// Enter that came before the window had read the paste leaves the command in
    /// the input box, where a second paste would make `/bypass/bypass` of it, as
    /// the engine's Enter again for a delivery has it (`arrival`). One still in that
    /// mode [`BYPASS_WITHIN_MS`] after the first look that found it so is unfit for
    /// work. A paste the host refused (it wrote nothing) is tried again at the next
    /// look, and so is an Enter it did not take.
    async fn switch_to_bypass(
        &self,
        host: &dyn PaneHost,
        pane: &Pane,
        snapshot: &Value,
    ) -> Readiness {
        let now = self.time.wall_ms();
        let mut switch = self.switch.get();
        let since = *switch.since.get_or_insert(now);
        if now - since >= BYPASS_WITHIN_MS {
            self.switch.set(switch);
            return Readiness::Unfit(stayed_in_normal());
        }
        let drawn = snapshot
            .get("outputQuietMs")
            .is_none_or(|quiet| quiet.as_f64().is_some_and(|ms| ms >= DRAWN_QUIET_MS));
        let due = switch
            .entered
            .or(switch.pasted)
            .is_none_or(|last| now - last >= BYPASS_AGAIN_MS);
        if drawn && due {
            if switch.pasted.is_some() && switch.entered.is_none() {
                if pane::write_keys(host, pane, &[ENTER]).await.is_ok() {
                    switch.entered = Some(now);
                }
            } else {
                let session = self.session.borrow().clone();
                let sent = channel::send(
                    host,
                    pane,
                    self.wire.path(),
                    session.as_deref(),
                    BYPASS,
                    self.read_empty.get(),
                )
                .await;
                if !sent.refused {
                    switch.pasted = Some(now);
                    switch.entered = None;
                }
            }
        }
        self.switch.set(switch);
        Readiness::Held(Held::Because(SWITCHING.to_owned()))
    }

    /// What holds a window whose wire says `said` before anything else is looked at:
    /// a conversation it has not named yet, or one it shows that is not the one it is
    /// known to be on.
    fn shows_another(&self, said: &Said) -> Option<Held> {
        match &said.shown {
            None => Some(Held::Because(HOLD.to_owned())),
            Some(shown) if !self.known(shown) => Some(Held::ShowsAnother),
            Some(_) => None,
        }
    }
}

impl Window for DevinWindow {
    fn opened(&self, _pid: Option<u32>) {}

    fn box_read_empty(&self, snapshot: &serde_json::Value) -> bool {
        input_box::read_empty(Harness::Devin, snapshot)
    }

    /// The window is on `session` from now on: a switch to full permission begun for
    /// the conversation it was on is that one's, and another's time runs from the
    /// first look that finds it in the normal mode.
    fn follow(&self, session: &str) {
        let mut known = self.session.borrow_mut();
        if known.as_deref() != Some(session) {
            self.switch.set(Switch::default());
        }
        *known = Some(session.to_owned());
        // The turn the record had open was a conversation's the window left.
        *self.left_open.borrow_mut() = None;
    }

    fn started<'a>(&'a self, _host: &'a dyn PaneHost) -> Work<'a, Result<Option<String>, String>> {
        Box::pin(async move {
            if self.session.borrow().is_some() {
                return Ok(None);
            }
            let deadline = self.time.wall_ms() + self.discover_for_ms;
            while self.time.wall_ms() < deadline {
                if let Ok(Some(session)) = selected_session(self.wire.path()) {
                    *self.session.borrow_mut() = Some(session.clone());
                    return Ok(Some(session));
                }
                self.time.sleep(DISCOVER_EVERY).await;
            }
            Err("Devin never said which session it opened (its wire log stayed empty)".to_owned())
        })
    }

    /// What the box on the screen shows, for the human told of a delivery held for it.
    fn unsent_words<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Option<String>> {
        Box::pin(input_box::words_on_screen(host, pane, Harness::Devin))
    }

    fn ready<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        Box::pin(async move {
            let said = self.wire.read(self.time.wall_ms())?;
            if let Some(held) = self.shows_another(&said) {
                return Ok(Readiness::Held(held));
            }
            let snapshot = pane::snapshot(host, pane)
                .await
                .map_err(|failed| failed.message)?;
            if snapshot.get("ok") != Some(&Value::Bool(true))
                || js::truthy(snapshot.get("pasteInFlight"))
            {
                return Ok(Readiness::Held(Held::Unsaid));
            }
            self.read_empty.set(None);
            // The count is the human's keys, a guess: the box on the screen says.
            if js::truthy(snapshot.get("unsent")) {
                match input_box::unsent(host, pane, Harness::Devin, &snapshot).await {
                    Unsent::ReadEmpty(typed) => self.read_empty.set(Some(typed)),
                    Unsent::Held => return Ok(Readiness::Held(Held::Unsent)),
                }
            }
            // The window as it is once the host answered: the human may have chosen
            // a mode, opened another conversation or begun a turn meanwhile, and
            // nothing goes into a window for what it was before.
            let now = self.wire.read(self.time.wall_ms())?;
            if let Some(held) = self.shows_another(&now) {
                return Ok(Readiness::Held(held));
            }
            // A prompt submitted since the engine's look found the window at rest (a
            // history row of its conversation, which its Devin writes at the Enter): a
            // turn begun, or queued, that the record may not show yet (Astra on the B1
            // fix). A window no look has seen is held to nothing.
            let submitted = self.history().map(|seen| seen.prompts);
            if submitted
                .zip(self.looked.get())
                .is_some_and(|(now, then)| now > then)
            {
                return Ok(Readiness::Held(Held::Unsaid));
            }
            if wants_bypass(now.mode.as_deref()) {
                return Ok(self.switch_to_bypass(host, pane, &snapshot).await);
            }
            self.switch.set(Switch::default());
            Ok(Readiness::Ready)
        })
    }

    fn deliver<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
        text: &'a str,
    ) -> Work<'a, Result<Admission, String>> {
        Box::pin(async move {
            // Windows' console drops a paste's non-ASCII marks on their way to Devin.
            let body = if self.env.on_windows() {
                console_text(&window_text(text))
            } else {
                window_text(text)
            };
            // Its store's history row of the whole paste is the TUI's word that it took
            // it (`took`).
            self.history.expect(&body, self.time.wall_ms());
            let session = self.session.borrow().clone();
            let read_empty = self.read_empty.take();
            let sent = channel::send(
                host,
                pane,
                self.wire.path(),
                session.as_deref(),
                &body,
                read_empty,
            )
            .await;
            Ok(admission(&sent, "Devin refused the paste", false))
        })
    }

    /// Its time to leave the normal mode runs from the first look that finds it
    /// there, drawn or not: `/bypass` waits for its screen, and the bound does not.
    fn ready_while_drawing(&self) -> bool {
        true
    }

    /// Its store has the paste's history row: the TUI submitted it, whether it runs it
    /// at once or queues it behind the turn at work. One the app's Stop cancels while
    /// queued is kept in the conversation, unanswered, and its agent reads it in the
    /// next turn (T-102, 3 of 3 runs on 3000.11.92): taken all the same. `held` is the
    /// contract's own false: `took` reaches the engine first, and holds the paste for good.
    fn took(&self) -> bool {
        self.history().is_some_and(|seen| seen.took)
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        Box::pin(async move {
            let Some(session) = self.session.borrow().clone() else {
                return Ok(Observed {
                    reading: None,
                    settled: false,
                    over: Over::No,
                    waiting: None,
                    failed: false,
                    quota: None,
                    switched: None,
                    unnamed: false,
                    took_back: false,
                    at_prompt: false,
                    shared: false,
                });
            };
            // The record and the wire log are read together: the log as it is
            // when the look begins, the record when its reading is released. A turn is
            // read from the record: a message of the user's its agent has not answered
            // is one at work, from the moment the store has it (`record_state`).
            let (reading, wire) = join(
                self.records
                    .look(Harness::Devin, &session, &Options::default()),
                async { self.wire.read(self.time.wall_ms()) },
            )
            .await;
            let wire = wire?;
            self.looked.set(self.history().map(|seen| seen.prompts));
            // What Devin's wire says of its quota is the live word beside the record's.
            let mut observed = observed_from(Arc::clone(&reading), wire.quota);
            observed.waiting = dialog_waiting(observed.reading.as_deref());
            // What the record held when the window was opened with no message is the old
            // process's, whatever it says of the turn: no process is in a turn that was
            // cut, and one the record cannot end ended. The window has been given nothing
            // since, so it is at rest, and a message of the user's written before the
            // open is no turn it began.
            if self
                .left_open
                .borrow()
                .as_ref()
                .is_some_and(|open| open.unchanged(&reading))
            {
                observed.settled = true;
            } else if self.begun_on_the_store(&observed) {
                observed.settled = false;
            }
            Ok(match wire.shown {
                None => unnamed(observed, HOLD),
                Some(shown) if !self.known(&shown) => switched_to(observed, shown),
                Some(_) => observed,
            })
        })
    }
}

#[cfg(test)]
mod tests;
