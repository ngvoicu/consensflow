//! A role that changed under a conversation whose harness keeps its first
//! instructions.
//!
//! A launch gives its window the role text (`Launch::instructions`). Most
//! harnesses take it again at every launch, so a role that changed since a
//! conversation began reaches it when the conversation is resumed. A harness
//! that keeps the instructions a conversation began with
//! (`Adapter::keeps_first_instructions`, Codex) reads the new text at a resume
//! and does not use it. Probed on Codex 0.162.1 through `codex app-server`,
//! 2026-10-13: a thread started with the developer instructions "Your code word
//! is ROLEONE." answered ROLEONE to a turn asking for the code word; a second
//! app-server resumed that thread (`thread/resume`) with the developer
//! instructions "Your code word is now ROLETWO; this replaces any earlier code
//! word.", and the same turn was answered ROLEONE again. Neither the
//! `developer_instructions` setting of a resumed window's server nor that field of
//! the resume request replaces what the thread's turns read (a thread resumed by an
//! app-server given no instructions answered ROLEONE on Codex 0.164.0, 2026-10-26),
//! so a chief's specs turned on, a member added to the staff or a release that
//! rewrote a text would never reach a conversation that is resumed. The engine says
//! it itself. The resume is given the role all the same (its broker gives it to the
//! window's main start and to its resumes and forks of its own threads, T-1394): a
//! compaction rebuilds the thread's developer message from the session's own
//! instructions, and a resumed window
//! given none lost its role at its thread's next compaction (T-1388, Codex 0.167.0).
//! A role told by a note is then in the thread twice once it compacts, the note among
//! its messages and the text the resume was given as the developer's: the same text.
//!
//! A chief whose window is open when its specs or the rules for its staff change is told
//! of the change at its next rest, on every harness, in a note of that section alone
//! (`role_news`). That note is a note of a role too: it carries the role text the
//! conversation holds once it is in, recorded in the write that confirms its delivery,
//! so a resume that finds it equal to the role now says nothing, and one that finds
//! it different says the whole role as below.
//!
//! The ledger records the role text each such conversation was last told. A
//! launch that resumes one whose text now differs has a note from ConsensFlow
//! written: the new text in full, said plainly to replace the earlier one. The
//! note is not on the window's command line. The window opens with no first
//! message, and once it is up the note is the first delivery of its queue, by the
//! way every message goes in (`next_in_line`): then the message the launch would
//! have given first (a handoff, a brief, the note that its turn was cut), then
//! the rest of the queue in its order. The role goes in once, as the developer
//! instructions a window always had (which a resumed thread's turns do not read until
//! it compacts). What the launch owes the note
//! until it is told, and what comes of a launch that cannot tell it, is in
//! [`prelude`].
//!
//! The ledger knows a replacement by the role text it holds
//! (`Ledger::note_role`), and by nothing in its words. A note of ConsensFlow's
//! about no task is not enough: the engine writes others to a chief (its handoff,
//! the note that its turn was cut), and a message that quotes the words of a
//! replacement is none. So every place that tells one apart (the replacements that
//! wait, the one that is confirmed, the turn of a member's record that answers
//! one) asks the ledger for its id, and a turn of a record is a replacement's by
//! the id in its header (`header_id`).
//!
//! The text is recorded when the note is delivered, not when it is written, and
//! in the very write that confirms the delivery (`confirm_delivered`, at the
//! places a delivery is confirmed), so that what the ledger holds is what the
//! conversation has really been told, and neither stands without the other. A note
//! that never comes in (its window did not open, went before the harness's record
//! showed it, or its delivery failed) leaves the record as it was, and the launch
//! after says the role again. One that was given back to the queue, its window
//! closed before the daemon saw it arrive, may have come in all the same: a launch
//! looks for it in the conversation's own record before it withdraws anything
//! (`settle_replacements`), and one that is there is confirmed and recorded, so
//! that a role the thread was told is never taken for one it was not: a human who
//! went back to the role the conversation began with is not left with a thread
//! that holds the other.
//!
//! A note that still waits when a launch begins is withdrawn, whatever the launch
//! is: it was written for a window that never took it, and a conversation that
//! begins now, or one on another harness, is not the one it says a role to. The
//! role is said anew only where the resumed conversation was told something else,
//! so that a conversation is not told a role twice over, nor one that is no
//! longer its role.
//!
//! Whether the window goes on with the conversation at all is decided before the
//! role is compared (`conversation_to_go_on_with`): a conversation its harness
//! holds nothing of (a Codex thread that never had a turn has no record for a
//! resume to find) is not resumed, its window opens afresh with the role as it is
//! now, as the window of any conversation that begins, and there
//! is nothing to tell it: no note is written for it, and the text it begins with
//! is what is recorded of it. A chief's launch asks its harness first, its handoff
//! depending on the answer. A member's first message is begun, and the stop its
//! window owes read, before its harness is asked, so that a pause that comes
//! during the ask is seen; its role is compared once the ask is answered, in the
//! plan of its launch, and where it is then to be said first the message that was
//! begun goes back to its queue with its attempt, to go in after the note
//! ([`prelude`]).
//!
//! A role too long for one message (more than 16,000 units: a chief's at any staff,
//! with specs on or not, on the real text, each command of which names the window's
//! `cf` by its whole path, T-1049; a staff of more than 24 rows without specs, two
//! roles to a row, before that path, and before each row named its model, harness-models; it was 37 and 65 before the chief's rules for
//! smaller contexts, T-1009, 2 and 30 before the sentence on the staff's rules,
//! T-1012, and 0 and 27 before the words the critical review asked back, T-1074) is
//! delivered cut to its opening, with the way to read the rest (`cf inbox read`),
//! and its opening says to read the rest now ([`OPENING_IN_PART`]). It is not given
//! in several messages. Delivered so, it is a role told in part, and not recorded
//! as told; it is told once the window has read it: `cf inbox read` printed the
//! note whole and said so, and the ledger then records the role told, in the
//! write that marks the note read (`receive_read`). A window runs that command the
//! moment it has the note, as the note tells it to, which is often before the
//! daemon has seen the paste arrive: a read that comes while the note is still
//! `delivering` is kept with it (`cf_ledger`'s `keep_read`, in its receipt) and told
//! by the write that confirms the delivery (`confirm_delivery`), in the conversation
//! it was made in. `cf` prints a role in one piece up to a bound of its own
//! (`cf_base::role_read`, 29,000 bytes and 500 lines, measured on Codex and Muse, the
//! harnesses a role goes to by a note, less the room its line takes): a staff of 141
//! rows with specs on, 166 without, two roles to a row and a name of twenty characters, the
//! model's id and effort, which a member is named by, on the same text (its bytes are the
//! bound; 143 and 168 before the ban on a task for a member by name came back, T-1513;
//! 151 and 174 before the chief was told of the four roles of spec new-roles, T-1480; 133 and 152 with a column of models in the table, 195 and 223 before it,
//! harness-models: the name says the model, so T-1234 took the column out, and the words
//! on the designer's order and on the chief's own model took some of the room it made).
//! A role longer than that `cf` prints in parts, each within the bound
//! (`cf_base::role_read::role_parts`, cut at a paragraph where it can be, and where a
//! table goes on past a part, its line says so): the first by the command the paste
//! gives, each next by the command the part before ends with (`--part N`), and the
//! opening says in how many ([`Telling::in_parts`]: two to a staff of 143 rows with
//! specs on and the most rules, three at 300). Each part printed is said to the board, and
//! logged as `message.part_read` (`receive_read_part`) whatever state the note is in: the
//! role is read once every part was printed in the conversation it goes to, in any order,
//! and told then or once its paste is confirmed, whichever is last (T-1513, review T-1510:
//! the human chose parts over a smaller text, 2026-11-12; review T-1516).
//!
//! The project's rules for its staff (`roles::with_staff_rules`) are part of
//! every text that says a role, and the ledger bounds them for it (at most 8,000
//! bytes and 150 lines, `cf_ledger::STAFF_RULES_MAX_BYTES`): a member's real text
//! with the most rules goes into its window whole, and a chief's with specs on,
//! the most rules and a staff of up to 5 rows (30 without specs) is printed in one piece
//! (7 and 32 read whole before the ban on a task for a
//! member by name, T-1513; 15 and 38 before the four roles of spec new-roles, T-1480; 19 and 39 with the model column, 29 and 58 before it), and a
//! larger one in parts: a row of the table is 60 bytes, and the text without a row is
//! about 28,641.
//! A role read so is said nothing of at the next Resume, unless it changed again.
//! One the window has not read by its next Resume is said again then, and the
//! human has a line on the board that says so ([`Dispatcher::not_read`]).
//! Nothing is said to the human of a role told in part before then: the window
//! may read it at once.
//!
//! It is the text as the window would be given it that is compared. For a chief
//! given specs that text names the Specmint skill by its path, and the path is
//! the same at every launch (the harness crate keeps one copy of ConsensFlow's
//! for each harness and variant, outside every launch's folder), so a Resume says
//! nothing of the role unless something changed: the project's specs, their
//! variant or their folder, the staff, a text a release rewrote, a release that
//! vendors another commit of the skill, or the harness's own copy of the skill
//! turning up or going. A conversation whose text is not known (one from before
//! the record, or one the human switched their window to) counts as changed, and
//! is told once.
//!
//! A harness that takes its role at every launch is never asked, and its launch
//! is what it always was.
//!
//! It asks nothing of a Windows launch's command line: a Codex window's role is in
//! a file its supervisor reads (the harness's `codex/role.rs`), and the note goes in
//! by paste once the window is up, which has no such limit.
//!
//! What is written down and not done (the limits of this):
//!
//! - A launch that fails and is tried again by itself (a chief, at the backoff)
//!   is tried with the head of its queue as the message it would have begun
//!   with, and a handoff keeps its place (`chief_first`). The note that its turn
//!   was cut, which a Resume wrote and put ahead of older work, does not: it
//!   goes behind what it was ahead of. The first launch would have said the
//!   role, the note and then the older work; the next says the role, the older
//!   work and then the note. The priority of the continuation is lost with a
//!   failed launch.
//! - What a launch finds in the conversation's own record is taken as it is
//!   read. Where the record cannot be read (a file that is not there for a
//!   moment, an adapter that fails) a replacement that waited is taken as not
//!   received: it is withdrawn, and the role is said again at the next launch. A
//!   thread that took it meanwhile then holds the role twice, and a human who
//!   had gone back to the role the conversation began with is not told that the
//!   thread holds another. A replacement whose delivery was given up is not
//!   looked for at all: it failed, and nothing reconciles it with what the
//!   thread may hold. Unreadable evidence is not told from none.
//! - A role told in part (cut at 16,000 units) and not read is not recorded, so
//!   the role the conversation was told before stays recorded: where the human
//!   goes back to that one, the launch says nothing, though the thread holds the
//!   cut words after it. A staff that shrinks, or a role that changes again, tells
//!   the thread a whole one.
//! - The parts read are counted per note, not per conversation: a window that read the
//!   first part, was cleared, and read the last in the new conversation, is told the role
//!   in that one. `cf inbox read --json` prints a role whole, as it did, and says it read
//!   only within the bound.
//! - A read kept while the note was on its way tells the role only in the conversation
//!   it was made in. One that began between the read and the confirmation (a `/clear`
//!   while the paste was on its way) was not shown what the read printed: it is told
//!   nothing by it, and the role is said again at its next Resume, with the line.
//! - A role note read while its paste was on its way and then given up (failed) is
//!   failed as any is: the read is not looked for, as the thread is not (the bullet
//!   above on a delivery given up). Any other message so read is `read`, not failed.
//! - A note whose paste is given back to the queue after the window read it (the
//!   window went before the daemon saw the paste arrive) keeps the read: a read does
//!   not receive a role, so the note waits for its paste, and the read tells the role
//!   if it comes in after all (`settle_replacements`, or the next paste of it). A
//!   launch withdraws a note that still waits, with the read.
//! - A resumed Codex window that is given no first message has the turn its
//!   record ended in read as the old process's, not as its own, until it begins
//!   one (`LeftOpen`), whether or not a replacement is due. That is a different
//!   reading from the one `main` made of every resume with no message, whatever
//!   the role.
//! - Attempts are spent within one window as much as across windows. A paste the
//!   harness refuses or never shows is given again, and given up after as many
//!   attempts as any message is given, so a paste that was taken and did not show
//!   can arrive twice. A replacement the harness's own queue took (a Codex
//!   broker's admission) is waited for in the record as any message that was
//!   queued is, with no time it is given up at.
//! - What goes into a chief's window after the replacement is given up as any
//!   message is where the harness refuses it as many times as a message is
//!   given attempts: the handoff or the note that its turn was cut is then lost
//!   as it would be at any other moment. It is never given up for being slow, and
//!   goes back to its queue with its window.
//! - A chief whose window went before it was told its role is tried again at the
//!   backoff where something waits for it, as a chief that could not start is;
//!   with nothing queued it waits for what comes. A member's message that the
//!   role went ahead of, and that rides in the paste of another by the time its
//!   launch is given up (its task was paused and resumed while the window
//!   waited), cannot fail with the launch: it goes with its carrier, which is
//!   the message the launch that follows is for.
//! - A pause that comes while the window of a member's launch that says the role
//!   first is prepared does not stop it from opening, as one does a launch whose
//!   message is begun and held (`launch_over`): nothing is begun for it to hold, so
//!   the window opens with the note, and the message waits on its paused task.

