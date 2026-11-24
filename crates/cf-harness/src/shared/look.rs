//! A look: one observation of a window, current and whole, and what it licenses (N17).
//!
//! Every decision about the window's input box reads one look and nothing else. A look is the
//! pane host's `pane.snapshot` that asked for the screen's last lines, taken with the screen's
//! lock held: the lines as drawn, the cursor, the two byte counts, the human's keys and the
//! quiets, all of one moment. The host names what a reader is given of the screen with `seen`.
//! A look exists only where it is current (`Look::of`): a host that did not answer, said not ok,
//! gave no `seen`, or said more was read than the screen shows gives none. **No look means look
//! again, never go.**
//!
//! An act on the input box (a paste, an Enter, a key that clears the box) is built from a look
//! only: the licences below are the functions that say go, and each go carries the look's
//! [`At`], which the host checks again where it writes. Another snapshot may say wait; only a
//! look says go.

use cf_proto::agents::Harness;
use cf_proto::panes::HumanInput;
use serde_json::Value;

use super::input_box::{self, Cursor, InputBox, SETTLED_MS, UNDRAWN_MS};
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
    /// More was read than the screen shows.
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

/// One observation of a window ([module](self)).
#[derive(Debug, Clone)]
pub struct Look {
    seen: String,
    typed: u64,
    unsent: bool,
    typed_quiet_ms: Option<u64>,
    output_quiet_ms: Option<u64>,
    screen_quiet_ms: Option<u64>,
    paste_in_flight: bool,
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
        let lines = snapshot.get("bottom").and_then(Value::as_array);
        let (Some(seen), Some(typed), Some(lines)) = (seen, typed, lines) else {
            return Err(NoLook::Old);
        };
        Ok(Self {
            seen: seen.to_owned(),
            typed,
            unsent: snapshot.get("unsent").and_then(Value::as_bool).unwrap_or(false),
            typed_quiet_ms: count("typedQuietMs"),
            output_quiet_ms: count("outputQuietMs"),
            screen_quiet_ms: count("screenQuietMs"),
            paste_in_flight: snapshot
                .get("pasteInFlight")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            lines: lines
                .iter()
                .filter_map(|line| line.as_str().map(str::to_owned))
                .collect(),
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

    /// Whether `harness`'s reader reads the box empty where the host counts the human's keys as
    /// left unsent, and the window has answered their last key: the last key [`SETTLED_MS`] or
    /// more before and the window having printed since ([`UNDRAWN_MS`] or more before where it
    /// has not). A paste told so goes in over the count, unless a key came since.
    pub fn read_empty_over_count(&self, harness: Harness) -> bool {
        // Whether the window answered the last key at all, by its output and not by what its
        // screen shows: a key that changes nothing shown (Backspace in an empty box) is
        // answered all the same (T-1083).
        let settled = self.typed_quiet_ms.is_some_and(|typed| {
            let drawn = self.output_quiet_ms.is_some_and(|printed| printed < typed);
            typed >= SETTLED_MS && (drawn || typed >= UNDRAWN_MS)
        });
        settled && self.reads(harness) == BoxReading::Empty
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

/// What the adapter and the engine know of the input box that no screen says.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxState {
    /// The box may hold a message of ours that did not go (a paste whose Enter was withheld, a
    /// message an interrupt put back, a `/compact` the human's cancel left): the paste clears it
    /// first.
    pub stale_paste: bool,
}

/// What a look licenses for a window that takes pastes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteLicence {
    Go {
        at: At,
        /// The look's reader read the box empty over the host's count of the human's keys.
        read_empty: bool,
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
    if over_count && !look.read_empty_over_count(harness) {
        return PasteLicence::Wait(Held::Unsent);
    }
    // A stale paste of ours is in the box where it holds anything: the paste empties it first,
    // from a box that reads. One the screen does not show holds the paste, as the human's own
    // words do.
    let stale = state.stale_paste && !over_count;
    if stale && reads == BoxReading::Unread {
        return PasteLicence::Wait(Held::Unsent);
    }
    if !over_count && !stale && reads == BoxReading::Holds {
        return PasteLicence::Wait(Held::Unsent);
    }
    let clear_first = (stale && reads != BoxReading::Empty).then_some(clear);
    PasteLicence::Go {
        at: look.at(),
        read_empty: over_count,
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
