//! What each harness's CLI is here and whether it is up to date: its version,
//! asked of the CLI; how it was installed, read from where it really lives
//! ([`release_source`]); the latest release, asked of a feed ([`feed`]); and
//! its update, run the way it was installed on request. Diagnostics never take
//! part in a launch, a binding, a reading or a delivery.
//!
//! The admin asks two things of the world, each through a seam of its own
//! that the daemon gives the system's and a test scripts: [`Latest`], the
//! latest release of a harness (Node's `latest` option), and [`Capture`],
//! the programs it runs to their end with both streams kept (Node's `run`
//! option, which defaults to the same `execFile` the version probe calls: one
//! seam serves both, since the daemon gives one `execFile` to both).
//!
//! Its answers are kept five minutes, one per harness, and an inspection
//! begun is shared by every call that asks while it runs. The verdict on a CLI
//! a launch checks (Codex, Devin, Muse Code) is the launch's own check, called at every
//! look and kept by nothing here: what keeps an answer is the launch's map of probes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_process::{CaptureFailed, Captured, Limits};
use cf_proto::agents::Harness;
use futures_util::future::{join_all, FutureExt, LocalBoxFuture, Shared};

use crate::contract::Work;
use crate::detect::{harness_path, known_harnesses};
use crate::seams::processes::{Probes, Processes, Program};
use crate::seams::Time;

pub mod feed;
mod inspect;
mod record;
mod row;
mod source;
mod update;
pub(crate) mod version;

pub use row::{Distribution, Ended, Extension, Outcome, Release, Row, Setup, Update, Version};
pub use source::{release_source, Format, Source};

/// How long a row is kept: five minutes, from when its look began.
const KEPT_MS: i64 = 300_000;

/// Where the admin learns the latest release of a harness (`latest`): the
/// feed `source` names, asked as [`feed`] says. The release as the feed
/// said it, or the words the page shows of why not.
pub trait Latest {
    fn latest<'a>(&'a self, id: Harness, source: &'a Source) -> Work<'a, Result<String, String>>;
}

/// Where the admin runs a program to its end (`execFile`): the version probe
/// and the update, each with the limits of its own. The program is started
/// as it starts here (`runnable`, the environment Windows needs): a seam
/// that is the system's does that, one that is scripted answers.
pub trait Capture {
    fn capture(
        &self,
        program: Program,
        limits: Limits,
    ) -> Work<'_, Result<Captured, CaptureFailed>>;

    /// [`capture`](Self::capture), saying the program's number once it has started. A seam
    /// that starts no program of the system's has none to say.
    fn capture_started(
        &self,
        program: Program,
        limits: Limits,
        started: Started,
    ) -> Work<'_, Result<Captured, CaptureFailed>> {
        drop(started);
        self.capture(program, limits)
    }
}

/// [`Capture::capture_started`]'s notice: the number of the program, once it has started.
pub type Started = Box<dyn FnOnce(u32)>;

/// An inspection begun, which every call asking for the same harness while
/// it runs shares.
type Pending = Shared<LocalBoxFuture<'static, Stamped>>;

/// A row with its stamp: the number its look took when it began, from one counter per harness.
/// A look begun later has a higher one, whatever the order the answers arrive in.
pub type Stamped = (u64, Rc<Row>);

/// What the page is told of the harnesses: their rows, each with its stamp, and the
/// harnesses whose update is running now, whichever page began it.
#[derive(Debug)]
pub struct Observed {
    pub rows: Vec<Stamped>,
    pub updating: Vec<Harness>,
}

/// How long a row may take: at this time it is answered with what it has.
const ROW_LIMIT: Duration = Duration::from_secs(40);

/// What the admin holds, shared with the inspections it begins.
struct Inner {
    env: Env,
    time: Rc<dyn Time>,
    latest: Rc<dyn Latest>,
    capture: Rc<dyn Capture>,
    /// What the launch's own checks ask through, the map of answers the launches keep.
    probes: Rc<Probes>,
    processes: Rc<dyn Processes>,
    /// The row each harness was last looked at for, with the number of the look that made it.
    cache: RefCell<HashMap<Harness, Stamped>>,
    pending: RefCell<HashMap<Harness, (u64, Pending)>>,
    /// The number of the last look begun of each harness, taken when the look begins and not
    /// when it is answered: the newest look's row is the one kept and the one the page shows.
    looks: RefCell<HashMap<Harness, u64>>,
    /// The harnesses whose update is running: a second update of one is refused.
    updating: RefCell<HashSet<Harness>>,
    /// How many updates of each harness have ended: a check that sees the number change
    /// while it collects its rows looks again, so that no row older than an update that has
    /// ended is the page's last word.
    ended: RefCell<HashMap<Harness, u64>>,
}

