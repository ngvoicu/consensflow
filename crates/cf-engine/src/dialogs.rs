//! A dialog of a window's harness that waits for a key mid-session and is not its prompt
//! (T-1183). Claude Code 3.0.6's window of a worker offered to install an LSP plugin ("Would
//! you like to install this LSP plugin?", four choices), and nobody heard of it: no record of
//! the conversation holds such a dialog, and only the screen shows it (field, 3.0.6, 2026-10-26).
//! The class is every harness's: Claude Code's offers to install a plugin, its permission
//! prompts that full permission does not cover and its choices at a usage limit, Codex's
//! nudge to switch models, Muse Code's card that its quota goes fast, Devin's offer to switch
//! its theme (`brain: research/harness-dialogs-inventory`).
//!
//! The offers a launch can turn off are off where the window is started (each adapter's own
//! launch settings: Claude Code's surveys, Codex's nudge and install offers), never in the
//! owner's own configuration. What is left is watched here, apart from what `trust` watches
//! as a window starts, and from the window's own question tool, which the board answers (a
//! harness that says a window waits on it says so, `dialog_waiting`):
//!
//! - **A harness that says a dialog of its own is open** ([`DialogSign::Open`]: Claude Code's
//!   status, `waiting` for a dialog that is not its question tool's) has its screen looked at
//!   for the dialog's words, and the dialog is told of once it has been open
//!   [`TELL_AFTER_MS`]. Claude Code does not say it of every dialog: with its offer to install
//!   an LSP plugin open in a turn at work its status said `busy` (the live run of 2026-11-03),
//!   and the screen told of it, as below.
//! - **A harness that says nothing** ([`DialogSign::Screen`]) has its screen looked at once a
//!   [`LOOK_EVERY_MS`] at most, and a dialog read there by its shape (a question with a line of
//!   choices, the harness's cursor on one, and no prompt drawn below them) counts once
//!   [`SURE_LOOKS`] looks in a row show it; the window then waits, as one whose harness says
//!   it does: nothing is pasted into it, for a paste's Enter would answer it.
//!
//! Either way the human, and whoever gave the window's task, hear it once a dialog, in words
//! that name the window, the question and its choices, and that say it waits. Nothing is
//! pressed in it but a decline on its harness's list of safe declines
//! ([`cf_harness::contract::Window::safe_decline`]: Claude Code's offer to install an LSP
//! plugin, "No, not now", which writes nothing, where an offer left to close by itself after
//! its 30 s counts against the owner's own offers in `~/.claude.json`, T-1192). No other choice
//! of a harness's dialog is ConsensFlow's to make (an install, a model, a setting of the
//! owner's). A decline is pressed after the note of the dialog has gone, which says it, at a
//! later look that still shows the dialog with that choice, by its one key, once: a look that
//! finds it gone, or drawn otherwise, presses nothing, and a key the window did not take is not
//! pressed again. A dialog that goes and comes again is told again.
//!
//! The screen is read through [`crate::host::EngineHost::screen`], which no Node trace holds.
//! Limits: a dialog that opens and closes between two looks is not seen, and a dialog drawn in
//! a shape the reader does not know (no cursor on a choice, a question that does not ask) is
//! not read off a screen whose harness says nothing of it.

use std::rc::Rc;

use cf_harness::contract::{Decline, Dialog, DialogSign, Observed, Pane, Waiting};
use cf_ledger::{NewNote, ParticipantView, ProjectView};
use serde_json::json;

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::shown::mask::mask;
use crate::shown::Shown;
use crate::trust::Trusting;

/// How often at most the screen of a window whose harness says nothing of its dialogs is
/// looked at for one; and of one whose harness says one is open, once it was told of.
pub(crate) const LOOK_EVERY_MS: i64 = 5_000;

/// How many looks in a row a dialog read by its shape alone must show before it counts: a
/// redraw caught halfway, or a list a conversation draws a moment, shows for one.
pub(crate) const SURE_LOOKS: u32 = 2;

/// How long a dialog its harness says is open must stay before it is told of: a permission
/// prompt a person in the window answers at once is not.
pub(crate) const TELL_AFTER_MS: i64 = 2_000;

/// The most characters of a dialog's words, or of one of its choices, a note quotes.
const QUOTED_CHARS: usize = 200;

