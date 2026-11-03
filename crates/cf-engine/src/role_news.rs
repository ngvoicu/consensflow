//! A setting that changes the chief's role while its window is open, told to
//! the window at its next rest.
//!
//! A chief's role text is built once, for a launch (`Dispatcher::role_text`), and
//! most of what it is built from is no setting: the work tiers, the window's `cf` and
//! the place of the skill change with a release or a launch, and the staff table
//! says "as of your launch" and sends the chief to `cf staff` for the staff as it
//! is. Two parts are the human's to change in Project settings (or Settings, for
//! specs in every project) while the window is open: **the project's specs** (on or
//! off, the folder they are kept in, the Specmint variant) and **the project's rules
//! for its staff**. Before this a chief kept the role it launched with until its
//! next launch (a Resume, a switch, a window lost and opened again): specs turned on
//! while it ran were planned without, specs turned off were still written, and a new
//! folder was written to the old one. A member is told nothing: its window is its
//! task's, a member at rest has ended its turn or asks the chief a question, and the
//! window of its next task opens with the role as it is then.
//!
//! The window is told at its rest, on every harness, by the way every message goes
//! in (`next_in_line`): a note of ConsensFlow's to the chief, written at a look that
//! found the window at rest, as a wake-up is (`wake_ups`), so it waits for the human's
//! unsent words and for a chief mid-turn as any message does. It says what changed and
//! what to do now, and only the section of the role that changed (`roles::news`), not
//! the whole role.
//!
//! **What the window was told** is kept for the window (`WindowPart::news`), from its
//! launch: the parts its role text was built from. A look compares it with what the
//! project says now (cheap: no text is built unless they differ), so one change is one
//! note, however many looks come before it goes in. A note that waits when the project
//! changes again is withdrawn and said anew from what the window was told; one that
//! comes to say what the window was told is withdrawn and none is written. What a note
//! tells is taken as told once it is delivered (the ledger's own word for it), and the
//! window that is closed has told nothing: its next launch is given the role as it is.
//!
//! **The Codex resume** (`role_change`) compares the role a conversation was last told
//! with the role it is to be given, and writes the whole role again where they differ.
//! The note is a note of the ledger that says a role (`Ledger::note_role`), with the role
//! text the conversation holds once it is told: the text of the launch's own staff table
//! with the specs and rules as they are now. The ledger records it in the write that
//! confirms the delivery, so a conversation told now is not told the same change again
//! at its resume, and one never told (its note waited when the window went, and is
//! withdrawn at the launch, as every note of a role is) is told it in the whole role at
//! the resume. A staff that changed meanwhile is still the resume's to say: the
//! recorded text has the launch's staff table.
//!
//! **The skill** a chief turned on specs for is put in its place before it is told
//! where it is (`Adapter::place_specmint`): the files of ConsensFlow's own copy for the
//! variant and the folder, written as a launch writes them. Where that fails the human
//! is told once on the board, the chief is not, and it has the specs at its next launch.

use cf_ledger::{NewNote, ParticipantView, ProjectView};
use cf_proto::specs::SpecsSetting;

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::roles::news::Change;
use crate::seams::EngineError;
use crate::specs::{ChiefSpecs, RoleText};

/// What a chief's role is built from that the human can change while its window is
/// open: the specs the project has now (none: off), and the project's rules for its
/// staff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parts {
    specs: Option<SpecsSetting>,
    rules: Option<String>,
}

/// What a window was told of its role when it launched, or by a note since: the
/// parts, and the staff table its role text was written with, which a note that says
/// the role as it now is keeps (a staff that changed is no setting to tell).
#[derive(Debug, Clone)]
pub(crate) struct Told {
    parts: Parts,
    staff: Vec<ParticipantView>,
}

/// A note that tells a window of a change and waits to go in.
#[derive(Debug, Clone)]
struct Written {
    message: i64,
    /// The parts the window is told of once it is delivered.
    parts: Parts,
}

/// What a window's part of the record keeps for its news.
#[derive(Debug, Default)]
pub(crate) struct NewsPart {
    /// None for a member's window, and for one that is closed.
    told: Option<Told>,
    waiting: Option<Written>,
    /// The parts the human was told on the board could not be put in place, so that
    /// it is not told again of the same.
    refused: Option<Parts>,
    /// The parts the last look at rest found the project to say, where they were not
    /// what the window was told: a note is written when the next look finds the same.
    /// What the app says is read from a file at each look, and a read that fails reads
    /// as specs off: it is no change to tell a chief of.
    seen: Option<Parts>,
}

impl NewsPart {
    /// What a chief's window launched with is told of.
    pub(crate) fn launched(told: Option<Told>) -> Self {
        Self {
            told,
            ..Self::default()
        }
    }
}

impl Told {
    /// What a window given `specs` as its role's section of them is told of, in
    /// `project`.
    pub(crate) fn of(project: &ProjectView, specs: Option<&SpecsSetting>) -> Self {
        Self {
            parts: Parts {
                specs: specs.cloned(),
                rules: project.staff_rules.clone(),
            },
            staff: project.participants.clone(),
        }
    }
}

impl Dispatcher {
    /// What `project` says now of the parts of `participant`'s role that a setting
    /// changes.
    fn parts_of(&self, project: &ProjectView, participant: &ParticipantView) -> Parts {
        Parts {
            specs: self.specs_setting(project, participant),
            rules: project.staff_rules.clone(),
        }
    }

