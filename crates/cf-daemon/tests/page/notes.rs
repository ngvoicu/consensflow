//! What did not answer as Node's did, noted where the stand-ins find it and
//! gathered by the player: an operation that is running must finish for its
//! reply to be compared, so what the dispatcher's stand-in finds wrong is not
//! panicked over there.

use std::cell::RefCell;

/// The problems noted since they were last gathered.
#[derive(Default)]
pub struct Notes(RefCell<Vec<String>>);

impl Notes {
    pub fn note(&self, why: impl Into<String>) {
        self.0.borrow_mut().push(why.into());
    }

    /// What was noted, gathered.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}
