//! A look: one observation of a window, current and whole, and what it licenses (N17).
//!
//! Every decision about the window's input box reads one look and nothing else. A look is the
//! pane host's `pane.snapshot` that asked for the screen's last lines, taken with the screen's
//! lock held: the lines as drawn, the cursor, the two byte counts, the human's keys and the
//! quiets, all of one moment. The host names what a reader is given of the screen with `seen`.
//! A look exists only where it is current (`Look::of`): a host that did not answer, said not ok,
//! gave no `seen`, or did not say `outputCaughtUp` is true (nothing waits at the window's
//! terminal and the screen shows all that was read) gives none. **No look means look again,
//! never go.**
//!
//! Where the host counts the person's keys as left unsent, a box read empty sets the count aside
//! only on evidence the host took at the key ([`Look::evidence`]). No length of time sets it
//! aside, and no output does.
//!
//! An act on the input box (a paste, an Enter, a key that clears the box) is built from a look
//! only: the licences below are the functions that say go, and each go carries the look's
//! [`At`], which the host checks again where it writes. Another snapshot may say wait; only a
//! look says go.

use cf_proto::agents::Harness;
use cf_proto::panes::{word_tail, Evidence, HumanInput, TAIL_LINES};
use serde_json::Value;

use super::dialog;
use super::input_box::{self, Cursor, InputBox, SETTLED_MS};
use crate::contract::Held;

/// How long the window's screen must have shown no change before an Enter that did not go with
/// the paste is pressed: a window that draws is at work, or taking the paste in.
pub const SCREEN_QUIET_FOR_ENTER_MS: u64 = 3_000;

/// What licensed an act on the input box: the look's `seen` (as the host named it) and its count
/// of the human's keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct At {
    pub seen: String,
    pub typed: u64,
}

/// Why there is no look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoLook {
    /// The host did not answer, answered late, or said not ok.
    NoAnswer,
    /// The window printed more than the screen shows: read and not drawn, or waiting at its
    /// terminal.
    Behind,
    /// The host gives no `seen` (older than this).
    Old,
}

impl NoLook {
    /// The words a wait for a look says.
    pub fn words(self) -> &'static str {
        match self {
            Self::NoAnswer => "its pane host did not answer",
            Self::Behind => "its screen does not yet show all that it printed",
            Self::Old => "its pane host does not name what its screen shows",
        }
    }
}

/// What a window that takes pastes says when it is asked for readiness with no look.
pub const NO_LOOK: &str = "its pane host gave no look at its screen";

/// What a harness's reader makes of the input box on a look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxReading {
    Empty,
    Holds,
    /// No box the harness's reader knows is shown (or the harness reads none).
    Unread,
}

/// A screen the host kept at one of the person's keys, as a look carries it.
#[derive(Debug, Clone)]
struct Kept {
    lines: Vec<String>,
    cursor: Option<Cursor>,
    current: bool,
}

impl Kept {
    fn of(value: &Value) -> Option<Self> {
        Some(Self {
            lines: drawn_lines(value.get("bottom")?)?,
            cursor: Cursor::of(value.get("cursor")),
            current: value.get("current").and_then(Value::as_bool) == Some(true),
        })
    }
}

/// The screens a look carries while the count is above zero (`unsentAt`).
#[derive(Debug, Clone, Default)]
struct UnsentAt {
    /// The screen before each of the person's writes that added a character, in order.
    typed_at: Vec<Kept>,
    /// Whether a screen the count's characters need is missing from [`Self::typed_at`]: more
    /// writes than the host keeps screens of, or one this look cannot read.
    typed_more: bool,
    /// The screen before the first write since the last character that added none and left the
    /// count above zero.
    pressed_over: Option<Kept>,
}

impl UnsentAt {
    fn of(value: Option<&Value>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        let sent = value.get("typedAt").and_then(Value::as_array);
        let typed_at: Vec<Kept> = sent.into_iter().flatten().filter_map(Kept::of).collect();
        Self {
            typed_more: value.get("typedMore") != Some(&Value::Bool(false))
                || sent.is_none_or(|sent| sent.len() != typed_at.len()),
            typed_at,
            pressed_over: value.get("pressedOver").and_then(Kept::of),
        }
    }
}