mod prelude;
#[cfg(test)]
mod tests;

use cf_base::js;
use cf_base::role_read::role_parts;
use cf_harness::records::{Item, Role};
use cf_ledger::{ConversationView, MessageView, NewNote, ParticipantView, ProjectView};
use cf_proto::cf_word::CfWord;
use serde_json::{json, Value};

use crate::delivery_text::{fits_whole, goes_whole, header_id};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::specs::RoleText;

pub(crate) use prelude::Prelude;

/// What a replacement says before the role text.
pub(crate) const OPENING: &str = "Your ConsensFlow role instructions have changed since this conversation began. The instructions below replace the ones you were given then: follow these from now on, and where they differ from what you were told before, these are right. This is context, not a task: say in one short line that you have them, and wait for the next message.";

/// What a replacement says before the role text where the two together are too
/// long for one message: the window is given only their opening and the command
/// that prints the rest, and is told to read the rest now.
pub(crate) const OPENING_IN_PART: &str = "Your ConsensFlow role instructions have changed since this conversation began. The instructions below replace the ones you were given then: follow these from now on, and where they differ from what you were told before, these are right. They are too long for one message, so only their opening follows; the line at its end gives the command that prints all of them. Run that command now, before anything else, and read everything it prints. This is context, not a task: once you have read them, say in one short line that you have them, and wait for the next message.";

