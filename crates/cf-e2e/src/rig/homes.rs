//! The homes of the real harnesses that a rig's windows run on in place of the
//! owner's: what the rig asks of each when it starts (the variables its windows
//! are given) and when it ends (what the run came to).
//!
//! A rig clears the environment its daemon, pane host and windows have, and sets
//! its own. A login the owner keeps in a folder of their own is reached by a
//! variable the rig sets (or does not set: Pi reads `~/.pi/agent` when
//! `PI_CODING_AGENT_DIR` names nothing), so what a window of a real harness did
//! to it was the owner's, and a Pi that refreshed its token rotated theirs
//! (T-696). A [`HarnessHome`] is the other way: a folder of the run's own that
//! holds a copy of the login, whose variables the rig sets **last**, over what
//! the rig set and over every [`Config::var`](super::Config::var) of the case,
//! and that is asked what the run came to when the windows are gone.
//!
//! Claude Code's config folder ([`ClaudeConfig`](super::ClaudeConfig)) is one: it
//! is held to the same start and end here, so the rig has one path for them all.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{ClaudeConfig, Config};
use crate::{Error, Result};

/// A home of a real harness's own that a rig's windows run on in place of the
/// owner's ([`Config::isolated`]).
pub trait HarnessHome: std::fmt::Debug + Send + Sync {
    /// The variables its windows are given, made when first asked for and the
    /// same after: a rig that starts again on the same home (the app's restart)
    /// finds what the run kept there. An error says why there is no home, said
    /// before anything is spent.
    fn vars(&self) -> Result<Vec<(String, String)>>;

    /// Where the login in the home is copied from, as a run says it.
    fn described(&self) -> String;

    /// Copies what the run's harness kept in the home that a kept home keeps
    /// (never a login) into `into`, the `.claude` of the home the rig made: asked
    /// when a rig whose home is kept ends, the windows gone, before
    /// [`HarnessHome::finish`].
    fn keep_records(&self, _into: &Path) -> Result {
        Ok(())
    }

    /// What the run came to in the home and in the owner's setup, asked when a
    /// rig that used the home ends, the windows gone: an error says what the run
    /// did that it was not to, and fails the case that ends the rig.
    fn finish(&self) -> Result;
}

/// Claude Code's config folder, as a home: the one variable it is.
#[derive(Debug)]
struct ClaudeHome(Arc<dyn ClaudeConfig>);

impl HarnessHome for ClaudeHome {
    fn vars(&self) -> Result<Vec<(String, String)>> {
        let folder: PathBuf = self.0.folder()?;
        Ok(vec![(
            "CLAUDE_CONFIG_DIR".to_owned(),
            folder.to_string_lossy().into_owned(),
        )])
    }

    fn described(&self) -> String {
        self.0.described()
    }

    fn keep_records(&self, into: &Path) -> Result {
        self.0.keep_records(into)
    }

    fn finish(&self) -> Result {
        self.0.finish()
    }
}

/// The homes a rig runs on, from the moment it asked them for their variables to
/// the moment it asked what the run came to.
pub(super) struct Homes {
    list: Vec<Arc<dyn HarnessHome>>,
    finished: bool,
}

impl Homes {
    /// The homes of `config`, asked for their variables in the order they were
    /// given (Claude Code's first): the first that cannot be made is why the rig
    /// does not start, before anything else is spent.
    pub(super) fn start(config: &Config) -> Result<(Self, Vec<(String, String)>)> {
        let mut list: Vec<Arc<dyn HarnessHome>> = Vec::new();
        if let Some(claude) = &config.claude_config {
            list.push(Arc::new(ClaudeHome(Arc::clone(claude))));
        }
        for home in &config.isolated {
            if !list.iter().any(|held| Arc::ptr_eq(held, home)) {
                list.push(Arc::clone(home));
            }
        }
        let mut vars = Vec::new();
        for home in &list {
            vars.extend(home.vars()?);
        }
        Ok((
            Self {
                list,
                finished: false,
            },
            vars,
        ))
    }

    /// Asks each home what the run came to, once: the answer to the case that
    /// ends the rig. `kept` is where a kept home keeps what the harnesses left.
    /// Every home is asked; the errors are said together.
    pub(super) fn finish(&mut self, kept: Option<&Path>) -> Result {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        let mut said = Vec::new();
        for home in &self.list {
            let kept = kept.map_or(Ok(()), |into| home.keep_records(into));
            if let Err(failed) = home.finish().and(kept) {
                said.push(failed);
            }
        }
        match said.len() {
            0 => Ok(()),
            1 => Err(said.remove(0)),
            _ => Err(Error::Daemon(
                said.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )),
        }
    }
}
