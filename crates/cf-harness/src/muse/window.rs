//! A Muse window: the TUI in a pane, its words pasted, its record its session's log.
//!
//! Muse names its session itself, and a launch cannot give it one (`--session-id` is
//! refused in the TUI). The log of the session the window's process opened says which,
//! about a second in (`runtime.session.route_facts` names the process: [`opened_by`]);
//! its SessionStart hook says it too, but only at the session's first prompt, which a
//! window opened with no first message may not have for a long time. A session the
//! window resumes from inside (`/resume`) is the hook's word, written down at its first
//! prompt (`session_hook`); one a `/clear` starts is said by its log alone, which names
//! the session it was started from (`previous_session_stream`), for Muse 1.4.13 runs no
//! SessionStart hook for it, so ConsensFlow cannot give it its role text. The window is
//! followed there.
//!
//! A resumed window's first message is pasted once its prompt shows, for `muse resume`
//! takes none. A message is pasted, and Enter sends it; one pasted while a turn works
//! goes to that turn (`active_run_delivery`), which its record shows at once, so the
//! window says nothing more of it (`took`, `held`). An interrupt before the turn's
//! first word puts the message back in the input box, and its record says so
//! (`run_retracted`): the box is cleared before the next paste, with Control-C, which
//! empties a box that holds something, at work or at rest, and stops a turn at work
//! with an empty box (T-787's probes, Muse 1.4.13). It is pressed only into a box the
//! screen shows holding something.
//!
//! [`opened_by`]: super::paths::opened_by

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::js;
use cf_base::text::{console_text, window_text};
use cf_proto::agents::Harness;
use serde_json::Value;

use super::paths::{begun_ms, cleared_into, opened_by};
use super::screen;
use super::session_hook::last_said;
use crate::contract::{
    Admission, Held, Observed, Pane, PaneHost, Readiness, Records, Window, Work,
};
use crate::records::{Options, Reading};
use crate::seams::{Services, Time};
use crate::shared::admission::admission;
use crate::shared::input_box::{self, Cursor, InputBox, Unsent};
use crate::shared::pane;
use crate::shared::record_state::{dialog_waiting, record_state, switched_to};

/// The key that empties Muse's input box of what it holds (Control-C).
pub(super) const CLEAR_INPUT: [u8; 1] = [0x03];

/// What a window that has not said yet which session it shows is held for.
const HOLD: &str = "Muse has not said yet which session its window shows";

/// What a window whose prompt is not drawn yet is held for.
const STARTING: &str = "Muse has not shown its prompt yet";

/// How often a window that names no session yet is looked at, and its screen while
/// its first message waits for the prompt.
const LOOK_EVERY: Duration = Duration::from_millis(250);

/// A Muse window, and what the engine told it of itself.
pub(super) struct MuseWindow {
    env: Env,
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    /// The session the window shows: its launch's, the one its process opened, or the
    /// one it was followed to.
    session: RefCell<Option<String>>,
    /// Where its SessionStart hook writes the sessions it starts.
    said: PathBuf,
    /// When it was launched, in ms since the epoch: no session older is its own.
    launched_ms: i64,
    /// How long it has to name its session, and to show its prompt for a first
    /// message pasted once it is up.
    first_message_ms: i64,
    /// A resumed window's first message, pasted once its prompt shows.
    first_message: Option<String>,
    /// Its pane and its process, once the pane opened.
    pane: RefCell<Option<Pane>>,
    pid: Cell<Option<u32>>,
    /// The latest look read its turn taken back: the message is in its input box.
    restored: Cell<bool>,
    /// The human's keys left something unsent by the pane host's count, and the latest
    /// readiness read the input box empty on the screen, at this count of their keys.
    read_empty: Cell<Option<u64>>,
}

impl MuseWindow {
    pub(super) fn new(
        services: &Services,
        session: Option<&str>,
        said: PathBuf,
        first_message_ms: i64,
        first_message: Option<String>,
    ) -> Self {
        Self {
            env: services.env.clone(),
            records: Rc::clone(&services.records),
            time: Rc::clone(&services.time),
            session: RefCell::new(session.map(str::to_owned)),
            said,
            launched_ms: services.time.wall_ms(),
            first_message_ms,
            first_message,
            pane: RefCell::default(),
            pid: Cell::default(),
            restored: Cell::new(false),
            read_empty: Cell::default(),
        }
    }

