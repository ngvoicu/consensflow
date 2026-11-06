//! A stand-in Pi for the reads of the model list: a folder of Pi's, an executable on the PATH, a
//! scripted child that answers (or does not) as Pi does, and the clock.

use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use serde_json::json;

use super::*;
use crate::testing::{finished, ChildScript, Driver, Ends, ManualTime, ScriptedProcesses};

/// The line Pi answers `get_available_models` with, of `ids` (`provider/id`), to the request
/// whose id is `request`.
pub(super) fn answer(request: &str, ids: &[&str]) -> String {
    let models: Vec<serde_json::Value> = ids
        .iter()
        .map(|id| {
            let (provider, id) = id.split_once('/').unwrap();
            json!({ "provider": provider, "id": id, "name": id })
        })
        .collect();
    json!({ "type": "response", "id": request, "success": true, "data": { "models": models } })
        .to_string()
}

pub(super) struct StandIn {
    _folder: tempfile::TempDir,
    pub agent: PathBuf,
    pub env: Env,
    pub processes: Rc<ScriptedProcesses>,
    pub time: Rc<ManualTime>,
}

impl StandIn {
    /// A Pi whose folder holds `files` (name, text), in an environment that also has `vars`.
    pub fn new(files: &[(&str, &str)], vars: &[(&str, &str)]) -> Self {
        let folder = tempfile::tempdir().unwrap();
        let pi = crate::testing::fake_executable(&folder.path().join("pi"));
        let agent = folder.path().join("agent");
        std::fs::create_dir_all(&agent).unwrap();
        for (name, text) in files {
            let at = agent.join(name);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(at, text).unwrap();
        }
        let env = Env::from_vars(
            [
                ("PATH", pi.parent().unwrap().as_os_str()),
                ("PI_CODING_AGENT_DIR", agent.as_os_str()),
            ]
            .into_iter()
            .chain(
                vars.iter()
                    .map(|(name, value)| (*name, std::ffi::OsStr::new(value))),
            ),
        );
        Self {
            _folder: folder,
            agent,
            env,
            processes: Rc::new(ScriptedProcesses::default()),
            time: Rc::new(ManualTime::new(0)),
        }
    }

    /// The `pi` that is started next: the lines it writes, and how it ends.
    pub fn child(&self, lines: &[String], ends: Ends) {
        self.processes.child(
            "pi",
            ChildScript {
                lines: lines.to_vec(),
                ends,
            },
        );
    }

    /// What that `pi` has written to its error stream, and the code it exits with.
    pub fn says(&self, said: &str, code: Option<i32>) {
        self.processes.child_said("pi", said, code);
    }

    /// The read, for a Pi that needs no clock.
    pub fn read(&self, fetch: bool) -> Result<ModelListing, String> {
        finished(Box::pin(listed(
            &self.env,
            &*self.processes,
            &*self.time,
            fetch,
        )))
    }

    /// The read, begun, for a Pi that waits on the clock.
    pub fn begin(&self, fetch: bool) -> Driver<Result<ModelListing, String>> {
        let (env, processes, time) = (
            self.env.clone(),
            Rc::clone(&self.processes),
            Rc::clone(&self.time),
        );
        let mut driver = Driver::default();
        driver.begin(
            0,
            async move { listed(&env, &*processes, &*time, fetch).await },
        );
        driver
    }
}
