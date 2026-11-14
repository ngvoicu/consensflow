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
//! A member's dialog that ConsensFlow will decline, or whose harness says it closes by itself,
//! asks its task's giver no decision: the note is shown to a chief, not typed. Other givers and
//! the human receive it as before; a dialog in the chief's own window is unchanged (CLN-75).
//!
//! **A decline can be a key with no choice of its own** (T-1300): Claude Code's Computer Use
//! permission prompt, which a member's `request_access` raises where ConsensFlow.app lacks
//! Accessibility or Screen Recording, lists two choices that cannot be pressed (one opens System
//! Settings in ConsensFlow's name, the other asks again, and a Screen Recording grant takes effect
//! only when the app restarts), so its decline is Escape, which closes it and grants nothing
//! ([`cf_harness::contract::Decline::listed`] false). It is made in a member's window only
//! ([`cf_harness::contract::Decline::members_only`]): the chief's window is the human's, and the
//! prompt is told there as any other and never closed. The member is told what ConsensFlow lacks
//! once its window rests, as a message of the board's, kept for its task when the task is paused.
//!
//! **Which dialog is which** (T-1277, after T-1261 and T-1273). Two reads of a screen are one
//! dialog or two, and the two mistakes are not alike: telling a dialog twice is a nuisance, and
//! taking two different dialogs for one is a failure (the second is never told, and a decline
//! meant for the first is pressed into it). Where they conflict the dialog is told twice. The
//! engine holds two reads to one dialog by **plain equality** of what the dialog's own window says
//! tells it from another read ([`cf_harness::contract::Window::dialog_identity`]): the words as
//! they were read for every harness but Claude Code, whose window blanks the time in its own
//! warning line ("⚠ Claude Code will automatically block this request in 1:57, …", redrawn each
//! second) and begins its offer to install an LSP plugin at the offer's own title (the tail slides
//! when the turn under the offer ends, and a line of the conversation takes the place above it).
//! A read whose identity differs is another dialog: it is told, and it has a deadline of its own.
//! A decline is pressed only into a read whose identity equals that of the dialog told of. The
//! identity first told is the dialog's for as long as it stays: nothing grows, nothing is held.
//! A note still quotes the read as it was.
//! The rule holds whatever the harness's status says (T-1300): where it says a dialog is open all along,
//! a second request that takes the place of the first is another dialog, told, with a deadline of its
//! own, and a decline is never pressed into it by the first's. Before, the status said "the dialog told
//! of" for any read, and a second hook's ask in the same words was never told. Only a look that reads no
//! words under such a status is the dialog told of. What a harness's window draws above the request (a
//! permission dialog's title and the row of the tool that waits above it) is not the request's, and its
//! identity leaves it out, or a tail that slid would tell the same prompt twice (`claude::dialog_identity`).
//!
//! The screen is read through [`crate::host::EngineHost::screen`], which no Node trace holds: the
//! pane host's tail, the last twelve lines with a letter or a digit in them, so no empty line and
//! no rule that says no word. A harness's own prompt is no dialog, and the reader has to tell it
//! from one on that tail (T-1240: Codex's composer, its footer and the last line of the
//! conversation were one under the other in one column, as a list of choices is, and a worker's
//! question above them was the dialog's words; a chief at rest was held for a dialog nobody had
//! opened): the cursor of an input box is the first line under a rule (`cf_harness::shared::
//! dialog`); Codex, whose box draws no rule and whose footer is the user's to configure, has its window
//! read no dialog by shape at all (`CodexWindow::dialog`), as the release head read none of its real
//! ones off the tail.
//! Limits: a dialog that opens and closes between two looks is not seen, and a dialog drawn in
//! a shape the reader does not know (no cursor on a choice, a question that does not ask) is
//! not read off a screen whose harness says nothing of it.

use std::rc::Rc;

use cf_harness::contract::{Decline, Dialog, DialogSign, Observed, Pane, Waiting, Window};
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

