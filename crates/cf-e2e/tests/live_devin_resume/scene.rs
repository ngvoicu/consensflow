//! A window of a case with what it is watched by, and the wait for a turn: the
//! daemon's own way of telling that a message arrived in a window and that its turn
//! ended is its harness's record (the header of the message in a user's item, then
//! an assistant's item that is the turn's end, with the record at rest), and the
//! window is at rest once it has printed nothing for a while and its terminal does
//! not say it waits for an answer. Whatever the check presses or pastes into the
//! window is on its trail.

use std::fmt::Display;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::ask::{Interrupt, Record};
use cf_e2e::live::members::{squash, Identity, Machine, ThisMachine};
use cf_e2e::live::window::Window;
use cf_e2e::live::{last_lines, secs};
use cf_e2e::rig::Rig;
use cf_e2e::Result;
use serde_json::Value;

use crate::life::Life;
use crate::trail::{plus, Trail};
use crate::turn::{reading, told, Turn};
use crate::watch::Watch;

/// How often the window, its record and Devin's store are looked at while a turn is
/// waited for.
pub const LOOK: Duration = Duration::from_millis(500);

/// How long a window must have printed nothing to be at rest, in milliseconds: the
/// daemon's own (`ENTER_AGAIN_QUIET_MS` of its arrival watch).
pub const QUIET_MS: f64 = 3_000.0;

/// How long a window that prints nothing, in a turn that has not ended, is held by
/// something (a dialog that waits for an answer), in milliseconds; and how many rows
/// of its screen are looked at then.
const STALL_MS: f64 = 8_000.0;
const ROWS: usize = 16;

/// How long a window whose terminal says it waits for an answer to a permission, or
/// whose screen shows the options of a dialog, is waited for: nobody answers a dialog of
/// the check's window, so a turn that waits for one does not end by itself.
pub const BLOCKED: Duration = Duration::from_secs(30);

/// How long after its end a turn is over though its window goes on printing: a
/// status that moves at rest would keep a window from ever being quiet.
const OVER: Duration = Duration::from_secs(10);

/// What the label of full permission says on a screen: Devin draws it in the line over
/// its input while the session is in that mode.
const BYPASS_LABEL: &str = "bypass permissions";

/// The first option of the permission dialog of Devin 3000.11 (`1 Yes  (Approve once)`):
/// the terminal marks a dialog when it opens, and says nothing when a key closes it, so
/// whether it is still open is read off the screen.
const APPROVE_ONCE: &str = "(Approve once)";

/// A window of the case, and what it is watched by.
pub struct Scene<'a> {
    pub rig: &'a Rig,
    pub window: &'a Window<'a>,
    /// Which process the window's program is, read as the window opened: what descends
    /// from it is the window's own tree. None where the pane host said no process, or
    /// the machine knew none of its number.
    pub own: Option<Identity>,
    pub trail: Trail,
    pub watch: Watch,
    pub life: Life,
}

/// Whether the check's last step (`last`, a time on its trail) came after `since`.
fn done_after(last: Option<Duration>, since: Duration) -> bool {
    last.is_some_and(|last| last > since)
}

/// Whether the rows of a screen show the label of full permission.
pub fn shows_bypass(rows: &[String]) -> bool {
    rows.iter().any(|row| row.contains(BYPASS_LABEL))
}

/// Whether the rows of a screen show the options of a permission dialog: the first of
/// them, the one that lets the call run once, as Devin words it.
pub fn shows_dialog(rows: &[String]) -> bool {
    rows.iter().any(|row| row.contains(APPROVE_ONCE))
}

/// The mode a window had, from the first the wire said to the last: `normal`, or
/// `normal → bypass` for one that was switched; `?` where the wire said none.
pub fn mode_story(first: Option<&str>, last: Option<&str>) -> String {
    match (first, last) {
        (Some(first), Some(last)) if first != last => format!("{first} → {last}"),
        (_, Some(last)) => last.to_owned(),
        _ => "?".to_owned(),
    }
}

/// The mode the wire last said and whether the screen shows the label of full
/// permission, as a line.
pub fn mode_words(wire: Option<&str>, shown: bool) -> String {
    format!(
        "the mode: the wire says {}; the screen {} \"{BYPASS_LABEL} on\"",
        wire.unwrap_or("nothing"),
        if shown { "shows" } else { "does not show" }
    )
}

