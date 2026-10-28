//! A Muse window: the TUI in a pane, its words pasted, its record its session's log.
//!
//! Muse names its session itself, and a launch cannot give it one (`--session-id` is
//! refused in the TUI). The log of the session the window's process opened says which,
//! about a second in (`runtime.session.route_facts` names the process: [`opened_by`]);
//! its SessionStart hook says it too, but only at the session's first prompt. No window
//! opens with a message on its command line, so a fresh window's first prompt comes
//! only after its first message is pasted, and that only once the window is ready,
//! which waits for its session: the log naming the process is the one way a fresh
//! window learns it (Muse 1.4.13 writes it about a second in). A session the
//! window resumes from inside (`/resume`) is the hook's word, written down at its first
//! prompt (`session_hook`); one a `/clear` starts is said by its log alone, which names
//! the session it was started from (`previous_session_stream`), for Muse 1.4.13 runs no
//! SessionStart hook for it: the window is followed there, and says the session was not
//! greeted (`greeted`), so the engine gives it its role text first.
//!
//! Every message, the first of a fresh or a resumed window too, is pasted once the
//! prompt shows (`ready`), and Enter sends it; one pasted while a turn works
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

use super::paths::{begun_ms, cleared_into, opened_by, Heads};
use super::screen;
use super::session_hook::{greeted, last_said};
use crate::contract::{
    Admission, Held, Observed, Over, Pane, PaneHost, Readiness, Records, Window, Work,
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

/// Why a paste waits while the message of a turn taken back is in a box the screen does
/// not show whole.
const OUT_OF_VIEW: &str = "the message of the turn Muse took back is still in its input box, whose first line is out of view: it is not cleared blind, and nothing is pasted behind it until the box shows whole (empty it in the window)";

/// How often a window that names no session yet is looked at.
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
    /// How long a fresh window has to name its session.
    first_message_ms: i64,
    /// Its process, once its pane opened.
    pid: Cell<Option<u32>>,
    /// The heads of the session logs read so far: each look walks the sessions begun
    /// since the launch, and a head that said where its session came from is not read
    /// again.
    heads: Heads,
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
    ) -> Self {
        Self {
            env: services.env.clone(),
            records: Rc::clone(&services.records),
            time: Rc::clone(&services.time),
            session: RefCell::new(session.map(str::to_owned)),
            said,
            launched_ms: services.time.wall_ms(),
            first_message_ms,
            pid: Cell::default(),
            heads: Heads::default(),
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
            .and_then(|pid| opened_by(pid, self.launched_ms, &self.env, &self.heads));
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
        Some(cleared_into(
            &from,
            self.launched_ms,
            &self.env,
            &self.heads,
        ))
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
    fn opened(&self, pid: Option<u32>) {
        if pid.is_some() {
            self.pid.set(pid);
        }
    }

    fn follow(&self, session: &str) {
        *self.session.borrow_mut() = Some(session.to_owned());
    }

    /// Its hook greeted the session where it wrote it down; one a `/clear` started it
    /// never does.
    fn greeted(&self, session: &str) -> bool {
        greeted(&self.said, session)
    }

    /// A window that knows its session (a resumed one) is started; a fresh one waits
    /// for the log of the session its process opened, the one way it learns its session
    /// before its first message is pasted.
    fn started<'a>(&'a self, _host: &'a dyn PaneHost) -> Work<'a, Result<Option<String>, String>> {
        Box::pin(async move {
            if self.session.borrow().is_some() {
                return Ok(None);
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

    fn box_read_empty(&self, snapshot: &serde_json::Value) -> bool {
        input_box::read_empty(Harness::Muse, snapshot)
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
            // an empty box at work stops the turn). A box the screen does not show whole
            // (a brief taller than the host's last lines: Muse keeps the box's tail in
            // view and scrolls its first line out, T-829's probe) is neither cleared
            // blind nor pasted behind.
            if self.restored.get() && !host.cleared() {
                let holds = Self::screen(host, pane)
                    .await
                    .and_then(|(lines, cursor)| screen::read(&lines, cursor));
                match holds {
                    Some(InputBox::Holds) => {
                        if let Err(cause) = pane::write_keys(host, pane, &CLEAR_INPUT).await {
                            let reason =
                                format!("the window's input box could not be cleared: {cause}");
                            return Ok(Admission::Refused { reason });
                        }
                    }
                    Some(InputBox::Empty) => {}
                    None => {
                        return Ok(Admission::Refused {
                            reason: OUT_OF_VIEW.to_owned(),
                        })
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
