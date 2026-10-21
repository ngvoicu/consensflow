//! What a run keeps of a window that stalls: one whose lane the board reads working or
//! starting for longer than the idle limit (`--idle-min`), as the Mac's Claude chief read
//! working for good after the owner's `/compact`, and zeewin's Codex chief starting for
//! good once the app had started again (the 2-hour live runs of 2026-10-19, neither of
//! which kept a screen of it); and of every window, where the project did not reach its
//! end (9). A stall is no break: there are no time limits on work, and a chief that plans
//! for long is no fault. It is evidence: the window's last screen while it read so, the
//! trace's last changes of its state, the ledger's last events, and what the system says
//! of its window's processes when it is found and, still stalled, at the run's end (each
//! process's state, the call its program waits in, the terminal's unread bytes: [`held`]),
//! kept in the local report; and the ledger itself, copied beside the report before the
//! home goes (zeewin's went, ledger and all).
//!
//! How long a lane has read a state is the trace's word where its last change of that
//! window is to the state (from the first of the changes in a row that kept it, whatever
//! their reasons), as every change of a window's state is traced; else the board's, from
//! the first look in a row that read it so (a window stuck as it opened may have no
//! change to trace). A look between turns may miss a moment at rest, which the trace
//! does not.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use self::held::{Listing, Prober, Taken, When};
use super::invariants::iso;
use super::ledger::Ledger;
use super::look::{Look, OpenWindow};
use super::mask::{cut, screen, Names};
use super::timeline::Timeline;

/// The states a lane stalls in.
pub const STALLED: [&str; 2] = ["working", "starting"];

/// How many of the trace's changes of a window's state, and of the ledger's events, the
/// evidence keeps: its last.
const CHANGES: usize = 6;
const EVENTS: usize = 30;

/// A window whose lane read a state for longer than the idle limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stall {
    pub handle: String,
    pub state: String,
    /// Since when it read so, in milliseconds.
    pub since: i64,
    /// The look that first found it had read so for longer than the limit.
    pub found: i64,
    /// The last look that read it so.
    pub last: i64,
    /// When a look first read it otherwise, and what it read then (`gone`: the board
    /// showed its lane no more); none while it stalls.
    pub cleared: Option<(i64, String)>,
    /// Its window's last screen while it read so, masked; none where the board showed
    /// it no window.
    pub screen: Vec<String>,
    /// The trace's last changes of its window's state by then, masked.
    pub changes: Vec<String>,
    /// The ledger's last events when it was found, masked.
    pub events: Vec<String>,
    /// What the system said of its window's processes when it was found and, where it
    /// still read so then, at the run's end.
    pub held: Vec<Held>,
}

/// What the system said of a stalled window's processes, masked ([`held::Taken`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub when: When,
    pub at: i64,
    /// A line for each process of its tree, or why there is none.
    pub processes: Result<Vec<String>, String>,
    /// The bytes its program wrote that the pane host has not read, or why not known.
    pub unread: Result<u64, String>,
    /// Where a sample of its program was kept, or why none was; none where the system has
    /// no sampler.
    pub sample: Option<Result<PathBuf, String>>,
    /// Where its program wrote bytes the pane host has not read, where a sample of the pane
    /// host was kept, or why none was.
    pub host: Option<Result<PathBuf, String>>,
}

/// What a take asks of the system once, for every window it takes: the list of every
/// process, asked when the first window needs it.
struct Asking<'p> {
    prober: &'p mut dyn Prober,
    listed: Option<Listing>,
}

impl<'p> Asking<'p> {
    fn new(prober: &'p mut dyn Prober) -> Self {
        Self {
            prober,
            listed: None,
        }
    }

    /// What the system says of the program `pid` and `others`, a sample kept as `label`.
    fn take(&mut self, pid: u32, others: &[u32], label: &str) -> Taken {
        let listed = self.listed.get_or_insert_with(|| self.prober.list());
        self.prober.take(listed, pid, others, label)
    }
}