impl<'a> Scene<'a> {
    /// A window of the rig, watched from now on: Devin's store is `store`, and the
    /// wire log of the window's launch `wire`. Every wait is cut to what is left of the
    /// case's time, `life`.
    pub fn new(
        rig: &'a Rig,
        window: &'a Window<'a>,
        store: PathBuf,
        wire: Option<PathBuf>,
        life: Life,
    ) -> Self {
        // While the window's program runs, its number is its own.
        let own = window
            .pid()
            .and_then(|pid| ThisMachine.identity(pid).ok().flatten());
        Self {
            rig,
            window,
            own,
            trail: Trail::new(),
            watch: Watch::new(window, store, wire),
            life,
        }
    }

    /// What the pane host says of the window now: how long it printed nothing, what
    /// was typed and not sent.
    pub fn snapshot(&self) -> Option<Value> {
        self.rig.snapshot(&self.window.pane(), 1)
    }

    /// How long the window has printed nothing, in milliseconds: none for one that
    /// never printed.
    pub fn quiet_ms(&self) -> Option<f64> {
        self.snapshot()?["outputQuietMs"].as_f64()
    }

    /// Whether the window has printed nothing for [`QUIET_MS`].
    pub fn quiet(&self) -> bool {
        self.quiet_ms().is_some_and(|quiet| quiet >= QUIET_MS)
    }

    /// Looks at the window, Devin's store and the wire once.
    pub fn look(&mut self) {
        self.watch.look(self.window, &self.trail);
    }

    /// Pastes `text` as the daemon does, noting `what` on the trail first.
    pub fn paste(&mut self, text: &str, what: impl Display) -> Result<()> {
        self.trail.note(what);
        self.window.paste(text)?;
        Ok(())
    }

    /// Presses the key `bytes` as the daemon does, noting `what` on the trail first.
    pub fn press(&mut self, bytes: &[u8], what: impl Display) -> Result<()> {
        self.trail.note(what);
        self.window.press(bytes)?;
        Ok(())
    }

