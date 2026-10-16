//! Where what a long run says, logs and keeps leaves it, and the one function every
//! text goes through on its way out ([`Edge::shown`]): what looks like a key or a token
//! hidden, the run's own names shown (its participants' handles and their windows'
//! ids, every look's, and the names of the files it keeps). Through it go the console
//! (the run's floor, all it notes and says, a window's screen it shows masked as one
//! text first: [`Edge::floor`]); the report it keeps, with
//! the timeline and the run's log in it (each text of the JSON, a window's screen as
//! one text, and the Markdown: [`Edge::keep`]); what stopped the run, which `cf-evals`
//! writes on its error output ([`Edge::failure`]); and what the rig says of itself when
//! it is dropped unclosed (the masker its configuration is given: [`Edge::masker`]).
//! Nothing of the long run writes around it: `tests/long_run/edge.rs` holds that.

use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use cf_e2e::files;
use cf_e2e::rig::Masker;
use serde_json::Value;

use super::mask::{mask, screen, Names};
use crate::run::floor::Floor;
use crate::{Error, Result};

/// The edge of a run, with the names it has learned: one for the run, shared by all it
/// hands out.
#[derive(Debug, Clone, Default)]
pub struct Edge {
    names: Arc<Mutex<Names>>,
}

impl Edge {
    /// `text` as it leaves the run: what looks like a key or a token hidden, the run's
    /// own names shown.
    pub fn shown(&self, text: &str) -> String {
        mask(text, &self.names())
    }

    /// The run's names from now include `names` (a look's).
    pub fn knows(&self, names: &Names) {
        self.names
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(names);
    }

    /// The rows of a window's screen as they leave the run: masked as one text, its
    /// wrapped rows joined, the run's own names shown.
    pub fn screen(&self, rows: &[String]) -> Vec<String> {
        screen(rows, &self.names())
    }

    /// The console of the run, on `out`: everything it notes and says, and the log it
    /// keeps, through [`Edge::shown`], and a window's screen it shows through
    /// [`Edge::screen`] first.
    pub fn floor<'o>(&self, out: &'o mut dyn Write) -> Floor<'o> {
        let (edge, rows) = (self.clone(), self.clone());
        Floor::masked(
            out,
            move |line| edge.shown(line),
            move |screen| rows.screen(screen),
        )
    }

    /// What the run's rig passes what it says of itself through, when it is dropped
    /// unclosed: [`Edge::shown`], with the names the run knows by then.
    pub fn masker(&self) -> Masker {
        let edge = self.clone();
        Masker::new(move |text| edge.shown(text))
    }

    /// Keeps the report `value` and its Markdown `text` in `folder`, as `<name>.json`
    /// and `<name>.md`, each text through [`Edge::shown`] (a window's screen as one
    /// text): where each is. The files' names are the run's own from then.
    pub fn keep(
        &self,
        folder: &Path,
        name: &str,
        value: &Value,
        text: &str,
    ) -> Result<(PathBuf, PathBuf)> {
        let json_file = folder.join(format!("{name}.json"));
        let text_file = folder.join(format!("{name}.md"));
        self.knows_files(&[&json_file, &text_file]);
        let shown = masked(value, &self.names());
        let pretty = serde_json::to_string_pretty(&shown)
            .map_err(|cause| Error::Refused(format!("the report is no JSON: {cause}")))?;
        files::write(&json_file, format!("{pretty}\n"))?;
        files::write(&text_file, self.shown(text))?;
        Ok((json_file, text_file))
    }

    /// What stopped the run, as it leaves it for `cf-evals` to write on its error
    /// output: through [`Edge::shown`], a usage's error a usage's still.
    pub fn failure(&self, failed: Error) -> Error {
        match failed {
            Error::Usage(said) => Error::Usage(self.shown(&said)),
            other => Error::Refused(self.shown(&other.to_string())),
        }
    }

    /// The names the run knows.
    fn names(&self) -> Names {
        self.names
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The folders and names of `paths`, files the run keeps, are the run's own: shown
    /// where it says where they are.
    fn knows_files(&self, paths: &[&Path]) {
        let mut names = self.names.lock().unwrap_or_else(PoisonError::into_inner);
        for path in paths {
            for part in path.components() {
                if let Component::Normal(part) = part {
                    let part = Path::new(part);
                    names.insert(part.to_string_lossy());
                    if let Some(stem) = part.file_stem() {
                        names.insert(stem.to_string_lossy());
                    }
                }
            }
        }
    }
}

/// `value` with each of its texts masked, the run's `names` shown; the rows of a
/// window's screen (an array of texts named `screen`) masked as one text.
fn masked(value: &Value, names: &Names) -> Value {
    match value {
        Value::String(text) => Value::String(mask(text, names)),
        Value::Array(items) => Value::Array(items.iter().map(|item| masked(item, names)).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(name, field)| {
                    let rows: Option<Vec<String>> = field
                        .as_array()
                        .filter(|_| name == "screen")
                        .and_then(|rows| {
                            rows.iter()
                                .map(|row| row.as_str().map(str::to_owned))
                                .collect()
                        });
                    let shown = match rows {
                        Some(rows) => Value::Array(
                            screen(&rows, names)
                                .into_iter()
                                .map(Value::String)
                                .collect(),
                        ),
                        None => masked(field, names),
                    };
                    (name.clone(), shown)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests;
