//! What a harness's own record of a conversation says: read from its native
//! store, never written, read on from where the last look stopped. A reading is
//! what a reading of the whole record would say: a record that shrank or was
//! replaced is read again from its start. One rewritten in place before its
//! last kilobyte, that kilobyte and its unterminated last line kept, is not
//! seen to change, as Node did not see it either. A final unterminated JSONL
//! append may be incomplete; a whole line that is not JSON is torn, and passed
//! over once a record follows it or its wait is over, where Node failed the
//! look (`shared::record::jsonl`).
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
pub use crate::shared::record::reading::{
    End, Item, Lines, Monitor, Notice, Pending, Reading, Record, Role, Settlement, Source,
};
pub use crate::shared::record::torn::Say;
pub use thread::Thread;

/// The reader of the conversation `session` in `harness`'s record, in the
/// places `env` names (`recordReader`): each look reads on from where the
/// last one stopped. A conversation with no session has none: its reading
/// is that the session is missing.
// Its error is only ever `Reading::Unknown` or `Reading::Missing`, a reason string, never a
// record (3.0.6-alpha.3's integration B, T-664; the openers are to return the reason itself).
#[allow(clippy::result_large_err)]
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

/// The machine's time zone as `Intl` names it, which a quota's reset named by
/// a time of day alone is read in. A zone the system has no name for stays the
/// system's own.
pub fn machine_zone() -> TimeZone {
    let system = TimeZone::system();
    system.iana_name().and_then(zone).unwrap_or(system)
}

/// How a [`Cache`] opens its readers: with [`reader`].
// Its error is only ever `Reading::Unknown` or `Reading::Missing`, a reason string, never a
// record (3.0.6-alpha.3's integration B, T-664; the openers are to return the reason itself).
#[allow(clippy::result_large_err)]
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
/// (`hasTranscript`): a file of it where the harness keeps them, Codex's
/// named as Codex names it. OpenCode and Devin keep a store, not a
/// transcript: never. A home the places are under, needed and missing, is the
/// failure.
pub fn has_transcript(harness: Harness, session: &str, env: &Env) -> Result<bool, String> {
    match harness {
        Harness::Claude => Ok(claude::paths::transcript(session, env)?.is_some()),
        Harness::Codex => codex::paths::rollout_kept(session, env),
        Harness::Pi => Ok(pi::paths::transcript(session, env)?.is_some()),
        Harness::Opencode | Harness::Devin => Ok(false),
    }
}

/// How many bytes `harness`'s transcript of `session` holds now, where a reader
/// of it reads a relaunch (Claude's alone): none where it keeps no transcript of
/// it. A home the places are under, needed and missing, is the failure.
pub fn transcript_length(
    harness: Harness,
    session: &str,
    env: &Env,
) -> Result<Option<u64>, String> {
    let file = match harness {
        Harness::Claude => claude::paths::transcript(session, env)?,
        Harness::Codex | Harness::Pi | Harness::Opencode | Harness::Devin => None,
    };
    file.map(|file| {
        std::fs::metadata(&file)
            .map(|found| found.len())
            .map_err(|failed| format!("{}: {failed}", file.display()))
    })
    .transpose()
}

/// Whether `harness`'s own store says, with no window open, that the window of
/// `session` took a paste that begins with `pasted`, handed over at `at_ms` or later:
/// none where it cannot tell. Devin's says, by its history row of what its TUI
/// submitted (`devin::history`); no other harness keeps such a word apart from its
/// record.
pub fn took(harness: Harness, session: &str, pasted: &str, at_ms: i64, env: &Env) -> Option<bool> {
    match harness {
        Harness::Devin => devin::took(env, session, pasted, at_ms),
        Harness::Claude | Harness::Codex | Harness::Pi | Harness::Opencode => None,
    }
}

#[cfg(test)]
mod tests;
