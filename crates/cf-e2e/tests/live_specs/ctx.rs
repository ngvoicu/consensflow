//! A project of the run, being worked on by the human: what the steps share of
//! reading the board, the windows the daemon opened and the folder the specs are
//! in, of waiting for them, and of the one thing the human does to the project, which
//! is to turn its specs on (and the chief's next launch, which they take effect at).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use cf_e2e::live::human::Human;
use cf_e2e::live::report::Check;
use cf_e2e::live::{wait, Log};
use cf_e2e::rig::{Project, Rig};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

use crate::evidence::{self, Files};
use crate::judge::{self, BoardTask};
use crate::launched::{self, RoleText};
use crate::plan::Planned;
use crate::project::Settings;

/// How long the chief's window has to end once the project is closed, how long
/// the next one has to be opened once it is resumed.
const ENDS: Duration = Duration::from_secs(90);
const OPENS: Duration = Duration::from_secs(120);

/// A project of the run.
pub struct Ctx<'a> {
    pub rig: &'a Rig,
    pub project: Project<'a>,
    pub human: Human<'a>,
    pub planned: &'a Planned,
    /// The project's folder.
    pub folder: &'a Path,
    pub settings: &'a Settings,
    pub log: &'a Log,
    /// The briefs of the tasks read so far: a brief is read with its task, and
    /// the board's cards have none.
    briefs: RefCell<BTreeMap<i64, String>>,
    /// The window the chief was launched in once its specs were on.
    on: RefCell<Option<Value>>,
    /// Where the spec the chief wrote is, from the project's folder, once it has.
    spec: RefCell<Option<String>>,
}

impl<'a> Ctx<'a> {
    pub fn new(
        rig: &'a Rig,
        project: Project<'a>,
        planned: &'a Planned,
        folder: &'a Path,
        settings: &'a Settings,
        log: &'a Log,
    ) -> Self {
        Self {
            rig,
            project,
            human: Human::new(rig, project, log),
            planned,
            folder,
            settings,
            log,
            briefs: RefCell::new(BTreeMap::new()),
            on: RefCell::new(None),
            spec: RefCell::new(None),
        }
    }

    /// A check of this project that held, with what was seen.
    pub fn passed(&self, what: &str, detail: impl Into<String>) -> Check {
        Check::passed(&self.planned.name(), what, detail)
    }

    /// A check of this project that did or did not hold, by the verdict of one of
    /// the judges: what was seen either way.
    pub fn judged(&self, what: &str, verdict: std::result::Result<String, String>) -> Check {
        match verdict {
            Ok(seen) => Check::passed(&self.planned.name(), what, seen),
            Err(seen) => Check::failed(&self.planned.name(), what, seen),
        }
    }

    /// [`Ctx::judged`], for a verdict that has nothing to say when it holds.
    pub fn held(&self, what: &str, verdict: std::result::Result<(), String>) -> Check {
        self.judged(what, verdict.map(|()| "as it should be".to_owned()))
    }

    /// Waits for `what`, saying what for and how long; `within` ends it.
    pub fn wait(
        &self,
        what: &str,
        within: Duration,
        check: impl FnMut() -> Result<bool>,
    ) -> Result {
        wait(self.rig, self.log, what, within, check)
    }

    /// The human types `words` to the chief, who is waited for to be idle first.
    pub fn tells(&self, words: &str) -> Result {
        let again = self.human.tells(words)?;
        if again > 0 {
            self.log.say(format!(
                "the chief's window took the human's text only after {again} more Enter"
            ));
        }
        Ok(())
    }

    /// The chief's turn is over: its window reads idle, and its screen holds still.
    pub fn settles(&self) -> Result {
        self.human.waits_for_the_chief()
    }

    /// The brief of the task `number`, as its drawer reads it; none when it cannot be.
    fn brief_of(&self, number: i64) -> String {
        if let Some(brief) = self.briefs.borrow().get(&number) {
            return brief.clone();
        }
        let brief = self
            .project
            .task(number)
            .ok()
            .and_then(|task| task["body"].as_str().map(str::to_owned))
            .unwrap_or_default();
        if !brief.is_empty() {
            self.briefs.borrow_mut().insert(number, brief.clone());
        }
        brief
    }

    /// The tasks of the board, each with its brief.
    pub fn tasks(&self) -> Result<Vec<BoardTask>> {
        let board = self.project.board()?;
        Ok(judge::board_tasks(&board, |number| self.brief_of(number)))
    }