    /// A greeting that says the window its role as it is now (`greet_switched`) tells it
    /// the parts as they are: what it was told is that.
    pub(crate) fn told_by_greeting(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        role: &RoleText,
    ) {
        if participant.role != "chief" {
            return;
        }
        let mut part = record.window.borrow_mut();
        if part.news.told.is_some() {
            part.news.told = Some(Told::of(
                project,
                role.specs.as_ref().map(ChiefSpecs::setting),
            ));
        }
    }

    /// Tells `participant`'s window, a chief's at rest in an open project, of what the
    /// human changed of its role since it was last told. Called at a look that found
    /// the window at rest, before the next message goes in: the note written here is
    /// the head of the queue when nothing waits before it.
    pub(crate) fn tell_role_news(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
    ) -> Result<(), EngineError> {
        if participant.role != "chief" || project.state != "open" {
            return Ok(());
        }
        if record.window.borrow().news.told.is_none() {
            return Ok(());
        }
        let wanted = self.parts_of(project, participant);
        if !self.settle_news(record, &wanted)? {
            return Ok(());
        }
        let (told, refused) = {
            let part = record.window.borrow();
            (part.news.told.clone(), part.news.refused.clone())
        };
        let Some(told) = told else {
            return Ok(());
        };
        if told.parts == wanted {
            record.window.borrow_mut().news.seen = None;
            return Ok(());
        }
        if refused.as_ref() == Some(&wanted) {
            return Ok(());
        }
        // A change is told when two looks in a row find it.
        if record.window.borrow().news.seen.as_ref() != Some(&wanted) {
            record.window.borrow_mut().news.seen = Some(wanted);
            return Ok(());
        }
        // The role a launch said first is in the window before this: a note that
        // changes it goes after, and what it changes is the role as that note says it.
        if self.prelude_waits(record)? || self.greeting(record)?.is_some() {
            return Ok(());
        }
        self.write_news(project, participant, record, &told, wanted)
    }

    /// What became of the note written before, if one was: whether the look may go
    /// on to write one. A note that went in is told: the window holds what it says.
    /// One that waits and says what the project says still waits for its turn (no
    /// other is written); one that says what it does not is withdrawn; one that is on
    /// its way goes in.
    fn settle_news(&self, record: &Record, wanted: &Parts) -> Result<bool, EngineError> {
        let Some(written) = record.window.borrow().news.waiting.clone() else {
            return Ok(true);
        };
        let state = self
            .seams
            .ledger
            .borrow()
            .message(written.message)?
            .map(|message| message.state);
        match state.as_deref() {
            Some("delivering") => Ok(false),
            Some("queued") if written.parts == *wanted => Ok(false),
            Some("queued") => {
                self.seams.ledger.borrow_mut().cancel_message(
                    written.message,
                    "the project's settings changed again before it went in",
                )?;
                record.window.borrow_mut().news.waiting = None;
                Ok(true)
            }
            Some("delivered" | "read") => {
                let mut part = record.window.borrow_mut();
                if let Some(told) = part.news.told.as_mut() {
                    told.parts = written.parts;
                }
                part.news.waiting = None;
                Ok(true)
            }
            // Withdrawn or given up: the window was not told.
            _ => {
                record.window.borrow_mut().news.waiting = None;
                Ok(true)
            }
        }
    }

    /// Writes the note that tells `participant`'s window the change from `told` to
    /// `wanted`, with the skill put in its place when specs are on.
    fn write_news(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        told: &Told,
        wanted: Parts,
    ) -> Result<(), EngineError> {
        let harness = participant.harness.as_deref().unwrap_or_default();
        let Some(adapter) = self.seams.adapters.adapter(harness) else {
            return Ok(());
        };
        // The role as the window would be told it now, with the staff table it
        // launched with: what its conversation is recorded to hold once the note is in.
        let mut as_launched = project.clone();
        as_launched.participants.clone_from(&told.staff);
        let role = match self.role_text(&as_launched, participant, &*adapter) {
            Ok(role) => role,
            Err(cause) => return self.news_refused(project, participant, record, wanted, &cause),
        };
        let launch = record.window.borrow().launch_id.clone();
        if let (Some(specs), Some(launch)) = (role.specs.as_ref(), launch.as_ref()) {
            if let Err(cause) = adapter.place_specmint(&specs.launch(), launch) {
                let cause = EngineError::said("launch-failed", cause);
                return self.news_refused(project, participant, record, wanted, &cause);
            }
        }
        let now = role.specs.as_ref().map(crate::specs::ChiefSpecs::text);
        let change = Change {
            specs: (told.parts.specs != wanted.specs)
                .then_some((told.parts.specs.as_ref(), now.as_ref())),
            rules: (told.parts.rules != wanted.rules)
                .then_some((told.parts.rules.as_deref(), wanted.rules.as_deref())),
        };
        let Some(body) = change.note() else {
            return Ok(());
        };
        let note = self.seams.ledger.borrow_mut().note_role(
            project.id,
            &participant.handle,
            &body,
            &role.instructions,
        )?;
        {
            let mut part = record.window.borrow_mut();
            part.news.waiting = Some(Written {
                message: note.id,
                parts: wanted,
            });
            part.news.seen = None;
        }
        self.changed();
        Ok(())
    }

    /// The human is told on the board, once, that the chief could not be told: it
    /// has the change at its next launch.
    fn news_refused(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        wanted: Parts,
        cause: &EngineError,
    ) -> Result<(), EngineError> {
        record.window.borrow_mut().news.refused = Some(wanted);
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: format!(
                    "@{} could not be told that the project's specs or the rules for its staff changed: {cause}. It has them when it next starts.",
                    participant.handle
                ),
            },
        )?;
        Ok(())
    }
}