/// The most characters of a dialog's words, of one of its choices, or of what it says above
/// them, a note quotes.
const QUOTED_CHARS: usize = 400;

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
    /// When the note of it went, and the dialog's identity then (`Window::dialog_identity` of the
    /// read): a decline is pressed only into a read whose identity equals it, its words, what it
    /// says above them and its choices the same.
    told_at: Option<i64>,
    told_as: Option<Dialog>,
    /// The decline of the dialog told of, where it is on its harness's list of safe declines,
    /// and whether its key was pressed.
    decline: Option<Decline>,
    declined: bool,
    /// Shown notes of this dialog, including a notice of a decline, until it is gone.
    shown_notes: Vec<i64>,
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
        if starting || observed.unnamed {
            return Ok(observed);
        }
        if observed.switched.is_some() || sign == DialogSign::Shut {
            self.end_dialog_notes(project, record)?;
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
        let mut absent = false;
        if due {
            let answer = self.seams.host.screen(&pane).await;
            if self.forgotten(record) {
                return Ok(observed);
            }
            let shown = answer.map(|answer| Shown::from_snapshot(&answer)).ok();
            // If only the screen can tell, a refused snapshot or one without a tail
            // says nothing about the dialog: keep its note and do not act on an old read.
            // A harness's explicit Open is still told, even when no words can be read.
            if sign == DialogSign::Screen && shown.as_ref().is_none_or(|shown| !shown.is_known()) {
                record.dialog.borrow_mut().looked = Some(now);
                return Ok(observed);
            }
            let lines = shown.as_ref().map_or(&[][..], Shown::lines);
            // The question of trusting the folder is `trust`'s, whenever it is asked.
            let read = window
                .dialog(lines)
                .filter(|_| window.toward_trust(lines).is_none())
                .filter(|dialog| sign == DialogSign::Open || (dialog.asks && !dialog.prompt_below));
            absent = sign == DialogSign::Screen && shown.is_some() && read.is_none();
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
        if absent {
            self.end_dialog_notes(project, record)?;
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
        // A dialog is told of by its identity, its window's words of what tells it from another
        // read of its screen: the words and what it says above them as they were read, but for
        // what its harness draws and redraws of its own. Another call's ask in the same words is
        // another dialog.
        let identity = dialog.as_ref().map(|dialog| window.dialog_identity(dialog));
        let key = identity.as_ref().map_or_else(String::new, |identity| {
            [identity.detail.join(" "), identity.question.clone()].join("\n")
        });
        // Once a dialog: the same words and the same place are the dialog told of, and a read
        // whose identity differs is another dialog, whatever the status says: a second request
        // that opens under the status of the first is told, and has a deadline of its own
        // (T-1300). A look that read no words under a status that says a dialog is open is the
        // dialog told of: there is nothing to tell it from.
        let before = record.dialog.borrow().told.clone();
        if before.as_deref() == Some(key.as_str())
            || (sign == DialogSign::Open && dialog.is_none() && before.is_some())
        {
            if due {
                self.decline(
                    project,
                    participant,
                    record,
                    &pane,
                    &*window,
                    dialog.as_ref(),
                )
                .await?;
            }
            return Ok(observed);
        }
        self.end_dialog_notes(project, record)?;
        // The chief's window is the human's: a decline that is for a member's window alone is
        // not made there, and the dialog is told as any other (T-1300).
        let decline = dialog
            .as_ref()
            .and_then(|dialog| window.safe_decline(dialog))
            .filter(|decline| !(decline.members_only && participant.role == "chief"));
        let body = told(&participant.handle, dialog.as_ref(), decline.as_ref());
        let ends_without_answer = decline.is_some()
            || dialog
                .as_ref()
                .is_some_and(|dialog| dialog.closes_after_s.is_some());
        {
            let mut watch = record.dialog.borrow_mut();
            watch.told = Some(key);
            watch.decline = decline;
            watch.declined = false;
            watch.told_at = Some(now);
            watch.told_as = identity;
        }
        if let Some(id) =
            self.tell_dialog_with_disposition(project, participant, &body, ends_without_answer)?
        {
            record.dialog.borrow_mut().shown_notes.push(id);
        }
        Ok(observed)
    }

    /// End just this dialog's shown notes; a task's older background aside may still hold.
    fn end_dialog_notes(&self, project: &ProjectView, record: &Record) -> Result<(), EngineError> {
        let notes = record.dialog.borrow().shown_notes.clone();
        for id in &notes {
            self.seams.ledger.borrow_mut().end_aside(project.id, *id)?;
        }
        record.dialog.borrow_mut().shown_notes.clear();
        if !notes.is_empty() {
            self.changed();
        }
        Ok(())
    }

    /// The decline of the dialog told of in `record`'s window, pressed once, where it is on its
    /// harness's list, the note of it has gone, and `shown`, what this step's look read of the
    /// screen, is that dialog still (its identity equals the identity told of, its choices the
    /// same) with that choice: a look that finds it gone, or drawn otherwise, presses nothing.
    async fn decline(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        pane: &Pane,
        window: &dyn Window,
        shown: Option<&Dialog>,
    ) -> Result<(), EngineError> {
        let now = self.now();
        let (key, decline) = {
            let watch = record.dialog.borrow();
            let Some(decline) = watch.decline.as_ref().filter(|_| !watch.declined) else {
                return Ok(());
            };
            // A dialog that is the human's to answer first waits its time from the note.
            let due = decline
                .after_ms
                .is_none_or(|after| watch.told_at.is_some_and(|told_at| now - told_at >= after));
            let still = shown.is_some_and(|shown| {
                let identity = window.dialog_identity(shown);
                watch.told_as.as_ref().is_some_and(|told| {
                    told.question == identity.question
                        && told.detail == identity.detail
                        && told.options == identity.options
                }) && (!decline.listed || shown.options.contains(&decline.choice))
            });
            if !due {
                return Ok(());
            }
            if !still {
                return Ok(());
            }
            (decline.key.clone(), decline.clone())
        };
        record.dialog.borrow_mut().declined = true;
        let body = json!({ "id": pane.id, "generation": pane.generation, "bytes": key });
        let _ = self.seams.host.request("pane.input", body).await;
        let Some(words) = decline.then_tell else {
            return Ok(());
        };
        // The press ended the window's turn: the words that send it on go in once it rests,
        // as any message of the board's, and whoever gave its task and the human hear of it.
        let number = match self.dialog_task(participant)? {
            Some(thread) => Some(thread.task.number),
            // The chief's `cf task pause T-n "…"` pauses a member's task, and the task is still the window's: a
            // note about no task, or about a paused one, is no row a member's queue lets go in,
            // so it is kept for the task and goes in once the chief resumes it (T-1300).
            None => self.paused_task(participant)?,
        };
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: participant.handle.clone(),
                task: number,
                body: words,
            },
        )?;
        let body = match decline.after_ms {
            Some(after) => format!(
                "Nobody answered @{}'s {} in {}, so ConsensFlow answered {} and told it to go on without that call.",
                participant.handle,
                decline.what,
                spoken(after),
                quoted(&decline.choice)
            ),
            None => format!(
                "ConsensFlow declined @{}'s {} by pressing {}, which writes nothing, and told it to go on without it.",
                participant.handle,
                decline.what,
                quoted(&decline.choice)
            ),
        };
        if let Some(id) = self.tell_dialog_with_disposition(project, participant, &body, true)? {
            record.dialog.borrow_mut().shown_notes.push(id);
        }
        Ok(())
    }

    /// The task of `participant`'s window whose giver hears of its dialogs: none for the chief's.
    fn dialog_task(
        &self,
        participant: &ParticipantView,
    ) -> Result<Option<cf_ledger::TaskThread>, EngineError> {
        if participant.role == "chief" {
            return Ok(None);
        }
        Ok(self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?)
    }

    /// The number of the task `participant`'s window holds paused, where it holds one: a member's
    /// last task, if it is paused and still its own. None for the chief.
    fn paused_task(&self, participant: &ParticipantView) -> Result<Option<i64>, EngineError> {
        if participant.role == "chief" {
            return Ok(None);
        }
        Ok(self
            .seams
            .ledger
            .borrow()
            .last_task(participant.id)?
            .filter(|thread| {
                thread.task.state == "paused"
                    && thread.task.assignee.as_deref() == Some(participant.handle.as_str())
            })
            .map(|thread| thread.task.number))
    }

    /// The note of a dialog in `participant`'s window, `body`, to the human and to whoever gave
    /// the window's task, where that is another than the human.
    pub(crate) fn tell_dialog(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        body: &str,
    ) -> Result<(), EngineError> {
        self.tell_dialog_with_disposition(project, participant, body, false)?;
        Ok(())
    }

    fn tell_dialog_with_disposition(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        body: &str,
        ends_without_answer: bool,
    ) -> Result<Option<i64>, EngineError> {
        let task = self.dialog_task(participant)?;
        let number = task.as_ref().map(|thread| thread.task.number);
        let requester = task
            .as_ref()
            .map(|thread| thread.task.requester.clone())
            .filter(|requester| requester != "human");
        let mut shown_id = None;
        for to in requester.into_iter().chain(["human".to_owned()]) {
            let note = NewNote {
                from: None,
                to,
                task: number,
                body: body.to_owned(),
            };
            if ends_without_answer && note.to != "human" {
                // `show` queues for a non-chief requester as before; the human's note is unchanged.
                let shown = self.seams.ledger.borrow_mut().show(
                    project.id,
                    &note,
                    &format!("dialog reported in @{}'s window", participant.handle),
                )?;
                if shown.receipt.get(cf_ledger::SHOWN).is_some() {
                    shown_id = Some(shown.id);
                }
            } else {
                self.seams.ledger.borrow_mut().note(project.id, &note)?;
            }
        }
        self.changed();
        Ok(shown_id)
    }
}

