//! What happened in a long run, in order: what the owner said, each task's change
//! of state, each window that opened, closed or exited, the project's own moments
//! the ledger logs (a wake-up call, the chief switched, the project closed or
//! resumed), each disruption, and each break when it was found. The tasks, the
//! windows and the log are told from one look to the next; the rest is said by the
//! run as it does it. The report keeps it as JSON and as a Markdown table.

use serde_json::{json, Value};

use super::ledger::Event;
use super::look::{ClosedBy, Look};
use super::mask::{cut, mask, Names};

/// One thing that happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moment {
    /// When, in milliseconds.
    pub at: i64,
    /// What kind of thing: `owner`, `task`, `window`, `disruption`, `break`,
    /// `unresolved` (what a check could not judge), `run`.
    pub kind: &'static str,
    pub text: String,
}

/// What happened, from the run's start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timeline {
    pub started: i64,
    pub moments: Vec<Moment>,
    /// The run's own names, every look's: shown where what looks like a token is
    /// hidden, here and wherever the run says something.
    pub names: Names,
}

impl Timeline {
    /// A timeline of a run that began at `started`.
    pub fn new(started: i64) -> Self {
        Self {
            started,
            moments: Vec::new(),
            names: Names::default(),
        }
    }

    /// Something of `kind` happened at `at`, said with what looks like a key or a
    /// token in it hidden, the run's own names shown.
    pub fn add(&mut self, at: i64, kind: &'static str, text: impl Into<String>) {
        self.moments.push(Moment {
            at,
            kind,
            text: mask(&text.into(), &self.names),
        });
    }

    /// `text` as the run says it: what looks like a key or a token hidden, its own
    /// names shown.
    pub fn masked(&self, text: &str) -> String {
        mask(text, &self.names)
    }

    /// What changed from the look `before` (none: the first) to `after`: each task's
    /// state, the windows the board shows open, and the windows that exited. The
    /// names `after` shows are the run's from then.
    pub fn changes(&mut self, before: Option<&Look>, after: &Look) {
        self.names.add(after);
        for task in after.ledger.tasks.iter().filter(|task| !task.deleted) {
            let was = before.and_then(|look| {
                look.ledger
                    .tasks
                    .iter()
                    .find(|old| old.id == task.id)
                    .map(|old| old.state.as_str())
            });
            if was == Some(task.state.as_str()) {
                continue;
            }
            let who = task
                .assignee
                .map(|id| format!(" @{}", after.ledger.handle(id)))
                .unwrap_or_default();
            let text = match was {
                Some(was) => format!("T-{} {was} → {}{who}", task.number, task.state),
                None => format!(
                    "T-{} {}{who}: {}",
                    task.number,
                    task.state,
                    cut(&task.title, 70, &self.names)
                ),
            };
            self.add(after.at, "task", text);
        }
        // A window opened again (a resumed task, a restart) keeps its id and has a
        // generation of its own.
        let open_before: Vec<(&str, i64)> = before
            .map(|look| {
                look.windows
                    .open
                    .iter()
                    .map(|window| (window.pane.as_str(), window.generation))
                    .collect()
            })
            .unwrap_or_default();
        for window in &after.windows.open {
            if !open_before.contains(&(window.pane.as_str(), window.generation)) {
                self.add(
                    after.at,
                    "window",
                    format!("@{}'s window {} opened", window.handle, window.pane),
                );
            }
        }
        let logged_before = before
            .and_then(|look| look.ledger.events.last())
            .map_or(0, |event| event.id);
        for event in after
            .ledger
            .events
            .iter()
            .filter(|event| event.id > logged_before)
        {
            if let Some(text) = logged(event) {
                let at = crate::clock::millis(&event.at).unwrap_or(after.at);
                self.add(at, "project", text);
            }
        }
        let exits_before = before.map_or(0, |look| look.windows.exits.len());
        for exit in after.windows.exits.iter().skip(exits_before) {
            let by = match exit.closed_by {
                ClosedBy::Daemon => "closed by ConsensFlow",
                ClosedBy::Run => "killed by the run",
                ClosedBy::Itself => "ended by itself",
            };
            let how = match (&exit.signal, exit.code) {
                (Some(signal), _) => format!(", {signal}"),
                (None, Some(code)) => format!(", code {code}"),
                (None, None) => String::new(),
            };
            self.add(
                exit.at,
                "window",
                format!("@{}'s window {} exited ({by}{how})", exit.handle, exit.pane),
            );
        }
    }

    /// `at` as a point of the run: `+1h02m03s`.
    pub fn offset(&self, at: i64) -> String {
        let seconds = (at - self.started).max(0) / 1000;
        let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
        if hours > 0 {
            format!("+{hours}h{minutes:02}m{seconds:02}s")
        } else {
            format!("+{minutes}m{seconds:02}s")
        }
    }

    /// The moments in the order they happened: a look is told when it was taken, and
    /// a disruption that takes minutes is said after the look before it.
    fn in_order(&self) -> Vec<&Moment> {
        let mut moments: Vec<&Moment> = self.moments.iter().collect();
        moments.sort_by_key(|moment| moment.at);
        moments
    }

    /// The timeline as a report keeps it.
    pub fn to_value(&self) -> Value {
        Value::Array(
            self.in_order()
                .into_iter()
                .map(|moment| {
                    json!({
                        "at": super::invariants::iso(moment.at),
                        "offset": self.offset(moment.at),
                        "kind": moment.kind,
                        "text": moment.text,
                    })
                })
                .collect(),
        )
    }

    /// The timeline as a Markdown table.
    pub fn markdown(&self) -> String {
        let mut table = String::from("| when | what | |\n|---|---|---|\n");
        for moment in self.in_order() {
            table.push_str(&format!(
                "| {} | {} | {} |\n",
                self.offset(moment.at),
                moment.kind,
                moment.text.replace('|', "\\|").replace('\n', " ")
            ));
        }
        table
    }
}

/// What an entry of the ledger's log says of the project, for the timeline: a
/// wake-up call, or one held back while work was on its way, the chief
/// switched, the project closed or resumed; none for the rest, which the tasks
/// and the windows say.
fn logged(event: &Event) -> Option<String> {
    let data = &event.data;
    let text = |value: &Value| value.as_str().unwrap_or("?").to_owned();
    match event.kind.as_str() {
        "project.wake_up" => Some(format!(
            "a wake-up call nudged the chief (m-{})",
            data["message"]
        )),
        "project.wake_up_skipped" => Some(format!(
            "a wake-up call was held back while work was on its way ({})",
            data["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|task| format!("T-{task}"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        "chief.switched" => Some(format!(
            "the chief is now {} ({}){}",
            text(&data["to"]["harness"]),
            text(&data["to"]["agent"]),
            if data["cut"] == true {
                ", its turn cut"
            } else {
                ""
            }
        )),
        "project.state" => Some(format!("the project is {}", text(&data["to"]))),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