/// Looks at the harnesses' CLIs and updates them.
pub struct HarnessAdmin {
    inner: Rc<Inner>,
}

/// Takes an inspection off the pending ones when the call that began it is
/// done with it, or let go of: Node's `finally`.
struct Clearing<'a> {
    inner: &'a Inner,
    harness: Harness,
    look: u64,
}

impl Drop for Clearing<'_> {
    fn drop(&mut self) {
        // Only its own: a newer look may have taken the place.
        let mut pending = self.inner.pending.borrow_mut();
        if pending
            .get(&self.harness)
            .is_some_and(|(look, _)| *look == self.look)
        {
            pending.remove(&self.harness);
        }
    }
}

/// The folder a program is run in: the environment's `HOME`, as Node's
/// `cwd: env.HOME`.
fn home_folder(env: &Env) -> Option<PathBuf> {
    env.os("HOME").map(PathBuf::from)
}

/// What a program of the admin is run with: in the home, with the
/// environment the admin has, and nothing else.
fn program(env: &Env, executable: PathBuf, args: Vec<String>) -> Program {
    Program {
        executable,
        args,
        cwd: home_folder(env),
        env: env.clone(),
    }
}

/// The limits a version probe is held to: 3 s and 8 KiB.
const PROBE: Limits = Limits {
    timeout: Duration::from_secs(3),
    max_buffer: 8192,
};

impl HarnessAdmin {
    /// An admin that looks at the CLIs `env` finds, at the time `time` says,
    /// asking `latest` for releases and running programs through `capture`. The launches'
    /// own checks of a CLI are asked through `probes` and `processes`, the map and the
    /// programs the engine's launches use: an answer a launch has kept is the page's too.
    pub fn new(
        env: Env,
        time: Rc<dyn Time>,
        latest: Rc<dyn Latest>,
        capture: Rc<dyn Capture>,
        probes: Rc<Probes>,
        processes: Rc<dyn Processes>,
    ) -> Self {
        Self {
            inner: Rc::new(Inner {
                env,
                time,
                latest,
                capture,
                probes,
                processes,
                cache: RefCell::new(HashMap::new()),
                pending: RefCell::new(HashMap::new()),
                looks: RefCell::new(HashMap::new()),
                updating: RefCell::new(HashSet::new()),
                ended: RefCell::new(HashMap::new()),
            }),
        }
    }

    /// What is known of the harness named `id`, or of every harness in the
    /// order they are listed in when none is named: a row kept less than five
    /// minutes is answered as it was, unless `refresh` asks to look again,
    /// and the CLI's path is the same. Every harness is looked at at once.
    /// `Unknown harness` for a name that is none.
    pub async fn check(&self, id: Option<&str>, refresh: bool) -> Result<Vec<Rc<Row>>, String> {
        Ok(self
            .observe(id, refresh)
            .await?
            .rows
            .into_iter()
            .map(|(_, row)| row)
            .collect())
    }

    /// [`check`](Self::check), each row with its stamp and the updates that are running.
    pub async fn observe(&self, id: Option<&str>, refresh: bool) -> Result<Observed, String> {
        let harnesses = match id {
            None => known_harnesses().to_vec(),
            Some(name) => vec![Harness::from_name(name).ok_or("Unknown harness")?],
        };
        let ended = |harness: Harness| self.inner.ended.borrow().get(&harness).copied();
        let before: Vec<_> = harnesses.iter().map(|harness| ended(*harness)).collect();
        let looks = harnesses.iter().map(|harness| self.look(*harness, refresh));
        let mut rows = join_all(looks).await;
        // A row collected before an update ended is older than the update: it is not the
        // answer. The look is made again, which finds the row the update left, and again
        // if another update ended meanwhile; the running list is read last, with nothing
        // awaited between it and the last look.
        let mut seen = before;
        loop {
            let moved: Vec<usize> = (0..harnesses.len())
                .filter(|at| ended(harnesses[*at]) != seen[*at])
                .collect();
            if moved.is_empty() {
                break;
            }
            for at in &moved {
                seen[*at] = ended(harnesses[*at]);
            }
            let again = join_all(moved.iter().map(|at| self.look(harnesses[*at], false))).await;
            for (at, row) in moved.into_iter().zip(again) {
                rows[at] = row;
            }
        }
        Ok(Observed {
            rows,
            updating: self.updating(),
        })
    }