    /// What the window showed when a turn did not end: whether it printed, what was
    /// typed in it and not sent, what the daemon's reader says of the record, and the
    /// end of its screen.
    fn held(&self, record: Option<&Record>) -> String {
        let snapshot = self.snapshot();
        let quiet = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot["outputQuietMs"].as_f64())
            .map_or_else(
                || "has printed nothing".to_owned(),
                |ms| format!("last printed {:.0} s ago", ms / 1000.0),
            );
        let unsent = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot["unsent"].as_str())
            .filter(|unsent| !unsent.is_empty())
            .map_or_else(
                || "nothing typed and unsent".to_owned(),
                |unsent| format!("typed and unsent: {}", squash(unsent, 100)),
            );
        format!(
            "the window {quiet}; {unsent}; {}; its screen: {}",
            reading(record),
            self.rows()
        )
    }

    /// The last rows of the window's screen as the pane host draws it (the screen
    /// itself, and not what was printed to make it), the rows that say nothing left out.
    pub fn screen_rows(&self) -> Vec<String> {
        let shown = self.rig.snapshot(&self.window.pane(), ROWS);
        shown
            .as_ref()
            .and_then(|shown| shown["tail"].as_array())
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|row| !row.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// The end of the window's screen, as a line: what holds a window that waits for an
    /// answer (a dialog) is on it, in whatever words.
    pub fn rows(&self) -> String {
        last_lines(&self.screen_rows().join("\n"))
    }

    /// What the window and its wire say of the mode the session is in, as a line.
    pub fn mode_line(&self) -> String {
        mode_words(
            self.watch.mode.as_deref(),
            shows_bypass(&self.screen_rows()),
        )
    }

    /// The mode the window had, from the first the wire said to the last.
    pub fn mode_over_time(&self) -> String {
        mode_story(self.watch.first_mode.as_deref(), self.watch.mode.as_deref())
    }

    /// Whether the screen shows the options of a permission dialog now.
    pub fn dialog_shown(&self) -> bool {
        shows_dialog(&self.screen_rows())
    }

    /// Where the window's terminal says it waits for an answer to a permission: since
    /// when, and its words. A mark that the check pressed or pasted something after is
    /// no longer one: it answered or cancelled the dialog, and the terminal says nothing
    /// of that.
    pub fn waiting(&self) -> Option<(Duration, String)> {
        let (since, mark) = self.watch.marks.now()?;
        (mark.asks_permission() && !done_after(self.trail.last_at(), *since))
            .then(|| (*since, mark.said()))
    }

    /// Whether the window is at rest: it has printed nothing for [`QUIET_MS`] (a window at
    /// work draws its status as it goes), and no dialog stands still on its screen (which
    /// prints nothing either). What its terminal marked last is no part of it: a key that
    /// closes a dialog is not marked.
    pub fn at_rest(&self) -> bool {
        self.quiet() && !self.dialog_shown()
    }

    /// Presses the keys that interrupt a turn, as the app presses them (`how`: the
    /// adapter's own), noting it on the trail first.
    pub fn interrupt(&mut self, how: &Interrupt) -> Result<()> {
        let closing = how.close_after.map_or_else(String::new, |after| {
            format!(", one more {} s after", secs(after))
        });
        self.trail
            .note(format!("interrupt (Escape {} times{closing})", how.presses));
        self.window.interrupt(how)?;
        Ok(())
    }

    /// How the wait for a turn ended, as it is told by what was seen of the window.
    fn turned(
        &self,
        since: Instant,
        seen: (bool, bool, bool),
        record: Option<&Record>,
        dialog: Option<String>,
    ) -> Turn {
        let (arrived, ended, closed) = seen;
        Turn {
            arrived,
            ended,
            closed,
            took: since.elapsed(),
            held: (!ended).then(|| self.held(record)),
            dialog,
        }
    }

    /// Waits up to `within` for the message whose header starts with `header` to
    /// arrive in the window's record and its turn to end, looking at the window every
    /// [`LOOK`]; `keys` is asked after each look, with how long the wait has been and
    /// whether the message arrived, what the daemon does while it waits (press Enter
    /// once more, paste again) and what the case does to the window besides. A turn
    /// that has not ended while the window prints nothing for [`STALL_MS`] is held by
    /// something, and the screen it holds is noted; one whose terminal says it waits
    /// for an answer to a permission, or whose screen shows the options of a dialog, for
    /// [`BLOCKED`] is not waited for longer, for nobody answers. A turn that ended is over
    /// once the window is quiet, or [`OVER`] later whatever it prints.
    /// `within` is cut to what the case has left.
    pub fn wait_turn(
        &mut self,
        header: &str,
        within: Duration,
        mut keys: impl FnMut(&mut Self, Duration, bool) -> Result<()>,
    ) -> Result<Turn> {
        let within = self.life.cap(within);
        let started = Instant::now();
        let mut ended_since: Option<Instant> = None;
        let mut dialog = Dialog::default();
        loop {
            sleep(LOOK);
            self.look();
            let record = self.window.record();
            let (arrived, ended) = told(record.as_ref(), header);
            let closed = self.window.closed();
            let quiet = self.quiet_ms();
            let seen = (arrived, ended, closed);
            if ended {
                let since = *ended_since.get_or_insert_with(Instant::now);
                if quiet.is_some_and(|ms| ms >= QUIET_MS) || since.elapsed() >= OVER {
                    return Ok(self.turned(started, seen, record.as_ref(), None));
                }
            } else {
                ended_since = None;
                if quiet.is_some_and(|ms| ms >= STALL_MS) {
                    let rows = self.rows();
                    self.watch.stalled(&rows, &self.trail);
                }
                if let Some(held) = dialog.held(self) {
                    return Ok(self.turned(started, seen, record.as_ref(), Some(held)));
                }
            }
            if closed || started.elapsed() >= within {
                let waiting = self.dialog_now();
                return Ok(self.turned(started, seen, record.as_ref(), waiting));
            }
            keys(self, started.elapsed(), arrived)?;
        }
    }

    /// What the terminal says the window waits for, and since when, where it does.
    fn dialog_now(&self) -> Option<String> {
        let (since, said) = self.waiting()?;
        Some(format!(
            "{said} since {}, as its terminal says",
            plus(since)
        ))
    }

    /// [`Scene::dialog_now`], where the window has waited so for [`BLOCKED`].
    pub fn held_by_dialog(&self) -> Option<String> {
        let (since, _) = self.waiting()?;
        (self.trail.at().saturating_sub(since) >= BLOCKED)
            .then(|| self.dialog_now())
            .flatten()
    }
}

/// Since when a window is seen to wait for an answer to a permission, by what its
/// terminal says or its screen shows: its terminal marks a dialog when it opens, and
/// has not always, so the options on the screen count as well.
#[derive(Debug, Default)]
pub struct Dialog {
    since: Option<Instant>,
}

