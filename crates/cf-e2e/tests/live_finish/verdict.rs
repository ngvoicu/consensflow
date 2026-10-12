//! What the run saw of the worker's task while it waited for it, and what it holds the
//! task to once it has: judged from what the board said of the worker's window at each
//! look, what the pane host listed of its tree, and what the chief was given. Every way
//! the task fell short is said together, so that a run that fails says all that was
//! wrong with it: what the daemon held the window for, which processes the host listed
//! while it did, and for how long. (The task is T-1: the project is the run's own, and
//! its chief gives the worker the one task. A time said of what the board or the host
//! showed is on the run's clock, the one the lines the run says as it goes begin with, so
//! that a worker run after others has their time before it; how long a task took is the
//! time from the first look to the last.)

use std::time::Duration;

use cf_e2e::live::held::{is_hold, Reasons};
use cf_e2e::live::members::plural;
use cf_e2e::live::secs;
use cf_e2e::live::tree::{Read, Tree};
use serde_json::Value;

/// What the pane host listed at one look: when, what the window was doing then, whether
/// its turn had ended, and the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The look, as the time since the run began.
    pub at: Duration,
    /// What the board said of the window: its state (`working`, `idle`), or `held` where
    /// it said the window was held for what its turn left.
    pub window: String,
    /// Whether the turn had ended: the window read idle, or was held for what it left.
    pub resting: bool,
    pub tree: Tree,
}

/// A process the host listed while the board said the window was held: from when to
/// when, and at how many looks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldProcess {
    pub pid: u64,
    pub command: String,
    pub first: Duration,
    pub last: Duration,
    pub looks: usize,
}

impl HeldProcess {
    /// The process, its command line whole, and when it was listed.
    fn line(&self) -> String {
        format!(
            "{} {} (listed from {} s to {} s, at {})",
            self.pid,
            self.command,
            secs(self.first),
            secs(self.last),
            plural(self.looks, "look")
        )
    }
}

/// What the run saw of the worker's task while it waited for it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// The state the task was last seen in.
    pub state: String,
    /// When the first look was made, and when the last was, each as the time since the
    /// run began.
    pub began: Option<Duration>,
    pub until: Duration,
    /// What the board said the worker's window was doing besides working, each word
    /// once with when: a window held for what its turn left says so, and which.
    pub reasons: Reasons,
    /// What the host listed at each look the board said the window was held.
    pub held: Vec<Listed>,
    /// What the host listed at the turn's end: at the first look that found the window at
    /// rest (idle, or held for what its turn left), and where none has, at the last look
    /// there was.
    pub end: Option<Listed>,
    /// Why the host could not be read, at the last look it could not be.
    pub unlisted: Option<String>,
    /// The id of the worker's window, once the board had one for it.
    pub pane: Option<String>,
    /// What the host said the window left running when it closed, if it has.
    pub exit: Option<Tree>,
    /// The result the chief was given, if it was.
    pub result: Option<String>,
}

impl Seen {
    /// One look at the worker, made `at` into the run: the task's `state`, the worker's
    /// `lane` of the board (what its window is doing, and why), and what the pane host
    /// listed of its tree (none where the window had no pane to ask about).
    pub fn look(&mut self, at: Duration, state: &str, lane: &Value, listing: Option<Read>) {
        self.state = state.to_owned();
        self.began.get_or_insert(at);
        self.until = at;
        let activity = &lane["activity"];
        let window = activity["state"].as_str().unwrap_or("unknown");
        let reason = activity["reason"].as_str().unwrap_or_default();
        self.reasons.note(at, reason);
        if let Some(pane) = lane["pane"]["id"].as_str() {
            self.pane = Some(pane.to_owned());
        }
        match listing {
            Some(Ok(tree)) => {
                let held = is_hold(reason);
                let listed = Listed {
                    at,
                    window: if held { "held" } else { window }.to_owned(),
                    resting: held || window == "idle",
                    tree,
                };
                if held {
                    self.held.push(listed.clone());
                }
                if !self.end.as_ref().is_some_and(|end| end.resting) {
                    self.end = Some(listed);
                }
            }
            Some(Err(said)) => self.unlisted = Some(said),
            None => {}
        }
    }

    /// How long the worker was watched, from the first look to the last: how long its
    /// task took where it ended.
    pub fn took(&self) -> Duration {
        self.until - self.began.unwrap_or(self.until)
    }

    /// Every process the host listed at the looks the board said the window was held,
    /// once each, in the order it first listed them.
    pub fn held_processes(&self) -> Vec<HeldProcess> {
        let mut found: Vec<HeldProcess> = Vec::new();
        for listed in &self.held {
            for process in &listed.tree.processes {
                let same = |known: &&mut HeldProcess| {
                    known.pid == process.pid && known.command == process.command
                };
                match found.iter_mut().find(same) {
                    Some(known) => {
                        known.last = listed.at;
                        known.looks += 1;
                    }
                    None => found.push(HeldProcess {
                        pid: process.pid,
                        command: process.command.clone(),
                        first: listed.at,
                        last: listed.at,
                        looks: 1,
                    }),
                }
            }
        }
        found
    }

