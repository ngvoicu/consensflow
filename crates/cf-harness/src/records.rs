//! What a harness's own record of a conversation says
//! (`hosts/lib/completion.js`): read from its native store, never written,
//! read on from where the last look stopped. A reading is what a reading of
//! the whole record would say: a record that shrank or was replaced is read
//! again from its start. One rewritten in place before its last kilobyte,
//! that kilobyte and its unterminated last line kept, is not seen to change,
//! as Node did not see it either. A final unterminated JSONL append may be
//! incomplete; a malformed whole line fails the look.
//!
//! Each harness's reader is in its own module, and [`reader`] is the switch
//! between them. The readers kept from look to look are a [`Cache`], which
//! [`open`] opens them for; [`answers`] reads a record whole, once. The engine
//! reads through a [`Thread`], which owns the caches and reads off its own
//! thread.
//!
//! A reset a refusal names at a time of day but in no zone is read in the
//! zone the caller gives as `local`, the machine's own where Node read the
//! process's.

use std::sync::Arc;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use jiff::tz::TimeZone;

use crate::{claude, codex, devin, opencode, pi};

mod thread;

pub use crate::shared::quota::{Level, Quota};
pub use crate::shared::record::cache::{Cache, Look, Open, Options, PiSettlement, IDLE_MS};
pub use crate::shared::record::reading::{Item, Reading, Record, Role, Settlement};
pub use thread::Thread;

/// The reader of the conversation `session` in `harness`'s record, in the
/// places `env` names (`recordReader`): each look reads on from where the
/// last one stopped. A conversation with no session has none: its reading
/// is that the session is missing.
pub fn reader(
    harness: Harness,
    session: &str,
    env: &Env,
    local: &TimeZone,
) -> Result<Box<dyn Look + Send>, Reading> {
    if session.is_empty() {
        return Err(Reading::Unknown("missing session id".to_owned()));
    }
    Ok(match harness {
        Harness::Claude => claude::record::reader(session, env, local),
        Harness::Codex => codex::record::reader(session, env),
        Harness::Pi => pi::record::reader(session, env, local),
        Harness::Opencode => opencode::record::reader(session, env, local),
        Harness::Devin => devin::record::reader(session, env),
    })
}

/// The zone `Intl` takes `name` for, as the machine's own is named
/// (`Intl.DateTimeFormat().resolvedOptions().timeZone`), the `local` a reader
/// reads a reset that names no zone in: ICU's names and offsets as well as
/// the database's; none for a name `Intl` refuses.
pub fn zone(name: &str) -> Option<TimeZone> {
    crate::shared::quota::time_zone(name)
}

/// How a [`Cache`] opens its readers: with [`reader`].
pub fn open(local: TimeZone) -> Open {
    Box::new(move |harness, session, env| reader(harness, session, env, &local))
}

/// What `harness`'s own record of `session` says at `now_ms`, told
/// `options`, read whole (`answers`): the one look of a reader of its own.
pub fn answers(
    harness: Harness,
    session: &str,
    env: &Env,
    options: &Options,
    local: &TimeZone,
    now_ms: i64,
) -> Arc<Reading> {
    match reader(harness, session, env, local) {
        Ok(mut reader) => reader.look(options, now_ms),
        Err(reading) => Arc::new(reading),
    }
}

/// Whether `harness` has kept a transcript of `session` at all
/// (`hasTranscript`): a file of it where the harness keeps them. OpenCode
/// and Devin keep a store, not a transcript: never. A home the places are
/// under, needed and missing, is the failure.
pub fn has_transcript(harness: Harness, session: &str, env: &Env) -> Result<bool, String> {
    let found = match harness {
        Harness::Claude => claude::paths::transcript(session, env)?,
        Harness::Codex => codex::paths::transcript(session, env)?,
        Harness::Pi => pi::paths::transcript(session, env)?,
        Harness::Opencode | Harness::Devin => None,
    };
    Ok(found.is_some())
}

#[cfg(test)]
mod tests;