impl Held {
    /// What the system said of the window `window` at `at`, as `asking` asked it, its
    /// samples kept as `label` (`chief-1-found`: the stall's handle, which of that handle's
    /// stalls it is, and when), masked.
    fn of(
        window: Option<&OpenWindow>,
        label: &str,
        when: When,
        at: i64,
        asking: &mut Asking<'_>,
        names: &Names,
    ) -> Self {
        let taken = match window.map(|window| (window.pid, window)) {
            None => Taken::none("the board showed it no window"),
            Some((None, _)) => Taken::none("the pane host named no process of it"),
            Some((Some(pid), window)) => {
                let others: Vec<u32> = window.background.iter().map(|one| one.pid).collect();
                asking.take(pid, &others, label)
            }
        };
        let masked = |why: String| cut(&why, 300, names);
        Self {
            when,
            at,
            processes: taken
                .processes
                .map(|lines| lines.iter().map(|line| cut(line, 300, names)).collect())
                .map_err(masked),
            unread: taken.unread.map_err(masked),
            sample: taken.sample.map(|kept| kept.map_err(masked)),
            host: taken.host.map(|kept| kept.map_err(masked)),
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "when": self.when.word(),
            "at": iso(self.at),
            "processes": match &self.processes {
                Ok(lines) => json!(lines),
                Err(why) => json!({ "unknown": why }),
            },
            "unread": match &self.unread {
                Ok(bytes) => json!({ "bytes": bytes }),
                Err(why) => json!({ "unknown": why }),
            },
            "sample": kept(self.sample.as_ref()),
            "host": kept(self.host.as_ref()),
        })
    }

    fn markdown(&self, timeline: &Timeline) -> String {
        let mut text = format!(
            "\nIts processes ({}, {}):\n\n```\n",
            self.when.word(),
            timeline.offset(self.at)
        );
        match &self.processes {
            Ok(lines) => text.push_str(&lines.join("\n")),
            Err(why) => text.push_str(&format!("not known: {why}")),
        }
        text.push_str("\n```\n");
        match &self.unread {
            Ok(bytes) => text.push_str(&format!("\nUnread by the pane host: {bytes} bytes.\n")),
            Err(why) => text.push_str(&format!("\nUnread by the pane host: not known ({why}).\n")),
        }
        match &self.sample {
            Some(Ok(file)) => {
                text.push_str(&format!("\nA sample of its program: {}\n", file.display()))
            }
            Some(Err(why)) => text.push_str(&format!("\nNo sample of its program: {why}\n")),
            None => {}
        }
        match &self.host {
            Some(Ok(file)) => text.push_str(&format!(
                "\nA sample of the pane host: {}\n",
                file.display()
            )),
            Some(Err(why)) => text.push_str(&format!("\nNo sample of the pane host: {why}\n")),
            None => {}
        }
        text
    }
}

/// A sample as the report keeps it: where it is, or why none was taken; nothing where none
/// was asked.
fn kept(sample: Option<&Result<PathBuf, String>>) -> Value {
    match sample {
        Some(Ok(file)) => json!(file),
        Some(Err(why)) => json!({ "notTaken": why }),
        None => Value::Null,
    }
}

/// A window of the last look before the app closed: whose, what its lane read, its last
/// screen and the trace's last changes of its state, masked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub handle: String,
    pub state: String,
    pub screen: Vec<String>,
    pub changes: Vec<String>,
}

/// What the run keeps of its stalls, and of its windows at an end it did not reach.
#[derive(Debug, Default)]
pub struct Stalls {
    /// What each lane read at the last look, by its participant's handle, and the first
    /// look in a row that read it so.
    reading: BTreeMap<String, (String, i64)>,
    /// Every stall, in the order found.
    pub kept: Vec<Stall>,
    /// The windows of the last look before the app closed.
    last: Vec<Window>,
    /// Those windows, where the project did not reach its end.
    pub at_the_end: Vec<Window>,
    /// The ledger's last events once the app had closed, where any of it is kept.
    pub events: Vec<String>,
    /// Where the ledger was copied to, or why it could not be.
    pub ledger: Option<Result<PathBuf, String>>,
}

impl Stalls {
    /// Takes `look`, with the limit `idle` (milliseconds): each lane that has read working
    /// or starting for longer is a stall, found once and its last screen kept at each look
    /// while it holds; one that reads otherwise is cleared. What to tell, each when.
    pub fn look(&mut self, look: &Look, idle: i64) -> Vec<(i64, String)> {
        let names = Names::of(look);
        let mut told = Vec::new();
        let lanes: BTreeMap<&str, &str> = look
            .windows
            .lanes
            .iter()
            .map(|lane| (lane.handle.as_str(), lane.state.as_str()))
            .collect();
        for stall in self.kept.iter_mut().filter(|stall| stall.cleared.is_none()) {
            let now = lanes.get(stall.handle.as_str()).copied();
            if now != Some(stall.state.as_str()) {
                let now = now.unwrap_or("gone").to_owned();
                told.push((
                    look.at,
                    format!(
                        "@{}'s window reads {now} after {} min {}",
                        stall.handle,
                        (look.at - stall.since) / 60_000,
                        stall.state
                    ),
                ));
                stall.cleared = Some((look.at, now));
            }
        }
        self.reading
            .retain(|handle, _| lanes.contains_key(handle.as_str()));
        for (handle, state) in lanes {
            let first = self
                .reading
                .entry(handle.to_owned())
                .and_modify(|(was, since)| {
                    if was != state {
                        (*was, *since) = (state.to_owned(), look.at);
                    }
                })
                .or_insert_with(|| (state.to_owned(), look.at))
                .1;
            if !STALLED.contains(&state) {
                continue;
            }
            let since = traced_since(look, handle, state).unwrap_or(first);
            if look.at - since <= idle {
                continue;
            }
            let shown = look
                .window_of(handle)
                .map(|window| screen(&window.tail, &names))
                .unwrap_or_default();
            if let Some(open) = self.kept.iter_mut().find(|stall| {
                stall.handle == handle && stall.state == state && stall.cleared.is_none()
            }) {
                open.last = look.at;
                open.screen = shown;
                open.changes = changes(look, handle, &names);
                continue;
            }
            told.push((
                look.at,
                format!(
                    "@{handle}'s window has read {state} for {} min, since {}: its screen, its processes and the ledger's last events are kept",
                    (look.at - since) / 60_000,
                    iso(since)
                ),
            ));
            self.kept.push(Stall {
                handle: handle.to_owned(),
                state: state.to_owned(),
                since,
                found: look.at,
                last: look.at,
                cleared: None,
                screen: shown,
                changes: changes(look, handle, &names),
                events: events(&look.ledger, &names),
                held: Vec::new(),
            });
        }
        told
    }

