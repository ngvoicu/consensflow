//! What each harness's CLI is here and whether it is up to date
//! (`src/harness-admin.js`): its version, asked of the CLI; how it was
//! installed, read from where it really lives ([`release_source`]); the
//! latest release, asked of a feed ([`feed`]); and its update, run the way it
//! was installed on request. Diagnostics never take part in a launch, a
//! binding, a reading or a delivery.
//!
//! The admin asks two things of the world, each through a seam of its own
//! that the daemon gives the system's and a test scripts: [`Latest`], the
//! latest release of a harness (Node's `latest` option), and [`Capture`],
//! the programs it runs to their end with both streams kept (Node's `run`
//! option, which defaults to the same `execFile` the version probe calls: one
//! seam serves both, since the daemon gives one `execFile` to both).
//!
//! Its answers are kept five minutes, one per harness, and an inspection
//! begun is shared by every call that asks while it runs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_process::{CaptureFailed, Captured, Limits};
use cf_proto::agents::Harness;
use futures_util::future::{join_all, FutureExt, LocalBoxFuture, Shared};

use crate::contract::Work;
use crate::detect::{harness_path, known_harnesses};
use crate::seams::processes::Program;
use crate::seams::Time;

pub mod feed;
mod inspect;
mod row;
mod source;
mod update;
mod version;

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
}

/// An inspection begun, which every call asking for the same harness while
/// it runs shares.
type Pending = Shared<LocalBoxFuture<'static, Rc<Row>>>;

/// What the admin holds, shared with the inspections it begins.
struct Inner {
    env: Env,
    time: Rc<dyn Time>,
    latest: Rc<dyn Latest>,
    capture: Rc<dyn Capture>,
    cache: RefCell<HashMap<Harness, Rc<Row>>>,
    pending: RefCell<HashMap<Harness, Pending>>,
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
}

impl Drop for Clearing<'_> {
    fn drop(&mut self) {
        self.inner.pending.borrow_mut().remove(&self.harness);
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
    /// asking `latest` for releases and running programs through `capture`.
    pub fn new(
        env: Env,
        time: Rc<dyn Time>,
        latest: Rc<dyn Latest>,
        capture: Rc<dyn Capture>,
    ) -> Self {
        Self {
            inner: Rc::new(Inner {
                env,
                time,
                latest,
                capture,
                cache: RefCell::new(HashMap::new()),
                pending: RefCell::new(HashMap::new()),
            }),
        }
    }

    /// What is known of the harness named `id`, or of every harness in the
    /// order they are listed in when none is named: a row kept less than five
    /// minutes is answered as it was, unless `refresh` asks to look again,
    /// and the CLI's path is the same. Every harness is looked at at once.
    /// `Unknown harness` for a name that is none.
    pub async fn check(&self, id: Option<&str>, refresh: bool) -> Result<Vec<Rc<Row>>, String> {
        let harnesses = match id {
            None => known_harnesses().to_vec(),
            Some(name) => vec![Harness::from_name(name).ok_or("Unknown harness")?],
        };
        let looks = harnesses
            .into_iter()
            .map(|harness| self.look(harness, refresh));
        Ok(join_all(looks).await)
    }

    /// One harness's row: the one kept, the inspection that is running, or an
    /// inspection begun now.
    async fn look(&self, harness: Harness, refresh: bool) -> Rc<Row> {
        let inner = &self.inner;
        let path =
            harness_path(harness, &inner.env).map(|found| found.to_string_lossy().into_owned());
        let now = inner.time.wall_ms();
        if !refresh {
            let kept = inner.cache.borrow().get(&harness).cloned();
            if let Some(row) = kept {
                if row.path == path && now - row.checked_at < KEPT_MS {
                    return row;
                }
            }
        }
        let running = inner.pending.borrow().get(&harness).cloned();
        if let Some(running) = running {
            return running.await;
        }
        let begun = inspection(Rc::clone(inner), harness, path, now)
            .boxed_local()
            .shared();
        inner.pending.borrow_mut().insert(harness, begun.clone());
        let _clearing = Clearing { inner, harness };
        begun.await
    }
}

/// An inspection, whose row is kept when it is done, before any call that
/// waited for it goes on.
async fn inspection(
    inner: Rc<Inner>,
    harness: Harness,
    path: Option<String>,
    checked_at: i64,
) -> Rc<Row> {
    let row = Rc::new(inspect::inspect(&inner, harness, path, checked_at).await);
    inner.cache.borrow_mut().insert(harness, Rc::clone(&row));
    row
}

#[cfg(test)]
mod tests;
