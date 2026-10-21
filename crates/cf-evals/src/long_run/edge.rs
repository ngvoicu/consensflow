//! Where what a long run says, logs and keeps leaves it, and the one function every
//! text goes through on its way out ([`Edge::shown`]): what looks like a key or a token
//! hidden, the run's own names shown (its participants' handles and their windows'
//! ids, every look's, and the names of the files it keeps). Through it go the console
//! (the run's floor, a quiet one: what it notes kept for the report, and what it says
//! said, a window's screen it shows masked as one text first: [`Edge::floor`]); the
//! report it keeps, with the timeline and the run's log in it (each text of the JSON, a
//! window's screen as one text, and the Markdown: [`Edge::keep`]); what stopped the run,
//! kept in a file, of which `cf-evals` writes where it is on its error output
//! ([`Edge::failure`]); the ledger, copied beside the report where a window stalled or
//! the project did not reach its end ([`Edge::keep_ledger`], said only by where it is);
//! and what the rig says of itself when it is dropped unclosed
//! (the masker its configuration is given: [`Edge::masker`]). Nothing of the long run
//! writes around it: `tests/long_run/edge.rs` holds that.
//!
//! The console and the error output are a tool's, where an agent runs the long run,
//! and leave the machine with it (as they do from zeewin through `cargo xtask
//! windows`): the run says there only its own words and identifiers (the verdict, the
//! summary, the known breaks, each break's and each unresolved's key, where the report
//! is), never a window's screen, a record's or a message's words, or a diagnostic. The
//! report keeps all of it, on the machine it ran on.
//!
//! A window's screen is what the pane host gives of it, its last 12 lines, and a
//! secret's name may be above them: such a value leaves unhidden. That is accepted
//! hygiene, not a leak (the lead's line of 2026-10-17, said in `mask.rs`): reading every
//! window's whole output to learn from cost memory and time that grew with the run.

use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use cf_e2e::files;
use cf_e2e::rig::Masker;
use serde_json::Value;

use super::mask::{mask, screen, Names};
use crate::measure::on_copy;
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
    /// [`Edge::screen`] first. A quiet one: what the run notes is kept for the report,
    /// and only what it says is said, which the run makes of its own words and
    /// identifiers alone (whatever runs it, an agent's tool or another machine's runner,
    /// takes its console off the machine).
    pub fn floor<'o>(&self, out: &'o mut dyn Write) -> Floor<'o> {
        let (edge, rows) = (self.clone(), self.clone());
        Floor::masked(
            out,
            move |line| edge.shown(line),
            move |screen| rows.screen(screen),
        )
        .quiet()
    }

    /// What the run's rigs pass what they say of themselves through, when one is dropped
    /// unclosed: each text through [`Edge::shown`] and each window's screen through
    /// [`Edge::screen`], with the names the run knows by then.
    pub fn masker(&self) -> Masker {
        let (text, screen) = (self.clone(), self.clone());
        Masker::new(
            move |said| text.shown(said),
            move |rows| screen.screen(rows),
        )
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

    /// Keeps a copy of the ledger `file` (a closed app's) in `folder`, as
    /// `<name>-ledger.db`: where it is. The file and its write-ahead log made one database
    /// (`VACUUM INTO`), whole and unmasked, as no ledger can be masked: it stays on the
    /// machine the run ran on, as the report beside it does, and leaves the run only by
    /// where it is (its name the run's own from then).
    pub fn keep_ledger(&self, folder: &Path, name: &str, file: &Path) -> Result<PathBuf> {
        let kept = folder.join(format!("{name}-ledger.db"));
        self.knows_files(&[&kept]);
        files::make_dir(folder)?;
        on_copy(file, |db| {
            db.execute("VACUUM INTO ?1", [kept.to_string_lossy()])?;
            Ok(())
        })?;
        Ok(kept)
    }

    /// Where a sample of a stalled window's program is kept: `<name>-<label>-sample.txt` in
    /// `folder` (the reports'), the folder made and the file the run's own, said only by
    /// where it is. The system's `sample` writes it.
    pub fn sample_file(&self, folder: &Path, name: &str, label: &str) -> Result<PathBuf> {
        // One extension: what the edge masks is a name up to its first dot, known as the
        // file's stem.
        let file = folder.join(format!("{name}-{label}-sample.txt"));
        self.knows_files(&[&file]);
        files::make_dir(folder)?;
        Ok(file)
    }

    /// What stopped the run before it kept its report, as it leaves it for `cf-evals` to
    /// write on its error output: a usage's error a usage's still, the person's own words
    /// through [`Edge::shown`]; any other kept whole through [`Edge::shown`] in a file of
    /// `folder` (the reports'), and said only by where it is. What stops a run may hold
    /// a window's words (a wait that gave up says the chief's screen, a rig's timeout
    /// what the daemon and the host wrote), and what runs the run takes its error output
    /// off the machine as it takes its console.
    pub fn failure(&self, failed: Error, folder: &Path) -> Error {
        let said = match failed {
            Error::Usage(said) => return Error::Usage(self.shown(&said)),
            other => self.shown(&other.to_string()),
        };
        let file = folder.join(format!(
            "{}-long-stopped.txt",
            crate::clock::stamp(jiff::Timestamp::now())
        ));
        self.knows_files(&[&file]);
        Error::Refused(match files::write(&file, format!("{said}\n")) {
            Ok(()) => self.shown(&format!(
                "the long run stopped before its report; what stopped it is kept in {}",
                file.display()
            )),
            Err(unkept) => self.shown(&format!(
                "the long run stopped before its report, and what stopped it could not be kept: {unkept}"
            )),
        })
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
