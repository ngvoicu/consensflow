//! Whether a harness that keeps a transcript (Claude Code, Codex) can go on with a
//! conversation of its own: where the transcript is, it can.
//!
//! Such a harness keeps one only once something was said in the conversation:
//! Claude Code writes none before the first message, and Codex writes a thread's
//! rollout at its first turn, so a window resumed on a conversation with no turn
//! finds nothing and ends at once (Codex's did, and a chief's own exit closed its
//! project: 2026-10-13). The file is there as soon as the harness has saved a turn,
//! and ConsensFlow's copy of the conversation is made a look at a time, so the
//! file, and not the copy, says whether there is anything to go on with. Only
//! where it is not there does the harness say no.

use cf_proto::agents::Harness;

use crate::contract::{Records, Work};

/// Whether `harness` can open a window on `session` again: yes where it kept a
/// transcript of it, no where it kept none, and none where that cannot be looked
/// for (a home the places are under, needed and missing).
pub(crate) fn transcript_kept<'a>(
    records: &'a dyn Records,
    harness: Harness,
    session: &'a str,
) -> Work<'a, Option<bool>> {
    Box::pin(async move { records.has_transcript(harness, session).await.ok() })
}