fn drawn_lines(value: &Value) -> Option<Vec<String>> {
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|line| line.as_str().map(str::to_owned))
            .collect(),
    )
}

/// One observation of a window ([module](self)).
#[derive(Debug, Clone)]
pub struct Look {
    seen: String,
    typed: u64,
    unsent: bool,
    typed_quiet_ms: Option<u64>,
    screen_quiet_ms: Option<u64>,
    paste_in_flight: bool,
    unsent_at: UnsentAt,
    lines: Vec<String>,
    cursor: Option<Cursor>,
    human: Option<HumanInput>,
}

impl Look {
    /// The look `snapshot` is, a host's answer to a `pane.snapshot` that asked for `bottom`.
    pub fn of(snapshot: &Value) -> Result<Self, NoLook> {
        if snapshot.get("ok") != Some(&Value::Bool(true)) {
            return Err(NoLook::NoAnswer);
        }
        let count = |name: &str| snapshot.get(name).and_then(Value::as_u64);
        if let (Some(read), Some(drawn)) = (count("outputRead"), count("outputDrawn")) {
            if read > drawn {
                return Err(NoLook::Behind);
            }
        }
        let seen = snapshot.get("seen").and_then(Value::as_str);
        let typed = count("typed");
        let lines = snapshot.get("bottom").and_then(drawn_lines);
        let (Some(seen), Some(typed), Some(lines)) = (seen, typed, lines) else {
            return Err(NoLook::Old);
        };
        // Current, as the host asked the terminal: absent, no Boolean or false is no look.
        if snapshot.get("outputCaughtUp") != Some(&Value::Bool(true)) {
            return Err(NoLook::Behind);
        }
        Ok(Self {
            seen: seen.to_owned(),
            typed,
            unsent: snapshot
                .get("unsent")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            typed_quiet_ms: count("typedQuietMs"),
            screen_quiet_ms: count("screenQuietMs"),
            paste_in_flight: snapshot
                .get("pasteInFlight")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            unsent_at: UnsentAt::of(snapshot.get("unsentAt")),
            lines,
            cursor: Cursor::of(snapshot.get("cursor")),
            human: HumanInput::of(snapshot),
        })
    }

    /// The screen's last lines as drawn.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// What this look licenses an act by.
    pub fn at(&self) -> At {
        At {
            seen: self.seen.clone(),
            typed: self.typed,
        }
    }

    /// What `harness`'s reader makes of the input box on this look, and of it only.
    pub fn reads(&self, harness: Harness) -> BoxReading {
        match input_box::read(harness, &self.lines, self.cursor) {
            Some(InputBox::Empty) => BoxReading::Empty,
            Some(InputBox::Holds) => BoxReading::Holds,
            None => BoxReading::Unread,
        }
    }

    /// The words in the box, for the human told of a delivery held for them.
    pub fn words(&self, harness: Harness) -> Option<String> {
        input_box::shown(harness, &self.lines, self.cursor)
    }

    /// The evidence, of where the person's keys went, that lets a box read empty set the
    /// host's count of them aside; none where there is none and the count holds:
    /// - **erased**: at the first key the count cannot follow after their last character, the
    ///   screen was current and `harness`'s reader reads its box on it as holding text;
    /// - **typed elsewhere**: the screen before every write that added a character showed a
    ///   dialog or a picker and no box, and the last of them was current, or the screen at the
    ///   first key after it was current and showed one too.
    ///
    /// The caller still asks that the box reads empty now.
    pub fn evidence(&self, harness: Harness) -> Option<Evidence> {
        let UnsentAt {
            typed_at,
            typed_more,
            pressed_over,
        } = &self.unsent_at;
        let erased = pressed_over.as_ref().is_some_and(|kept| {
            kept.current
                && input_box::read(harness, &kept.lines, kept.cursor) == Some(InputBox::Holds)
        });
        if erased {
            return Some(Evidence::Erased);
        }
        let elsewhere = !typed_more
            && typed_at.iter().all(|kept| shows_a_dialog(harness, kept))
            && typed_at.last().is_some_and(|last| {
                // A screen that was not current may have had the dialog's end on its way; a
                // later one that was current and still shows a dialog says it had not.
                last.current
                    || pressed_over
                        .as_ref()
                        .is_some_and(|over| over.current && shows_a_dialog(harness, over))
            });
        elsewhere.then_some(Evidence::Elsewhere)
    }

