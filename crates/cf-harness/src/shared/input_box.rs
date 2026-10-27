//! What a harness's input box holds, as its screen shows it. The pane host counts
//! the human's keys for what they leave unsent and holds a paste while any are
//! (`unsent`), but keys are a guess: those typed into a harness's dialog or
//! picker and left there with Escape count, though the box is empty, and every
//! delivery to the window waited for words that were never there (T-288). A
//! harness whose box its adapter can read says whether it is empty; the count is
//! the guess only where it cannot. Escape does not clear the count: every harness
//! keeps words in its box at one Escape, and only Claude Code clears them at two
//! (`cargo xtask live escape-unsent`, 2026-10-23).

use cf_proto::agents::Harness;
use serde_json::Value;

use crate::contract::{Pane, PaneHost};

/// What an input box on the screen holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputBox {
    /// Nothing is typed in it.
    Empty,
    /// Something is, or something that may be.
    Holds,
}

/// Where the cursor is on a screen's last lines as drawn (the pane host's
/// `cursor`): the line, by its place among them, how many characters of it are
/// before the cursor, whether it is shown, and how many characters of the line
/// from the cursor on are drawn plain, not styled: a harness draws the hint in an
/// empty box in a colour or faint, and words typed there plain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
    pub shown: bool,
    pub plain_after: usize,
}

impl Cursor {
    /// The cursor a snapshot's `cursor` says, where it says one.
    pub fn of(value: Option<&Value>) -> Option<Self> {
        let value = value?;
        let number = |name: &str| {
            value
                .get(name)
                .and_then(Value::as_u64)
                .and_then(|number| usize::try_from(number).ok())
        };
        Some(Self {
            line: number("line")?,
            col: number("col")?,
            shown: value.get("shown").and_then(Value::as_bool)?,
            plain_after: number("plainAfter")?,
        })
    }
}

/// What the input box of a `harness` window holds, where `screen` (its last lines
/// as drawn, the pane host's `bottom`) shows it, the cursor where `cursor` says;
/// none where it shows no box the harness's reader knows, or the harness has no
/// reader (Codex, Pi and OpenCode take their messages through their own channels,
/// which no typing holds back).
pub fn read(harness: Harness, screen: &[String], cursor: Option<Cursor>) -> Option<InputBox> {
    match harness {
        Harness::Claude => crate::claude::input_box::read(screen, cursor),
        Harness::Devin => crate::devin::input_box::read(screen, cursor),
        Harness::Muse => crate::muse::read_input_box(screen, cursor),
        Harness::Codex | Harness::Pi | Harness::Opencode => None,
    }
}

/// Whether `line` is a rule drawn across the screen: `─`, with a label in it at
/// most (Devin's says its permission mode), beginning and ending with the rule.
pub(crate) fn is_rule(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("────") && line.ends_with('─')
}

/// The last input box on `screen` whose first line begins with the prompt mark
/// `mark`, between a rule above it and the next rule below.
pub(crate) struct Boxed<'a> {
    /// Its first line's place on the screen.
    pub(crate) line: usize,
    /// How many characters of that line come before the text after the mark: where
    /// the cursor is at a box with nothing typed in it.
    pub(crate) start: usize,
    /// Its first line after the mark.
    pub(crate) typed: &'a str,
    /// Whether any line after that holds anything.
    pub(crate) more: bool,
}

impl Boxed<'_> {
    /// Whether `cursor` says the box holds only a hint: it is at the box's start,
    /// right after the mark and its space, and nothing from it on is drawn plain. Words
    /// typed and the cursor moved back to their start (Home) are drawn plain.
    pub(crate) fn only_a_hint(&self, cursor: Option<Cursor>) -> bool {
        !self.more
            && cursor.is_some_and(|cursor| {
                cursor.line == self.line && cursor.col == self.start && cursor.plain_after == 0
            })
    }
}