    /// What the system says, by `prober`, of the processes of each window `look` found
    /// stalled: once, when the stall is found.
    pub fn take(&mut self, look: &Look, prober: &mut dyn Prober) {
        let names = Names::of(look);
        let mut asking = Asking::new(prober);
        let labels = self.labels(When::Found);
        for (stall, label) in self
            .kept
            .iter_mut()
            .zip(labels)
            .filter(|(stall, _)| stall.cleared.is_none() && stall.held.is_empty())
        {
            let window = look.window_of(&stall.handle);
            let held = Held::of(window, &label, When::Found, look.at, &mut asking, &names);
            stall.held.push(held);
        }
    }

    /// The label each stall's samples are kept under `when`, in the order the stalls were
    /// found: its handle, which of that handle's stalls it is, and when (`chief-2-found`), so
    /// that a window that stalls again keeps the samples of each stall.
    fn labels(&self, when: When) -> Vec<String> {
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        self.kept
            .iter()
            .map(|stall| {
                let nth = seen.entry(stall.handle.as_str()).or_default();
                *nth += 1;
                format!("{}-{nth}-{}", stall.handle, when.word())
            })
            .collect()
    }

    /// What the system says, by `prober`, of the processes of each window that `last`, the
    /// last look before the app closes, still read stalled: once more, at the run's end.
    fn take_at_end(&mut self, last: Option<&Look>, prober: &mut dyn Prober) {
        let Some(look) = last else {
            return;
        };
        let names = Names::of(look);
        let mut asking = Asking::new(prober);
        let labels = self.labels(When::End);
        for (stall, label) in self
            .kept
            .iter_mut()
            .zip(labels)
            .filter(|(stall, _)| stall.cleared.is_none() && stall.last == look.at)
        {
            let window = look.window_of(&stall.handle);
            let held = Held::of(window, &label, When::End, look.at, &mut asking, &names);
            stall.held.push(held);
        }
    }

    /// The windows of `last`, the last look before the app closes, kept for the end; and of
    /// each that still read stalled, what the system says of its processes by `prober`,
    /// once more ([`Stalls::take_at_end`]).
    pub fn before_close(&mut self, last: Option<&Look>, prober: &mut dyn Prober) {
        self.take_at_end(last, prober);
        let Some(look) = last else {
            return;
        };
        let names = Names::of(look);
        self.last = look
            .windows
            .lanes
            .iter()
            .map(|lane| Window {
                handle: lane.handle.clone(),
                state: lane.state.clone(),
                screen: look
                    .window_of(&lane.handle)
                    .map(|window| screen(&window.tail, &names))
                    .unwrap_or_default(),
                changes: changes(look, &lane.handle, &names),
            })
            .collect();
    }

    /// Once the app has closed, its ledger read as `ledger` (or as the last look read it,
    /// where it could not be read): where the project did not reach its end (`unended`),
    /// the windows of the last look before the close are kept; where they are or a stall
    /// was found, so are the ledger's last events. Whether anything is kept, the ledger
    /// to be copied with it.
    pub fn after_close(&mut self, unended: bool, ledger: Option<&Ledger>) -> bool {
        if unended {
            self.at_the_end = std::mem::take(&mut self.last);
        }
        let keeps = unended || !self.kept.is_empty();
        if keeps {
            if let Some(ledger) = ledger {
                self.events = events(ledger, &Names::of_ledger(ledger));
            }
        }
        keeps
    }