    /// Whether the host counts the human's keys as left unsent.
    pub fn unsent(&self) -> bool {
        self.unsent
    }

    pub fn typed_quiet_ms(&self) -> Option<u64> {
        self.typed_quiet_ms
    }

    pub fn screen_quiet_ms(&self) -> Option<u64> {
        self.screen_quiet_ms
    }

    pub fn paste_in_flight(&self) -> bool {
        self.paste_in_flight
    }

    /// What the host said of the human's input, for the trace of a delivery held for it.
    pub fn human_input(&self) -> Option<HumanInput> {
        self.human.clone()
    }
}

/// Whether the screen `kept` shows a dialog or a picker of `harness`'s own, which took the key
/// and not the input box: the dialog reader that `Window::dialog` of Claude Code, Devin and Muse
/// Code each builds on reads one on it, given its lines as the host's tail gives them (no empty
/// line, no rule that says no word), with no prompt of Claude Code's or Muse's below its choices;
/// and the box reader reads no box on it. A conversation that quotes a list is drawn over the
/// box, and a key typed there went into the box.
fn shows_a_dialog(harness: Harness, kept: &Kept) -> bool {
    input_box::read(harness, &kept.lines, kept.cursor).is_none()
        && dialog::read(&word_tail(kept.lines.iter().cloned(), TAIL_LINES)).is_some_and(|shown| {
            !crate::claude::trust::shows_prompt(&shown.below)
                && !crate::muse::shows_prompt(&shown.below)
        })
}

/// What the adapter and the engine know of the input box that no screen says.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxState {
    /// The box may hold a message of ours that did not go (a paste whose Enter was withheld, a
    /// message an interrupt put back, a `/compact` the human's cancel left): the paste clears it
    /// first.
    pub stale_paste: bool,
    /// The stale words are ours and were put back by our own stop (an interrupt of ours that
    /// took the brief back out of the conversation): where the box cannot be read, the clear
    /// goes blind with the paste if no key of the person's was counted (N6).
    pub put_back_by_our_stop: bool,
}

/// What a look licenses for a window that takes pastes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteLicence {
    Go {
        at: At,
        /// The look's reader read the box empty over the host's count of the person's keys, on
        /// this evidence.
        read_empty: Option<Evidence>,
        /// The key that empties the box, written with the paste: a stale paste is in the box.
        clear_first: Option<&'static [u8]>,
    },
    Wait(Held),
}

