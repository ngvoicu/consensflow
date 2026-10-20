//! What a run keeps of a window that stalls: one whose lane the board reads working or
//! starting for longer than the idle limit (`--idle-min`), as the Mac's Claude chief read
//! working for good after the owner's `/compact`, and zeewin's Codex chief starting for
//! good once the app had started again (the 2-hour live runs of 2026-10-19, neither of
//! which kept a screen of it); and of every window, where the project did not reach its
//! end (9). A stall is no break: there are no time limits on work, and a chief that plans
//! for long is no fault. It is evidence: the window's last screen while it read so, the
//! trace's last changes of its state, and the ledger's last events, kept in the local
//! report; and the ledger itself, copied beside the report before the home goes (zeewin's
//! went, ledger and all).
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

use super::invariants::iso;
use super::ledger::Ledger;
use super::look::Look;
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
                    "@{handle}'s window has read {state} for {} min, since {}: its screen and the ledger's last events are kept",
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
            });
        }
        told
    }

    /// The windows of `last`, the last look before the app closed: kept for the end.
    pub fn before_close(&mut self, last: Option<&Look>) {
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

#[cfg(test)]
mod tests;
