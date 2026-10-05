//! The records, read on a thread of their own (step 3.5). A first look at a
//! long transcript takes up to a second, which the engine's one thread must
//! not spend: [`Thread`] hands each look to a worker that owns the readers,
//! and the engine goes on with other work until the worker answers.
//!
//! - One owner of the readers. The worker holds five caches, one per harness,
//!   as each adapter kept its own `cachedAnswers` (`hosts/lib/completion.js`),
//!   each sweeping out the conversations nobody reads any more by a clock of
//!   its own.
//! - A look carries what it needs, owned: the harness, the session, its
//!   options, and the time of the engine's clock when it was asked. The worker
//!   hands the reader that time and reads no clock of its own. A look is
//!   asked when its future is first polled, as the engine begins work in place.
//! - Looks are read one at a time in the order they were asked, so the looks
//!   of one conversation read on from one another. A look is answered with
//!   the reading the cache returns, the same `Arc`: a refusal that has not
//!   changed is one `Arc<Quota>` look after look, which is how the engine
//!   tells an old refusal from a new one.
//! - A look that was asked is read, whatever becomes of the future that asked
//!   it: what a reader has read of its record and what it carries is not lost
//!   to a caller that gave up, and the next look goes on from it. A look still
//!   waiting for room in the queue is not asked yet.
//! - The queue is bounded: a look asked when it is full waits for room.
//! - A reader that panics fails its look, as any other record that cannot be
//!   read does, and not the worker (see [`reopening`]).
//!
//! Dropping the [`Thread`] closes the queue: the worker reads what was asked,
//! and ends.

use std::any::Any;
use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::Arc;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use jiff::tz::TimeZone;
use tokio::sync::{mpsc, oneshot};

use super::{has_transcript, open, Cache, Open, Options, Reading, IDLE_MS};
use crate::contract::{Records, Work};
use crate::seams::Time;
use reopening::Reopening;

mod reopening;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod support;
#[cfg(test)]
mod tests;

/// How many asks wait for the worker: more than the windows a daemon looks at
/// together.
const QUEUE: usize = 256;

/// What a worker says of a look it never answered.
const ENDED: &str = "the records thread ended";

/// What makes the openers of the readers: each conversation has an opener of
/// its own, which its [`Reopening`] keeps to open the conversation again.
type Opens = Arc<dyn Fn() -> Open + Send + Sync>;

/// The harnesses' records, read on a thread of their own, at the times of the
/// engine's clock. It belongs to the engine's thread, as the clock does.
pub struct Thread {
    time: Rc<dyn Time>,
    /// The worker's queue.
    asks: mpsc::Sender<Ask>,
}

/// What the engine asks of the worker, owned: nothing borrowed crosses.
enum Ask {
    Look {
        harness: Harness,
        session: String,
        options: Options,
        /// The engine's clock when the look was asked.
        now_ms: i64,
        answer: oneshot::Sender<Arc<Reading>>,
    },
    HasTranscript {
        harness: Harness,
        session: String,
        answer: oneshot::Sender<Result<bool, String>>,
    },
}

impl Thread {
    /// The records of the places `env` names, read on a thread of their own,
    /// at the times `time` says. A reset that names a time of day in no zone
    /// is read in `zone`, the machine's own. The caches begin when this is
    /// called, as the clock reads then.
    ///
    /// Fails where the system will not start the thread.
    pub fn new(env: Env, zone: TimeZone, time: Rc<dyn Time>) -> io::Result<Self> {
        Self::start(env, time, QUEUE, move || open(zone.clone()))
    }

    /// A thread whose queue holds `queue` asks, and whose readers `opens`
    /// opens.
    fn start(
        env: Env,
        time: Rc<dyn Time>,
        queue: usize,
        opens: impl Fn() -> Open + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let opens: Opens = Arc::new(opens);
        let caches = Caches::new(&opens, time.wall_ms());
        let (asks, waiting) = mpsc::channel(queue);
        std::thread::Builder::new()
            .name("records".to_owned())
            .spawn(move || serve(waiting, &env, caches))?;
        Ok(Self { time, asks })
    }

    /// What the worker answers to the ask that `ask` makes with the place to
    /// answer in: none if the worker ended before it did.
    async fn ask<T>(&self, ask: impl FnOnce(oneshot::Sender<T>) -> Ask) -> Option<T> {
        let (answer, answered) = oneshot::channel();
        self.asks.send(ask(answer)).await.ok()?;
        answered.await.ok()
    }
}

impl Records for Thread {
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        Box::pin(async move {
            let asked = self.ask(|answer| Ask::Look {
                harness,
                session: session.to_owned(),
                options: options.clone(),
                now_ms: self.time.wall_ms(),
                answer,
            });
            asked
                .await
                .unwrap_or_else(|| Arc::new(Reading::unreadable(ENDED)))
        })
    }

    fn has_transcript<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async move {
            let asked = self.ask(|answer| Ask::HasTranscript {
                harness,
                session: session.to_owned(),
                answer,
            });
            asked.await.unwrap_or_else(|| Err(ENDED.to_owned()))
        })
    }
}

/// The caches of the five harnesses, each with its own sweep.
struct Caches {
    claude: Cache,
    codex: Cache,
    pi: Cache,
    opencode: Cache,
    devin: Cache,
}

impl Caches {
    /// Caches that begin at `now_ms`, each opening its readers with `opens`.
    fn new(opens: &Opens, now_ms: i64) -> Self {
        let cache = || {
            let opens = Arc::clone(opens);
            Cache::new(
                Box::new(move |harness, session, env| {
                    Reopening::open(opens(), harness, session, env)
                }),
                IDLE_MS,
                now_ms,
            )
        };
        Self {
            claude: cache(),
            codex: cache(),
            pi: cache(),
            opencode: cache(),
            devin: cache(),
        }
    }

    fn of(&mut self, harness: Harness) -> &mut Cache {
        match harness {
            Harness::Claude => &mut self.claude,
            Harness::Codex => &mut self.codex,
            Harness::Pi => &mut self.pi,
            Harness::Opencode => &mut self.opencode,
            Harness::Devin => &mut self.devin,
        }
    }
}

/// The worker: takes each ask in the order it was made, and ends once the
/// queue is closed and every ask in it has been read.
fn serve(mut waiting: mpsc::Receiver<Ask>, env: &Env, mut caches: Caches) {
    // A caller that gave up has dropped its end of the answer: what it asked
    // was read all the same, and sending the answer to nobody is no failure.
    while let Some(ask) = waiting.blocking_recv() {
        match ask {
            Ask::Look {
                harness,
                session,
                options,
                now_ms,
                answer,
            } => {
                let reading = panics_to_text(|| {
                    caches
                        .of(harness)
                        .look(harness, &session, env, &options, now_ms)
                })
                .unwrap_or_else(|panicked| {
                    Arc::new(Reading::unreadable(&format!(
                        "the reader panicked: {panicked}"
                    )))
                });
                let _ = answer.send(reading);
            }
            Ask::HasTranscript {
                harness,
                session,
                answer,
            } => {
                let found = panics_to_text(|| has_transcript(harness, &session, env))
                    .unwrap_or_else(|panicked| {
                        Err(format!("the transcript check panicked: {panicked}"))
                    });
                let _ = answer.send(found);
            }
        }
    }
}

/// What `work` answers, or the message it panicked with.
fn panics_to_text<T>(work: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(work)).map_err(|payload| panic_message(&*payload))
}

/// The message of a panic: the text it was given, or none that can be said.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_owned())
}
