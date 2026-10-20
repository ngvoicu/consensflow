//! What a window does wrong, once, as Devin has: each is told by a variable that
//! names a word, and meets the first message that holds it, and none after it.
//!
//! - `FAKE_DEVIN_DROP`: the window's TUI never takes the paste. It goes to no agent,
//!   and neither the store nor the wire says anything of it, as Devin dropped what was
//!   typed to a window resumed on a session it had no row of (a live run of
//!   3000.11.3, 2026-10-14).
//! - `FAKE_DEVIN_CRASH`: the window ends at once as its agent takes the message, its
//!   row in the store and its prompt on the wire, before the agent says a word, as a
//!   window ends in the middle of its task (B2 of the field reports of 2026-10-15,
//!   windows that closed mid-task on the owner's Windows machine). Only a window opened
//!   afresh does so: the one resumed on its session after it goes on.
//!
//! Nothing in a person's environment sets the variables, and a window with none does
//! nothing wrong.

pub const DROP_VARIABLE: &str = "FAKE_DEVIN_DROP";
pub const CRASH_VARIABLE: &str = "FAKE_DEVIN_CRASH";

/// The word the fault meets a message by, until it has met one.
#[derive(Debug, Default)]
pub struct Fault {
    word: Option<String>,
}

impl Fault {
    /// What a variable set to `variable` asks for: an empty word is none.
    pub fn of(variable: Option<&str>) -> Self {
        Self {
            word: variable.filter(|word| !word.is_empty()).map(str::to_owned),
        }
    }

    /// Whether the message `text` meets the fault: the first that holds the word
    /// does, and none after it.
    pub fn meets(&mut self, text: &str) -> bool {
        let met = self.word.as_deref().is_some_and(|word| text.contains(word));
        if met {
            self.word = None;
        }
        met
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_message_that_holds_the_word_meets_it_and_nothing_after_it() {
        let mut fault = Fault::of(Some("RESULT_ONE"));
        assert!(!fault.meets("[ConsensFlow m-4 · T-2 · result from @w]\nRESULT_TWO"));
        assert!(fault.meets("[ConsensFlow m-3 · T-1 · result from @w]\nRESULT_ONE"));
        assert!(!fault.meets("[ConsensFlow m-3 · T-1 · result from @w]\nRESULT_ONE"));
    }

    #[test]
    fn a_window_told_no_word_does_nothing_wrong() {
        for told in [None, Some("")] {
            let mut fault = Fault::of(told);
            assert!(!fault.meets("anything"), "{told:?}");
            assert!(!fault.meets(""), "{told:?}");
        }
    }
}