impl Dialog {
    /// Looks at the window once: what it waits for, where it has waited for [`BLOCKED`]
    /// (its terminal's words, and where it said none, the screen's).
    pub fn held(&mut self, scene: &Scene) -> Option<String> {
        let said = scene.dialog_now();
        let present = said.is_some() || scene.dialog_shown();
        self.waited(present, Instant::now())
            .filter(|waited| *waited >= BLOCKED)
            .map(|_| {
                said.unwrap_or_else(|| {
                    "waiting at a permission dialog, whose options are on the screen (its terminal marked nothing new)"
                        .to_owned()
                })
            })
    }

    /// How long a dialog has been there, at `now`, where `present` says it is: none where
    /// it is not (a dialog that went and came back begins again).
    fn waited(&mut self, present: bool, now: Instant) -> Option<Duration> {
        if !present {
            self.since = None;
            return None;
        }
        Some(now.saturating_duration_since(*self.since.get_or_insert(now)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_is_waited_for_from_when_it_was_first_seen_and_begins_again_once_it_has_gone() {
        let mut dialog = Dialog::default();
        let begun = Instant::now();
        let later = |seconds: u64| begun + Duration::from_secs(seconds);
        assert_eq!(dialog.waited(false, begun), None);
        assert_eq!(dialog.waited(true, later(1)), Some(Duration::ZERO));
        assert_eq!(
            dialog.waited(true, later(31)),
            Some(Duration::from_secs(30))
        );
        assert!(Duration::from_secs(30) >= BLOCKED);
        // It went (a key closed it) and came back: the wait begins again.
        assert_eq!(dialog.waited(false, later(40)), None);
        assert_eq!(dialog.waited(true, later(41)), Some(Duration::ZERO));
        assert_eq!(dialog.waited(true, later(50)), Some(Duration::from_secs(9)));
    }

    #[test]
    fn a_mark_is_answered_by_whatever_the_check_pressed_or_pasted_after_it_and_by_nothing_else() {
        let at = Duration::from_secs;
        // Nothing done yet, or only before the mark: the dialog it marked is open.
        assert!(!done_after(None, at(70)));
        assert!(!done_after(Some(at(52)), at(70)));
        assert!(!done_after(Some(at(70)), at(70)));
        // The app's interrupt, or the next message, after it: the dialog is gone.
        assert!(done_after(Some(at(95)), at(70)));
    }

    #[test]
    fn the_mode_a_window_had_is_told_from_the_first_the_wire_said_to_the_last() {
        assert_eq!(mode_story(Some("normal"), Some("normal")), "normal");
        assert_eq!(
            mode_story(Some("normal"), Some("bypass")),
            "normal → bypass"
        );
        assert_eq!(mode_story(None, Some("bypass")), "bypass");
        assert_eq!(mode_story(None, None), "?");
    }

    #[test]
    fn the_options_of_a_permission_dialog_are_found_in_the_rows_of_a_screen() {
        let dialog = [
            "└ $ echo ran > shell.txt".to_owned(),
            "❭ 1 Yes  (Approve once)".to_owned(),
            "6 No".to_owned(),
        ];
        assert!(shows_dialog(&dialog));
        let cancelled = [
            "✗ Canceled due to user interrupt".to_owned(),
            "❭ Ask Devin to build features".to_owned(),
        ];
        assert!(!shows_dialog(&cancelled));
        assert!(!shows_dialog(&[]));
    }

    #[test]
    fn the_label_of_full_permission_is_found_in_the_rows_of_a_screen_and_the_mode_is_said_with_it()
    {
        let bypassed = [
            "Ran command".to_owned(),
            "──────── (bypass permissions on) ─".to_owned(),
            "❭ Ask Devin to build features".to_owned(),
        ];
        assert!(shows_bypass(&bypassed));
        let plain = [
            "──────────────────────────".to_owned(),
            "❭ Ask Devin to build features".to_owned(),
        ];
        assert!(!shows_bypass(&plain));
        assert!(!shows_bypass(&[]));
        assert_eq!(
            mode_words(Some("normal"), false),
            "the mode: the wire says normal; the screen does not show \"bypass permissions on\""
        );
        assert_eq!(
            mode_words(Some("bypass"), true),
            "the mode: the wire says bypass; the screen shows \"bypass permissions on\""
        );
        assert!(mode_words(None, false).contains("the wire says nothing"));
    }
}