/// What the looks found of a window's dialogs.
#[derive(Debug, Default)]
pub(crate) struct DialogWatch {
    /// When its screen was last looked at for one.
    looked: Option<i64>,
    /// What the last look read, and how many looks in a row read the same words.
    shown: Option<(Dialog, u32)>,
    /// Since when its harness has said one is open.
    open_since: Option<i64>,
    /// The words of the dialog the human was told of, until a look finds it gone.
    told: Option<String>,
    /// The decline of the dialog told of, where it is on its harness's list of safe declines,
    /// and whether its key was pressed.
    decline: Option<Decline>,
    declined: bool,
}

impl Dispatcher {
    /// The look `observed` at `record`'s window, with what its screen shows of a dialog of its
    /// harness's: where one counts and its harness says nothing of it, the window waits on it;
    /// where its harness says one is open, the wait names its words. The human and whoever gave
    /// the window's task are told of it once.
    pub(crate) async fn watch_dialog(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        mut observed: Observed,
    ) -> Result<Observed, EngineError> {
        let (window, pane) = self.window_of(record);
        let (Some(window), Some(pane)) = (window, pane) else {
            return Ok(observed);
        };
        // A window that starts is `trust`'s to watch, and one that has not said, or no longer
        // says, which conversation it shows is between two.
        let starting = record.window.borrow().trust != Trusting::Past;
        let sign = window.dialog_sign(&observed);
        if starting || observed.unnamed || observed.switched.is_some() || sign == DialogSign::Shut {
            *record.dialog.borrow_mut() = DialogWatch::default();
            return Ok(observed);
        }
        let now = self.now();
        let due = {
            let mut watch = record.dialog.borrow_mut();
            if sign == DialogSign::Open {
                watch.open_since.get_or_insert(now);
            } else {
                watch.open_since = None;
            }
            // While its harness says one is open and it is not told of yet, every step looks.
            let every = if sign == DialogSign::Open && watch.told.is_none() {
                0
            } else {
                LOOK_EVERY_MS
            };
            watch.looked.is_none_or(|looked| now - looked >= every)
        };
        if due {
            let answer = self.seams.host.screen(&pane).await;
            if self.forgotten(record) {
                return Ok(observed);
            }
            let shown = answer.map(|answer| Shown::from_snapshot(&answer)).ok();
            let lines = shown.as_ref().map_or(&[][..], Shown::lines);
            // The question of trusting the folder is `trust`'s, whenever it is asked.
            let read = window
                .dialog(lines)
                .filter(|_| window.toward_trust(lines).is_none())
                .filter(|dialog| sign == DialogSign::Open || (dialog.asks && !dialog.prompt_below));
            let mut watch = record.dialog.borrow_mut();
            watch.looked = Some(now);
            watch.shown = read.map(|dialog| {
                let looks = match &watch.shown {
                    Some((before, looks)) if before.question == dialog.question => looks + 1,
                    _ => 1,
                };
                (dialog, looks)
            });
        }
        let (counts, dialog) = {
            let watch = record.dialog.borrow();
            let dialog = watch.shown.as_ref().map(|(dialog, _)| dialog.clone());
            let counts = match sign {
                DialogSign::Open => watch
                    .open_since
                    .is_some_and(|since| now - since >= TELL_AFTER_MS),
                _ => watch
                    .shown
                    .as_ref()
                    .is_some_and(|(_, looks)| *looks >= SURE_LOOKS),
            };
            (counts, dialog)
        };
        if !counts {
            if sign == DialogSign::Screen && dialog.is_none() {
                let mut watch = record.dialog.borrow_mut();
                watch.told = None;
                watch.decline = None;
                watch.declined = false;
            }
            return Ok(observed);
        }
        // The window waits on it: its harness said so, or its screen does.
        let words = dialog.as_ref().map(|dialog| quoted(&dialog.question));
        observed.waiting = Some(Waiting {
            reason: Some(match &words {
                Some(words) => format!("a dialog of its harness waits for a key: {words}"),
                None => observed
                    .waiting
                    .as_ref()
                    .and_then(|waiting| waiting.reason.clone())
                    .map_or_else(
                        || "a dialog of its harness waits for a key".to_owned(),
                        |reason| format!("a dialog of its harness waits for a key ({reason})"),
                    ),
            }),
        });
        let key = dialog
            .as_ref()
            .map_or_else(String::new, |dialog| dialog.question.clone());
        // Once a dialog: one its harness says is open stays the one told of until it closes,
        // whatever words a later look reads of it.
        let before = record.dialog.borrow().told.clone();
        if before.as_deref() == Some(key.as_str()) || (sign == DialogSign::Open && before.is_some())
        {
            if due {
                self.decline(record, &pane, dialog.as_ref()).await;
            }
            return Ok(observed);
        }
        let decline = dialog
            .as_ref()
            .and_then(|dialog| window.safe_decline(dialog));
        let body = told(&participant.handle, dialog.as_ref(), decline.as_ref());
        {
            let mut watch = record.dialog.borrow_mut();
            watch.told = Some(key);
            watch.decline = decline;
            watch.declined = false;
        }
        self.tell_dialog(project, participant, &body)?;
        Ok(observed)
    }

