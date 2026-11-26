//! The machine's computer use, as the daemon gives it to the engine and the
//! page (T-540): ConsensFlow's own macOS grants, which a program in its panes is
//! given as its own, whether Claude Code's computer use is on for a folder, and
//! whether Codex's is set up (T-548), each read from the harness's own
//! configuration in the daemon's environment and never written. The page's
//! Open System Settings opens one of the two panes where the human gives a
//! grant: a fixed address the daemon chooses by the pane's name, never words the
//! page wrote.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use cf_base::env::Env;
use cf_engine::seams::{ComputerUse, ScreenPermissions, Waited};
use cf_harness::claude::computer_use;
use cf_harness::contract::Work;
use cf_process::{hand_over, screen_grants, with_required, Handed, Run};
use futures_util::future::LocalBoxFuture;
use tokio::sync::oneshot;

/// A System Settings pane where the human gives ConsensFlow a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Accessibility,
    ScreenRecording,
}

impl Pane {
    /// The pane the page names (`accessibility`, `screen-recording`), or none.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "accessibility" => Some(Self::Accessibility),
            "screen-recording" => Some(Self::ScreenRecording),
            _ => None,
        }
    }

    /// The address that opens it in System Settings.
    pub fn address(self) -> &'static str {
        match self {
            Self::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            Self::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
        }
    }
}

/// What opens a System Settings pane: the system's own, or a test's.
pub type Opens = Rc<dyn Fn(Pane) -> LocalBoxFuture<'static, Result<(), String>>>;

/// How long the system is waited for before a call says nothing: a permission query goes to
/// the system's privacy service, and a read of Claude Code's configuration to a disk, and
/// either can wait for much longer than the daemon may (T-2203).
const ASKED_WITHIN: Duration = Duration::from_secs(5);

/// What the worker does with Claude Code's configuration, which it alone holds.
type Job = Box<dyn FnOnce(&computer_use::Configuration) + Send>;

/// The machine's computer use in the daemon's environment, with Claude Code's
/// configuration as it last read it: one for the engine and the page.
///
/// Every call into the system is made on a thread of its own, never the daemon's one thread:
/// the daemon waits for its answer as for any other, up to [`ASKED_WITHIN`], and goes on with
/// what it has where none came. A call that never ends holds that thread and no other, and
/// the calls after it say nothing until it does.
pub struct MachineComputerUse {
    env: Env,
    grants: Arc<dyn Fn() -> Option<ScreenPermissions> + Send + Sync>,
    jobs: mpsc::Sender<Job>,
    within: Duration,
}

impl MachineComputerUse {
    /// As the daemon runs: macOS's own grants.
    pub fn new(env: Env) -> Self {
        Self::granting(
            env,
            Arc::new(|| {
                screen_grants().map(|grants| ScreenPermissions {
                    accessibility: grants.accessibility,
                    screen_recording: grants.screen_recording,
                })
            }),
        )
    }

    /// With `grants` in place of the system's: what a test gives.
    pub fn granting(
        env: Env,
        grants: Arc<dyn Fn() -> Option<ScreenPermissions> + Send + Sync>,
    ) -> Self {
        let (jobs, work) = mpsc::channel::<Job>();
        // The thread ends with the machine's last sender; its configuration is its own.
        let spawned = std::thread::Builder::new()
            .name("computer-use".to_owned())
            .spawn(move || {
                let configuration = computer_use::Configuration::default();
                while let Ok(job) = work.recv() {
                    job(&configuration);
                }
            });
        drop(spawned);
        Self {
            env,
            grants,
            jobs,
            within: ASKED_WITHIN,
        }
    }

    /// The machine, waiting `within` for each call to the system.
    #[must_use]
    pub fn waiting(mut self, within: Duration) -> Self {
        self.within = within;
        self
    }

    /// What `call` says, made on the worker's thread and waited for no longer than
    /// [`Self::waiting`]: [`Waited`] where the worker is gone or did not answer in time.
    async fn ask<T: Send + 'static>(
        &self,
        call: impl FnOnce(&computer_use::Configuration) -> T + Send + 'static,
    ) -> Result<T, Waited> {
        let silent = Waited(self.within);
        let (answer, answered) = oneshot::channel();
        let job: Job = Box::new(move |configuration| {
            let _ = answer.send(call(configuration));
        });
        self.jobs.send(job).map_err(|_| silent)?;
        tokio::time::timeout(self.within, answered)
            .await
            .map_err(|_| silent)?
            .map_err(|_| silent)
    }
}

