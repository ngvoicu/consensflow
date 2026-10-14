//! What the run finds in the project's folder: the specs the chief wrote in the
//! specs folder it was given (each `SPEC.md` or `SPEC.html`, read by
//! [`crate::judge`]), the registry beside them, and, for a failure to say where
//! they went, the spec files and registries anywhere else below the project.

use std::path::Path;

use cf_e2e::{checkout, files, Error};

use crate::judge::{self, Spec};

/// The folder Specmint documents, and a project that names none keeps its specs in.
pub const DEFAULT_FOLDER: &str = ".specs";

/// A spec file the chief wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Where it is, from the project's folder, with slashes: `docs/specs/hello/SPEC.md`.
    pub path: String,
    /// Its file's name: `SPEC.md` or `SPEC.html`.
    pub file: String,
    /// What it says, or why it could not be read as a spec.
    pub spec: Result<Spec, String>,
}

/// What the project's folder holds of specs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Files {
    /// The spec files in the specs folder, by path.
    pub specs: Vec<Found>,
    /// The registry in the specs folder (`registry.md`): its text, when there is one.
    pub registry: Option<String>,
    /// The paths of the spec files and registries below the project anywhere else.
    pub elsewhere: Vec<String>,
    /// Whether the specs folder is there at all.
    pub folder_exists: bool,
}

impl Files {
    /// The one spec the chief wrote, as an error that says what else it found
    /// where there is none or more than one.
    pub fn the_spec(&self, folder: &str) -> Result<&Found, String> {
        match self.specs.as_slice() {
            [one] => Ok(one),
            [] if self.elsewhere.is_empty() => Err(format!("{folder} holds no spec")),
            [] => Err(format!(
                "{folder} holds no spec, and these are elsewhere: {}",
                self.elsewhere.join(", ")
            )),
            many => Err(format!(
                "{folder} holds {} specs, not one: {}",
                many.len(),
                many.iter()
                    .map(|found| found.path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// That no spec was written anywhere below the project: no spec file and no
    /// registry, in the specs folder or outside it.
    pub fn nothing_of_specs(&self) -> Result<(), String> {
        let mut found: Vec<String> = self.specs.iter().map(|found| found.path.clone()).collect();
        found.extend(self.elsewhere.iter().cloned());
        if self.registry.is_some() {
            found.push("a registry".to_owned());
        }
        if found.is_empty() {
            return Ok(());
        }
        Err(format!("the chief wrote {}", found.join(", ")))
    }
}

/// What the project's folder `project` holds of specs, the specs folder being
/// `folder` below it (with slashes).
pub fn look(project: &Path, folder: &str) -> Result<Files, Error> {
    let mut seen = Files {
        folder_exists: project.join(folder).is_dir(),
        ..Files::default()
    };
    let inside = format!("{}/", folder.trim_end_matches('/'));
    for path in checkout::files_below(project, &[".git", "node_modules"])? {
        let relative = path
            .strip_prefix(project)
            .map(|under| {
                under
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default();
        let name = relative.rsplit('/').next().unwrap_or_default().to_owned();
        let is_spec = name == "SPEC.md" || name == "SPEC.html";
        let is_registry = name == "registry.md";
        if !is_spec && !is_registry {
            continue;
        }
        if !relative.starts_with(&inside) {
            seen.elsewhere.push(relative);
        } else if is_registry && relative == format!("{inside}registry.md") {
            seen.registry = Some(files::read_string(&path)?);
        } else if is_spec {
            let spec = judge::read(&name, &files::read_string(&path)?);
            seen.specs.push(Found {
                path: relative,
                file: name,
                spec,
            });
        }
    }
    Ok(seen)
}

#[cfg(test)]
#[path = "evidence/tests.rs"]
mod tests;