/// [`Boxed`], where `screen` shows one: none where it shows no such box, for a
/// picker's mark on a choice has no rule right above it, and a box drawn halfway
/// has no rule below it yet.
pub(crate) fn boxed(screen: &[String], mark: char) -> Option<Boxed<'_>> {
    let first = screen
        .iter()
        .rposition(|line| line.trim_start().starts_with(mark))?;
    if first == 0 || !is_rule(&screen[first - 1]) {
        return None;
    }
    let end = first + screen[first..].iter().position(|line| is_rule(line))?;
    let line = &screen[first];
    let indent = line
        .chars()
        .take_while(|character| character.is_whitespace())
        .count();
    let typed = line.trim_start().trim_start_matches(mark).trim();
    let more = screen[first + 1..end]
        .iter()
        .any(|line| !line.trim().is_empty());
    Some(Boxed {
        line: first,
        start: indent + 2,
        typed,
        more,
    })
}

/// How long the human's last key must be past before the screen is believed over
/// the count: a key typed a moment before the look may not be drawn yet.
pub(crate) const SETTLED_MS: u64 = 1_000;

/// How long a key the window has printed nothing since must be past before the
/// screen is believed over the count. A key not drawn yet reads as a box without
/// it, and the paste would go in behind it, the two sent as one; but a key that
/// changes nothing on the screen (Escape at a box it leaves as it is) is never
/// drawn. `cargo xtask live escape-unsent` (2026-10-23) measured the longest a key
/// took to be drawn at 69 ms in Devin 3000.11.92 and 89 ms in Claude Code 2.1.309
/// (its model picker closing): this is over fifty times that. Ctrl+U at an empty
/// box, Escape at a box it leaves as it is, and keys typed into Claude's model
/// picker, which takes no filter, were drawn in neither.
pub(crate) const UNDRAWN_MS: u64 = 5_000;

/// What the human left unsent in a window, where the pane host's count says they
/// left something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unsent {
    /// The box may hold it: the delivery waits ([`crate::contract::Held::Unsent`]).
    Held,
    /// The box read empty when the host had taken this many writes of the human's
    /// keys, the last of them [`SETTLED_MS`] or more before and the window having
    /// printed since ([`UNDRAWN_MS`] or more before where it has not): a paste told so
    /// goes in (`readEmpty`), unless a key came since.
    ReadEmpty(u64),
}

/// What a `harness` window holds unsent, the count up in the snapshot `counted`:
/// its screen's last lines as drawn, read by the harness's reader ([`read`]). A box
/// it does not read, a key too recent, or a host that gives no lines is the count's
/// word; and so is a host whose snapshot says no `typed`, which would refuse a paste
/// that says `readEmpty` (its request takes no field it does not know).
pub(crate) async fn unsent(
    host: &dyn PaneHost,
    pane: &Pane,
    harness: Harness,
    counted: &Value,
) -> Unsent {
    if counted.get("typed").and_then(Value::as_u64).is_none() {
        return Unsent::Held;
    }
    let Ok(snapshot) = super::pane::snapshot_drawn(host, pane).await else {
        return Unsent::Held;
    };
    read_unsent(harness, &snapshot)
}

/// [`unsent`], of the snapshot `snapshot` (with its `bottom`).
pub(crate) fn read_unsent(harness: Harness, snapshot: &Value) -> Unsent {
    let quiet = |name: &str| snapshot.get(name).and_then(Value::as_u64);
    let settled = quiet("typedQuietMs").is_some_and(|typed| {
        let drawn = quiet("outputQuietMs").is_some_and(|printed| printed < typed);
        typed >= SETTLED_MS && (drawn || typed >= UNDRAWN_MS)
    });
    let typed = snapshot.get("typed").and_then(Value::as_u64);
    let lines: Option<Vec<String>> =
        snapshot
            .get("bottom")
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .filter_map(|line| line.as_str().map(str::to_owned))
                    .collect()
            });
    let cursor = Cursor::of(snapshot.get("cursor"));
    match (settled, typed, lines) {
        (true, Some(typed), Some(lines))
            if read(harness, &lines, cursor) == Some(InputBox::Empty) =>
        {
            Unsent::ReadEmpty(typed)
        }
        _ => Unsent::Held,
    }
}
