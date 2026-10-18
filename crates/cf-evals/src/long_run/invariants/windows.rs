//! The windows (6, B2; 8, B1). Every window the board shows open has its process
//! and the pane host answers for it; no window closes by itself while its task is
//! at work (T-18 on Windows: paused, "window closed"); and no process a window's
//! tree ran is left once its window is gone (a stranger). No tool call comes back
//! "User skipped this tool call", Devin's own words from its permission dialog
//! (T-18 again: every write, shell call and `cf ask` after a restart), and no
//! dialog waits in a window unanswered: ConsensFlow opens every window in full
//! permission, and the owner's script answers the chief's own question dialog.
//!
//! The words are read whatever their case, where a tool's call comes back (a tool's
//! item of the copy) and on a screen line that says them alone (a box's edges and
//! marks aside): a person or an agent who quotes them in a sentence skipped nothing,
//! nor does a line that quotes them (Markdown's `>`, or the words in quotes). A dialog
//! is timed while its window is open: a window closed (which the daemon's trace says
//! by no change of state) answers nothing, and waits for nothing.
//!
//! A process left after its window went is said, and left running: the run ends
//! only what it started itself. One the system could not be asked of, or whose
//! identity was not taken while its window showed it, is unresolved: a break of its
//! own, for the watch could not hold the run to the sixth. So is a close of the app
//! whose pane host's last word was not read to its end: an exit it told after, and the
//! processes that exit names, the watch never had.
//!
//! What the watch could not see of an open window's tree is a gap ([`gaps`]): a tree
//! the pane host could not say at two looks in a row, and the processes past the
//! first the host names (it names at most 16, and says how many there are besides),
//! which the watch does not follow. A gap is no break, and no run with one passes.

use super::work::item_line;
use super::{iso, Break, Evidence, Settings, Unjudged};
use crate::clock;
use crate::long_run::look::{ClosedBy, Look};
use crate::long_run::mask::{cut, Names};

/// What Devin's dialog says of a call it skipped.
pub const SKIPPED: &str = "User skipped this tool call";

/// The marks a quotation opens or closes with.
const QUOTES: [char; 9] = ['"', '\'', '`', '“', '”', '‘', '’', '«', '»'];

/// Whether `text` holds Devin's words of a call it skipped, in any case.
fn holds_skipped(text: &str) -> bool {
    text.to_lowercase().contains(&SKIPPED.to_lowercase())
}

/// Whether a line of a screen says Devin's words of a skipped call and nothing
/// else: what is around them is a box's edges, a mark, a stop, but no quote (a
/// Markdown `>` before them, or quotation marks about them).
fn says_skipped(line: &str) -> bool {
    let decoration = |c: char| !c.is_alphanumeric() && c != '>' && !QUOTES.contains(&c);
    let words = line
        .trim_start_matches(decoration)
        .trim_end_matches(decoration);
    words.eq_ignore_ascii_case(SKIPPED)
}

/// The states a task is at work in, whose window must not close by itself.
const AT_WORK: [&str; 2] = ["working", "waiting"];

/// What the log says moved a task to the state its `to` names: a move of its own, an
/// assignment (to `queued`) and a release (to `open`).
const MOVES: [&str; 3] = ["task.state", "task.assigned", "task.released"];

