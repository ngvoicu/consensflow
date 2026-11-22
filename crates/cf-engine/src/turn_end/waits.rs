//! Whether a turn's final answer says its writer ends the turn to wait for its own work
//! (T-1923, from the advice T-1919).
//!
//! A turn's end is the task's result. A member that started work of its own (a run, a
//! build waiting its turn) and that its harness does not report as running sometimes ends its
//! turn on a line about waiting: "I'm ending this turn until it runs.", "I'll check them at the
//! next wakeup." Nothing is pending that the board can see, so the line was taken as the
//! result, twice on the owner's board on 2026-11-21 (T-1887's m-12797, T-1910's m-12911).
//! Nothing wakes such a member when its work ends: a wake the harness armed (Claude Code's
//! scheduled wakeup, Muse's cron, agy's schedule) is not counted, and the window is closed with
//! the result.
//!
//! Now [`Dispatcher::collect`](crate::dispatcher::Dispatcher) takes no result from such an
//! end: it types the member one note, to go on and wait inside its turn, and the requester is
//! told nothing. The member's next turn ends with the result. A second such end, after the
//! same delivery of the task, is the result as it was ([`go_on_told`]).
//!
//! **What tells one is the last paragraph of the member's own words**, read as
//! [`asks`](super::asks) reads it (what is not the member's voice taken out), and only a clause
//! in the first person and in English that says one of:
//! - it ends its turn to wait ("I'm ending this turn until it runs", "ending my turn to
//!   wait");
//! - it goes on later, when or once its work ends ("I'll check them at the next wakeup", "I'll
//!   resume when it reports", "once the run ends, I'll continue").
//!
//! A negation in the clause takes it back ("I won't resume when it reports"). A wait for the
//! chief's answer is [`asks_the_chief`](super::asks::asks_the_chief)'s, and is read first.
//! The rule is on English words, like that one: a member that says it in other words has its
//! words taken as the result, as before. Over the 3,411 results of members in a ledger, the
//! reading matched 6, which the requester had sent back every time, and no accepted result.

use std::sync::LazyLock;

use cf_ledger::{ParticipantView, TaskThread};
use cf_proto::ledger::MessageView;
use regex::Regex;

use super::asks::{last_paragraph, negated, plain, without_blocks};

/// What begins every go-on note: how [`go_on_told`] finds the one already sent.
pub(crate) const GO_ON_NOTE_BEGINS: &str =
    "ConsensFlow: your last turn ended to wait for your own work";

/// What the member's own work does that it goes on for.
const GO: &str = "(?:check|continue|resume|pick (?:it |this |them )?(?:back )?up|go on|carry on|come back|report|finish|return)";

/// The first person ends its turn to wait.
#[allow(clippy::expect_used)]
static ENDS_TO_WAIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:i'm|i am|i'll|i will) (?:now |still )?(?:end|ending) (?:this|my|the) turn(?: here)? (?:until|while|to wait|and (?:will )?wait)\b",
    )
    .expect("ends-to-wait pattern")
});

/// "I'll resume when it reports."
#[allow(clippy::expect_used)]
static GOES_ON_WHEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\bi(?:'ll| will) {GO}\b[^.!?\n]{{0,80}}\b(?:when|once|after|until|at the next|as soon as)\b"
    ))
    .expect("goes-on-when pattern")
});

/// "Once the run ends, I'll continue."
#[allow(clippy::expect_used)]
static WHEN_ENDS_GOES_ON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\b(?:when|once|after|as soon as) [^.!?\n]{{0,60}}\b(?:finish|end|complete|report|run|return|land)(?:e?s)?\b[^.!?\n]{{0,20}}, i(?:'ll| will) {GO}\b"
    ))
    .expect("when-ends-goes-on pattern")
});

/// Whether the last paragraph of a member's words says it ends its turn to wait for its own
/// work.
pub(crate) fn ends_to_wait(words: &str) -> bool {
    let paragraph = plain(&last_paragraph(&without_blocks(words)));
    [&*ENDS_TO_WAIT, &*GOES_ON_WHEN, &*WHEN_ENDS_GOES_ON]
        .into_iter()
        .any(|pattern| {
            pattern
                .find_iter(&paragraph)
                .any(|found| !negated(&paragraph[..found.start()]))
        })
}

/// The note that sends the member on: the first words are [`GO_ON_NOTE_BEGINS`].
pub(crate) fn go_on_note(number: i64) -> String {
    format!(
        "{GO_ON_NOTE_BEGINS}. T-{number}: the end of your turn is its result, and nothing wakes you when your own work ends, so those words were taken for no result. Go on now: wait for your work inside this turn (check it, again and again, until it ends), and end the turn with the result. If your last words were the result, answer with them."
    )
}

/// Whether `message` is the go-on note: ConsensFlow's own words to a member that begin so.
pub(crate) fn is_go_on_note(message: &MessageView) -> bool {
    message.sender.is_none() && message.body.starts_with(GO_ON_NOTE_BEGINS)
}

/// Whether the member was already sent the note since the latest message of the task's own
/// that went to it: once for each delivery of the task, whatever a restart did.
pub(crate) fn go_on_told(thread: &TaskThread, participant: &ParticipantView) -> bool {
    let delivery = thread
        .messages
        .iter()
        .filter(|message| message.kind == "task" && message.recipient_id == participant.id)
        .map(|message| message.id)
        .max()
        .unwrap_or(0);
    thread.messages.iter().any(|message| {
        message.id > delivery
            && message.kind == "note"
            && message.recipient_id == participant.id
            && is_go_on_note(message)
    })
}

#[cfg(test)]
mod tests;
