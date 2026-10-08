//! The engine's seams that are the daemon's to give (`dispatcher.js`'s
//! options, `src/core/daemon.js:137-155`): what a window starts with (its
//! environment and its role text), the launch ids and the launch files, an
//! adapter for each harness, and where the engine's work runs.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use cf_engine::roles::{role_instructions, staff_of};
use cf_engine::runtime::{Executor, LocalWork, Spawn};
use cf_engine::seams::{Adapters, LaunchFiles, LaunchIds, PaneEnv, Roles};
use cf_harness::contract::{Adapter, LaunchId};
use cf_harness::launch::adapter;
use cf_harness::seams::{uuid, Services, SystemEntropy};
use cf_ledger::{ParticipantView, ProjectView};
use cf_proto::agents::Harness;

use crate::errors::{contain, Errors};

mod boundary;

pub use boundary::{DaemonRecords, DaemonTime};

/// The environment a window's pane starts with before its harness's own
/// (`paneEnv`, `daemon.js:148-154`): where the API is, which project and
/// participant the window is, and a PATH with the bundle's `bin` first, so
/// the `cf` of this install is the one a window finds. The token is added at
/// launch.
pub struct WindowEnv {
    /// The API's address, with no slash at its end.
    pub url: String,
    path: String,
}

impl WindowEnv {
    /// For a daemon given `env`, whose API is at `url` and whose bundle's
    /// `bin` is `bin`.
    pub fn new(env: &Env, url: &str, bin: &str) -> Self {
        let delimiter = if cfg!(windows) { ';' } else { ':' };
        Self {
            url: url.to_owned(),
            path: match env.os("PATH").filter(|path| !path.is_empty()) {
                Some(path) => format!("{bin}{delimiter}{}", path.to_string_lossy()),
                None => bin.to_owned(),
            },
        }
    }
}

impl PaneEnv for WindowEnv {
    fn env(&self, participant: &ParticipantView, project: &ProjectView) -> Vec<(String, String)> {
        let mut env = vec![
            ("CONSENSFLOW_URL".to_owned(), self.url.clone()),
            ("CONSENSFLOW_PROJECT".to_owned(), project.id.to_string()),
            (
                "CONSENSFLOW_PARTICIPANT".to_owned(),
                participant.handle.clone(),
            ),
        ];
        env.push(("PATH".to_owned(), self.path.clone()));
        env
    }
}

/// The text each role's window starts with (`roles`): the role's own, with the
/// project's staff in the chief's, and the `cf` of this window named.
pub struct RoleTexts {
    env: Env,
    /// The bundle's `cf` as a window names it.
    pane_cf: String,
}

impl RoleTexts {
    /// The role texts of a daemon given `env`, naming the bundle's `cf` as
    /// `pane_cf` does.
    pub fn new(env: Env, pane_cf: String) -> Self {
        Self { env, pane_cf }
    }
}

impl Roles for RoleTexts {
    fn instructions(
        &self,
        participant: &ParticipantView,
        project: &ProjectView,
    ) -> Result<String, String> {
        role_instructions(
            &self.env,
            &participant.role,
            &staff_of(project),
            Some(&self.pane_cf),
        )
        .map_err(|failed| failed.to_string())
    }
}

/// Each launch's id, new (`randomUUID`). Panics where the system gives no
/// randomness, which no launch may then be made without.
pub struct RandomLaunchIds;

impl LaunchIds for RandomLaunchIds {
    fn draw(&self) -> LaunchId {
        let drawn = uuid(&SystemEntropy).unwrap_or_else(|failed| {
            panic!("the system gave no randomness: {failed}");
        });
        LaunchId::new(&drawn).unwrap_or_else(|| panic!("a uuid that is none: {drawn}"))
    }
}

/// The files a launch wrote, forgotten once no window will read them
/// (`launchFiles.forget`): a failure to remove them is written down and goes
/// no further.
pub struct LaunchFolders {
    home: PathBuf,
    errors: Rc<Errors>,
}