/// What [`OPENING_IN_PART`] and [`BEGAN_WITHOUT_IN_PART`] say of the rest where `cf inbox
/// read` prints the note in `parts` parts, as it does a role it cannot print within the
/// bound its window reads whole at (`cf_base::role_read::role_parts`).
fn read_in_parts(parts: usize) -> String {
    format!("They are too long for one message, so only their opening follows, and too long for one read, so the command the line at its end gives prints them in {parts} parts: the first, which ends with the command that prints the next. Run that command now, before anything else, then the command each part ends with, until a part says it is the last, and read everything they print.")
}

/// What a note that gives the role to a session no hook greeted says before the role
/// text ([`Dispatcher::greet_switched`]).
pub(crate) const BEGAN_WITHOUT: &str = "This conversation began without your ConsensFlow role instructions: it was started in your window (a /clear, or a new session), and your harness gave it none. They follow in full: follow them from now on. This is context, not a task: say in one short line that you have them, and wait for the next message.";

/// [`BEGAN_WITHOUT`] where the note is too long for one message.
pub(crate) const BEGAN_WITHOUT_IN_PART: &str = "This conversation began without your ConsensFlow role instructions: it was started in your window (a /clear, or a new session), and your harness gave it none. They are too long for one message, so only their opening follows; the line at its end gives the command that prints all of them. Run that command now, before anything else, and read everything it prints: follow them from now on. This is context, not a task: once you have read them, say in one short line that you have them, and wait for the next message.";

