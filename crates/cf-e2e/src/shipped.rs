//! The one way a case starts an app of the release's identity: the bundle the release ships,
//! on GitHub's runner, where its workflow asks for it and no live app runs. Everywhere else a
//! smoke starts a copy of the smoke's identity (`cf_base::bundle`): to macOS a copy started
//! with the release's identity is the live app (2026-10-28, a copy of it came to the front over
//! the live app, and a Quit reached the live app next). The release run is the one place the
//! bundle as it ships, signed under the Developer ID with its hardened runtime, is run: that is
//! kept, and kept to where nothing of a live app can be reached.

use std::path::{Path, PathBuf};

use crate::process::{own_var, Run};
use crate::Result;

/// The variable the release workflow sets (`1`) to smoke the bundle it ships.
pub const SHIPPED_VARIABLE: &str = "CONSENSFLOW_SMOKE_SHIPPED";

/// Leave to start the bundle the release ships, at `app`: made only by [`ask`], where the
/// workflow asked, on GitHub's runner, with no live app running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shipped {
    app: PathBuf,
}

impl Shipped {
    /// Leave for a test of the rig's start.
    #[cfg(all(test, unix))]
    pub(crate) fn for_a_test(app: &Path) -> Self {
        Self {
            app: app.to_path_buf(),
        }
    }

    /// The bundle it is leave to start.
    pub fn app(&self) -> &Path {
        &self.app
    }
}

/// What a smoke starts: the bundle the release ships, or a copy (and why, where it was asked
/// for the shipped one and that was not given).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Shipped(Shipped),
    Copy(Option<String>),
}

/// What the smoke of `app` starts, from what the workflow asked (`asked`, the variable's),
/// where it runs (`actions`, `GITHUB_ACTIONS`) and the bundles whose programs run now, each
/// with its identity where it can be read.
pub(crate) fn decide(
    app: &Path,
    asked: Option<&str>,
    actions: Option<&str>,
    running: &[(PathBuf, Option<String>)],
) -> Answer {
    if asked != Some("1") {
        return Answer::Copy(None);
    }
    if actions != Some("true") {
        return Answer::Copy(Some(format!(
            "{SHIPPED_VARIABLE} asks for the bundle as it ships, which is started on GitHub's \
             runner alone: here a copy of the smoke's identity is"
        )));
    }
    let live = running
        .iter()
        .find(|(_, identity)| match identity.as_deref() {
            None => true,
            Some(identity) => {
                identity != cf_base::bundle::SMOKE
                    && (identity == cf_base::bundle::RELEASE
                        || identity.starts_with(&format!("{}.", cf_base::bundle::RELEASE)))
            }
        });
    if let Some((bundle, identity)) = live {
        return Answer::Copy(Some(format!(
            "{} runs ({}): the bundle as it ships is started only where no live app does, and a \
             copy of the smoke's identity is",
            bundle.display(),
            identity.as_deref().unwrap_or("its identity cannot be read")
        )));
    }
    Answer::Shipped(Shipped {
        app: app.to_path_buf(),
    })
}

/// What the smoke of `app` starts, as this machine and this run say.
pub fn ask(app: &Path) -> Result<Answer> {
    let asked = own_var(SHIPPED_VARIABLE);
    let actions = own_var("GITHUB_ACTIONS");
    if asked.as_deref() != Some("1") {
        return Ok(Answer::Copy(None));
    }
    Ok(decide(
        app,
        asked.as_deref(),
        actions.as_deref(),
        &running_bundles()?,
    ))
}

/// The bundles whose programs run now, as the process table names them, each with its identity
/// where its `Info.plist` says one: the table is read, and nothing is reached.
pub fn running_bundles() -> Result<Vec<(PathBuf, Option<String>)>> {
    let shown = Run::new("/bin/ps").args(["-axo", "comm="]).run()?;
    let mut bundles: Vec<(PathBuf, Option<String>)> = Vec::new();
    for line in shown.stdout.lines() {
        let Some(app) = cf_base::bundle::bundle_of(Path::new(line.trim())) else {
            continue;
        };
        if bundles.iter().any(|(seen, _)| *seen == app) {
            continue;
        }
        let plist = app.join("Contents").join("Info.plist");
        let read = Run::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
            .arg(&plist)
            .run()?;
        let identity = (read.code == Some(0)).then(|| read.stdout.trim().to_owned());
        bundles.push((app, identity));
    }
    Ok(bundles)
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "/runner/work/app/src-tauri/target/release/bundle/macos/ConsensFlow.app";

    fn shipped() -> Answer {
        Answer::Shipped(Shipped {
            app: PathBuf::from(APP),
        })
    }

    fn bundle(path: &str, identity: Option<&str>) -> (PathBuf, Option<String>) {
        (PathBuf::from(path), identity.map(str::to_owned))
    }

    /// The shipped bundle is started only where the workflow asked, on GitHub's runner, and no
    /// program of a ConsensFlow bundle but the smoke's runs (nor one whose identity cannot be
    /// read): the live app and the Candidate are live apps both. Anything else is a copy.
    #[test]
    fn the_shipped_bundle_is_started_only_where_asked_on_the_runner_with_no_live_app() {
        let app = Path::new(APP);
        let none: Vec<(PathBuf, Option<String>)> = Vec::new();
        let others = vec![
            bundle("/System/Applications/Finder.app", Some("com.apple.finder")),
            bundle("/tmp/m/ConsensFlow.app", Some(cf_base::bundle::SMOKE)),
        ];
        assert_eq!(decide(app, Some("1"), Some("true"), &none), shipped());
        assert_eq!(decide(app, Some("1"), Some("true"), &others), shipped());
        assert_eq!(decide(app, None, Some("true"), &none), Answer::Copy(None));
        assert_eq!(
            decide(app, Some("0"), Some("true"), &none),
            Answer::Copy(None)
        );
        for (actions, running) in [
            (None, none.clone()),
            (Some("false"), none.clone()),
            (
                Some("true"),
                vec![bundle(
                    "/Applications/ConsensFlow.app",
                    Some(cf_base::bundle::RELEASE),
                )],
            ),
            (
                Some("true"),
                vec![bundle(
                    "/Users/x/Applications/ConsensFlow Candidate.app",
                    Some("dev.ngvoicu.consensflow.candidate"),
                )],
            ),
            (Some("true"), vec![bundle("/Applications/Unread.app", None)]),
        ] {
            let answer = decide(app, Some("1"), actions, &running);
            assert!(
                matches!(&answer, Answer::Copy(Some(why)) if !why.is_empty()),
                "{actions:?} {running:?}: {answer:?}"
            );
        }
    }

    /// The table names this test's own program, which is no bundle's: what it reads is the
    /// bundles that run, and it reaches none of them.
    #[test]
    fn the_running_bundles_are_read_from_the_table() {
        let bundles = running_bundles().unwrap();
        assert!(bundles
            .iter()
            .all(|(app, _)| app.extension().is_some_and(|it| it == "app")));
    }
}
