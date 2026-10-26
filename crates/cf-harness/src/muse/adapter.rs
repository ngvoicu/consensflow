//! Muse Code, as the engine launches its windows: not yet. Its record is read and its
//! sessions are told resumable or not, and a launch is refused in plain words, an
//! older Muse first for its version (3.0.6's first phase, T-703; the window is
//! MUS-08's).

use std::path::Path;
use std::rc::Rc;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use cf_proto::specs::Variant;

use super::index;
use super::version::{self, Version};
use crate::contract::{Adapter, Launch, Prepared, Records, Skill, Work};
use crate::detect::executable;
use crate::records::Options;
use crate::seams::Services;

/// What a launch of a Muse window is refused with while ConsensFlow opens none.
pub(crate) const NOT_YET: &str = "ConsensFlow cannot open a Muse Code window yet.";

/// Muse Code, as the engine launches its windows.
pub struct MuseAdapter {
    env: Env,
    records: Rc<dyn Records>,
}

impl MuseAdapter {
    /// The adapter of the windows the engine's `services` serve.
    pub fn new(services: &Services) -> Self {
        Self {
            env: services.env.clone(),
            records: Rc::clone(&services.records),
        }
    }
}

impl Adapter for MuseAdapter {
    /// Refused: a Muse older than the minimum for its version, as the launcher's
    /// `.muse-version` names it, and any other because no window is opened on Muse yet.
    fn prepare<'a>(&'a self, _launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let program = executable(Harness::Muse, &self.env)?;
            let installed = version::installed(Path::new(&program));
            if installed
                .as_deref()
                .and_then(Version::of)
                .is_some_and(|found| !found.supported())
            {
                return Err(version::required());
            }
            Err(NOT_YET.to_owned())
        })
    }

    /// The session's log says (`index::resumable`), and Muse's index only where the
    /// log cannot be read.
    fn resumable<'a>(&'a self, session: &'a str) -> Work<'a, Option<bool>> {
        Box::pin(async move {
            let looked = self
                .records
                .look(Harness::Muse, session, &Options::default())
                .await;
            index::resumable(&looked, index::row(session, &self.env))
        })
    }

    /// No window, so no skill to find.
    fn specmint(&self, _variant: Variant) -> Result<Skill, String> {
        Err(NOT_YET.to_owned())
    }
}

#[cfg(test)]
mod tests;
