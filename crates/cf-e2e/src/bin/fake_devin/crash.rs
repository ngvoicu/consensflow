//! A turn that ends its window's program under it, as a harness does that crashes in
//! the middle of its task (the field report of 2026-10-15, B2: a Devin worker's
//! window on Windows closed mid-task, and ConsensFlow said only that it had). A line
//! `EXIT <code> <words>` is a turn that works as any does, then says the words on the
//! window's screen, its last, and ends the program with the code: nothing of the
//! turn's end is written, no answer in the store and no end of the turn on the wire.

use std::sync::OnceLock;

use cf_e2e::pattern;
use regex::Regex;

/// How a turn ends its window's program.
#[derive(Debug, PartialEq, Eq)]
pub struct Crash {
    /// What the program ends with.
    pub code: i32,
    /// The last words it says on its screen.
    pub words: String,
}

/// The crash the words of a turn ask for, if they do.
pub fn of(words: &str) -> Option<Crash> {
    static EXIT: OnceLock<Regex> = OnceLock::new();
    let asked = pattern::once(&EXIT, r"(?m)^EXIT (\d+) (.+)$").captures(words)?;
    Some(Crash {
        code: asked[1].parse().ok()?,
        words: asked[2].to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exit_line_ends_the_program_with_its_code_after_its_words_and_no_other_line_does() {
        assert_eq!(
            of("EXIT 3 fatal: the agent's runtime went away"),
            Some(Crash {
                code: 3,
                words: "fatal: the agent's runtime went away".to_owned()
            })
        );
        assert_eq!(of("say EXIT 3 x"), None);
        assert_eq!(of("EXIT soon x"), None);
        assert_eq!(of("Reply with exactly: EXIT"), None);
    }
}
