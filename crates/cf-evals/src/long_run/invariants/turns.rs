//! A turn's word against the record (5, A5): ConsensFlow tells a window its turn
//! was cut when the app quit, or the project was closed, in the middle of it; a
//! window whose record ended at rest was cut in nothing. Seen on 3.0.4 twice: a
//! Claude chief at rest, an idle compaction (a boundary and a summary that reads as
//! the user's), and at the Resume the note "ConsensFlow stopped in the middle of
//! your turn, which was cut", which the chief went to work on. Claude Code's record
//! is read whole here, the compaction no turn, as far as it was read through the
//! note's moment (what was read before a later reading failed stays read); where it
//! was not, the note is unresolved, never judged on the copy. Another harness's
//! record is read from the daemon's copy of it.

use super::order::message_row;
use super::work::lines_of;
use super::{Break, Evidence, Gap, Unjudged};
use crate::clock;
use crate::long_run::look::Look;
use crate::long_run::mask::Names;
use crate::long_run::record::Stood;

/// What a cut note says: the chief's, and a member's.
pub const CUT: [&str; 2] = [
    "stopped in the middle of your turn, which was cut",
    "ConsensFlow quit while you were working on",
];

/// 5: a cut note to a window whose record was at rest when it was written.
pub fn false_cut(look: &Look) -> Vec<Break> {
    cut_notes(look).0
}

/// 5, unresolved: a cut note to a Claude Code window whose own record was not read
/// through the moment the note was written. The copy of its conversation cannot tell an
/// idle compaction (whose summary reads as the user's) from a turn, so it is not read
/// instead.
pub fn false_cut_unread(look: &Look) -> Vec<Unjudged> {
    cut_notes(look).1
}

/// The breaks of the fifth, and the cut notes it could not judge.
fn cut_notes(look: &Look) -> (Vec<Break>, Vec<Unjudged>) {
    let names = Names::of(look);
    let (mut found, mut unread) = (Vec::new(), Vec::new());
    for note in look.ledger.messages.iter().filter(|message| {
        message.sender.is_none()
            && message.kind == "note"
            && CUT.iter().any(|said| message.body.contains(said))
    }) {
        let (Some(window), Some(at)) = (
            look.ledger.participant(note.recipient),
            clock::millis(&note.created_at),
        ) else {
            continue;
        };
        let record = if Look::is_claude(window) {
            let read = look
                .conversation_at(window.id, at)
                .ok_or_else(|| "its record was never read".to_owned())
                .and_then(|conversation| look.record_through(conversation, at));
            match read {
                Ok(record) => Some(record),
                Err(why) => {
                    unread.push(
                        Unjudged::new(
                            5,
                            format!("5:m-{}", note.id),
                            format!(
                                "@{} was told its turn was cut (m-{}), and its own record was not read through then ({why}): whether a turn was cut cannot be judged",
                                window.handle, note.id
                            ),
                        )
                        .gap(Gap::Again),
                    );
                    continue;
                }
            }
        } else {
            None
        };
        let (stood, lines) = match record.and_then(|record| record.stood_at(at)) {
            Some((stood, _)) => (stood, record.map(|record| record.lines_by(at, 5))),
            None => {
                let items = look.items_by(window.id, at);
                let Some(last) = items.last() else {
                    continue;
                };
                let stood = if last.role == "assistant" && last.complete {
                    Stood::AtRest
                } else {
                    Stood::InFlight
                };
                (stood, Some(lines_of(&items, 5, &names)))
            }
        };
        if stood != Stood::AtRest {
            continue;
        }
        found.push(
            Break::new(
                5,
                format!("5:m-{}", note.id),
                format!(
                    "@{} was told its turn was cut (m-{}), and its record was at rest",
                    window.handle, note.id
                ),
            )
            .with(Evidence {
                rows: vec![message_row(note, &names)],
                screen: Vec::new(),
                record: lines.unwrap_or_default(),
            }),
        );
    }
    (found, unread)
}
