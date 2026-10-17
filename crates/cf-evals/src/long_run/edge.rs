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
//!
//! A window's screen is what the pane host gives of it: its last lines, at most
//! twelve (`TAIL_LINES`, the host's own cut), of a screen that keeps no scrollback.
//! A secret's name may be above them and its value in them. So the edge reads whole
//! what every window of the run printed (each rig hands it over as it starts,
//! [`Edge::reads`]), and hides the secrets it finds there wherever it sees them
//! ([`Names::learn`]): before a screen leaves the run, before the report is kept, and
//! before the rig says what it shows of itself. It reads a window again only once the
//! window printed more, and never for a line of the run's own words.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use cf_e2e::files;
use cf_e2e::rig::{Masker, Printed};
use serde_json::Value;

use super::mask::{mask, screen, Names};
use crate::run::floor::Floor;
use crate::{Error, Result};

/// What the windows of a rig printed, as the edge reads it: each window by its id with
/// how many bytes it printed so far, and the whole of what it printed as text.
pub trait Streams: Send + Sync {
    /// Each window that printed, by its id, with how many bytes it printed so far.
    fn panes(&self) -> Vec<(String, usize)>;
    /// Everything the window `pane` printed so far, as the text on its screen.
    fn screen(&self, pane: &str) -> String;
}

impl Streams for Printed {
    fn panes(&self) -> Vec<(String, usize)> {
        Printed::panes(self)
    }

    fn screen(&self, pane: &str) -> String {
        Printed::screen(self, pane)
    }
}

/// What the run's rigs printed, and how much of each window's the edge read whole.
#[derive(Default)]
struct Printing {
    rigs: Vec<Arc<dyn Streams>>,
    /// By the rig's place in `rigs` and the window's id: how many bytes it had printed.
    read: BTreeMap<(usize, String), usize>,
}

impl std::fmt::Debug for Printing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Printing of {} rigs", self.rigs.len())
    }
}

/// The edge of a run, with the names it has learned and what its windows printed: one
/// for the run, shared by all it hands out.
#[derive(Debug, Clone, Default)]
pub struct Edge {
    names: Arc<Mutex<Names>>,
    printing: Arc<Mutex<Printing>>,
}

impl Edge {
    /// `text` as it leaves the run: what looks like a key or a token hidden, and the
    /// secrets learned so far; the run's own names shown.
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

    /// What the windows of a rig of the run print, `printed`, is read whole from now.
    pub fn reads(&self, printed: impl Streams + 'static) {
        self.printing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .rigs
            .push(Arc::new(printed));
    }

    /// The names the run knows, once every window that printed more since it was last
    /// read is read whole ([`Names::learn`]).
    pub fn learned(&self) -> Names {
        let mut printing = self.printing.lock().unwrap_or_else(PoisonError::into_inner);
        let mut names = self.names();
        let Printing { rigs, read } = &mut *printing;
        for (rig, printed) in rigs.iter().enumerate() {
            for (pane, length) in printed.panes() {
                if read.get(&(rig, pane.clone())) == Some(&length) {
                    continue;
                }
                let rows: Vec<String> = printed.screen(&pane).lines().map(str::to_owned).collect();
                names.learn(&rows);
                read.insert((rig, pane), length);
            }
        }
        self.knows(&names);
        names
    }

    /// The rows of a window's screen as they leave the run: masked as one text, its
    /// wrapped rows joined, in the light of all the run's windows printed
    /// ([`Edge::learned`]), the run's own names shown.
    pub fn screen(&self, rows: &[String]) -> Vec<String> {
        screen(rows, &self.learned())
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

    /// What the run's rigs pass what they say of themselves through, when one is dropped
    /// unclosed: each text through [`Edge::shown`] and each window's screen through
    /// [`Edge::screen`], with the names the run knows by then; and what each rig's
    /// windows print, handed to it as the rig starts, is read whole ([`Edge::reads`]).
    pub fn masker(&self) -> Masker {
        let (text, screen, reads) = (self.clone(), self.clone(), self.clone());
        Masker::new(
            move |said| text.shown(said),
            move |rows| screen.screen(rows),
        )
        .reading(move |printed| reads.reads(printed))
    }

    /// Keeps the report `value` and its Markdown `text` in `folder`, as `<name>.json`
    /// and `<name>.md`, each text through [`Edge::shown`] (a window's screen as one
    /// text), in the light of all the run's windows printed ([`Edge::learned`]): where
    /// each is. The files' names are the run's own from then.
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
        let shown = masked(value, &self.learned());
        let pretty = serde_json::to_string_pretty(&shown)
            .map_err(|cause| Error::Refused(format!("the report is no JSON: {cause}")))?;
        files::write(&json_file, format!("{pretty}\n"))?;
        files::write(&text_file, self.shown(text))?;
        Ok((json_file, text_file))
    }

    /// What stopped the run, as it leaves it for `cf-evals` to write on its error
    /// output: masked as [`Edge::shown`] masks, but in the light of all the run's windows
    /// printed ([`Edge::learned`]), for it may quote a window's screen; a usage's error
    /// a usage's still.
    pub fn failure(&self, failed: Error) -> Error {
        let names = self.learned();
        match failed {
            Error::Usage(said) => Error::Usage(mask(&said, &names)),
            other => Error::Refused(mask(&other.to_string(), &names)),
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
