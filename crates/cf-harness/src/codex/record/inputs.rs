//! The input of a turn, which Codex writes twice: as the model's own message
//! (`response_item`, a user's message with an id like `msg_…`) and as the item
//! Codex completes itself (`item_completed`, a `UserMessage`, under an id of its
//! own). Nothing in either names the other; what they share is the turn and the
//! words. Two assistant records that tell one message share an id, and so are
//! one item (`add_item`); these two are told apart from two inputs that happen
//! to say the same words (a person's "continue", twice) by being counted: each
//! telling is the twin of one telling of the other kind, not of two.
//!
//! The first telling is the item, with its id: the ledger keeps an item by its
//! id, and an id that changed when the second record came would be two rows.
//!
//! What stays told twice is what no twin is found for, and no message is lost
//! to the rule: of the 2,010 `UserMessage` items in 709 rollouts on the owner's
//! machine (September and October 2026), 1,987 have a twin by turn and words.
//! The 23 that have none say other words than the model's message does: an
//! input with an attached image (the model's message wraps the words in
//! `<image …>` tags) and the prompt Codex gives its approval reviewer ("The
//! following is the Codex agent history whose request action you are
//! assessing"). Those stay as they were, told twice.

use std::sync::Arc;

use serde_json::Value;

use super::Rollout;
use crate::shared::record::jsonl::Stop;
use crate::shared::record::key::Key;
use crate::shared::record::reading::{native_id, Role};

/// Which record told an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Telling {
    /// The model's own message: a `response_item`.
    Model,
    /// The item Codex completed: an `item_completed` `UserMessage`.
    Codex,
}

/// An input told once, in a turn, that the other telling has not repeated.
struct Told {
    telling: Telling,
    words: Arc<str>,
    /// Its item, among the rollout's.
    place: usize,
}

/// The inputs of a turn that were told once so far. A message the model was
/// given that no person wrote (the environment Codex runs in, instructions) is
/// told once for good, and stays here, beside the rest.
#[derive(Default)]
pub(super) struct Inputs(Vec<Told>);

impl Inputs {
    /// The item that the input `words`, told by `telling`, repeats: that of the
    /// first input of the other telling with the same words and no twin yet, which
    /// is then no longer waiting for one. None where the input is the first
    /// of its words, as a message with no twin always is.
    pub(super) fn twin_of(&mut self, telling: Telling, words: &str) -> Option<usize> {
        let at = self
            .0
            .iter()
            .position(|told| told.telling != telling && *told.words == *words)?;
        Some(self.0.remove(at).place)
    }

    /// The input `words`, told by `telling`, is the item at `place`, and waits
    /// for its twin.
    pub(super) fn told(&mut self, telling: Telling, words: Arc<str>, place: usize) {
        self.0.push(Told {
            telling,
            words,
            place,
        });
    }
}

impl Rollout {
    /// A user's input as `telling` told it: an item, unless it is the twin of
    /// the input the other telling made an item of, which the twin's id then
    /// names too. A record with no id fails the look, as any item's does; one
    /// that names no turn has no twin.
    pub(super) fn add_input(
        &mut self,
        id: Option<&Value>,
        telling: Telling,
        turn: &Key,
        text: String,
        at: Value,
        seq: &Value,
    ) -> Result<(), Stop> {
        let id = native_id(id, "codex item", seq).map_err(Stop::Failed)?;
        if let Some(place) = self
            .turn(turn)
            .and_then(|open| open.inputs.twin_of(telling, &text))
        {
            self.places.entry(id).or_insert(place);
            return Ok(());
        }
        let words: Arc<str> = Arc::from(text.as_str());
        let place = self.add_named(id, Role::User, text, true, at);
        if let Some(open) = self.turn(turn) {
            open.inputs.told(telling, words, place);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_input_is_the_twin_of_one_of_the_other_telling_and_each_has_one() {
        let mut inputs = Inputs::default();
        inputs.told(Telling::Model, Arc::from("go"), 3);
        inputs.told(Telling::Model, Arc::from("go"), 5);
        inputs.told(Telling::Codex, Arc::from("stop"), 6);
        // Not the same telling, not other words.
        assert_eq!(inputs.twin_of(Telling::Model, "go"), None);
        assert_eq!(inputs.twin_of(Telling::Codex, "halt"), None);
        // The first waiting, once each.
        assert_eq!(inputs.twin_of(Telling::Codex, "go"), Some(3));
        assert_eq!(inputs.twin_of(Telling::Codex, "go"), Some(5));
        assert_eq!(inputs.twin_of(Telling::Codex, "go"), None);
        assert_eq!(inputs.twin_of(Telling::Model, "stop"), Some(6));
    }
}
