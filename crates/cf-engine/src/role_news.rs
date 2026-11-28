//! A setting that changes the chief's role while its window is open, told to
//! the window at its next rest.
//!
//! A chief's role text is built once, for a launch (`Dispatcher::role_text`), and
//! most of what it is built from is no setting: the work tiers, the window's `cf` and
//! the place of the skill change with a release or a launch, and the staff table
//! says "as of your launch" and sends the chief to `cf staff` for the staff as it
//! is. Three parts are the human's to change while the window is open: **the roles the
//! staff holds** (a member added, removed or given other roles: the chief's commands and
//! the paragraphs on when a role is given work are those of the held roles alone, so a
//! role gained or lost is told, and a second holder, or a tier, is not), and, in Project
//! settings (or Settings, for specs in every project), **the project's specs** (on or
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
//! **Every text built for the window** takes its role lines from the held roles that text
//! tells, and its table from the staff the window launched with: the role at a launch, the
//! note, the whole role where "Do with your own hands" goes off, and the text recorded as
//! told. So what is recorded is what was said.
//!
//! **A switch to another conversation** (`/clear`, `/new`, `/resume`) shows the chief a
//! conversation that holds the role its window launched with (its harness gives it by a
//! hook or by the system prompt it started with, on every harness whose session was
//! greeted), not what was told since: what the window was told is the launch's again, a
//! note that waits for the conversation left is withdrawn, and the next rest tells the new
//! conversation what it was not given. A session no hook greeted is given its role whole
//! (`greet_switched`).
//!
//! **The Codex resume** (`role_change`) compares the role a conversation was last told
//! with the role it is to be given, and writes the whole role again where they differ.
//! The note is a note of the ledger that says a role (`Ledger::note_role`), with the role
//! text the conversation holds once it is told: the text of the launch's own staff table
//! with the held roles, the specs and the rules as they are now. The ledger records it in the write that
//! confirms the delivery, so a conversation told now is not told the same change again
//! at its resume, and one never told (its note waited when the window went, and is
//! withdrawn at the launch, as every note of a role is) is told it in the whole role at
//! the resume. A staff that changed meanwhile is still the resume's to say: the
//! recorded text has the launch's staff table.
//!
//! **The skill** a chief turned on specs for is put in its place before it is told
//! where it is (`Adapter::place_specmint`): the files of ConsensFlow's own copy for the
//! variant and the folder, written as a launch writes them. Where that fails the human
//! is told once on the board, the chief is not told of the specs, and it has them at its
//! next launch: what else changed at the same rest (its roles, the rules, the switch) is told
//! and recorded all the same, with the specs it was told before.

use cf_harness::contract::Adapter;
use cf_ledger::{NewNote, ParticipantView, ProjectView};
use cf_proto::specs::SpecsSetting;

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::roles::news::Change;
use crate::roles::{held_roles, role_section, staff_of};
use crate::seams::EngineError;
use crate::specs::{ChiefSpecs, RoleText};

/// What a chief's role is built from that the human can change while its window is
/// open: the roles its staff holds (none for a chief that works with its own hands, whose
/// text names none), the specs the project has now (none: off), and the project's rules
/// for its staff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parts {
    held: Vec<&'static str>,
    specs: Option<SpecsSetting>,
    rules: Option<String>,
    own_hands: bool,
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
    /// What the window's launch told it: what a conversation it shows after a switch holds.
    launched: Option<Told>,
    waiting: Option<Written>,
    /// The parts the human was told on the board could not be told at all, so that
    /// it is not told again of the same.
    refused: Option<Parts>,
    /// The specs the human was told could not be put in place, so that they are not tried
    /// again until the project says others: the window is told what else changed with the
    /// specs it was told before.
    refused_specs: Option<Option<SpecsSetting>>,
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
            launched: told.clone(),
            told,
            ..Self::default()
        }
    }
}

