//! The engine's seams that are the daemon's to give (`dispatcher.js`'s
//! options, `src/core/daemon.js:137-155`): what a window starts with (its
//! environment and its role text), the launch ids and the launch files, an
//! adapter for each harness, and where work that goes on apart runs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use cf_engine::roles::{role_instructions, staff_of};
use cf_engine::runtime::{LocalWork, Spawn};
use cf_engine::seams::{Adapters, LaunchFiles, LaunchIds, PaneEnv, Roles};
use cf_harness::contract::{Adapter, LaunchId};
use cf_harness::launch::adapter;
use cf_harness::seams::{uuid, Services, SystemEntropy};
use cf_ledger::{ParticipantView, ProjectView};
use cf_proto::agents::Harness;

use crate::errors::Errors;

/// The environment a window's pane starts with before its harness's own
/// (`paneEnv`, `daemon.js:148-154`): where the API is, which project and
/// participant the window is, the runtime the daemon was given to name
/// (`CONSENSFLOW_NODE`: whoever starts the daemon says it, the daemon passes
/// it on and guesses nothing), and a PATH with the bundle's `bin` first, so
/// the `cf` of this install is the one a window finds. The token is added at
/// launch.
pub struct WindowEnv {
    /// The API's address, with no slash at its end.
    pub url: String,
    node: Option<String>,
    path: String,
}

impl WindowEnv {
    /// For a daemon given `env`, whose API is at `url` and whose bundle's
    /// `bin` is `bin`.
    pub fn new(env: &Env, url: &str, bin: &str) -> Self {
        let delimiter = if cfg!(windows) { ';' } else { ':' };
        Self {
            url: url.to_owned(),
            node: env
                .os("CONSENSFLOW_NODE")
                .filter(|node| !node.is_empty())
                .map(|node| node.to_string_lossy().into_owned()),
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
        if let Some(node) = &self.node {
            env.push(("CONSENSFLOW_NODE".to_owned(), node.clone()));
        }
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

/// Where work that goes on apart from its caller runs: a task of its own on
/// the daemon's local set, whose panic is written down and gone past.
pub struct DaemonSpawn {
    errors: Rc<Errors>,
}

impl DaemonSpawn {
    /// Work apart whose panic is written to `errors`.
    pub fn new(errors: Rc<Errors>) -> Self {
        Self { errors }
    }
}

impl Spawn for DaemonSpawn {
    fn spawn(&self, work: LocalWork) {
        self.errors.spawn("a task failed", work);
    }
}

#[cfg(test)]
mod tests;