/// What a note that says a role says before it: that it replaces the role the conversation
/// began with ([`OPENING`]), or that it gives one to a session that began with none
/// ([`BEGAN_WITHOUT`]).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Telling {
    Replaces,
    BeganWithout,
}

impl Telling {
    /// Its opening where the note goes in whole.
    fn whole(self) -> &'static str {
        match self {
            Self::Replaces => OPENING,
            Self::BeganWithout => BEGAN_WITHOUT,
        }
    }

    /// Its opening where the note goes in cut, and `cf inbox read` prints it whole.
    fn in_part(self) -> &'static str {
        match self {
            Self::Replaces => OPENING_IN_PART,
            Self::BeganWithout => BEGAN_WITHOUT_IN_PART,
        }
    }

    /// Its opening where the note goes in cut, and `cf inbox read` prints it in `parts`.
    fn in_parts(self, parts: usize) -> String {
        match self {
            Self::Replaces => format!("Your ConsensFlow role instructions have changed since this conversation began. The instructions below replace the ones you were given then: follow these from now on, and where they differ from what you were told before, these are right. {} This is context, not a task: once you have read them all, say in one short line that you have them, and wait for the next message.", read_in_parts(parts)),
            Self::BeganWithout => format!("This conversation began without your ConsensFlow role instructions: it was started in your window (a /clear, or a new session), and your harness gave it none. {} Follow them from now on. This is context, not a task: once you have read them all, say in one short line that you have them, and wait for the next message.", read_in_parts(parts)),
        }
    }
}

/// Why a greeting that waited is withdrawn: the session was given its role since.
const GREETED_SINCE: &str = "a hook of the harness gave the session its role since";

/// Why a replacement that waited is withdrawn.
const WITHDRAWN: &str = "the window it was written for never took it";

/// What a launch's first message and role text come to, once the role was
/// compared with the one the conversation was last told.
pub(crate) struct Compared {
    /// The launch's first message, on its command line: the message the launch
    /// was given, but for a replacement that waited and was withdrawn; and none
    /// where the role is to be said first, since the window then opens with none.
    pub(crate) first: Option<MessageView>,
    /// What goes into the window before anything else of its queue once it is up:
    /// the replacement of the role, and the message it displaced, if there was
    /// one. None where the role is not to be said.
    pub(crate) prelude: Option<Prelude>,
    /// The role text the comparison was made with, for the launch to use and not
    /// ask for again; none where there was nothing to compare.
    pub(crate) role: Option<RoleText>,
}

impl Dispatcher {
    /// What a launch of `participant` finds in the conversation's own record
    /// before it begins: a replacement given back to the queue (its window closed
    /// before the daemon saw it arrive) that the record shows the window took. It
    /// was told, whatever the ledger says: it is confirmed and the role recorded,
    /// and not withdrawn as a note that never came in. The ones it confirmed.
    pub(crate) async fn settle_replacements(
        &self,
        participant: &ParticipantView,
    ) -> Result<Vec<i64>, EngineError> {
        let waiting = self.waiting_replacements(participant)?;
        let mut told = Vec::new();
        for note in waiting {
            let shown = self
                .recorded_paste_of(participant.id, note.id, None, None)
                .await?;
            if let Some(item) = shown {
                let begun = self.seams.ledger.borrow_mut().begin_delivery(note.id)?;
                self.confirm_delivered(&begun.message, &json!({ "item": item }))?;
                told.push(note.id);
            }
        }
        Ok(told)
    }