    /// What the report keeps: none where nothing was.
    pub fn to_value(&self, timeline: &Timeline) -> Value {
        if self.kept.is_empty() && self.at_the_end.is_empty() {
            return Value::Null;
        }
        let window = |window: &Window| {
            json!({
                "handle": window.handle,
                "state": window.state,
                "screen": window.screen,
                "changes": window.changes,
            })
        };
        json!({
            "windows": self.kept.iter().map(|stall| json!({
                "handle": stall.handle,
                "state": stall.state,
                "since": iso(stall.since),
                "found": iso(stall.found),
                "foundAt": timeline.offset(stall.found),
                "last": iso(stall.last),
                "minutes": (stall.last - stall.since) / 60_000,
                "cleared": stall.cleared.as_ref().map(|(at, now)| json!({ "at": iso(*at), "to": now })),
                "screen": stall.screen,
                "changes": stall.changes,
                "events": stall.events,
                "held": stall.held.iter().map(Held::to_value).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "atTheEnd": self.at_the_end.iter().map(window).collect::<Vec<_>>(),
            "events": self.events,
            "ledger": match &self.ledger {
                Some(Ok(file)) => json!(file),
                Some(Err(why)) => json!({ "notCopied": why }),
                None => Value::Null,
            },
        })
    }

    /// What the report's Markdown keeps: none where nothing was.
    pub fn markdown(&self, timeline: &Timeline) -> String {
        if self.kept.is_empty() && self.at_the_end.is_empty() {
            return String::new();
        }
        let block = |title: &str, lines: &[String]| {
            if lines.is_empty() {
                String::new()
            } else {
                format!("\n{title}:\n\n```\n{}\n```\n", lines.join("\n"))
            }
        };
        let mut text = String::from("\n## Stalls\n");
        for stall in &self.kept {
            text.push_str(&format!(
                "\n### @{} · {} since {} ({}, {} min)\n",
                stall.handle,
                stall.state,
                iso(stall.since),
                timeline.offset(stall.found),
                (stall.last - stall.since) / 60_000
            ));
            if let Some((at, now)) = &stall.cleared {
                text.push_str(&format!("\nIt read {now} at {}.\n", timeline.offset(*at)));
            }
            text.push_str(&block("screen", &stall.screen));
            text.push_str(&block("changes", &stall.changes));
            text.push_str(&block("events", &stall.events));
            for held in &stall.held {
                text.push_str(&held.markdown(timeline));
            }
        }
        if !self.at_the_end.is_empty() {
            text.push_str("\n### The windows at the end\n");
            for window in &self.at_the_end {
                text.push_str(&format!("\n@{} · {}\n", window.handle, window.state));
                text.push_str(&block("screen", &window.screen));
                text.push_str(&block("changes", &window.changes));
            }
        }
        text.push_str(&block("the ledger's last events", &self.events));
        match &self.ledger {
            Some(Ok(file)) => text.push_str(&format!("\nThe ledger: {}\n", file.display())),
            Some(Err(why)) => text.push_str(&format!("\nThe ledger was not copied: {why}\n")),
            None => {}
        }
        text
    }
}

/// Since when the trace says `handle`'s window has read `state` by `look`: from the first
/// of its last changes in a row to that state; none where its last change is to another,
/// or it has none.
fn traced_since(look: &Look, handle: &str, state: &str) -> Option<i64> {
    let mut since = None;
    for change in look
        .windows
        .activity
        .iter()
        .rev()
        .filter(|change| change.handle == handle && change.at <= look.at)
    {
        if change.state != state {
            break;
        }
        since = Some(change.at);
    }
    since
}

/// The trace's last changes of `handle`'s window's state by `look`, masked.
fn changes(look: &Look, handle: &str, names: &Names) -> Vec<String> {
    let mut said: Vec<String> = look
        .windows
        .activity
        .iter()
        .rev()
        .filter(|change| change.handle == handle && change.at <= look.at)
        .take(CHANGES)
        .map(|change| {
            let reason = change
                .reason
                .as_deref()
                .map(|reason| format!(" ({})", cut(reason, 200, names)))
                .unwrap_or_default();
            format!("{} {}{reason}", iso(change.at), change.state)
        })
        .collect();
    said.reverse();
    said
}

/// The ledger's last events, masked whole and then cut.
fn events(ledger: &Ledger, names: &Names) -> Vec<String> {
    let mut said: Vec<String> = ledger
        .events
        .iter()
        .rev()
        .take(EVENTS)
        .map(|event| {
            cut(
                &format!("e-{} {} {} {}", event.id, event.at, event.kind, event.data),
                300,
                names,
            )
        })
        .collect();
    said.reverse();
    said
}

pub mod held;
#[cfg(test)]
mod tests;
