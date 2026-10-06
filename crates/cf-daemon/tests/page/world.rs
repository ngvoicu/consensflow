//! The files and variables the page reads, put in place before the step that
//! reads them (the `world` steps): the first holds everything, later ones what
//! changed, a variable or a file that went being `null`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use serde_json::Value;

/// The folder of the run and the environment of the daemon in it.
pub struct World {
    root: PathBuf,
    env: BTreeMap<String, String>,
}

impl World {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            env: BTreeMap::new(),
        }
    }

    /// The step put in place; whether the environment is another now.
    pub fn put(&mut self, step: &Value) -> bool {
        let mut changed = false;
        for (name, value) in step["env"].as_object().into_iter().flatten() {
            changed |= match value.as_str() {
                Some(value) => {
                    self.env.insert(name.clone(), value.to_owned()).as_deref() != Some(value)
                }
                None => self.env.remove(name).is_some(),
            };
        }
        for (path, file) in step["files"].as_object().into_iter().flatten() {
            let at = self.root.join(path);
            if file.is_null() {
                let _ = std::fs::remove_file(&at);
                continue;
            }
            let text = file["text"]
                .as_str()
                .expect("a file of text: the page's traces hold no other");
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(&at, text).unwrap();
            #[cfg(unix)]
            if file["executable"] == true {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        changed
    }

    /// The daemon's environment: the variables the traces name, as they are.
    pub fn env(&self) -> Env {
        Env::from_vars(self.env.clone())
    }
}
