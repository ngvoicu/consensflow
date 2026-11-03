//! What the unit tests of the screens, and of the API in front of them, stand
//! on: screens over a home of their own with the harnesses a test says
//! installed, whose admin asks scripted feeds and runs scripted programs.

use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use cf_harness::admin::HarnessAdmin;
use cf_harness::testing::{fake_executable, ManualTime, ScriptedCapture, ScriptedLatest, EPOCH_MS};

use super::Screens;

/// The UI token of the screens a test makes.
pub(crate) const TOKEN: &str = "the-ui-token";

/// Screens over a home in a temporary folder, and what a test reaches into.
pub(crate) struct Rig {
    pub(crate) home: tempfile::TempDir,
    /// Where each harness installed is: its stand-in.
    pub(crate) installed: Vec<(String, PathBuf)>,
    pub(crate) screens: Screens,
    pub(crate) latest: Rc<ScriptedLatest>,
    pub(crate) capture: Rc<ScriptedCapture>,
}

impl Rig {
    /// Screens with the harnesses named in `installed` on the PATH, each a
    /// stand-in.
    pub(crate) fn new(installed: &[&str]) -> Self {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let installed: Vec<(String, PathBuf)> = installed
            .iter()
            .map(|harness| ((*harness).to_owned(), fake_executable(&bin.join(harness))))
            .collect();
        let (latest, capture) = (
            Rc::new(ScriptedLatest::default()),
            Rc::new(ScriptedCapture::default()),
        );
        let env = environment(&home.path().to_string_lossy(), &bin.to_string_lossy());
        let screens = Screens {
            token: TOKEN.to_owned(),
            admin: HarnessAdmin::new(
                env.clone(),
                Rc::new(ManualTime::new(EPOCH_MS)),
                Rc::clone(&latest) as _,
                Rc::clone(&capture) as _,
            ),
            env,
        };
        Self {
            home,
            installed,
            screens,
            latest,
            capture,
        }
    }
}
}

/// Screens over no home at all: for the tests that never reach a route that reads one.
pub(crate) fn inert() -> Screens {
    let env = Env::default();
    Screens {
        token: TOKEN.to_owned(),
        admin: HarnessAdmin::new(
            env.clone(),
            Rc::new(ManualTime::new(EPOCH_MS)),
            Rc::new(ScriptedLatest::default()),
            Rc::new(ScriptedCapture::default()),
        ),
        env,
    }
}

/// The environment of a daemon on `root` that finds programs in `bin` alone, and
/// on Windows what finds and starts a `.cmd` there (`PATHEXT`, `SystemRoot`,
/// `ComSpec`).
fn environment(root: &str, bin: &str) -> Env {
    let process = Env::from_process();
    let windows = ["SystemRoot", "ComSpec", "PATHEXT"]
        .into_iter()
        .filter(|_| cfg!(windows))
        .filter_map(|name| Some((name.to_owned(), process.text(name)?.to_owned())));
    let home = format!("{root}{}home", std::path::MAIN_SEPARATOR);
    Env::from_vars(
        [
            (
                "CONSENSFLOW_HOME",
                format!("{root}{}consensflow", std::path::MAIN_SEPARATOR),
            ),
            ("HOME", home.clone()),
            ("USERPROFILE", home),
            ("PATH", bin.to_owned()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .chain(windows),
    )
}