    /// The launch of `participant`'s window in `project`, `first` its first
    /// message if it has one: a conversation that is resumed, on a harness that
    /// keeps its first instructions, whose recorded text is not the one its
    /// window would be given now has the replacement of its role written, to go
    /// in first once the window is up; the window opens with no first message. A
    /// replacement that still waits is withdrawn before, whatever this launch is.
    pub(crate) fn compare_role(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        resumed: Option<&ConversationView>,
        first: Option<MessageView>,
    ) -> Result<Compared, EngineError> {
        let withdrawn = self.withdraw_replacements(participant)?;
        let first = first.filter(|message| !withdrawn.contains(&message.id));
        let apart = |first| {
            Ok(Compared {
                first,
                prelude: None,
                role: None,
            })
        };
        let Some(conversation) = resumed else {
            return apart(first);
        };
        let harness = participant.harness.as_deref().unwrap_or_default();
        let kept = self
            .seams
            .adapters
            .adapter(harness)
            .filter(|adapter| adapter.keeps_first_instructions());
        // A launch that cannot go (no adapter) says why in its own step, which asks for
        // the text again where this could not.
        let Some(adapter) = kept else {
            return apart(first);
        };
        let Ok(role) = self.role_text(project, participant, &*adapter) else {
            return apart(first);
        };
        let told = self
            .seams
            .ledger
            .borrow()
            .conversation_role_text(conversation.id)?;
        if told.as_deref() == Some(role.instructions.as_str()) {
            return Ok(Compared {
                first,
                prelude: None,
                role: Some(role),
            });
        }
        let unread = self.unread_role(participant, &role.instructions)?;
        let replacement = self.replace_role(project, participant, &role)?;
        if let Some(unread) = unread {
            self.not_read(&unread, replacement.id)?;
        }
        let prelude = Prelude {
            replacement: replacement.id,
            displaced: first.as_ref().map(|message| message.id),
            chief: participant.role == "chief",
            since: self.now(),
            begun: false,
        };
        Ok(Compared {
            first: None,
            prelude: Some(prelude),
            role: Some(role),
        })
    }

    /// A window its human switched to `session`, a session no hook of its harness
    /// greeted with the role text as it began (`Window::greeted`: Muse Code's `/clear`
    /// runs no SessionStart hook), began that conversation with none. On a harness that
    /// takes its role at every launch (no `keeps_first_instructions`: one that keeps them
    /// is told at its launches, `compare_role`), the role is written for it and goes in
    /// first (`DeliveryPart::greeting`), and is recorded told once delivered. It is no
    /// launch's prelude: the window is up, and is given the note as it is given any
    /// message, its failure any message's (T-931 P1-1). A session a hook greeted, one
    /// already told the role as it is now, a window whose launch prelude is not done
    /// with, and one whose greeting still waits (it goes into the newer session) are
    /// given nothing.
    pub(crate) fn greet_switched(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        session: &str,
    ) -> Result<(), EngineError> {
        let window = record.window.borrow().window.clone();
        if window.is_none_or(|window| window.greeted(session))
            || self.prelude_waits(record)?
            || self.greeting(record)?.is_some()
        {
            return Ok(());
        }
        let harness = participant.harness.as_deref().unwrap_or_default();
        let Some(adapter) = self
            .seams
            .adapters
            .adapter(harness)
            .filter(|adapter| !adapter.keeps_first_instructions())
        else {
            return Ok(());
        };
        let Ok(role) = self.role_text(project, participant, &*adapter) else {
            return Ok(());
        };
        let conversation = self.seams.ledger.borrow().current_conversation(record.id)?;
        if let Some(conversation) = conversation {
            let told = self
                .seams
                .ledger
                .borrow()
                .conversation_role_text(conversation.id)?;
            if told.is_some_and(|told| told == role.instructions) {
                return Ok(());
            }
        }
        let note = self.seams.ledger.borrow_mut().note_role(
            project.id,
            &participant.handle,
            &note_of(
                Telling::BeganWithout,
                &role.instructions,
                &self.seams.roles.cf(participant),
            ),
            &role.instructions,
        )?;
        record.delivery.borrow_mut().greeting = Some(note.id);
        // It says the role as it is now: what a note of a setting's change would say
        // is said already (`role_news`).
        self.told_by_greeting(project, participant, record, &role);
        self.changed();
        Ok(())
    }

    /// The greeting of `record`'s window while it waits to go in (`greet_switched`), as
    /// the ledger has it now. One that went in, was given up or withdrawn is let go. One
    /// whose session a hook of the harness greeted since (a later harness that runs it on
    /// a `/clear` at its first prompt) is withdrawn: the session has its role (T-931 P3-4).
    pub(crate) fn greeting(&self, record: &Record) -> Result<Option<MessageView>, EngineError> {
        let Some(id) = record.delivery.borrow().greeting else {
            return Ok(None);
        };
        let note = self.seams.ledger.borrow().message(id)?;
        let Some(note) = note.filter(|note| matches!(note.state.as_str(), "queued" | "delivering"))
        else {
            record.delivery.borrow_mut().greeting = None;
            return Ok(None);
        };
        if note.state == "queued" && self.greeted_since(record)? {
            self.seams
                .ledger
                .borrow_mut()
                .cancel_message(note.id, GREETED_SINCE)?;
            record.delivery.borrow_mut().greeting = None;
            self.changed();
            return Ok(None);
        }
        Ok(Some(note))
    }

