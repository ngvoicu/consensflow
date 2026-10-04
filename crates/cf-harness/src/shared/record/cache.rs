//! Readers kept from look to look (`cachedAnswers`, `hosts/lib/completion.js`),
//! for a caller that looks at the same conversations every second: the
//! delivery watcher, over a chief's transcript of 135 MB. Each conversation
//! keeps its reader, so a look reads only what its harness wrote since the
//! last one, and a record that did not change answers the reading the last
//! look did, the same one. A conversation nobody has looked at for a while
//! (its window closed) is forgotten, so a daemon that runs for weeks keeps
//! only what it still reads.
//!
//! Looks at one conversation take turns: [`Cache::look`] takes the cache
//! whole, so one cannot start before the one before it is done.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;

use cf_base::env::Env;

use super::reading::Reading;

/// A conversation's record, read on from look to look.
pub trait Look {
    /// What the record says now. Never a failure: a record that cannot be
    /// read is an unknown reading.
    fn look(&mut self) -> Arc<Reading>;
}

/// How a cache opens the reader of a conversation, by its harness's kind,
/// its session and the environment that says where the harness keeps it:
/// the reader, or the reading of a conversation no reader reads (no
/// session, a kind no harness is), which is never kept.
pub type Open = Box<dyn Fn(&str, &str, &Env) -> Result<Box<dyn Look + Send>, Reading> + Send>;

/// How long a conversation nobody looks at keeps its reader: ten minutes.
pub const IDLE_MS: i64 = 10 * 60_000;

/// The readers of the conversations looked at lately.
pub struct Cache {
    open: Open,
    idle_ms: i64,
    /// When the forgotten were last swept out.
    swept: i64,
    known: HashMap<(String, String), Kept>,
}

/// A reader, and when it was last looked through.
struct Kept {
    reader: Box<dyn Look + Send>,
    read_at: i64,
}

impl Cache {
    /// A cache that opens readers with `open` and forgets one unread for
    /// `idle_ms`, made at `now_ms`.
    pub fn new(open: Open, idle_ms: i64, now_ms: i64) -> Self {
        Self {
            open,
            idle_ms,
            swept: now_ms,
            known: HashMap::new(),
        }
    }

    /// A look at `session` of the harness `kind`, at `now_ms`. The
    /// environment opens its reader on the first look; a later look's is
    /// not read. Every `idle_ms`, the conversations unread that long are
    /// forgotten first, this one too: it is then read anew.
    pub fn look(&mut self, kind: &str, session: &str, env: &Env, now_ms: i64) -> Arc<Reading> {
        if now_ms - self.swept >= self.idle_ms {
            self.swept = now_ms;
            let idle_ms = self.idle_ms;
            self.known.retain(|_, kept| now_ms - kept.read_at < idle_ms);
        }
        let kept = match self.known.entry((kind.to_owned(), session.to_owned())) {
            Entry::Occupied(kept) => kept.into_mut(),
            Entry::Vacant(place) => match (self.open)(kind, session, env) {
                Ok(reader) => place.insert(Kept {
                    reader,
                    read_at: now_ms,
                }),
                Err(reading) => return Arc::new(reading),
            },
        };
        kept.read_at = now_ms;
        kept.reader.look()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A reader that says which reader it is and how many looks it took.
    struct Counting {
        reader: usize,
        looks: usize,
    }

    impl Look for Counting {
        fn look(&mut self) -> Arc<Reading> {
            self.looks += 1;
            Arc::new(Reading::Unknown(format!(
                "reader {} look {}",
                self.reader, self.looks
            )))
        }
    }

    /// A cache over counting readers, `none` a kind no reader reads.
    fn cache(idle_ms: i64) -> Cache {
        let opened = AtomicUsize::new(0);
        Cache::new(
            Box::new(move |kind, _, _| {
                if kind == "none" {
                    return Err(Reading::Unknown(format!("unknown kind: {kind}")));
                }
                let reader = opened.fetch_add(1, Ordering::Relaxed) + 1;
                Ok(Box::new(Counting { reader, looks: 0 }))
            }),
            idle_ms,
            0,
        )
    }

    fn said(reading: &Reading) -> &str {
        match reading {
            Reading::Unknown(reason) => reason,
            Reading::Known(_) => "known",
        }
    }

    #[test]
    fn each_conversation_keeps_its_reader() {
        let env = Env::default();
        let mut cache = cache(IDLE_MS);
        let mut look = |kind, session, now| said(&cache.look(kind, session, &env, now)).to_owned();
        assert_eq!(look("codex", "a", 0), "reader 1 look 1");
        assert_eq!(look("codex", "b", 1), "reader 2 look 1");
        assert_eq!(look("pi", "a", 2), "reader 3 look 1");
        assert_eq!(look("codex", "a", 3), "reader 1 look 2");
    }

    #[test]
    fn a_conversation_nobody_reads_any_more_is_forgotten_then_read_anew() {
        // completion.test.mjs: "a conversation nobody reads any more is forgotten".
        let env = Env::default();
        let mut cache = cache(1000);
        let mut look = |session, now| said(&cache.look("codex", session, &env, now)).to_owned();
        assert_eq!(look("a", 0), "reader 1 look 1");
        assert_eq!(look("a", 999), "reader 1 look 2", "read again soon: kept");
        assert_eq!(
            look("b", 1500),
            "reader 2 look 1",
            "a sweep, a is not idle long enough"
        );
        assert_eq!(look("a", 1998), "reader 1 look 3");
        // The next sweep is due at 2500: b, unread since 1500, goes with it.
        assert_eq!(look("a", 2600), "reader 1 look 4");
        assert_eq!(
            look("b", 2700),
            "reader 3 look 1",
            "forgotten, then read anew"
        );
        // An entry asked for after its idle time is swept out by the same look.
        assert_eq!(look("a", 4000), "reader 4 look 1");
    }

    #[test]
    fn a_conversation_no_reader_reads_is_answered_afresh_and_never_kept() {
        let env = Env::default();
        let mut cache = cache(IDLE_MS);
        let first = cache.look("none", "a", &env, 0);
        let second = cache.look("none", "a", &env, 1);
        assert_eq!(said(&first), "unknown kind: none");
        assert_eq!(first, second);
        assert!(!Arc::ptr_eq(&first, &second), "another reading each time");
        assert!(cache.known.is_empty());
    }

    #[test]
    fn a_cache_can_move_to_another_thread() {
        fn sendable<T: Send>() {}
        sendable::<Cache>();
    }
}