/// The roles the chief's text says the staff of `project` holds: none for a chief that
/// works with its own hands, whose text names no role.
fn held_told(project: &ProjectView) -> Vec<&'static str> {
    if project.own_hands {
        return Vec::new();
    }
    held_roles(&staff_of(project))
}

/// The project's rules for its staff as the chief's role says them: a chief that does the
/// work with its own hands has no rules in its role (they are in the board's guide, as they
/// are now), so a change to them is no change to tell it.
fn rules_told(project: &ProjectView) -> Option<String> {
    project.staff_rules.clone().filter(|_| !project.own_hands)
}

impl Told {
    /// What a window given `specs` as its role's section of them is told of, in
    /// `project`.
    pub(crate) fn of(project: &ProjectView, specs: Option<&SpecsSetting>) -> Self {
        Self {
            parts: Parts {
                held: held_told(project),
                specs: specs.cloned(),
                rules: rules_told(project),
                own_hands: project.own_hands,
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
            held: held_told(project),
            specs: self.specs_setting(project, participant),
            rules: rules_told(project),
            own_hands: project.own_hands,
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

    /// `record`'s window shows another conversation that holds the role its launch gave,
    /// not what was told since: it was told what the launch told it, a note that waits for
    /// the conversation left is withdrawn, and the next look says what this one lacks.
    pub(crate) fn holds_launch_role(&self, record: &Record) -> Result<(), EngineError> {
        let written = {
            let mut part = record.window.borrow_mut();
            let Some(launched) = part.news.launched.clone() else {
                return Ok(());
            };
            part.news.told = Some(launched);
            part.news.seen = None;
            part.news.waiting.take()
        };
        let Some(written) = written else {
            return Ok(());
        };
        let queued = self
            .seams
            .ledger
            .borrow()
            .message(written.message)?
            .is_some_and(|message| message.state == "queued");
        if queued {
            self.seams.ledger.borrow_mut().cancel_message(
                written.message,
                "the window went to another conversation before it went in",
            )?;
            self.changed();
        }
        Ok(())
    }

    /// Tells `participant`'s window, a chief's, at rest in an open project, of what the
    /// human changed of its role since it was last told. Called at a look that found
    /// the window at rest, before the next message goes in: the note written here is
    /// the head of the queue when nothing waits before it.
    pub(crate) fn tell_role_news(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
    ) -> Result<(), EngineError> {
        // A window that was launched by a chief's launch has something it was told: a member's,
        // and a window that is closed, has none.
        if record.window.borrow().news.told.is_none() {
            return Ok(());
        }
        let mut wanted = self.parts_of(project, participant);
        // Specs that could not be put in place are not tried again while the project says
        // the same: the rest is told with what the window was told of them.
        {
            let mut part = record.window.borrow_mut();
            if part.news.refused_specs.as_ref() != Some(&wanted.specs) {
                part.news.refused_specs = None;
            } else if let Some(told) = part.news.told.as_ref() {
                wanted.specs = told.parts.specs.clone();
            }
        }
        if !self.settle_news(record, &wanted)? {
            return Ok(());
        }
        let (told, refused) = {
            let mut part = record.window.borrow_mut();
            // The human was told of one thing that could not be put in place: a setting that
            // is another now is another to try, and one that comes back is tried anew.
            if part.news.refused.as_ref() != Some(&wanted) {
                part.news.refused = None;
            }
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
    /// other is written); one that says what it does not is withdrawn. (A look at rest
    /// finds no note on its way: a window with a delivery on its way is not at rest.)
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
    /// `wanted`, with the skill put in its place when specs are on. A specs change that
    /// cannot be put in place holds back nothing else: the human is told of it once, and
    /// the window is told the rest, with the specs it was told before.
    fn write_news(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        told: &Told,
        mut wanted: Parts,
    ) -> Result<(), EngineError> {
        let harness = participant.harness.as_deref().unwrap_or_default();
        let Some(adapter) = self.seams.adapters.adapter(harness) else {
            return Ok(());
        };
        let mut made = self.told_role(project, participant, record, &*adapter, told, &wanted);
        if made.is_err() && wanted.specs != told.parts.specs {
            if let Err(cause) = made {
                self.specs_refused(project, participant, record, wanted.specs.clone(), &cause)?;
            }
            wanted.specs = told.parts.specs.clone();
            if wanted == told.parts {
                return Ok(());
            }
            made = self.told_role(project, participant, record, &*adapter, told, &wanted);
        }
        let role = match made {
            Ok(role) => role,
            Err(cause) => return self.news_refused(project, participant, record, wanted, &cause),
        };
        let cf = self.seams.roles.cf(participant);
        let roles = if told.parts.held == wanted.held || wanted.own_hands {
            None
        } else {
            match role_section(&wanted.held, &cf) {
                Ok(section) => Some(section),
                Err(cause) => {
                    let cause = EngineError::said("no-role-text", cause.to_string());
                    return self.news_refused(project, participant, record, wanted, &cause);
                }
            }
        };
        let now = role.specs.as_ref().map(crate::specs::ChiefSpecs::text);
        let change = Change {
            roles,
            specs: (told.parts.specs != wanted.specs)
                .then_some((told.parts.specs.as_ref(), now.as_ref())),
            rules: (told.parts.rules != wanted.rules)
                .then_some((told.parts.rules.as_deref(), wanted.rules.as_deref())),
        };
        let other = if wanted.own_hands {
            change.note_for(true)
        } else {
            change.note()
        };
        let body = if told.parts.own_hands != wanted.own_hands {
            // The role as it is now is told whole: the short text, and the ordinary chief's
            // whole role where the switch went off.
            if wanted.own_hands {
                crate::roles::news::own_hands_on(&role.instructions)
            } else {
                crate::role_change::body_of(&role.instructions, &cf)
            }
        } else if let Some(body) = other {
            body
        } else {
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

    /// The role as the window would be told it once it holds `wanted`: with the staff table
    /// it launched with, the held roles and the specs of `wanted`, and the skill of specs
    /// that changed put in its place. It is what its conversation is recorded to hold once
    /// the note is in.
    fn told_role(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        adapter: &dyn Adapter,
        told: &Told,
        wanted: &Parts,
    ) -> Result<RoleText, EngineError> {
        let mut as_launched = project.clone();
        as_launched.participants.clone_from(&told.staff);
        let role = self.role_text_holding(
            &as_launched,
            participant,
            adapter,
            &wanted.held,
            wanted.specs.clone(),
        )?;
        let launch = record.window.borrow().launch_id.clone();
        if wanted.specs != told.parts.specs {
            if let (Some(specs), Some(launch)) = (role.specs.as_ref(), launch.as_ref()) {
                adapter
                    .place_specmint(&specs.launch(), launch)
                    .map_err(|cause| EngineError::said("launch-failed", cause))?;
            }
        }
        Ok(role)
    }

    /// The human is told on the board, once, that the chief could not be told of the
    /// specs: it has them at its next launch.
    fn specs_refused(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        specs: Option<SpecsSetting>,
        cause: &EngineError,
    ) -> Result<(), EngineError> {
        record.window.borrow_mut().news.refused_specs = Some(specs);
        self.tell_human(
            project,
            format!(
                "@{} could not be told that the project's specs changed: {cause}. It has them when it next starts.",
                participant.handle
            ),
        )
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
        self.tell_human(
            project,
            format!(
                "@{} could not be told that its role changed: {cause}. It has the change when it next starts.",
                participant.handle
            ),
        )
    }

    /// A note of ConsensFlow's on the board for the human.
    fn tell_human(&self, project: &ProjectView, body: String) -> Result<(), EngineError> {
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body,
            },
        )?;
        Ok(())
    }
}