    /// Whether a hook of the harness greeted the session `record`'s window shows now.
    fn greeted_since(&self, record: &Record) -> Result<bool, EngineError> {
        let window = record.window.borrow().window.clone();
        let conversation = self.seams.ledger.borrow().current_conversation(record.id)?;
        let session = conversation.and_then(|conversation| conversation.native_session);
        Ok(window
            .zip(session)
            .is_some_and(|(window, session)| window.greeted(&session)))
    }

    /// Whether what a launch or a switch put ahead of `record`'s queue (its prelude) is
    /// not done with: its replacement still owed ([`Dispatcher::owed`], a failed one
    /// included, which the next look settles), or the message it displaced still to go.
    pub(crate) fn prelude_waits(&self, record: &Record) -> Result<bool, EngineError> {
        if self.owed(record)?.is_some() {
            return Ok(true);
        }
        let displaced = record
            .delivery
            .borrow()
            .prelude
            .as_ref()
            .and_then(|prelude| prelude.displaced);
        let Some(displaced) = displaced else {
            return Ok(false);
        };
        Ok(self
            .seams
            .ledger
            .borrow()
            .message(displaced)?
            .is_some_and(|message| matches!(message.state.as_str(), "queued" | "delivering")))
    }

    /// The replacements of `participant`'s role that still wait, as the ledger
    /// knows them.
    fn waiting_replacements(
        &self,
        participant: &ParticipantView,
    ) -> Result<Vec<MessageView>, EngineError> {
        let ledger = self.seams.ledger.borrow();
        let notes = ledger.role_notes_to(participant.id)?;
        Ok(ledger
            .pending(participant.id)?
            .into_iter()
            .filter(|message| message.state == "queued" && notes.contains(&message.id))
            .collect())
    }

    /// The replacements of `participant`'s role that still wait, withdrawn: their
    /// window went before they landed. They say what may not be the role now, or
    /// what a replacement of this launch will say again.
    pub(crate) fn withdraw_replacements(
        &self,
        participant: &ParticipantView,
    ) -> Result<Vec<i64>, EngineError> {
        let waiting = self.waiting_replacements(participant)?;
        let mut ledger = self.seams.ledger.borrow_mut();
        let mut withdrawn = Vec::new();
        for note in waiting {
            ledger.cancel_message(note.id, WITHDRAWN)?;
            withdrawn.push(note.id);
        }
        Ok(withdrawn)
    }

    /// The note that says `role` to `participant`, written. What its
    /// conversation was told is recorded when it is delivered.
    fn replace_role(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        role: &RoleText,
    ) -> Result<MessageView, EngineError> {
        Ok(self.seams.ledger.borrow_mut().note_role(
            project.id,
            &participant.handle,
            &body_of(&role.instructions, &self.seams.roles.cf(participant)),
            &role.instructions,
        )?)
    }

    /// The harness's own record proves `message` arrived: it is confirmed. A
    /// replacement of a role that went in whole also records the conversation as
    /// told it, in the same write. One that was cut (more than a message goes in
    /// whole at) tells the conversation its role in part: it is not recorded until
    /// the window reads it whole (`cf inbox read`, which the ledger records). One
    /// whose recipient has no current conversation any more (it ended while the
    /// note was on its way) is confirmed and records nothing: there is no
    /// conversation to have told.
    pub(crate) fn confirm_delivered(
        &self,
        message: &MessageView,
        receipt: &Value,
    ) -> Result<(), EngineError> {
        let tells = self.tells_a_role(message)?;
        let mut ledger = self.seams.ledger.borrow_mut();
        if tells == Some(true) {
            ledger.confirm_delivery_telling(message.id, Some(receipt))?;
        } else {
            ledger.confirm_delivery(message.id, Some(receipt))?;
        }
        // Its window took work: it came up, whatever its record showed before (CHF-06).
        if let Some(record) = self.record(message.recipient_id) {
            record.window.borrow_mut().took = true;
        }
        Ok(())
    }

    /// Whether `message` says its recipient's conversation a role: whole (`true`),
    /// which its confirmation records as told, or cut (`false`), which its read
    /// records. None for a message that says no role.
    pub(crate) fn tells_a_role(&self, message: &MessageView) -> Result<Option<bool>, EngineError> {
        let says_a_role = self
            .seams
            .ledger
            .borrow()
            .role_notes_to(message.recipient_id)?
            .contains(&message.id);
        let cf = self.cf_for(message)?;
        Ok(says_a_role.then(|| goes_whole(message, &[], &cf)))
    }

