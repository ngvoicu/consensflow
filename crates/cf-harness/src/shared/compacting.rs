//! The status a harness draws while it compacts its conversation's context: the window
//! is at work, and what it was handed waits for the compaction to end (T-901). Each
//! harness says it in its own words, on the status line above its input box, and none
//! of them writes anything to its record while it compacts that says so: Claude Code and
//! Pi write the compaction once it is over, Codex an open turn and nothing more until
//! then (field, 3.0.6-alpha.2: 45 minutes of it). Read from the binaries and packages,
//! 2026-10-28:
//!
//! - Claude Code 2.1.312: "✻ Compacting conversation… (12s)", the glyph turning, or
//!   the same words dim with none; "Running PreCompact hooks…" before it.
//! - Codex 0.165.0: "• Compacting context (2m 57s • esc to interrupt)", then
//!   "└ Making room to continue." under it (the field's screen).
//! - Pi 1.4.0: "Compacting context... (escape to cancel)" after `/compact`,
//!   "Auto-compacting... (escape to cancel)" by itself, and "Context overflow
//!   detected, Auto-compacting... (escape to cancel)".
//!
//! Devin 3000.11 and OpenCode 1.19 draw none this reads: Devin compacts in the middle of
//! a turn, its record and its wire at work all the while, and refuses `/compact` while
//! busy; OpenCode writes its compaction's user message and streams its summary as a
//! turn of its own, which its record reads at work.
//!
//! Only a status line counts: its words begin the line, after the glyph that turns
//! before them. What is done ("Conversation compacted", "• Context compacted") is not
//! compacting, nor is a conversation that quotes these words in the middle of a line.

/// Whether one of `screen`'s lines (the window's last ones) is a status line that begins,
/// past its glyph, with one of `statuses`.
pub(crate) fn drawn(screen: &[String], statuses: &[&str]) -> bool {
    screen.iter().any(|line| {
        let words = line.trim_start_matches(|c: char| !c.is_alphanumeric());
        statuses.iter().any(|status| words.starts_with(status))
    })
}

/// Claude Code's statuses while it compacts.
pub(crate) const CLAUDE: [&str; 3] = [
    "Compacting conversation…",
    "Compacting conversation...",
    "Running PreCompact hooks…",
];

/// Codex's.
pub(crate) const CODEX: [&str; 1] = ["Compacting context ("];

/// Pi's.
pub(crate) const PI: [&str; 3] = [
    "Compacting context... (escape to cancel)",
    "Auto-compacting... (escape to cancel)",
    "Context overflow detected, Auto-compacting... (escape to cancel)",
];

#[cfg(test)]
mod tests;
