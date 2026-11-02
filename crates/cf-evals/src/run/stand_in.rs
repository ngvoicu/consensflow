//! The stand-in members of a run: a scenario with `stand_in_reviews` has one reviewer in its staff,
//! and one with `stand_in_work` has one worker, each a roster agent on a model of its own
//! ([`REVIEWER_MODEL`], [`WORKER_MODEL`]), and the eval's `claude` wrapper starts the window of that
//! model as the stand-in agent (`fake-agent` of `crates/cf-e2e`) with the folder of this run's
//! answers for it in `CF_TEST_SCRIPTED_RESULTS`: the stand-in answers each task brief with the next
//! answer of the scenario ([`crate::scenarios::Scenario::stand_in_reviews`], `stand_in_work`), and
//! writes the files of a worker's answer in the project first.
//!
//! A folder is the run's and nothing else's. It holds a marker, `.eval-run`, and the answers as
//! `1.md`, `2.md`, and so on (a worker's files beside them, in `1.files`, `2.files`); it is made
//! again as each play begins (the stand-in counts the briefs it has taken in the folder, since every
//! window is a fresh one, and keeps a journal of them there), and removed when the run is over, so
//! that a wrapper left on disk refuses the model. Both the wrapper and the stand-in refuse to answer
//! without the marker: the stand-in never answers a real launch.

use std::fs;
use std::path::{Path, PathBuf};

use cf_e2e::files;
use serde_json::Value;

use super::machine::Machine;
use crate::scenarios::{Scenario, ScriptedWork};
use crate::{Error, Result};

/// The model the stand-in reviewer's roster agent is on, which the wrapper reads in the
/// arguments of a window ([`crate::plan::with_stand_in_reviewer`] puts the agent on it).
pub use crate::plan::STAND_IN_MODEL as REVIEWER_MODEL;
/// The model of the stand-in worker's roster agent ([`crate::plan::with_stand_in_worker`]).
pub use crate::plan::STAND_IN_WORKER_MODEL as WORKER_MODEL;

/// The variable that names the folder of the answers, as the stand-in reads it
/// (`crates/cf-e2e/src/bin/fake_agent/scripted.rs`).
pub const FOLDER: &str = "CF_TEST_SCRIPTED_RESULTS";

/// The file that says the folder is an eval run's.
pub const MARKER: &str = ".eval-run";

/// The file of the folder in which the stand-in writes down each brief it takes.
pub const JOURNAL: &str = "journal.jsonl";

/// One stand-in's folder, and what it answers.
#[derive(Debug)]
struct Part {
    folder: PathBuf,
    answers: Vec<ScriptedWork>,
}

impl Part {
    /// The folder made again, with the marker and the answers, and no brief taken.
    fn restart(&self) -> Result {
        // A folder that is not there is already fresh.
        let _ = fs::remove_dir_all(&self.folder);
        files::make_dir(&self.folder)?;
        files::write(&self.folder.join(MARKER), "")?;
        for (at, answer) in self.answers.iter().enumerate() {
            let number = at + 1;
            files::write(&self.folder.join(format!("{number}.md")), &answer.result)?;
            for (path, text) in &answer.writes {
                files::write(
                    &self.folder.join(format!("{number}.files")).join(path),
                    text,
                )?;
            }
        }
        Ok(())
    }
}

/// The stand-in members of one run.
#[derive(Debug)]
pub struct StandIns {
    agent: PathBuf,
    reviewer: Option<Part>,
    worker: Option<Part>,
}

impl StandIns {
    /// The stand-ins of a run of `scenario` on `machine`, played by the program `agent`; none for a
    /// scenario that has none. The folders are not made yet.
    pub fn of(machine: &Machine, scenario: &Scenario, agent: &Path) -> Option<Self> {
        scenario.has_a_stand_in().then(|| Self {
            agent: agent.to_path_buf(),
            reviewer: (!scenario.stand_in_reviews.is_empty()).then(|| Part {
                folder: machine.scripted_reviews(),
                answers: scenario
                    .stand_in_reviews
                    .iter()
                    .map(|review| ScriptedWork {
                        result: review.clone(),
                        writes: Vec::new(),
                    })
                    .collect(),
            }),
            worker: (!scenario.stand_in_work.is_empty()).then(|| Part {
                folder: machine.scripted_work(),
                answers: scenario.stand_in_work.clone(),
            }),
        })
    }

    /// The program that plays the stand-ins.
    pub fn agent(&self) -> &Path {
        &self.agent
    }

    /// The folder of the stand-in reviewer's answers, if the run has one.
    pub fn reviewer_folder(&self) -> Option<&Path> {
        self.reviewer.as_ref().map(|part| part.folder.as_path())
    }

    /// The folder of the stand-in worker's answers, if the run has one.
    pub fn worker_folder(&self) -> Option<&Path> {
        self.worker.as_ref().map(|part| part.folder.as_path())
    }

