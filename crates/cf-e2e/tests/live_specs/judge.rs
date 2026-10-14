//! What the run judges the chief by, from what it wrote and what the daemon
//! shows, never from what it says: the spec files in the project's folder (the
//! markdown variants' `SPEC.md`, the HTML variants' `SPEC.html`), the registry,
//! and the tasks on the board with what each needs. Each is a function of text,
//! and held here to fixtures of what Specmint writes. (What the chief's window
//! was launched with is [`crate::launched`]'s.)

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use serde_json::Value;

/// What a task's code looks like: `HELLO-01`, and the tdd variants' `TEST-HELLO-01`
/// and `IMPL-HELLO-02`.
const CODE: &str = r"[A-Z][A-Z0-9]*(?:-[A-Z][A-Z0-9]*)*-\d+";

/// A task of a spec, by its code (`HELLO-01`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecTask {
    pub code: String,
    pub done: bool,
}

/// A phase of a spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase {
    pub number: u32,
    pub tasks: Vec<SpecTask>,
}

/// What a spec file says of its work: its id and its phases, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub id: String,
    pub phases: Vec<Phase>,
}

impl Spec {
    /// Every task of every phase.
    pub fn tasks(&self) -> impl Iterator<Item = &SpecTask> {
        self.phases.iter().flat_map(|phase| phase.tasks.iter())
    }

    /// How many tasks are done, of how many.
    pub fn progress(&self) -> (usize, usize) {
        let all: Vec<&SpecTask> = self.tasks().collect();
        (all.iter().filter(|task| task.done).count(), all.len())
    }
}

/// The spec a markdown file holds (`SPEC.md`): the `id` of its frontmatter,
/// the phases `## Phase N: … [status]` (or `### Phase N`, as a Codex chief wrote
/// them under a `## Phases` heading, 2026-10-14) and the tasks `- [ ] [CODE-01]` or
/// `- [x] [CODE-01]` under each.
pub fn markdown(text: &str) -> Result<Spec, String> {
    let id = Regex::new(r"(?m)^id:[ \t]*(\S+)[ \t]*$")
        .map_err(|cause| cause.to_string())?
        .captures(text)
        .map(|found| found[1].trim_matches(['"', '\'']).to_owned())
        .ok_or("the spec has no id in its frontmatter")?;
    let phase = Regex::new(r"^#{2,3}\s+Phase\s+(\d+)\b").map_err(|cause| cause.to_string())?;
    let task = Regex::new(&format!(r"^\s*-\s*\[([ xX])\]\s*\[({CODE})\]"))
        .map_err(|cause| cause.to_string())?;
    let mut phases: Vec<Phase> = Vec::new();
    for line in text.lines() {
        if let Some(found) = phase.captures(line) {
            phases.push(Phase {
                number: found[1].parse().unwrap_or(0),
                tasks: Vec::new(),
            });
        } else if let (Some(found), Some(current)) = (task.captures(line), phases.last_mut()) {
            current.tasks.push(SpecTask {
                code: found[2].to_owned(),
                done: &found[1] != " ",
            });
        }
    }
    Ok(Spec { id, phases })
}

