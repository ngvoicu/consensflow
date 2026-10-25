//! The machine's computer use, as the daemon gives it to the engine and the
//! page (T-540): ConsensFlow's own macOS grants, which a program in its panes is
//! given as its own, and whether Claude Code's computer use is on for a folder,
//! read from Claude's own configuration in the daemon's environment and never
//! written. The page's Open System Settings opens one of the two panes where the
//! human gives a grant: a fixed address the daemon chooses by the pane's name,
//! never words the page wrote.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_engine::seams::{ComputerUse, ScreenPermissions};
use cf_harness::claude::computer_use;
use cf_process::{hand_over, screen_grants, with_required, Handed, Run};
use futures_util::future::LocalBoxFuture;

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

/// The machine's computer use in the daemon's environment.
pub struct MachineComputerUse {
    env: Env,
    grants: Rc<dyn Fn() -> Option<ScreenPermissions>>,
}

impl MachineComputerUse {
    /// As the daemon runs: macOS's own grants.
    pub fn new(env: Env) -> Self {
        Self::granting(
            env,
            Rc::new(|| {
                screen_grants().map(|grants| ScreenPermissions {
                    accessibility: grants.accessibility,
                    screen_recording: grants.screen_recording,
                })
            }),
        )
    }

    /// With `grants` in place of the system's: what a test gives.
    pub fn granting(env: Env, grants: Rc<dyn Fn() -> Option<ScreenPermissions>>) -> Self {
        Self { env, grants }
    }
}

impl ComputerUse for MachineComputerUse {
    fn permissions(&self) -> Option<ScreenPermissions> {
        (self.grants)()
    }

    fn claude(&self, directory: &str) -> computer_use::ComputerUse {
        computer_use::computer_use(&self.env, directory)
    }
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
mod tests {
    use super::*;

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
