//! The messages Claude Code's queue operations say are waiting, taken, or
//! taken back, by their content. A turn is not over while any is.

use std::collections::VecDeque;

use super::text::same_queued_content;

/// The contents of the messages in each state, the oldest first.
#[derive(Default)]
pub(super) struct Queues {
    /// Enqueued, not yet taken.
    queued: VecDeque<String>,
    /// Taken for a turn that has not begun in the record.
    dequeued: VecDeque<String>,
    /// Taken back by the user (`popAll`), to be sent anew.
    popped: VecDeque<String>,
}

impl Queues {
    /// How many messages wait in any state (`queuedTurns`).
    pub(super) fn turns(&self) -> usize {
        self.queued.len() + self.dequeued.len() + self.popped.len()
    }

    /// An `enqueue`.
    pub(super) fn enqueue(&mut self, content: String) {
        self.queued.push_back(content);
    }

    /// A `dequeue`: the oldest queued message is taken, or an empty one when
    /// the record never queued it.
    pub(super) fn dequeue(&mut self) {
        self.dequeued
            .push_back(self.queued.pop_front().unwrap_or_default());
    }

    /// A `popAll`: the message it names is taken back, out of the queue as
    /// the queue held it (an envelope with other attributes, perhaps) when it
    /// held it, else as the operation said it.
    pub(super) fn pop_all(&mut self, content: String) {
        let queued = position(&self.queued, &content).and_then(|at| self.queued.remove(at));
        self.popped.push_back(queued.unwrap_or(content));
    }

    /// A `remove`: the message it names, forgotten, queued or taken.
    pub(super) fn remove(&mut self, content: &str) {
        if let Some(at) = position(&self.queued, content) {
            self.queued.remove(at);
        }
        if let Some(at) = position(&self.dequeued, content) {
            self.dequeued.remove(at);
        }
    }

    /// An assistant's record: what was taken back is sent, or never will be.
    pub(super) fn answered(&mut self) {
        self.popped.clear();
    }

    /// The user's real prompt `text` begins a turn, and is out of the queues.
    /// The message taken back of its text is spent. The prompt is the oldest
    /// message taken when the record says it came from the queue, or one was
    /// taken; else it is the queued message of its own text.
    pub(super) fn consumed(&mut self, text: &str, from_queue: bool) {
        if let Some(at) = self.popped.iter().position(|entry| entry == text) {
            self.popped.remove(at);
        }
        if from_queue || !self.dequeued.is_empty() {
            self.dequeued.pop_front();
        } else if let Some(at) = self.queued.iter().position(|entry| entry == text) {
            self.queued.remove(at);
        }
    }
}

/// The place of the first message `removed` names (`sameQueuedContent`).
fn position(entries: &VecDeque<String>, removed: &str) -> Option<usize> {
    entries
        .iter()
        .position(|entry| same_queued_content(entry, removed))
}