impl ComputerUse for MachineComputerUse {
    fn permissions(&self) -> Work<'_, Result<Option<ScreenPermissions>, Waited>> {
        let grants = Arc::clone(&self.grants);
        Box::pin(async move { self.ask(move |_| grants()).await })
    }

    fn claude<'a>(
        &'a self,
        directory: &'a str,
    ) -> Work<'a, Result<computer_use::ComputerUse, Waited>> {
        let (env, asked) = (self.env.clone(), directory.to_owned());
        Box::pin(async move {
            self.ask(move |configuration| configuration.computer_use(&env, &asked))
                .await
        })
    }

    fn codex(&self) -> Work<'_, Result<cf_harness::codex::computer_use::ComputerUse, Waited>> {
        let env = self.env.clone();
        Box::pin(async move {
            self.ask(move |_| cf_harness::codex::computer_use::computer_use(&env))
                .await
        })
    }
}

/// The bundle identifiers of ConsensFlow's apps: the app's and the Candidate's.
const OWN_BUNDLE: &str = "dev.ngvoicu.consensflow";

/// The app macOS gives the daemon's grants to where it is not ConsensFlow: the app a
/// daemon was started from (a `cf ui` in Terminal is Terminal's, and so are its grants),
/// by the bundle identifier macOS names it with in the environment
/// (`__CFBundleIdentifier`). None where it is ConsensFlow's, or not named.
pub fn grants_of(env: &Env) -> Option<String> {
    let app = env
        .text("__CFBundleIdentifier")
        .filter(|app| !app.is_empty())?;
    let own = app == OWN_BUNDLE
        || app
            .strip_prefix(OWN_BUNDLE)
            .is_some_and(|rest| rest.starts_with('.'));
    (!own).then(|| app.to_owned())
}

/// How long `open` is waited for, for what it says when it fails at once.
const WITHIN: Duration = Duration::from_secs(5);

/// The system's own opener of System Settings panes: `/usr/bin/open` with the
/// pane's address, on macOS; elsewhere there is none to open.
pub fn system_opener(env: Env) -> Opens {
    Rc::new(move |pane| {
        let env = env.clone();
        Box::pin(async move {
            if !cfg!(target_os = "macos") {
                return Err("System Settings is a macOS app".to_owned());
            }
            let run = Run {
                program: PathBuf::from("/usr/bin/open"),
                args: vec![pane.address().into()],
                verbatim: false,
            };
            match hand_over(&run, &with_required(&env, &env), WITHIN).await? {
                Handed::Running | Handed::Ended(Some(0)) => Ok(()),
                Handed::Ended(Some(code)) => Err(format!("open ended with {code}")),
                Handed::Ended(None) => Err("open was ended by a signal".to_owned()),
            }
        })
    })
}

#[cfg(test)]
mod off_thread;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grants_are_another_apps_where_the_daemon_was_started_from_one() {
        let of = |app: Option<&str>| {
            grants_of(&Env::from_vars(
                app.map(|app| ("__CFBundleIdentifier", app)),
            ))
        };
        assert_eq!(
            of(Some("com.apple.Terminal")),
            Some("com.apple.Terminal".to_owned())
        );
        assert_eq!(
            of(Some("dev.ngvoicu.consensflowx")),
            Some("dev.ngvoicu.consensflowx".to_owned())
        );
        for own in [
            Some("dev.ngvoicu.consensflow"),
            Some("dev.ngvoicu.consensflow.candidate"),
            Some(""),
            None,
        ] {
            assert_eq!(of(own), None, "{own:?}");
        }
    }

    #[test]
    fn the_page_names_two_panes_and_each_opens_its_privacy_pane() {
        assert_eq!(Pane::named("accessibility"), Some(Pane::Accessibility));
        assert_eq!(Pane::named("screen-recording"), Some(Pane::ScreenRecording));
        for other in ["", "Accessibility", "x-apple.systempreferences:", "camera"] {
            assert_eq!(Pane::named(other), None, "{other}");
        }
        assert!(Pane::Accessibility
            .address()
            .ends_with("?Privacy_Accessibility"));
        assert!(Pane::ScreenRecording
            .address()
            .ends_with("?Privacy_ScreenCapture"));
    }
}
