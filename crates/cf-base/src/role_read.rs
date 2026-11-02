//! How much of a role printed by `cf inbox read` the window it was written to
//! reads whole. A role too long for one message goes into its window as its
//! opening and that command (the engine's `role_change`), and counts as told once
//! `cf` printed it whole and said so to the board. The command and the engine
//! read this one bound, so the size of a role the engine counts as read is the
//! size `cf` acknowledges.
//!
//! A role goes by a note only to the windows of two harnesses: a Codex thread
//! that keeps its first instructions and is resumed, and a Muse session that
//! began with none (its `/clear`). Both read a command's output whole well past
//! the 10,000 bytes `cf` proves for every harness (the `cut` of the `cf` crate):
//! probed on 2026-10-31 with one `cat` of a 29,923-byte, 500-line file of the
//! chief's role text, its last line a marker.
//! - **Codex 0.166.0** (`codex exec`): the tool output its rollout records is the
//!   whole file, the last line in it and no "truncated".
//! - **Muse 1.4.14** (`muse exec --json`): its tool result says
//!   `"truncated": false`, `"original_output_tokens": 7481`, and its output is
//!   the file byte for byte.
//!
//! Each model answered the marker. The bound is under what was shown, in bytes
//! and in lines both.

/// An output of fewer bytes than this is read whole by the window of every
/// harness a role goes to by a note.
pub const ROLE_READ_WHOLE_BYTES: usize = 29_000;

/// An output of fewer lines than this is read whole by the same windows.
pub const ROLE_READ_WHOLE_LINES: usize = 500;

/// Whether what `cf` printed of a role, `printed`, is read whole by the window
/// the role was written to.
pub fn role_read_whole(printed: &str) -> bool {
    printed.len() < ROLE_READ_WHOLE_BYTES && printed.matches('\n').count() < ROLE_READ_WHOLE_LINES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_is_read_whole_only_below_the_bound_in_bytes_and_in_lines() {
        assert!(role_read_whole(&"x".repeat(ROLE_READ_WHOLE_BYTES - 1)));
        assert!(!role_read_whole(&"x".repeat(ROLE_READ_WHOLE_BYTES)));
        assert!(role_read_whole(&"\n".repeat(ROLE_READ_WHOLE_LINES - 1)));
        assert!(!role_read_whole(&"\n".repeat(ROLE_READ_WHOLE_LINES)));
        // Bytes, not characters: 15,000 two-byte characters are 30,000 bytes.
        assert!(!role_read_whole(&"é".repeat(15_000)));
    }
}