    /// The last note that told `participant` the role `instructions` and that
    /// its window has and has not read: a note that reached the window (the last
    /// of those that did), cut to fit in one message, delivered and not read since.
    /// None where the last one to reach it went in whole, was read, or said
    /// another role.
    fn unread_role(
        &self,
        participant: &ParticipantView,
        instructions: &str,
    ) -> Result<Option<MessageView>, EngineError> {
        let cf = self.seams.roles.cf(participant);
        let ledger = self.seams.ledger.borrow();
        for id in ledger.role_notes_to(participant.id)?.into_iter().rev() {
            let Some(note) = ledger.message(id)? else {
                continue;
            };
            if !matches!(note.state.as_str(), "delivered" | "read") {
                continue;
            }
            let unread = note.state == "delivered"
                && !goes_whole(&note, &[], &cf)
                && ledger.role_of_note(id)?.as_deref() == Some(instructions);
            return Ok(unread.then_some(note));
        }
        Ok(None)
    }

    /// The human is told on the board that `unread`, a role cut to fit in one
    /// message, was not read by its window before this Resume, which tells it the
    /// role again in the note `again`. The cut is at 16,000 UTF-16 units, which a
    /// person counts as characters but for the few that take two (an emoji), so the
    /// line says "about 16,000 characters".
    pub(crate) fn not_read(&self, unread: &MessageView, again: i64) -> Result<(), EngineError> {
        self.seams.ledger.borrow_mut().note(
            unread.project_id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: unread.task_number,
                body: format!(
                    "@{} did not read its new role before this Resume: it is too long for one message, and m-{} went in cut at about 16,000 characters with `cf inbox read m-{}` for the rest. It is told again now, in m-{again}.",
                    unread.recipient, unread.id, unread.id
                ),
            },
        )?;
        Ok(())
    }
}

/// What a note that replaces the role with `instructions` says, in the window whose `cf`
/// is `cf`: the words that say so, and the text in full.
fn body_of(instructions: &str, cf: &CfWord) -> String {
    note_of(Telling::Replaces, instructions, cf)
}

/// A note that says `instructions` after the opening of `telling`, in the window whose `cf`
/// is `cf`: the opening for a note that goes in whole, where the two go into a window in
/// one message; else the one that tells the window to read the rest now, where `cf inbox
/// read` prints it in one piece; else the one that says in how many parts it prints it and
/// to read them all. The count is taken of the note with that opening in it, whose words
/// move where the parts are cut, until the opening says the count of its own note.
fn note_of(telling: Telling, instructions: &str, cf: &CfWord) -> String {
    let note = format!("{}\n\n{instructions}", telling.whole());
    if fits_whole(&note) {
        return note;
    }
    let note = format!("{}\n\n{instructions}", telling.in_part());
    let mut parts = role_parts(&cf.in_text(&note)).len();
    if parts == 1 {
        return note;
    }
    loop {
        let note = format!("{}\n\n{instructions}", telling.in_parts(parts));
        let counted = role_parts(&cf.in_text(&note)).len();
        // A count that moves its own cut by a digit settles at the larger.
        if counted <= parts {
            return note;
        }
        parts = counted;
    }
}

/// What a member wrote in answer to its task: where its answer ends, and its final
/// answer.
pub(crate) struct Answers<'a> {
    /// The last assistant item that answers the task: the answer is over when it is
    /// complete. None where a message to the member came after it (a notification of
    /// its harness's that woke a turn), whose answer is still to come.
    pub(crate) last: Option<&'a Item>,
    /// What the member wrote after the last tool call or message it was given: its
    /// final answer, in the items it was written in. What it wrote before is the
    /// progress of its turn, which stays in the transcript, and so is all it wrote
    /// of a turn that ended before, which a job's or an agent's end woke it to go on
    /// after. Where it wrote no words after its last tool call, the last item of that turn
    /// that has some is its final answer, so that a result is never empty (the chief's
    /// decision on T-262): what decides on the words after the last tool call alone reads
    /// `after_the_last_tool`. A run so taken says it is ([`Run::last_words`]).
    pub(crate) final_run: Run<'a>,
    /// The final answer as it stood at the last complete item the member wrote: what
    /// Finish takes where the turn after it is still under way (a call open, a
    /// notification's turn begun). None where it wrote none complete.
    pub(crate) complete_run: Option<Run<'a>>,
    /// What the member wrote after the message it was given and before its first tool call,
    /// in the items it was written in, where a tool call came after it: its reply. A turn
    /// that answers a question puts the answer first, and goes on; the words of that answer
    /// are not the turn's last (T-1151: Devin, Claude Code, Codex and Muse all write text, run
    /// a tool, and write the last words after it). A harness's own words in the member's place
    /// (`synthetic`) are no reply; a note it marks as progress (Codex's `commentary`) is the
    /// reply all the same, as it is the first words a question gets. Holds no words where the
    /// member ran a tool before it wrote (a call that says nothing is an item with none),
    /// where it ran none (its one run is the final answer), or where it wrote nothing.
    pub(crate) reply: Vec<&'a Item>,
    /// What the member wrote after the last tool call or message it was given, and nothing
    /// in its place where that holds no words (`final_run` takes the last words before it
    /// then). A tell's turn that answered with `cf answer` and wrote nothing after it is told
    /// nothing more (`turn_end::paused`; Calliope's P2 on T-262, T-878).
    pub(crate) after_the_last_tool: Vec<&'a Item>,
}

