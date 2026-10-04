//! OpenCode's record of a session (`opencodeReader`,
//! `hosts/lib/completion/opencode.js`): its SQLite store, read on from the
//! session's last event.
//!
//! OpenCode writes each change to a message or a part as an event of the
//! conversation, numbered in order, in the same transaction as the row (a
//! 1.1 GB store of 1.18.33 and 1.18.34, checked on 2026-10-03: every message
//! and part row is its latest event's data). So a look reads the events
//! after the last one it saw, and again only the rows they name. The
//! conversation's message, part or event count disagreeing with what was
//! read (a row removed, an event written out of order), or an event of a
//! type not followed here, has the store read whole. A look whose rows and
//! events changed nothing answers what the last did, the same reading; one
//! that fails forgets all it read.
//!
//! A column is held as `node:sqlite` hands it over (`Cell`), and made what
//! OpenCode's JavaScript made of it where it made it.
//!
//! Kept from Node on purpose: an item whose row's id is no text fails the
//! look (`answer`); JSON past serde's limits in a row's data fails the look;
//! and of rows sorted by a comparator that is no order, or one that fails on
//! more than one of them, the order or the row named may be another
//! (`shared::record::sort`).

mod answer;
mod read;
mod stores;

use std::sync::Arc;

use cf_base::env::Env;
use jiff::tz::TimeZone;

use crate::shared::record::cache::{Look, Options};
use crate::shared::record::key::Keys;
use crate::shared::record::reading::Reading;
use crate::shared::record::sqlite::Reads;
use read::{Onward, Read};
use stores::Which;

/// The reader of the session `session`, in the OpenCode store `env` names.
/// A reset a refusal names at a time of day, and in no zone, is read in
/// `local`.
pub fn reader(session: &str, env: &Env, local: &TimeZone) -> Box<dyn Look + Send> {
    Box::new(Reader {
        session: session.to_owned(),
        env: env.clone(),
        local: local.clone(),
        store: None,
        read: None,
        answer: None,
        keys: Keys::default(),
        #[cfg(test)]
        between: None,
    })
}

/// A session's reader: what its store said so far.
struct Reader {
    session: String,
    env: Env,
    local: TimeZone,
    /// The store the last look read.
    store: Option<Which>,
    /// What the looks read of it: none to read it whole.
    read: Option<Read>,
    /// The last look's answer, while nothing it rests on changes.
    answer: Option<Arc<Reading>>,
    /// What makes the keys of what the store says, from look to look.
    keys: Keys,
    /// What a test does between a whole read's messages and its parts, as
    /// the oracle's `betweenOpenCodeSnapshotReads` did.
    #[cfg(test)]
    between: Option<Box<dyn FnMut() + Send>>,
}

impl Look for Reader {
    /// Takes no options and reads no clock: the store alone answers.
    fn look(&mut self, _options: &Options, _now_ms: i64) -> Arc<Reading> {
        self.read_store().unwrap_or_else(|reason| {
            self.read = None;
            self.answer = None;
            Arc::new(Reading::unreadable(&reason))
        })
    }
}

impl Reader {
    /// What the session says now, or why it cannot be read.
    fn read_store(&mut self) -> Result<Arc<Reading>, String> {
        let Some((store, which)) = stores::open(&self.env, &self.session)? else {
            return Ok(Arc::new(Reading::unreadable(&format!(
                "no opencode store for {}",
                self.session
            ))));
        };
        if self.store.as_ref() != Some(&which) {
            self.store = Some(which);
            self.read = None;
        }
        store.read(|reads| self.look_in(reads))
    }

    /// A look in one transaction of the store.
    fn look_in(&mut self, reads: &Reads<'_>) -> Result<Arc<Reading>, String> {
        let session = self.session.clone();
        if reads
            .get("select 1 from session where id = ?", [&session])?
            .is_none()
        {
            self.read = None;
            return Ok(Arc::new(Reading::unreadable(&format!(
                "no opencode session {session}"
            ))));
        }
        // Taken while it is read on: a look that fails leaves none.
        let (read, changed) = match self.read.take() {
            None => (self.read_whole(reads, &session)?, true),
            Some(mut read) => match read.onward(reads, &session, &mut self.keys)? {
                Onward::Whole => (self.read_whole(reads, &session)?, true),
                Onward::Read { changed } => (read, changed),
            },
        };
        let read = self.read.insert(read);
        if let (false, Some(answer)) = (changed, &self.answer) {
            return Ok(Arc::clone(answer));
        }
        let answer = Arc::new(Reading::Known(answer::answer(
            read,
            &self.local,
            &mut self.keys,
        )?));
        self.answer = Some(Arc::clone(&answer));
        Ok(answer)
    }

    /// `readWhole`: the session's messages, its parts, then its events.
    fn read_whole(&mut self, reads: &Reads<'_>, session: &str) -> Result<Read, String> {
        let mut read = Read::messages(reads, session, &mut self.keys)?;
        #[cfg(test)]
        if let Some(between) = self.between.as_mut() {
            between();
        }
        read.parts_and_events(reads, session, &mut self.keys)?;
        Ok(read)
    }
}

#[cfg(test)]
mod tests;