    /// The decline of the dialog told of in `record`'s window, pressed once, where it is on its
    /// harness's list, the note of it has gone, and `shown`, what this step's look read of the
    /// screen, is that dialog still with that choice: a look that finds it gone, or drawn
    /// otherwise, presses nothing.
    async fn decline(&self, record: &Record, pane: &Pane, shown: Option<&Dialog>) {
        let key = {
            let watch = record.dialog.borrow();
            let Some(decline) = watch.decline.as_ref().filter(|_| !watch.declined) else {
                return;
            };
            let still = shown.is_some_and(|shown| {
                watch.told.as_deref() == Some(shown.question.as_str())
                    && shown.options.contains(&decline.choice)
            });
            if !still {
                return;
            }
            decline.key.clone()
        };
        record.dialog.borrow_mut().declined = true;
        let body = json!({ "id": pane.id, "generation": pane.generation, "bytes": key });
        let _ = self.seams.host.request("pane.input", body).await;
    }

    /// The note of a dialog in `participant`'s window, `body`, to the human and to whoever gave
    /// the window's task, where that is another than the human.
    fn tell_dialog(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        body: &str,
    ) -> Result<(), EngineError> {
        let task = if participant.role == "chief" {
            None
        } else {
            self.seams
                .ledger
                .borrow()
                .active_task(participant.id, false)?
        };
        let number = task.as_ref().map(|thread| thread.task.number);
        let requester = task
            .as_ref()
            .map(|thread| thread.task.requester.clone())
            .filter(|requester| requester != "human");
        for to in requester.into_iter().chain(["human".to_owned()]) {
            self.seams.ledger.borrow_mut().note(
                project.id,
                &NewNote {
                    from: None,
                    to,
                    task: number,
                    body: body.to_owned(),
                },
            )?;
        }
        self.changed();
        Ok(())
    }
}

/// What the human and the task's giver are told of a dialog in `handle`'s window: its words and
/// choices where its screen showed them, when its harness closes it by itself where it says,
/// and the choice ConsensFlow makes of it, `decline`, where it is on its harness's list.
pub(crate) fn told(handle: &str, dialog: Option<&Dialog>, decline: Option<&Decline>) -> String {
    let shows = match dialog {
        Some(dialog) => {
            let choices: Vec<String> = dialog.options.iter().map(|option| quoted(option)).collect();
            format!(
                "@{handle}'s window shows a dialog of its harness that waits for a key, not its prompt: {}, its choices {}.",
                quoted(&dialog.question),
                choices.join(", ")
            )
        }
        None => format!(
            "@{handle}'s window shows a dialog of its harness that waits for a key, not its prompt; its screen shows no words ConsensFlow can read of it."
        ),
    };
    if let Some(decline) = decline {
        return format!(
            "{shows} ConsensFlow declines it for the window now, choosing {}, which writes nothing; nothing else is typed into the window meanwhile.",
            quoted(&decline.choice)
        );
    }
    let closes = dialog
        .and_then(|dialog| dialog.closes_after_s)
        .map_or_else(String::new, |seconds| {
            format!(" Its harness closes it by itself {seconds} s after it opened.")
        });
    format!(
        "{shows} The window waits on it, and nothing is typed into it meanwhile. ConsensFlow presses nothing in it: answer it in that window, or leave it.{closes}"
    )
}

/// `words` in quotation marks, its secrets hidden, cut at [`QUOTED_CHARS`].
fn quoted(words: &str) -> String {
    let masked = mask(words.trim()).text;
    let cut: String = masked.chars().take(QUOTED_CHARS).collect();
    let more = if cut.len() < masked.len() { "…" } else { "" };
    format!("\u{201c}{cut}{more}\u{201d}")
}

#[cfg(test)]
mod tests;