/// Whether a paste may go into a window on this look, `clear` being the key that empties its box.
pub fn licence_paste(
    look: &Look,
    harness: Harness,
    state: BoxState,
    clear: &'static [u8],
) -> PasteLicence {
    if look.paste_in_flight() {
        return PasteLicence::Wait(Held::Because(
            "a paste is on its way to the window".to_owned(),
        ));
    }
    // A key a moment ago may not be drawn yet: the box on the screen may lack it.
    if look
        .typed_quiet_ms()
        .is_some_and(|typed| typed < SETTLED_MS)
    {
        return PasteLicence::Wait(if look.unsent() {
            Held::Unsent
        } else {
            Held::Because("you typed in this window a moment ago".to_owned())
        });
    }
    let reads = look.reads(harness);
    let over_count = look.unsent();
    // The box read empty over the count sets it aside only on evidence; a box that holds text,
    // or cannot be read, is their words.
    let read_empty = if over_count {
        if reads != BoxReading::Empty {
            return PasteLicence::Wait(Held::Unsent);
        }
        let Some(evidence) = look.evidence(harness) else {
            return PasteLicence::Wait(Held::Unproven);
        };
        // A window draws a dialog's end and the keys typed ahead of it in two frames, a moment
        // apart, and a look between them reads the box empty. A screen that never holds still
        // keeps the message until Ctrl+U: the engine says so once the wait has lasted.
        if look
            .screen_quiet_ms()
            .is_none_or(|quiet| quiet < SETTLED_MS)
        {
            return PasteLicence::Wait(Held::Settling);
        }
        Some(evidence)
    } else {
        None
    };
    // A stale paste of ours is in the box where it holds anything: the paste empties it first.
    // A box the screen does not show is cleared blind, with the paste, only where the host has
    // counted no key of the person's in this window (N6): nobody has typed, so the words in it
    // are the ones our own stop put back. Any key since, an arrow that recalls their own words
    // among them, leaves it held, and the words say what is in the box, never "what you typed".
    let stale = state.stale_paste && !over_count;
    if stale && reads == BoxReading::Unread && !(state.put_back_by_our_stop && look.at().typed == 0)
    {
        if !state.put_back_by_our_stop {
            return PasteLicence::Wait(Held::Unsent);
        }
        // Said as what is in the box and what lifts the hold: Enter sends it, and the record
        // then no longer shows it put back. Nothing here says "typed", for nothing is unsent.
        return PasteLicence::Wait(Held::ForHuman(
            "its input box holds a message ConsensFlow put back there: press Enter in this window to send it"
                .to_owned(),
        ));
    }
    if !over_count && !stale && reads == BoxReading::Holds {
        return PasteLicence::Wait(Held::Unsent);
    }
    let clear_first = (stale && reads != BoxReading::Empty).then_some(clear);
    PasteLicence::Go {
        at: look.at(),
        read_empty,
        clear_first,
    }
}

/// What an Enter is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterFor {
    /// A paste the window has not sent, its record never having shown it.
    PasteNotSent,
    /// A paste that the window may still hold in its box: its box reads it there.
    PasteStillThere,
    /// A turn that never began: whatever the box reads.
    TurnNeverBegan,
    /// Devin's `/bypass`.
    DevinBypass,
}

/// What a look licenses of an Enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnterLicence {
    Go(At),
    Wait(&'static str),
}

/// Whether an Enter that is not a paste's first may be pressed on this look.
pub fn licence_enter(look: &Look, harness: Harness, needs: EnterFor) -> EnterLicence {
    if look.unsent() {
        return EnterLicence::Wait("the human has unsent text in the window");
    }
    if look
        .screen_quiet_ms()
        .is_none_or(|quiet| quiet < SCREEN_QUIET_FOR_ENTER_MS)
    {
        return EnterLicence::Wait("the window's screen is not still");
    }
    let reads = look.reads(harness);
    let fits = match needs {
        EnterFor::TurnNeverBegan => true,
        EnterFor::PasteNotSent => reads != BoxReading::Empty,
        EnterFor::PasteStillThere | EnterFor::DevinBypass => reads == BoxReading::Holds,
    };
    if fits {
        EnterLicence::Go(look.at())
    } else {
        EnterLicence::Wait("its input box does not hold the paste")
    }
}

/// What the arrival's whole wait comes to, of a paste the record never showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterWait {
    /// No look: its rest begins again and nothing goes.
    LookAgain,
    /// The window's screen changed within the last seconds: it is at work, or taking the paste in.
    Printing,
    /// The box holds the paste: an Enter, on this look.
    EnterAgain(At),
    /// A box that holds nothing the reader reads as the paste: pasted again.
    PasteAgain,
}

pub fn after_wait(look: Result<&Look, &NoLook>, harness: Harness) -> AfterWait {
    let Ok(look) = look else {
        return AfterWait::LookAgain;
    };
    if look
        .screen_quiet_ms()
        .is_some_and(|quiet| quiet < SCREEN_QUIET_FOR_ENTER_MS)
    {
        return AfterWait::Printing;
    }
    match look.reads(harness) {
        BoxReading::Holds => AfterWait::EnterAgain(look.at()),
        BoxReading::Empty | BoxReading::Unread => AfterWait::PasteAgain,
    }
}

#[cfg(test)]
mod tests;