    /// The folders as a play begins: made again, with the marker and the answers, and no brief taken.
    pub fn restart(&self) -> Result {
        for part in [&self.reviewer, &self.worker].into_iter().flatten() {
            part.restart()?;
        }
        Ok(())
    }

    /// What the stand-in worker was asked in the play so far, and answered: each brief it took, as
    /// the stand-in wrote it down, with the answer it was given and the files it wrote. None where
    /// the run has no stand-in worker.
    pub fn work_taken(&self) -> Result<Option<Vec<Taken>>> {
        let Some(part) = &self.worker else {
            return Ok(None);
        };
        let file = part.folder.join(JOURNAL);
        // No brief taken: no journal.
        let text = fs::read_to_string(&file).unwrap_or_default();
        text.lines()
            .map(|line| {
                let value: Value = serde_json::from_str(line).map_err(|cause| {
                    Error::Refused(format!("a line of {} is no JSON ({cause})", file.display()))
                })?;
                Ok(Taken::of(&value))
            })
            .collect::<Result<Vec<_>>>()
            .map(Some)
    }
}

impl Drop for StandIns {
    /// The run is over: the folders go with it, and a wrapper left on disk refuses.
    fn drop(&mut self) {
        for part in [&self.reviewer, &self.worker].into_iter().flatten() {
            let _ = fs::remove_dir_all(&part.folder);
        }
    }
}

/// A task brief the stand-in worker took: which brief it was in the run, the answer of the script it
/// was given, the brief whole, the files the answer wrote, and the files the brief told it to make or
/// change that the answer did not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    pub taken: i64,
    pub given: i64,
    pub brief: String,
    pub wrote: Vec<String>,
    pub unheld: Vec<String>,
}

impl Taken {
    /// A line of the stand-in's journal.
    fn of(line: &Value) -> Self {
        let brief = line["brief"].as_str().unwrap_or_default().to_owned();
        let wrote: Vec<String> = line["wrote"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|path| path.as_str().map(str::to_owned))
            .collect();
        Self {
            taken: line["taken"].as_i64().unwrap_or_default(),
            given: line["given"].as_i64().unwrap_or_default(),
            unheld: unheld(&brief, &wrote),
            brief,
            wrote,
        }
    }

    /// The taken brief as a report keeps it.
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "taken": self.taken,
            "given": self.given,
            "brief": self.brief,
            "wrote": self.wrote,
            "unheld": self.unheld,
        })
    }
}

/// The files that `brief` tells a worker to make or change which the answer did not write
/// (`wrote`, paths below the project): the script holds nothing for them. A path counts where a
/// line of the brief has a word of making or changing (create, write, change, replace, fix, …, and
/// the owner's Romanian) and none of leaving alone (do not, never, leave, only read, nu modifica,
/// …). That is a check of words, and its limit: a run's report keeps the brief whole for a person.
fn unheld(brief: &str, wrote: &[String]) -> Vec<String> {
    use std::sync::OnceLock;

    use regex::Regex;
    static MAKES: OnceLock<Regex> = OnceLock::new();
    static ALONE: OnceLock<Regex> = OnceLock::new();
    static PATH: OnceLock<Regex> = OnceLock::new();
    let compile = |cell: &'static OnceLock<Regex>, source: &str| -> &'static Regex {
        cell.get_or_init(|| {
            #[allow(clippy::expect_used)]
            Regex::new(source).expect("a pattern written in the stand-in's own source")
        })
    };
    let makes = compile(
        &MAKES,
        r"(?i)\b(?:creat\w*|writ\w*|add(?:s|ed|ing)?|chang\w*|modif\w*|edit\w*|replac\w*|updat\w*|fix(?:es|ed|ing)?|correct\w*|rewrit\w*|remov\w*|delet\w*|creează|creeaz|scrie|adaugă|schimbă|modifică|editează|înlocuiește|corectează|șterge)\b",
    );
    let alone = compile(
        &ALONE,
        r"(?i)\b(?:do\s+not|don't|never|leave|alone|untouched|read[- ]only|only\s+read|no\s+other|nothing\s+else|nu\s+modifica|nu\s+atinge|nu\s+schimba|niciun\s+alt)\b",
    );
    let path = compile(
        &PATH,
        r"[A-Za-z0-9_][A-Za-z0-9_./-]*\.(?:html|md|css|js|json|toml|txt)\b",
    );
    let mut wanted: Vec<String> = Vec::new();
    for line in brief
        .lines()
        .filter(|line| makes.is_match(line) && !alone.is_match(line))
    {
        for found in path.find_iter(line) {
            let file = found.as_str().trim_start_matches("./").to_owned();
            if !wanted.contains(&file) {
                wanted.push(file);
            }
        }
    }
    wanted
        .into_iter()
        .filter(|file| {
            !wrote
                .iter()
                .any(|written| written == file || file.ends_with(&format!("/{written}")))
        })
        .collect()
}

#[cfg(test)]
mod tests;