/// 6: an open window with no process or no host, a window that closed by itself
/// mid-task, a stranger, and what the watch could not judge of the run's processes.
pub fn processes(look: &Look) -> Vec<Break> {
    let names = Names::of(look);
    let mut found = Vec::new();
    for window in &look.windows.open {
        let gone = !window.hosted || (window.pid.is_some() && !window.running);
        if !gone {
            continue;
        }
        let said = if window.hosted {
            format!(
                "@{}'s window is open on the board, and its process {} is gone",
                window.handle,
                window.pid.unwrap_or_default()
            )
        } else {
            format!(
                "@{}'s window is open on the board, and the pane host has no {}",
                window.handle, window.pane
            )
        };
        found.push(
            Break::new(
                6,
                format!("6:gone:{}:{}", window.pane, window.generation),
                said,
            )
            .with(Evidence {
                rows: vec![format!("{} generation {}", window.pane, window.generation)],
                screen: window.tail.clone(),
                record: Vec::new(),
            })
            .twice(),
        );
    }
    for exit in look
        .windows
        .exits
        .iter()
        .filter(|exit| exit.closed_by == ClosedBy::Itself)
    {
        let tasks = at_work_at(look, &exit.handle, exit.at);
        if tasks.is_empty() {
            continue;
        }
        found.push(
            Break::new(
                6,
                format!("6:closed:{}:{}", exit.pane, exit.generation),
                format!(
                    "@{}'s window closed by itself while {} was at work",
                    exit.handle,
                    tasks
                        .iter()
                        .map(|number| format!("T-{number}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
            .with(Evidence {
                rows: vec![format!(
                    "{} generation {} exited at {}: code {}, signal {}",
                    exit.pane,
                    exit.generation,
                    iso(exit.at),
                    exit.code
                        .map_or_else(|| "none".to_owned(), |code| code.to_string()),
                    exit.signal.as_deref().unwrap_or("none")
                )],
                screen: exit.tail.clone(),
                record: Vec::new(),
            }),
        );
    }
    for stranger in &look.windows.strangers {
        found.push(
            Break::new(
                6,
                format!("6:stranger:{}", stranger.process.pid),
                format!(
                    "process {} ({}) still ran, and its window {} is gone since {}; it was left running, for the run ends only what it started",
                    stranger.process.pid,
                    cut(&stranger.process.command, 120, &names),
                    stranger.pane,
                    iso(stranger.since),
                ),
            )
            .with(Evidence {
                rows: vec![format!(
                    "{} {} (began {})",
                    stranger.process.pid, stranger.process.command, stranger.began
                )],
                ..Evidence::default()
            }),
        );
    }
    for left in &look.windows.unresolved {
        found.push(
            Break::new(
                6,
                format!("6:unresolved:{}", left.process.pid),
                format!(
                    "process {} ({}) of the window {}, gone since {}, could not be judged: {}",
                    left.process.pid,
                    cut(&left.process.command, 120, &names),
                    left.pane,
                    iso(left.since),
                    left.why
                ),
            )
            .with(Evidence {
                rows: vec![format!("{} {}", left.process.pid, left.process.command)],
                ..Evidence::default()
            }),
        );
    }
    for unread in &look.windows.unread {
        found.push(Break::new(
            6,
            format!("6:unread:{}", unread.at),
            format!(
                "the pane host's last word was not read to its end when the app closed at {}: an exit it told after, and the processes it names, are none the watch has ({})",
                iso(unread.at),
                unread.why
            ),
        ));
    }
    found
}

/// 6, unseen: each open window whose tree the pane host could not say (and whose exit
/// the look has not taken: a window that closed as its tree was asked for is no gap,
/// its exit listing what its close ended); each window whose tree runs more than the
/// host names, whether or not its exit was taken; and each exit that names as many
/// processes as the host names at most ([`NAMED`]), which may have ended more.
pub fn gaps(look: &Look) -> Vec<Unjudged> {
    let mut found = Vec::new();
    for window in &look.windows.open {
        let exited = look
            .windows
            .exits
            .iter()
            .any(|exit| exit.pane == window.pane && exit.generation == window.generation);
        if let Some(why) = window.tree_unread.as_ref().filter(|_| !exited) {
            found.push(Unjudged {
                twice: true,
                ..Unjudged::new(
                    6,
                    format!("6:tree:{}:{}", window.pane, window.generation),
                    format!(
                        "what @{}'s window {} runs could not be read, so whether a process of it outlives it is not known: {why}",
                        window.handle, window.pane
                    ),
                )
            });
        }
        if window.background_total > window.background.len() {
            found.push(Unjudged::new(
                6,
                format!("6:unnamed:{}:{}", window.pane, window.generation),
                format!(
                    "@{}'s window {} runs {} processes its turns started, and the pane host names {}: the rest are not followed, so whether one outlives the window is not known",
                    window.handle,
                    window.pane,
                    window.background_total,
                    window.background.len()
                ),
            ));
        }
    }
    for exit in look
        .windows
        .exits
        .iter()
        .filter(|exit| exit.ended.len() >= NAMED)
    {
        found.push(Unjudged::new(
            6,
            format!("6:exit-unnamed:{}:{}", exit.pane, exit.generation),
            format!(
                "@{}'s window {} exited naming {} processes its close ended, as many as the pane host names: it may have ended more, which are not followed, so whether one outlives the window is not known",
                exit.handle,
                exit.pane,
                exit.ended.len()
            ),
        ));
    }
    found
}

/// How many of a window's processes the pane host names at most, in a tree it says
/// and in an exit (`cf_proto::panes::BACKGROUND_PROCESSES`, the product's): a tree
/// says how many there are besides, an exit does not.
const NAMED: usize = 16;

/// 8: a tool call skipped, in a record or on a screen, and a dialog that has
/// waited too long.
pub fn skipped_or_waiting(look: &Look, settings: &Settings) -> Vec<Break> {
    let names = Names::of(look);
    let mut found = Vec::new();
    for item in look
        .ledger
        .items
        .iter()
        .filter(|item| item.role == "tool" && holds_skipped(&item.text))
    {
        let handle = look
            .ledger
            .conversations
            .iter()
            .find(|conversation| conversation.id == item.conversation)
            .map_or("?", |conversation| {
                look.ledger.handle(conversation.participant)
            });
        found.push(
            Break::new(
                8,
                format!("8:skipped:{}:{}", item.conversation, item.id),
                format!("a tool call of @{handle}'s came back \"{SKIPPED}\""),
            )
            .with(Evidence {
                record: vec![item_line(item, &names)],
                ..Evidence::default()
            }),
        );
    }
    for window in &look.windows.open {
        if window.tail.iter().any(|line| says_skipped(line)) {
            found.push(
                Break::new(
                    8,
                    format!("8:skipped-screen:{}:{}", window.pane, window.generation),
                    format!("@{}'s window shows \"{SKIPPED}\"", window.handle),
                )
                .with(Evidence {
                    screen: window.tail.clone(),
                    ..Evidence::default()
                }),
            );
        }
    }
    let mut handles: Vec<&str> = look
        .windows
        .activity
        .iter()
        .map(|change| change.handle.as_str())
        .collect();
    handles.sort_unstable();
    handles.dedup();
    for handle in handles {
        let Some(last) = look.activity_of(handle, look.at) else {
            continue;
        };
        if last.state != "waiting" || look.at - last.at < settings.dialog {
            continue;
        }
        // Timed while the window that waits is open: one that closed since, or was
        // opened again, has no dialog of then.
        let closed_since = look
            .windows
            .exits
            .iter()
            .any(|exit| exit.handle == handle && exit.at >= last.at);
        if look.window_of(handle).is_none() || closed_since {
            continue;
        }
        found.push(
            Break::new(
                8,
                format!("8:dialog:{handle}:{}", last.at),
                format!(
                    "a dialog has waited in @{handle}'s window for {} min{}",
                    (look.at - last.at) / 60_000,
                    last.reason
                        .as_deref()
                        .map(|reason| format!(" ({reason})"))
                        .unwrap_or_default()
                ),
            )
            .with(Evidence {
                rows: vec![format!("waiting since {}", iso(last.at))],
                screen: look
                    .window_of(handle)
                    .map(|window| window.tail.clone())
                    .unwrap_or_default(),
                record: Vec::new(),
            }),
        );
    }
    found
}

/// The tasks `handle` held at `at`, by number, with whether each was at work then:
/// whom each was given to by then, by the log ([`holder_at`]; the task's assignee now,
/// where the log says no assignment of it), and the log's last word on its state by
/// then ([`MOVES`]; the task's state now, where it has none). A task given to another
/// since is still the one the window held when it closed, and one given to it is at
/// work once it moved on from `queued`.
///
/// The rig is told of a window's exit before the daemon is, so a pause in the exit's
/// own millisecond may be the one the exit caused, after it, or someone's, before it or
/// after it (the owner's, as the window crashed and before the daemon took the exit).
/// It is the exit's where its `because` says the window closed or went: the task was
/// in the state it paused from. Anyone else's says whose it is and nothing of when, and
/// nothing but the two times orders it against the exit (the log and the host's exits
/// are two sources): which came first is untold.
fn held_at(look: &Look, handle: &str, at: i64) -> Vec<(i64, AtExit)> {
    let worker = look.ledger.by_handle(handle).map(|worker| worker.id);
    let closed = format!("@{handle}'s window ");
    look.ledger
        .tasks
        .iter()
        .filter(|task| match holder_at(look, task.number, at) {
            Some(holder) => holder.as_deref() == Some(handle),
            None => task.assignee.is_some() && task.assignee == worker,
        })
        .map(|task| {
            let last = look.ledger.events.iter().rev().find(|event| {
                MOVES.contains(&event.kind.as_str())
                    && event.data["task"].as_i64() == Some(task.number)
                    && clock::millis(&event.at).is_some_and(|when| when <= at)
            });
            let state = match last {
                None => Some(task.state.as_str()),
                Some(event) => {
                    let to = event.data["to"].as_str().unwrap_or_default();
                    if to == "paused" && clock::millis(&event.at) == Some(at) {
                        let because = event.data["because"].as_str().unwrap_or_default();
                        because
                            .starts_with(&closed)
                            .then(|| event.data["from"].as_str().unwrap_or_default())
                    } else {
                        Some(to)
                    }
                }
            };
            let at_exit = match state {
                Some(state) if AT_WORK.contains(&state) => AtExit::AtWork,
                Some(_) => AtExit::Not,
                None => AtExit::Untold,
            };
            (task.number, at_exit)
        })
        .collect()
}

/// Whether a task a window held was at work when the window exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtExit {
    AtWork,
    Not,
    /// A pause in the exit's own millisecond that does not say it is the exit's.
    Untold,
}

/// The tasks `handle` held that were at work at `at`, by number ([`held_at`]).
fn at_work_at(look: &Look, handle: &str, at: i64) -> Vec<i64> {
    held_at(look, handle, at)
        .into_iter()
        .filter(|(_, at_exit)| *at_exit == AtExit::AtWork)
        .map(|(number, _)| number)
        .collect()
}

/// 6, unresolved: a window that closed by itself holding a task whose pause shares the
/// exit's millisecond and does not say it is the exit's ([`held_at`]): whether the task
/// was at work when it closed, nothing tells.
pub fn untold_exits(look: &Look) -> Vec<Unjudged> {
    look.windows
        .exits
        .iter()
        .filter(|exit| exit.closed_by == ClosedBy::Itself)
        .filter_map(|exit| {
            let untold: Vec<String> = held_at(look, &exit.handle, exit.at)
                .into_iter()
                .filter(|(_, at_exit)| *at_exit == AtExit::Untold)
                .map(|(number, _)| format!("T-{number}"))
                .collect();
            (!untold.is_empty()).then(|| {
                Unjudged::new(
                    6,
                    format!("6:closed:{}:{}", exit.pane, exit.generation),
                    format!(
                        "@{}'s window closed by itself at {}, and {} paused in the same millisecond, which nothing but the two times orders: whether it was at work when the window closed cannot be told",
                        exit.handle,
                        iso(exit.at),
                        untold.join(", ")
                    ),
                )
            })
        })
        .collect()
}

/// Whom T-`number` was given to at `at`, by the handle the log names, as the last of
/// its assignments by then says: `task.created` and `task.opened` name whom it was
/// given to (none, given to a tier), and `task.assigned` the session it went to. (A
/// release leaves the task `open`, which its state says.) None where the log says no
/// assignment of it at all.
fn holder_at(look: &Look, number: i64, at: i64) -> Option<Option<String>> {
    let named = |value: &serde_json::Value| value.as_str().map(str::to_owned);
    let mut holder = None;
    for event in look.ledger.events.iter().filter(|event| {
        event.data["task"].as_i64() == Some(number)
            && clock::millis(&event.at).is_some_and(|when| when <= at)
    }) {
        match event.kind.as_str() {
            "task.created" | "task.opened" => holder = Some(named(&event.data["to"])),
            "task.assigned" => holder = Some(named(&event.data["assignee"])),
            _ => {}
        }
    }
    holder
}
