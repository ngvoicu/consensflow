//! What a harness shows its model of a command's output, and so which output
//! `cf` may say it wrote whole. `cf task get` and `cf inbox read` print an
//! answer in full and tell the board so, and the board then counts the answer
//! received: its reader has it. A harness cuts a long output before its model
//! reads it, so for an answer past the cut that is not so, and an answer
//! marked read is never pasted: its task goes on, and what the agent says next
//! ("I still need …") can become the result. So an output that may be cut says
//! nothing of the answers in it, and they come as text.
//!
//! The cuts, read in the harnesses installed here on 2026-10-07, in their own
//! files (none was run):
//! - **Claude Code 2.1.292**: 30,000 characters of a command's output reach
//!   the model inline, and past that it gets the first 2,000 bytes and the
//!   path of a file with the rest. The setting is `bashOutputMaxChars`, else
//!   `BASH_MAX_OUTPUT_LENGTH`: default `30000` (`Isr` in the binary, and the
//!   setting's own description: "default 30000; values clamp to 4000-128000"),
//!   upper limit 150,000. A person may set it as low as 4,000; that is theirs.
//! - **Codex 0.160.1**: 10,000 tokens, the `truncation_policy` of every one of
//!   the eleven models its binary bundles (`mode: tokens, limit: 10000`),
//!   which `tool_output_token_limit` overrides; past it the output is cut to
//!   its head and its tail with "…N tokens truncated…" between. What 10,000
//!   tokens come to in bytes is not stated by anything installed here: Codex
//!   counts tokens by an estimate of its own, which is not the model's. Its
//!   open source is understood to take four bytes to a token (about 40,000
//!   bytes), and that was not checked. But a token covers one byte at the
//!   least, whatever the estimate, so fewer than 10,000 bytes are fewer than
//!   10,000 tokens: that much Codex shows whole, provably.
//! - **Pi 1.0.4**: 2,000 lines or 51,200 bytes, whichever comes first, and the
//!   tail is what is kept (`dist/core/tools/truncate.js`, `DEFAULT_MAX_LINES`
//!   and `DEFAULT_MAX_BYTES`, and the bash tool's own description).
//! - **OpenCode 1.18.35**: 2,000 lines or 51,200 bytes, saved to a file past
//!   that (`tool_output.max_lines` and `max_bytes`, "default: 2000" and
//!   "default: 51200" in its configuration schema).
//! - **Devin 3000.6.14**: cuts too (its binary holds "[output truncated]" and
//!   "Output truncated at line "), and no number was found, in the binary or
//!   in the docs it ships. It is not known to be above these.
//!
//! The smallest cut that can be proven is Codex's: fewer than 10,000 bytes
//! are fewer than its 10,000 tokens. A byte count under 10,000 is a count of
//! UTF-16 units and of characters under it too (a character is one byte or
//! more, and a unit never fewer than a character), so under Claude Code's
//! 30,000 characters, and under Pi's and OpenCode's bytes: so bytes are what
//! is counted, and one number serves them all. No harness with a number cuts
//! below 2,000 lines. A thread longer than this says nothing of its answers,
//! which then come as text as well: a duplicate, never a loss.

/// An output shorter than this, in bytes, is shown whole by every harness
/// whose cut was found as a number: Codex's 10,000 tokens are the smallest
/// that can be proven, as a token is never less than a byte.
const SEEN_WHOLE_BYTES: usize = 10_000;

/// An output of fewer lines than this is shown whole by every harness whose
/// cut was found as a number: Pi's and OpenCode's 2,000 are the smallest.
const SEEN_WHOLE_LINES: usize = 2_000;

/// Whether what a command printed, `printed`, is shorter than the smallest cut
/// a harness makes of one: all of it reaches the model.
pub(super) fn seen_whole(printed: &str) -> bool {
    printed.len() < SEEN_WHOLE_BYTES && printed.matches('\n').count() < SEEN_WHOLE_LINES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_output_is_whole_to_every_harness_only_below_the_smallest_cut() {
        assert!(seen_whole(""));
        assert!(seen_whole(&"x".repeat(SEEN_WHOLE_BYTES - 1)));
        assert!(!seen_whole(&"x".repeat(SEEN_WHOLE_BYTES)));
        assert!(seen_whole(&"\n".repeat(SEEN_WHOLE_LINES - 1)));
        assert!(!seen_whole(&"\n".repeat(SEEN_WHOLE_LINES)));
    }

    #[test]
    fn what_is_counted_is_bytes_so_that_characters_of_more_than_one_are_not_missed() {
        // Half the characters, all the bytes: 16,000 two-byte characters.
        assert!(!seen_whole(&"é".repeat(SEEN_WHOLE_BYTES / 2)));
        assert!(seen_whole(&"é".repeat(SEEN_WHOLE_BYTES / 2 - 1)));
    }
}