/// The spec an HTML file holds (`SPEC.html`): the `id` of its `spec-meta`, each
/// `<details class="phase" data-phase="N">` and each `<li class="task"
/// data-task="CODE" data-status="…">` under it (`completed` is done).
pub fn html(text: &str) -> Result<Spec, String> {
    let meta = Regex::new(r#"(?s)<script[^>]*id="spec-meta"[^>]*>(.*?)</script>"#)
        .map_err(|cause| cause.to_string())?
        .captures(text)
        .ok_or("the spec has no spec-meta")?;
    let meta: Value = serde_json::from_str(meta[1].trim())
        .map_err(|cause| format!("the spec's meta is no JSON: {cause}"))?;
    let id = meta["id"]
        .as_str()
        .ok_or("the spec's meta has no id")?
        .to_owned();
    let element = Regex::new(r"<(details|li)\b[^>]*>").map_err(|cause| cause.to_string())?;
    let attribute = |tag: &str, name: &str| {
        Regex::new(&format!(r#"\b{name}="([^"]*)""#))
            .ok()?
            .captures(tag)
            .map(|found| found[1].to_owned())
    };
    let mut phases: Vec<Phase> = Vec::new();
    for found in element.find_iter(text) {
        let tag = found.as_str();
        if tag.starts_with("<details") && tag.contains("phase") {
            if let Some(number) = attribute(tag, "data-phase") {
                phases.push(Phase {
                    number: number.parse().unwrap_or(0),
                    tasks: Vec::new(),
                });
            }
        } else if let (Some(code), Some(current)) = (attribute(tag, "data-task"), phases.last_mut())
        {
            current.tasks.push(SpecTask {
                code,
                done: attribute(tag, "data-status").as_deref() == Some("completed"),
            });
        }
    }
    Ok(Spec { id, phases })
}

/// The spec in the file `name` of the specs folder, by its name: `SPEC.md` is
/// markdown and `SPEC.html` is HTML.
pub fn read(name: &str, text: &str) -> Result<Spec, String> {
    if name.ends_with(".html") {
        html(text)
    } else {
        markdown(text)
    }
}

/// That the spec is in the shape the variant gives: the HTML variants write
/// `SPEC.html` and the others `SPEC.md`, and the tdd variants' tasks are tests
/// and their implementations (`TEST-…`, `IMPL-…`). A chief that used another
/// variant's skill, one that triggers on the same words, shows it here.
pub fn in_the_shape_of(variant: &str, file: &str, spec: &Spec) -> Result<(), String> {
    let wanted = if variant.ends_with("-html") {
        "SPEC.html"
    } else {
        "SPEC.md"
    };
    if file != wanted {
        return Err(format!(
            "{variant} writes {wanted}, and the chief wrote {file}"
        ));
    }
    let kind = |prefix: &str| spec.tasks().any(|task| task.code.starts_with(prefix));
    if variant.starts_with("tdd") && !(kind("TEST-") && kind("IMPL-")) {
        return Err(format!(
            "{variant} has a test and an implementation for each piece of work (TEST-…, IMPL-…), and the spec's tasks are {}",
            spec.tasks()
                .map(|task| task.code.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(())
}

/// What the registry says of one spec: its status, and how many tasks are done of
/// how many.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub status: String,
    pub done: usize,
    pub total: usize,
}

/// The row of the registry (`registry.md`) of the spec `id`: the table's
/// columns are `ID | Title | Status | Priority | Progress | Updated`.
pub fn registered(text: &str, id: &str) -> Option<Registered> {
    let progress = Regex::new(r"(\d+)\s*/\s*(\d+)").ok()?;
    text.lines().find_map(|line| {
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.first() != Some(&id) || cells.len() < 5 {
            return None;
        }
        let shown = progress.captures(cells[4])?;
        Some(Registered {
            status: cells[2].to_owned(),
            done: shown[1].parse().ok()?,
            total: shown[2].parse().ok()?,
        })
    })
}

/// A task as the board shows it: its card (the number, the state, who asked for
/// it, its title and the tasks it needs) and its brief, which the card leaves to
/// the task's drawer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardTask {
    pub number: i64,
    pub state: String,
    pub requester: String,
    pub title: String,
    pub body: String,
    pub needs: Vec<i64>,
}

impl BoardTask {
    /// Whether the chief's text of the task (its title and its brief) names the
    /// spec's task `code`, as its own word.
    fn names(&self, code: &str) -> bool {
        Regex::new(&format!(r"\b{}\b", regex::escape(code)))
            .is_ok_and(|word| word.is_match(&self.title) || word.is_match(&self.body))
    }
}

/// The tasks of a board (the ones no lane has and each lane's), by number, as the
/// page draws their cards; `brief_of` says a task's brief, which is read with the
/// task and not with the board.
pub fn board_tasks(board: &Value, brief_of: impl Fn(i64) -> String) -> Vec<BoardTask> {
    let lanes = board["lanes"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|lane| lane["tasks"].as_array().into_iter().flatten());
    let open = board["open"].as_array().into_iter().flatten();
    let mut seen: BTreeMap<i64, BoardTask> = BTreeMap::new();
    for card in open.chain(lanes) {
        let Some(number) = card["number"].as_i64() else {
            continue;
        };
        seen.insert(
            number,
            BoardTask {
                number,
                state: text(&card["state"]),
                requester: text(&card["requester"]),
                title: text(&card["title"]),
                body: brief_of(number),
                needs: card["needs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|need| need["number"].as_i64())
                    .collect(),
            },
        );
    }
    seen.into_values().collect()
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// The board tasks the chief made from the spec: each by the spec task its text
/// names, with the phase that task is of, in the order of the spec. A task that was
/// called off is none of them: the chief may have made it again. (A brief that
/// names an item of another phase besides its own is a task of that phase too, and
/// so fails the order a chief gave it: the section of specs has each brief name the
/// item it comes from, and one only.)
pub fn from_the_spec<'a>(spec: &Spec, tasks: &'a [BoardTask]) -> Vec<(u32, &'a BoardTask)> {
    let mut made: Vec<(u32, &BoardTask)> = Vec::new();
    for phase in &spec.phases {
        for task in &phase.tasks {
            for found in tasks.iter().filter(|candidate| {
                candidate.requester == "chief"
                    && candidate.state != "cancelled"
                    && candidate.names(&task.code)
            }) {
                if !made
                    .iter()
                    .any(|(number, task)| *number == phase.number && task.number == found.number)
                {
                    made.push((phase.number, found));
                }
            }
        }
    }
    made
}

/// The numbers of the tasks `number` needs, directly or through the tasks they need.
fn upstream(number: i64, tasks: &[BoardTask]) -> BTreeSet<i64> {
    let mut found = BTreeSet::new();
    let mut open = vec![number];
    while let Some(next) = open.pop() {
        let needs = tasks
            .iter()
            .find(|task| task.number == next)
            .map_or(&[][..], |task| task.needs.as_slice());
        for need in needs {
            if found.insert(*need) {
                open.push(*need);
            }
        }
    }
    found
}

/// That the tasks the chief made from the spec follow its phases: every phase has
/// one, a task of a phase after the first needs (directly or through other tasks)
/// a task of the phase just before it, and none needs a task of a later phase. A
/// need on a task that is none of the spec's, a research task or the like, is
/// neither. What the run saw, in a line, or each way it fell short.
pub fn follow_the_phases(spec: &Spec, tasks: &[BoardTask]) -> Result<String, String> {
    if spec.phases.is_empty() {
        return Err("the spec has no phases to follow".to_owned());
    }
    let made = from_the_spec(spec, tasks);
    let order: Vec<u32> = spec.phases.iter().map(|phase| phase.number).collect();
    let mut problems = Vec::new();
    for phase in &spec.phases {
        if !made.iter().any(|(number, _)| *number == phase.number) {
            let codes: Vec<&str> = phase.tasks.iter().map(|task| task.code.as_str()).collect();
            problems.push(format!(
                "no task of the chief's names a task of phase {} ({})",
                phase.number,
                codes.join(", ")
            ));
        }
    }
    for (phase, task) in &made {
        let at = order.iter().position(|number| number == phase).unwrap_or(0);
        let needed = upstream(task.number, tasks);
        let of_phase = |wanted: u32| {
            made.iter()
                .filter(move |(number, _)| *number == wanted)
                .map(|(_, task)| task.number)
        };
        for later in &order[at + 1..] {
            for number in of_phase(*later).filter(|number| needed.contains(number)) {
                problems.push(format!(
                    "T-{} of phase {phase} needs T-{number}, a task of the later phase {later}",
                    task.number
                ));
            }
        }
        if let Some(previous) = at.checked_sub(1).map(|at| order[at]) {
            if !of_phase(previous).any(|number| needed.contains(&number)) {
                problems.push(format!(
                    "T-{} of phase {phase} needs {:?}: none is a task of phase {previous}",
                    task.number, task.needs
                ));
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    Ok(made
        .iter()
        .map(|(phase, task)| format!("phase {phase}: T-{} needs {:?}", task.number, task.needs))
        .collect::<Vec<_>>()
        .join("; "))
}

/// That the spec shows the work the chief accepted: each task of the spec that an
/// accepted board task names is done in it, one at least is, and the registry's row
/// says what the spec does. What the run saw, in a line, or each way it fell short.
pub fn progress_shows(
    spec: &Spec,
    registry: Option<&Registered>,
    tasks: &[BoardTask],
) -> Result<String, String> {
    let accepted: Vec<&BoardTask> = tasks
        .iter()
        .filter(|task| task.requester == "chief" && task.state == "accepted")
        .collect();
    let owed: Vec<&SpecTask> = spec
        .tasks()
        .filter(|task| accepted.iter().any(|board| board.names(&task.code)))
        .collect();
    let mut problems = Vec::new();
    if owed.is_empty() {
        problems.push("no task of the spec was accepted on the board".to_owned());
    }
    let undone: Vec<&str> = owed
        .iter()
        .filter(|task| !task.done)
        .map(|task| task.code.as_str())
        .collect();
    if !undone.is_empty() {
        problems.push(format!(
            "the board accepted {}, which the spec still shows undone",
            undone.join(", ")
        ));
    }
    let (done, total) = spec.progress();
    match registry {
        None => problems.push(format!("the registry has no row for {}", spec.id)),
        Some(row) if (row.done, row.total) != (done, total) => problems.push(format!(
            "the registry says {}/{} of {} and the spec has {done}/{total}",
            row.done, row.total, spec.id
        )),
        Some(_) => {}
    }
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    Ok(format!(
        "{done} of {total} tasks done in the spec, and in the registry"
    ))
}

#[cfg(test)]
#[path = "judge/tests.rs"]
mod tests;