/// What the human and the task's giver are told of a dialog in `handle`'s window: its words and
/// choices where its screen showed them, when its harness closes it by itself where it says,
/// and the choice ConsensFlow makes of it, `decline`, where it is on its harness's list.
pub(crate) fn told(handle: &str, dialog: Option<&Dialog>, decline: Option<&Decline>) -> String {
    let shows = match dialog {
        Some(dialog) => {
            let choices: Vec<String> = dialog.options.iter().map(|option| quoted(option)).collect();
            let says = if dialog.detail.is_empty() {
                String::new()
            } else {
                format!(" Above it, it says: {}.", quoted(&dialog.detail.join(" ")))
            };
            format!(
                "@{handle}'s window shows a dialog of its harness that waits for a key, not its prompt: {}, its choices {}.{says}",
                quoted(&dialog.question),
                choices.join(", ")
            )
        }
        None => format!(
            "@{handle}'s window shows a dialog of its harness that waits for a key, not its prompt; its screen shows no words ConsensFlow can read of it."
        ),
    };
    if let Some(decline) = decline.filter(|decline| decline.after_ms.is_some()) {
        let span = spoken(decline.after_ms.unwrap_or(0));
        let then = if decline.then_tell.is_some() {
            ", and tells it to go on without that call"
        } else {
            ""
        };
        return format!(
            "{shows} The window waits on it, and nothing is typed into it meanwhile. It is yours to answer in that window; if nobody does within {span}, ConsensFlow answers {} for it, which writes nothing{then}. It never answers yes.",
            quoted(&decline.choice)
        );
    }
    if let Some(decline) = decline.filter(|decline| !decline.listed) {
        // A key that closes it, no choice of its own listed: pressed, not chosen.
        let then = if decline.then_tell.is_some() {
            ", and tells it to go on without it"
        } else {
            ""
        };
        return format!(
            "{shows} ConsensFlow declines it for the window now, by pressing {}, which closes it and writes nothing{then}; nothing else is typed into the window meanwhile.",
            quoted(&decline.choice)
        );
    }
    if let Some(decline) = decline {
        return format!(
            "{shows} ConsensFlow declines it for the window now, choosing {}, which writes nothing; nothing else is typed into the window meanwhile.",
            quoted(&decline.choice)
        );
    }
    let closes = dialog
        .and_then(|dialog| dialog.closes_after_s)
        .map_or_else(String::new, |seconds| {
            format!(" Its harness closes it by itself within {seconds} s.")
        });
    format!(
        "{shows} The window waits on it, and nothing is typed into it meanwhile. ConsensFlow presses nothing in it: answer it in that window, or leave it.{closes}"
    )
}

/// A span of `ms` milliseconds in words: whole minutes, or seconds under one.
fn spoken(ms: i64) -> String {
    match (ms / 60_000, ms / 1_000) {
        (0, 1) => "1 second".to_owned(),
        (0, seconds) => format!("{seconds} seconds"),
        (1, _) => "1 minute".to_owned(),
        (minutes, _) => format!("{minutes} minutes"),
    }
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
