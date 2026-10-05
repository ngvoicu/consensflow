//! A reader a panic leaves nothing of. A look that fails ends a reader's state
//! and starts it over (`Followed::read`): the records it visited and the place
//! it stopped at go together. A look that panics ends it halfway, the records
//! it visited kept and the place it stopped at not, and the next look would
//! visit them twice. [`Reopening`] takes its reader out of itself for the
//! length of a look: a panic drops the reader with the look, and the next look
//! opens the conversation again and reads its record from the start.

use std::sync::Arc;

use cf_base::env::Env;
use cf_proto::agents::Harness;

use crate::records::{Look, Open, Options, Reading};

/// The reader of one conversation, and what it takes to open it again.
pub(super) struct Reopening {
    open: Open,
    harness: Harness,
    session: String,
    env: Env,
    /// None while a look reads, and after one panicked.
    reader: Option<Box<dyn Look + Send>>,
}

impl Reopening {
    /// The reader `open` opens for the conversation, which `open` opens again
    /// after a look panics; or, for a conversation `open` cannot read, its
    /// reading, which a cache never keeps.
    pub(super) fn open(
        open: Open,
        harness: Harness,
        session: &str,
        env: &Env,
    ) -> Result<Box<dyn Look + Send>, Reading> {
        let reader = open(harness, session, env)?;
        Ok(Box::new(Self {
            open,
            harness,
            session: session.to_owned(),
            env: env.clone(),
            reader: Some(reader),
        }))
    }
}

impl Look for Reopening {
    fn look(&mut self, options: &Options, now_ms: i64) -> Arc<Reading> {
        let mut reader = match self.reader.take() {
            Some(reader) => reader,
            None => match (self.open)(self.harness, &self.session, &self.env) {
                Ok(reader) => reader,
                Err(reading) => return Arc::new(reading),
            },
        };
        let reading = reader.look(options, now_ms);
        self.reader = Some(reader);
        reading
    }
}