    /// The numbers of the tasks of the board now.
    pub fn numbers(&self) -> Result<BTreeSet<i64>> {
        Ok(self.tasks()?.into_iter().map(|task| task.number).collect())
    }

    /// The tasks the chief put on the board that were not there at `before`.
    pub fn new_chief_tasks(&self, before: &BTreeSet<i64>) -> Result<Vec<BoardTask>> {
        Ok(self
            .tasks()?
            .into_iter()
            .filter(|task| task.requester == "chief" && !before.contains(&task.number))
            .collect())
    }

    /// The windows the daemon opened for the chief, oldest first.
    pub fn chief_windows(&self) -> Vec<Value> {
        let id = format!("p{}-chief", self.project.id());
        self.rig
            .open_frames()
            .into_iter()
            .filter(|frame| frame["id"] == id.as_str())
            .collect()
    }

    /// The windows the daemon opened for the sessions of the stand-in worker.
    pub fn worker_windows(&self) -> Vec<Value> {
        let prefix = format!("p{}-worker", self.project.id());
        self.rig
            .open_frames()
            .into_iter()
            .filter(|frame| {
                frame["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(&prefix))
            })
            .collect()
    }

    /// The text of the role a window was launched with, the part of it that is
    /// ConsensFlow's: read from where its harness was given it.
    pub fn role_text(&self, frame: &Value) -> std::result::Result<String, String> {
        let text = match launched::role_of(frame) {
            Some(RoleText::Inline(text)) => text,
            Some(RoleText::File(file)) => std::fs::read_to_string(&file)
                .map_err(|failed| format!("the role text {file} cannot be read: {failed}"))?,
            None => {
                return Err(format!(
                    "the window {} was given its role in no way the run reads",
                    frame["id"]
                ))
            }
        };
        Ok(launched::ours(&text).to_owned())
    }

    /// What the project's folder holds of specs, the specs folder being `folder`.
    pub fn files(&self, folder: &str) -> Result<Files> {
        evidence::look(self.folder, folder)
    }

    /// The path of the spec the chief wrote, once the run has seen it.
    pub fn spec_path(&self) -> Option<String> {
        self.spec.borrow().clone()
    }

    /// Notes where the spec the chief wrote is.
    pub fn found_spec(&self, path: &str) {
        *self.spec.borrow_mut() = Some(path.to_owned());
    }

    /// The human turns the project's specs on, on its page, and the chief, whose
    /// text is as it was until its next launch, is launched again: the project is
    /// closed and resumed, as the human's Close and Resume do. The window the chief
    /// has with its specs on, as its `pane.open` says it, once it is idle; the same
    /// each time it is asked for.
    pub fn with_specs_on(&self) -> Result<Value> {
        if let Some(frame) = self.on.borrow().as_ref() {
            return Ok(frame.clone());
        }
        let id = self.project.id();
        let said = self.rig.page("project.specs", self.settings.request(id))?;
        if said["ok"] != true {
            return Err(Error::Daemon(format!("project.specs was refused: {said}")));
        }
        if said["project"]["specs"] != self.settings.view() {
            return Err(Error::Daemon(format!(
                "project.specs answered that the project's specs are {}, not {}",
                said["project"]["specs"],
                self.settings.view()
            )));
        }
        self.log.say(format!(
            "specs are on for the project: {}",
            self.settings.view()
        ));
        let windows = self.chief_windows().len();
        let chief = format!("p{id}-chief");
        for op in ["project.close", "project.resume"] {
            let done = self.rig.page(op, json!({ "project": id }))?;
            if done["ok"] != true {
                return Err(Error::Daemon(format!("{op} was refused: {done}")));
            }
            if op == "project.close" {
                self.wait("the chief's window to end", ENDS, || {
                    let ended = self
                        .rig
                        .exits()
                        .iter()
                        .filter(|exit| exit["id"] == chief.as_str())
                        .count();
                    Ok(ended >= windows)
                })?;
            }
        }
        self.wait("the chief's next window to be opened", OPENS, || {
            Ok(self.chief_windows().len() > windows)
        })?;
        let frame = self.chief_windows().pop().ok_or_else(|| {
            Error::Daemon("the chief's next window was opened, and is gone".to_owned())
        })?;
        // The board's lane still reads the window that ended until it shows this one
        // (each window has a generation of its own): not idle before it does.
        self.wait("the board to show the chief's next window", OPENS, || {
            Ok(self.human.chief()?["pane"]["generation"] == frame["generation"])
        })?;
        self.settles()?;
        *self.on.borrow_mut() = Some(frame.clone());
        Ok(frame)
    }
}