    /// How long the board said the window was held, as a phrase: from when to when, and
    /// whether it still was when the run stopped looking. None where it never said so.
    fn held_for(&self) -> Option<String> {
        let (from, to) = self.reasons.held_between()?;
        Some(format!(
            "from {} s to {} s of the run, which is {} s{}",
            secs(from),
            secs(to),
            secs(to - from),
            if to >= self.until {
                ", and it still was at the last look"
            } else {
                ""
            }
        ))
    }
}

/// `text` on one line, as a worker may break what it says.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether the worker's `result` holds the `output` of its command, in whichever case
/// and spacing it wrote it (a Windows machine's name is in capitals, and a reply may
/// put it between marks).
fn says(result: &str, output: &str) -> bool {
    one_line(result)
        .to_lowercase()
        .contains(&one_line(output).to_lowercase())
}

/// The task is done, its result reached the chief and holds the `output` of the
/// worker's command, and the board never said the window was held for what its turn
/// left: or every way it was not, the hold said with the words of the board, the time
/// it lasted, and each process the host listed while it did.
pub fn judge(seen: &Seen, output: &str) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    // The hold comes first: it is the finding, and what a table of runs says of a run.
    if let (Some(held), Some(phrase)) = (seen.reasons.held(), seen.held_for()) {
        problems.push(format!(
            "the board said the worker's window was held for work its turn left, {phrase}: {held}"
        ));
        problems.push(listing_of_the_hold(seen));
    }
    if seen.state != "done" {
        problems.push(format!(
            "T-1 is {}, not done after {} s",
            seen.state,
            secs(seen.took())
        ));
    }
    match &seen.result {
        None => problems.push("the chief was given no result".to_owned()),
        Some(body) if !says(body, output) => problems.push(format!(
            "the result does not hold what the command printed ({output}): {body}"
        )),
        Some(_) => {}
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// What the host listed while the board said the window was held, a process to a
/// line with its whole command line: or that it listed none.
fn listing_of_the_hold(seen: &Seen) -> String {
    let processes = seen.held_processes();
    if processes.is_empty() {
        return "the pane host listed no process of the window's tree at the looks the board said so"
            .to_owned();
    }
    let counted = seen
        .held
        .iter()
        .map(|listed| listed.tree.total)
        .max()
        .unwrap_or_default();
    let lines: Vec<String> = processes
        .iter()
        .map(|process| format!("    {}", process.line()))
        .collect();
    format!(
        "what the pane host listed as the turn's work while the window was held (it counted {counted} at the look it counted most):\n{}",
        lines.join("\n")
    )
}

/// `count` processes, as it is said: `no process`, `1 process`, `3 processes`.
fn processes(count: usize) -> String {
    match count {
        0 => "no process".to_owned(),
        1 => "1 process".to_owned(),
        count => format!("{count} processes"),
    }
}

/// The run in a line, for the table that ends it: what held where the worker passed,
/// and else the first thing that was wrong with it.
pub fn summary(seen: &Seen, verdict: &Result<(), Vec<String>>) -> String {
    match verdict {
        Ok(()) => format!(
            "T-1 done after {} s with nothing held back; the host listed {}",
            secs(seen.took()),
            seen.end.as_ref().map_or_else(
                || "nothing".to_owned(),
                |end| format!(
                    "{} at {}",
                    processes(end.tree.total),
                    if end.resting {
                        "the turn's end"
                    } else {
                        "its last look"
                    }
                )
            )
        ),
        Err(problems) => problems.first().cloned().unwrap_or_default(),
    }
}

/// What the pane host listed as the turn's work at the turn's end, whichever way the
/// run came out: none is the answer of a worker that left nothing, and on Windows,
/// where the host tells a turn's work by when each process began, what it lists is the
/// tree a pass shows too.
pub fn at_the_turns_end(seen: &Seen) -> String {
    match (&seen.end, &seen.unlisted) {
        (Some(end), _) => format!(
            "what the pane host listed as the turn's work {} (at {} s, the window {}):{}",
            if end.resting {
                "at the turn's end"
            } else {
                "at the last look, no look having found the turn at its end"
            },
            secs(end.at),
            end.window,
            after_colon(&end.tree)
        ),
        (None, Some(said)) => {
            format!("the pane host could not say what the turn left at its end: {said}")
        }
        (None, None) => {
            "the pane host was never asked what the turn left: the worker's window was never seen"
                .to_owned()
        }
    }
}

/// What the pane host said the window left running when it closed (its `pane.exit`),
/// whichever way the run came out: the processes of its tree that began after the turn
/// did, taken as the close began. It is the host's own count of what the turn left, with
/// no look in between that may have missed it.
pub fn at_the_close(seen: &Seen) -> String {
    match &seen.exit {
        Some(left) => format!(
            "what the pane host said the window left running when it closed (its pane.exit):{}",
            after_colon(left)
        ),
        None => "the pane host said nothing of what the window left when it closed: it had not closed when the run stopped looking".to_owned(),
    }
}

/// A tree as it follows a colon: `none` on the same line where it has no process, and
/// else a line to each of its lines, indented.
fn after_colon(tree: &Tree) -> String {
    if tree.processes.is_empty() {
        return " none".to_owned();
    }
    let lines: Vec<String> = tree
        .block()
        .lines()
        .map(|line| format!("    {line}"))
        .collect();
    format!("\n{}", lines.join("\n"))
}

// The tests are in `verdict/`: a module that a path names looks for its own parts
// beside it, so these are named as well.
#[cfg(test)]
#[path = "verdict/tests.rs"]
mod tests;