/// A final answer, in the items it was written in (what it derefs to), and whether those are
/// the last words its turn wrote before its last tool call, which a result takes where the
/// turn wrote none after it (T-262): a result of them says so above them, so that the chief
/// can tell the last words the turn had from a report (`turn_end::words_of`; T-1310's review
/// of T-878, T-1311).
#[derive(Debug, Clone, Default)]
pub(crate) struct Run<'a> {
    items: Vec<&'a Item>,
    pub(crate) last_words: bool,
}

impl<'a> std::ops::Deref for Run<'a> {
    type Target = Vec<&'a Item>;

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

/// Whether `item` holds words of the member's: some text, and not the harness's own (a
/// note it marks as its progress, `commentary`, or words it wrote in the member's place,
/// `synthetic`).
fn has_words(item: &Item) -> bool {
    !item.commentary && !item.synthetic && !js::trim(&item.text).is_empty()
}

/// A final run, or where it has no words of the member's, the last item with some (`said`),
/// which are its turn's last words.
fn with_words<'a>(run: &[&'a Item], said: Option<&'a Item>) -> Run<'a> {
    match said {
        Some(said) if !run.iter().any(|item| has_words(item)) => Run {
            items: vec![said],
            last_words: true,
        },
        _ => Run {
            items: run.to_vec(),
            last_words: false,
        },
    }
}

/// The answer to the task in `items`, a member's record after its latest message: what
/// is of a turn that answers a replacement of the role is not, and none where nothing
/// else answers. Where a message to the member came after its last words, what answers
/// that is still to come (no `last`), and the last complete answer is kept all the
/// same, for Finish to take. A user's message that says a role begins a turn whose words are the
/// window's word of its role and none of its task's, and the next user's message ends
/// it; it ends no answer that came before it, and a tool's output in it ends none
/// either. `replacements` are the ids of the replacements the ledger knows were written
/// to the member; a message is one by the id of its header, and not by any words. The
/// summary a compaction writes in the user's place is no message to the member at all,
/// and changes nothing of this: Claude Code compacts a conversation that is at rest,
/// after the answer (Astra's review of round 1). What the harness wrote in the
/// conversation itself (a hook's context, a notification it wrote to its record alone)
/// is no message to the member and no answer.
pub(crate) fn answers_to_the_task<'a>(
    items: &'a [Item],
    replacements: &[i64],
) -> Option<Answers<'a>> {
    let mut of_the_role = false;
    let mut last = None;
    let mut final_run = Vec::new();
    let mut complete_run = None;
    // The last item with words of the member's since its latest message, for a final run
    // with none.
    let mut said = None;
    let mut first = FirstRun::default();
    for item in items.iter().filter(|item| !item.compaction) {
        match item.role {
            Role::User => {
                of_the_role =
                    header_id(&item.text).is_some_and(|message| replacements.contains(&message));
                // A message to the member (its own, or its harness's notification) is
                // answered by what follows it, if anything does.
                if !of_the_role {
                    final_run.clear();
                    last = None;
                }
                // Nor are the words before it any of that answer's.
                said = said.filter(|_| of_the_role);
            }
            Role::Tool if !of_the_role => final_run.clear(),
            Role::Assistant if !of_the_role => {
                last = Some(item);
                final_run.push(item);
                if has_words(item) {
                    said = Some(item);
                }
                if item.complete {
                    complete_run = Some(with_words(&final_run, said));
                }
            }
            _ => {}
        }
        first.see(item, of_the_role);
    }
    (last.is_some() || complete_run.is_some()).then_some(Answers {
        last,
        final_run: with_words(&final_run, said),
        complete_run,
        reply: first.reply(),
        after_the_last_tool: final_run,
    })
}

/// What a member wrote before its first tool call, as the walk over its record meets it
/// (`Answers::reply`): the assistant's items from its latest message to its first tool call,
/// and whether a tool call came. A message to the member begins it again, a replacement of the
/// role (`of_the_role`) none; the harness's own words are none of it.
#[derive(Default)]
struct FirstRun<'a> {
    run: Vec<&'a Item>,
    over: bool,
}

impl<'a> FirstRun<'a> {
    /// The next item of the record, `of_the_role` whether it is of a turn on a replacement.
    fn see(&mut self, item: &'a Item, of_the_role: bool) {
        if of_the_role {
            return;
        }
        match item.role {
            Role::User => *self = Self::default(),
            Role::Tool => self.over = true,
            Role::Assistant if !self.over && !item.synthetic => self.run.push(item),
            _ => {}
        }
    }

    /// Its words, where a tool call came after them: with none, the one run is the turn's
    /// final answer, and it is no reply apart from it.
    fn reply(self) -> Vec<&'a Item> {
        if self.over {
            self.run
        } else {
            Vec::new()
        }
    }
}