    /// The session the window shows now, as it last said it: the session its process
    /// opened latest since its launch, or the one its hook wrote down, whichever came
    /// later (a `/resume` of an older one is said only by the hook), else the one it is
    /// known on; and from there, the last of the sessions a `/clear` started from it
    /// ([`cleared_into`]: Muse runs no hook for those, and their logs name no process).
    /// None where nothing has said one.
    fn shown(&self) -> Option<String> {
        let opened = self
            .pid
            .get()
            .and_then(|pid| opened_by(pid, self.launched_ms, &self.env));
        let said = match (last_said(&self.said), opened) {
            (Some((said, at)), Some(opened)) => {
                Some(if begun_ms(&opened).is_some_and(|begun| begun > at) {
                    opened
                } else {
                    said
                })
            }
            (Some((said, _)), None) => Some(said),
            (None, opened) => opened,
        };
        let from = said.or_else(|| self.session.borrow().clone())?;
        Some(cleared_into(&from, self.launched_ms, &self.env))
    }

    /// The window's last lines as the pane host draws them, and where its cursor is:
    /// none where the host says nothing.
    async fn screen(host: &dyn PaneHost, pane: &Pane) -> Option<(Vec<String>, Option<Cursor>)> {
        let snapshot = pane::snapshot_drawn(host, pane).await.ok()?;
        let lines = snapshot
            .get("bottom")?
            .as_array()?
            .iter()
            .filter_map(|line| line.as_str().map(str::to_owned))
            .collect();
        Some((lines, Cursor::of(snapshot.get("cursor"))))
    }

    /// Pastes the first message of a resumed window once its prompt shows: in the time
    /// it has, the pane host admitting it at the paste. One Muse holds no session for
    /// (another window has it open) is the launch's failure, in Muse's words.
    async fn first_paste(&self, host: &dyn PaneHost, text: &str) -> Result<Option<String>, String> {
        let pane = self
            .pane
            .borrow()
            .clone()
            .ok_or_else(|| "the Muse window's pane never opened".to_owned())?;
        let deadline = self.time.wall_ms() + self.first_message_ms;
        loop {
            if let Some((lines, _)) = Self::screen(host, &pane).await {
                if let Some(said) = screen::held_elsewhere(&lines) {
                    return Err(format!("Muse did not resume the session: {said}"));
                }
                if screen::shows_prompt(&lines) {
                    break;
                }
            }
            if self.time.wall_ms() >= deadline {
                return Err(
                    "Muse never showed its prompt for the window's first message".to_owned(),
                );
            }
            self.time.sleep(LOOK_EVERY).await;
        }
        let sent = pane::write_paste(host, &pane, &self.body(text), None).await;
        // A paste the host did not admit went back to its queue: no failure of the launch.
        if sent.ok || sent.refused {
            return Ok(None);
        }
        Err(format!(
            "the Muse window's first message may not have gone in: {}",
            sent.cause.or(sent.error).unwrap_or_default()
        ))
    }

    /// `text` as the window is given it: Windows' console drops a paste's non-ASCII
    /// marks on their way, as it does Devin's.
    fn body(&self, text: &str) -> String {
        if self.env.on_windows() {
            console_text(&window_text(text))
        } else {
            window_text(text)
        }
    }
}

impl Window for MuseWindow {
    fn opened(&self, pane: &Pane, pid: Option<u32>) {
        *self.pane.borrow_mut() = Some(pane.clone());
        if pid.is_some() {
            self.pid.set(pid);
        }
    }

    fn follow(&self, session: &str) {
        *self.session.borrow_mut() = Some(session.to_owned());
    }

    /// A window that knows its session pastes the first message it takes once up, if
    /// it has one; one that does not waits for its process to open one.
    fn started<'a>(&'a self, host: &'a dyn PaneHost) -> Work<'a, Result<Option<String>, String>> {
        Box::pin(async move {
            if self.session.borrow().is_some() {
                return match self.first_message.as_deref() {
                    Some(text) => self.first_paste(host, text).await,
                    None => Ok(None),
                };
            }
            let deadline = self.time.wall_ms() + self.first_message_ms;
            while self.time.wall_ms() < deadline {
                if let Some(session) = self.shown() {
                    *self.session.borrow_mut() = Some(session.clone());
                    return Ok(Some(session));
                }
                self.time.sleep(LOOK_EVERY).await;
            }
            Err("Muse never said which session it opened (no log of its process came)".to_owned())
        })
    }

