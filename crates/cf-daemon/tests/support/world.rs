//! The world a trace starts from (its `world` steps): the folder the test made,
//! `«root»`, with the files it put there, and the environment the daemon runs in
//! over it. The first step holds everything; each later one what changed, a
//! variable or a file that went being `null`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use cf_base::env::Env;
use regex::Regex;
use serde_json::Value;

/// `«root»` and the path that follows it, as far as a path's own characters go.
static ROOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new("«root»([A-Za-z0-9._/-]*)").unwrap());

/// The time a player gives where a trace says `«now»`: the stamps of a roster
/// file, which a writer took from its own clock.
const GIVEN: &str = "2026-01-01T00:00:00.000Z";

/// The folder, in a temporary place that goes with it, and the variables set.
pub struct World {
    dir: tempfile::TempDir,
    env: BTreeMap<String, String>,
}

impl World {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("a folder for the world"),
            env: BTreeMap::new(),
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// `text` with every `«root»` and the path after it written as this machine
    /// writes it: with its own separator, and for text that is JSON, escaped as
    /// JSON escapes it (a Windows path has backslashes).
    pub fn expand(&self, text: &str, json: bool) -> String {
        ROOTED
            .replace_all(text, |found: &regex::Captures<'_>| {
                let path = found[1]
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .fold(self.path().to_path_buf(), |path, part| path.join(part));
                let path = path.to_string_lossy().into_owned();
                if json {
                    path.replace('\\', "\\\\").replace('"', "\\\"")
                } else {
                    path
                }
            })
            .into_owned()
    }

    /// The file at `path` (under the folder, `/` between the parts).
    fn file(&self, path: &str) -> PathBuf {
        path.split('/')
            .fold(self.path().to_path_buf(), |file, part| file.join(part))
    }

    /// Puts a `world` step in place: its variables, and its files, a `null`
    /// taking one away. A program is the script the trace has, made executable;
    /// on Windows it is the shape of an npm shim, as `harness_path` finds one
    /// there. Whether the environment is another now.
    pub fn put(&mut self, step: &Value) -> bool {
        let mut changed = false;
        for (name, value) in step["env"].as_object().into_iter().flatten() {
            changed |= match value.as_str() {
                Some(value) => {
                    let value = self.expand(value, false);
                    let before = self.env.insert(name.clone(), value.clone());
                    before.as_deref() != Some(value.as_str())
                }
                None => self.env.remove(name).is_some(),
            };
        }
        for (path, file) in step["files"].as_object().into_iter().flatten() {
            let target = self.file(path);
            if file.is_null() {
                let _ = fs::remove_file(&target);
                continue;
            }
            fs::create_dir_all(target.parent().expect("a file in a folder")).expect("a folder");
            let executable = file["executable"].as_bool().unwrap_or(false);
            if executable && cfg!(windows) {
                cf_harness::testing::fake_window_executable(&shim_of(&target));
                continue;
            }
            let text = file["text"]
                .as_str()
                .expect("a file of text: no trace holds another");
            fs::write(&target, text.replace("«now»", GIVEN)).expect("a file");
            #[cfg(unix)]
            if executable {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
                    .expect("a program made executable");
            }
        }
        changed
    }

    /// The daemon's environment in this world: the variables the traces name,
    /// the home as `USERPROFILE` too, and on Windows what finds and starts a
    /// `.cmd` there.
    pub fn env(&self) -> Env {
        let process = Env::from_process();
        let windows = ["SystemRoot", "ComSpec", "PATHEXT"]
            .into_iter()
            .filter(|_| cfg!(windows))
            .filter_map(|name| Some((name.to_owned(), process.text(name)?.to_owned())));
        let profile = self
            .env
            .get("HOME")
            .map(|home| ("USERPROFILE".to_owned(), home.clone()));
        Env::from_vars(self.env.clone().into_iter().chain(windows).chain(profile))
    }
}

/// What `fake_window_executable` makes a shim of: a program is found on Windows
/// by its name with `.cmd`, so a trace's own `.cmd` twin of a stand-in is the
/// shim of the stand-in beside it.
fn shim_of(target: &Path) -> PathBuf {
    if target
        .extension()
        .is_some_and(|extension| extension == "cmd")
    {
        target.with_extension("")
    } else {
        target.to_path_buf()
    }
}
