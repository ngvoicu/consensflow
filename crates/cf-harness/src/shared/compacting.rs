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
//! Only a status line counts: its words begin the line, past whitespace, the frame its
//! harness draws it in (Pi draws it inside the rule above its input box: "── ⠧ Compacting
//! context... (escape to cancel) ──", `cargo xtask live compaction-screens pi`) and one of
//! the glyphs its harness turns before them (Codex's bullet, which blinks hollow; Claude
//! Code's star, a `*` off macOS; Pi's braille spinner). What is done ("Conversation compacted",
//! "• Context compacted") is not compacting, nor is a line that quotes the words: a `cat`
//! or `git diff` of these readers, a Markdown bullet or quote, a conversation's line that
//! begins with anything else (Calliope's P2-1 of T-917).

/// A harness's status lines while it compacts: what of the frame it draws them in may come
/// first, the glyphs that may turn before them, and the words they begin with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Statuses {
    frame: &'static [char],
    glyphs: &'static [char],
    words: &'static [&'static str],
}

/// Whether one of `screen`'s lines (the window's last ones) is one of `statuses`.
pub(crate) fn drawn(screen: &[String], statuses: &Statuses) -> bool {
    screen.iter().any(|line| {
        let line =
            line.trim_start_matches(|c: char| c.is_whitespace() || statuses.frame.contains(&c));
        let words = line
            .strip_prefix(|c: char| statuses.glyphs.contains(&c))
            .map_or(line, str::trim_start);
        statuses
            .words
            .iter()
            .any(|status| words.starts_with(status))
    })
}

/// Claude Code's statuses while it compacts: the spinner's words, which its renderer ends
/// with an ellipsis, and its hooks before it.
pub(crate) const CLAUDE: Statuses = Statuses {
    frame: &[],
    glyphs: &['·', '✢', '✳', '✶', '✻', '✽', '*'],
    words: &["Compacting conversation", "Running PreCompact hooks"],
};

/// Codex's.
pub(crate) const CODEX: Statuses = Statuses {
    frame: &[],
    glyphs: &['•', '◦'],
    words: &["Compacting context ("],
};

/// Pi's, up to the key that cancels, which is its keymap's (`keyText("app.interrupt")`).
pub(crate) const PI: Statuses = Statuses {
    frame: &['─'],
    glyphs: &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
    words: &[
        "Compacting context... (",
        "Auto-compacting... (",
        "Context overflow detected, Auto-compacting... (",
    ],
};

#[cfg(test)]
mod tests;
