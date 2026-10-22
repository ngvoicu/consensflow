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

/// What the input box of a `harness` window holds, where `screen` (its last lines
/// as drawn, the pane host's `bottom`) shows it; none where it shows no box the
/// harness's reader knows, or the harness has no reader (Codex, Pi and OpenCode
/// take their messages through their own channels, which no typing holds back).
pub fn read(harness: Harness, screen: &[String]) -> Option<InputBox> {
    match harness {
        Harness::Claude => crate::claude::input_box::read(screen),
        Harness::Devin => crate::devin::input_box::read(screen),
        Harness::Codex | Harness::Pi | Harness::Opencode => None,
    }
}

/// Whether `line` is a rule drawn across the screen: `─`, with a label in it at
/// most (Devin's says its permission mode), beginning and ending with the rule.
pub(crate) fn is_rule(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("────") && line.ends_with('─')
}

/// The text of the last input box on `screen` whose first line begins with the
/// prompt mark `mark`, between a rule above it and the next rule below: its
/// first line after the mark, and whether any line after that holds anything.
/// None where the screen shows no such box: a picker's mark on a choice has no
/// rule right above it, and a box drawn halfway has no rule below it yet.
pub(crate) fn boxed(screen: &[String], mark: char) -> Option<(&str, bool)> {
    let first = screen
        .iter()
        .rposition(|line| line.trim_start().starts_with(mark))?;
    if first == 0 || !is_rule(&screen[first - 1]) {
        return None;
    }
    let end = first + screen[first..].iter().position(|line| is_rule(line))?;
    let typed = screen[first].trim_start().trim_start_matches(mark).trim();
    let more = screen[first + 1..end]
        .iter()
        .any(|line| !line.trim().is_empty());
    Some((typed, more))
}

/// How long the human's last key must be past before the screen is believed over
/// the count: a key typed a moment before the look may not be drawn yet.
pub(crate) const SETTLED_MS: u64 = 1_000;

/// What the human left unsent in a window, where the pane host's count says they
/// left something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unsent {
    /// The box may hold it: the delivery waits ([`crate::contract::Held::Unsent`]).
    Held,
    /// The box read empty when the host had taken this many writes of the human's
    /// keys, the last of them [`SETTLED_MS`] or more before: a paste told so goes in
    /// (`readEmpty`), unless a key came since.
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
    let settled = snapshot
        .get("typedQuietMs")
        .and_then(Value::as_u64)
        .is_some_and(|quiet| quiet >= SETTLED_MS);
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
    match (settled, typed, lines) {
        (true, Some(typed), Some(lines)) if read(harness, &lines) == Some(InputBox::Empty) => {
            Unsent::ReadEmpty(typed)
        }
        _ => Unsent::Held,
    }
}
