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
//! human has a line on the board that says so ([`Dispatcher::not_read`](crate::dispatcher::Dispatcher::not_read)).
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
//! - A role is read only by the parts printed in the conversation it goes to: one that
//!   began between two parts, or between the parts and the paste's confirmation (a
//!   `/clear`), was not shown them all, is told nothing by them, and the role is said
//!   again at its next Resume, with the line. `cf inbox read --json` prints a role whole,
//!   as it did, and says it read only within the bound: the ledger refuses that read of a
//!   role `cf` prints in more than one part.
//! - A role note printed while its paste was on its way and then given up (failed) is
//!   failed as any is: its parts printed are not looked for, as the thread is not (the
//!   bullet above on a delivery given up). Any other message so read is `read`, not failed.
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

mod answers;
mod comparison;
mod confirmation;
mod greeting;
mod prelude;
mod replacement;
#[cfg(test)]
mod tests;

use cf_base::role_read::role_parts;
use cf_harness::records::Item;
#[cfg(test)]
use cf_harness::records::Role;
#[cfg(test)]
use cf_ledger::MessageView;
use cf_proto::cf_word::CfWord;

use crate::delivery_text::fits_whole;
#[cfg(test)]
use crate::specs::RoleText;

pub(crate) use answers::answers_to_the_task;
pub(crate) use comparison::Compared;
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