    /// The harnesses whose update is running, in the order they are listed in.
    pub fn updating(&self) -> Vec<Harness> {
        let running = self.inner.updating.borrow();
        known_harnesses()
            .iter()
            .copied()
            .filter(|harness| {
                running.contains(harness) || record::running(&self.inner.env, *harness)
            })
            .collect()
    }

    /// One harness's row: the one kept, the inspection that is running, or an
    /// inspection begun now. A harness whose launch checks its CLI has its verdict
    /// asked at every look, whatever is kept of the rest; a refresh forgets what the
    /// launches kept of the CLI and does not join a look in flight.
    async fn look(&self, harness: Harness, refresh: bool) -> Stamped {
        let inner = &self.inner;
        let path =
            harness_path(harness, &inner.env).map(|found| found.to_string_lossy().into_owned());
        let now = inner.time.wall_ms();
        let kept = inner
            .cache
            .borrow()
            .get(&harness)
            .map(|(stamp, row)| (*stamp, Rc::clone(row)))
            .filter(|(_, row)| row.path == path && now - row.checked_at < KEPT_MS);
        let checked = inspect::checks(harness);
        if !refresh && !checked {
            if let Some(row) = kept {
                return row;
            }
        }
        if !refresh {
            let running = inner
                .pending
                .borrow()
                .get(&harness)
                .map(|(_, run)| run.clone());
            if let Some(running) = running {
                return running.await;
            }
        }
        if refresh && checked {
            if let Some(path) = &path {
                inner.probes.forget(std::path::Path::new(path));
            }
        }
        let look = {
            let mut looks = inner.looks.borrow_mut();
            let next = looks.entry(harness).or_insert(0);
            *next += 1;
            *next
        };
        let begun = inspection(
            Rc::clone(inner),
            harness,
            path,
            now,
            look,
            kept.map(|(_, row)| row).filter(|_| checked && !refresh),
        )
        .boxed_local()
        .shared();
        inner
            .pending
            .borrow_mut()
            .insert(harness, (look, begun.clone()));
        let _clearing = Clearing {
            inner,
            harness,
            look,
        };
        begun.await
    }
}

/// An inspection, whose row is kept when it is done, before any call that
/// waited for it goes on, unless a newer look has been made since it began. The row of
/// any installed harness is cut at [`ROW_LIMIT`], so the page is answered by then whichever
/// row hangs. What the look asked and did not get is let go of as it is cut (the probes
/// retire themselves), and what was answered since stays.
async fn inspection(
    inner: Rc<Inner>,
    harness: Harness,
    path: Option<String>,
    checked_at: i64,
    look: u64,
    kept: Option<Rc<Row>>,
) -> Stamped {
    let work = inspect::inspect(&inner, harness, path.clone(), checked_at, kept.as_deref());
    let kept = kept.is_some();
    let mut cut = false;
    let row = if path.is_some() {
        let mut work = Box::pin(work);
        let mut timer = inner.time.sleep(ROW_LIMIT);
        match futures_util::future::select(work.as_mut(), timer.as_mut()).await {
            futures_util::future::Either::Left((row, _)) => row,
            futures_util::future::Either::Right(_) => {
                cut = true;
                inspect::unfinished(harness, path, checked_at)
            }
        }
    } else {
        work.await
    };
    let row = Rc::new(row);
    let stamped = (look, Rc::clone(&row));
    let mut cache = inner.cache.borrow_mut();
    let stale = cache.get(&harness).is_some_and(|(held, _)| *held > look);
    // A row cut short has no feed's answer, and one made over a kept feed is no new
    // look at it: neither replaces what is kept.
    if !stale && !cut && !kept {
        cache.insert(harness, (look, Rc::clone(&row)));
    }
    stamped
}

#[cfg(test)]
mod tests;