impl LaunchFolders {
    /// The launch files under `home`; a failure to remove some is written to
    /// `errors`.
    pub fn new(home: PathBuf, errors: Rc<Errors>) -> Self {
        Self { home, errors }
    }
}

impl LaunchFiles for LaunchFolders {
    fn forget(&self, launch: &LaunchId) {
        let home = self.home.to_string_lossy();
        if let Err(failed) = cf_harness::forget_launch(&home, launch) {
            self.errors.log().error(
                "a launch's files could not be removed",
                Some(&failed.to_string()),
            );
        }
    }
}

/// An adapter for each harness, built once with the daemon's services, by the
/// word the ledger names the harness with (`createAdapters`,
/// `src/adapters/index.js`).
pub struct HarnessAdapters {
    by_harness: HashMap<Harness, Rc<dyn Adapter>>,
}

impl HarnessAdapters {
    /// An adapter for each harness there is, built from `services`.
    pub fn new(services: &Services) -> Self {
        Self {
            by_harness: Harness::ALL
                .into_iter()
                .map(|harness| (harness, adapter(harness, services)))
                .collect(),
        }
    }
}

impl Adapters for HarnessAdapters {
    fn adapter(&self, harness: &str) -> Option<Rc<dyn Adapter>> {
        let harness = Harness::from_kind(harness)?;
        self.by_harness.get(&harness).cloned()
    }
}

/// Where the engine's work runs, which is the engine's [`Executor`] and never
/// the local set's own tasks, and the daemon's part of the executor's
/// contract: the work is spawned onto it ([`Spawn`], which the engine is made
/// with, and [`DaemonSpawn::apart`]); a request's work is begun with
/// [`cf_engine::runtime::begin`]; [`DaemonSpawn::drain`] is called where Node's
/// event loop went on to its next callback (after the frames of one read of
/// the bridge, the first part of an HTTP request and each poll of a connection,
/// a timer of the pass loop); what comes from outside the executor, a timer's
/// expiry or a worker thread's answer, is made such a callback
/// ([`DaemonSpawn::arrival`]); and [`DaemonSpawn::drive`] is called once, the
/// driver being a backstop for what no call of `drain` has run. A panic in work
/// is what it was in Node, written down and gone past: the executor ends that
/// work and nothing else.
pub struct DaemonSpawn {
    errors: Rc<Errors>,
    executor: Rc<Executor>,
}

impl DaemonSpawn {
    /// An executor of its own, the panics of whose work are written to `errors`.
    pub fn new(errors: Rc<Errors>) -> Self {
        Self {
            errors,
            executor: Rc::new(Executor::new()),
        }
    }

    /// Where a panic is written down.
    pub fn errors(&self) -> &Rc<Errors> {
        &self.errors
    }

    /// Spawns the executor's driver on the local set the caller is in: it
    /// runs what is woken from outside a drain (an answer from the bridge, a
    /// timer, a socket), and never ends. Once.
    pub fn drive(&self) {
        let driver = self.executor.driver();
        self.errors
            .spawn("the executor's driver failed", async move {
                match driver.await {}
            });
    }

    /// Runs what is woken, and what that wakes, to the end: called where Node's
    /// event loop went on to its next callback, which ran its microtasks first.
    pub fn drain(&self) {
        self.executor.drain();
    }

    /// `work`, run apart on the executor: a panic in it is written down as
    /// `what`, and nothing else is the worse for it.
    pub fn apart(&self, what: &'static str, work: impl Future<Output = ()> + 'static) {
        let errors = Rc::clone(&self.errors);
        self.executor.spawn(Box::pin(async move {
            if let Err(panicked) = contain(work).await {
                errors.caught(what, &panicked);
            }
        }));
    }
}

impl Spawn for DaemonSpawn {
    fn spawn(&self, work: LocalWork) {
        self.apart("a task failed", work);
    }
}

#[cfg(test)]
mod tests;