    fn came_up(&self) -> Work<'_, Option<Option<String>>> {
        Box::pin(async move {
            if self.session.borrow().is_some() {
                return None;
            }
            let session = self.shown()?;
            *self.session.borrow_mut() = Some(session.clone());
            Some(Some(session))
        })
    }

    fn shows_prompt(&self, screen: &[String]) -> bool {
        screen::shows_prompt(screen)
    }

    fn ready<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        Box::pin(async move {
            let Some(known) = self.session.borrow().clone() else {
                return Ok(Readiness::Held(Held::Because(HOLD.to_owned())));
            };
            if self.shown().is_some_and(|shown| shown != known) {
                return Ok(Readiness::Held(Held::ShowsAnother));
            }
            let snapshot = pane::snapshot_drawn(host, pane)
                .await
                .map_err(|failed| failed.message)?;
            if snapshot.get("ok") != Some(&Value::Bool(true))
                || js::truthy(snapshot.get("pasteInFlight"))
            {
                return Ok(Readiness::Held(Held::Unsaid));
            }
            let lines: Vec<String> = snapshot
                .get("bottom")
                .and_then(Value::as_array)
                .map(|lines| {
                    lines
                        .iter()
                        .filter_map(|line| line.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            // A host that draws no lines says nothing of them: the window is taken as
            // drawn, as the engine takes it.
            if !lines.is_empty() {
                if let Some(said) = screen::held_elsewhere(&lines) {
                    return Ok(Readiness::Unfit(format!(
                        "Muse did not resume the session: {said}"
                    )));
                }
                if !screen::shows_prompt(&lines) {
                    return Ok(Readiness::Held(Held::Because(STARTING.to_owned())));
                }
            }
            self.read_empty.set(None);
            if js::truthy(snapshot.get("unsent")) {
                match input_box::read_unsent(Harness::Muse, &snapshot) {
                    Unsent::ReadEmpty(typed) => self.read_empty.set(Some(typed)),
                    Unsent::Held => return Ok(Readiness::Held(Held::Unsent)),
                }
            }
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
            let known = self.session.borrow().clone();
            if known.is_none() || self.shown().is_some_and(|shown| Some(shown) != known) {
                return Ok(Admission::Refused {
                    reason: "Muse is showing another session".to_owned(),
                });
            }
            // A turn taken back put its message in the box, which this one would go in
            // after, to be sent as one: the box is emptied first, unless the engine just
            // did, and only where the screen shows it holding something (Control-C into
            // an empty box at work stops the turn).
            if self.restored.get() && !host.cleared() {
                let holds = Self::screen(host, pane)
                    .await
                    .and_then(|(lines, cursor)| screen::read(&lines, cursor));
                if holds == Some(InputBox::Holds) {
                    if let Err(cause) = pane::write_keys(host, pane, &CLEAR_INPUT).await {
                        let reason =
                            format!("the window's input box could not be cleared: {cause}");
                        return Ok(Admission::Refused { reason });
                    }
                }
            }
            let read_empty = self.read_empty.take();
            let sent = pane::write_paste(host, pane, &self.body(text), read_empty).await;
            let admitted = admission(&sent, "Muse refused the paste", false);
            if matches!(admitted, Admission::Admitted { .. }) {
                self.restored.set(false);
            }
            Ok(admitted)
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        Box::pin(async move {
            let Some(session) = self.session.borrow().clone() else {
                return Ok(Observed {
                    reading: None,
                    settled: false,
                    over: false,
                    waiting: None,
                    failed: false,
                    quota: None,
                    switched: None,
                    unnamed: false,
                    took_back: false,
                    at_prompt: false,
                });
            };
            let reading = self
                .records
                .look(Harness::Muse, &session, &Options::default())
                .await;
            let took_back = matches!(&*reading, Reading::Known(record) if record.took_back);
            let mut observed = record_state(reading);
            observed.waiting = dialog_waiting(observed.reading.as_deref());
            observed.took_back = took_back;
            self.restored.set(took_back);
            Ok(match self.shown() {
                Some(shown) if shown != session => switched_to(observed, shown),
                _ => observed,
            })
        })
    }
}

#[cfg(test)]
mod tests;
