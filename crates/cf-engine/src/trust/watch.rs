//! One look of the trust watch and what follows from it: the state the watch keeps
//! between looks ([`Watch`]), and a step for each thing a look can find.

use std::rc::Rc;

use cf_harness::contract::{
    confirms, presses_again, Pane, Question, ServersChosen, Trusts, Window,
};
use cf_ledger::{ParticipantView, ProjectView};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;

use super::{
    dialog, owes_first, Ended, Seen, Trusting, ASKS_WITHIN_MS, GONE_LOOKS, MOST_KEYS, MOST_MOVES,
    NO_PROMPT_MS, PRESS_AGAIN_LOOKS, TELL_LOOKS, UNREAD_MS,
};

/// The screen the last key was pressed on, how many looks have found it since, and
/// the key.
type PressedOn = (Vec<String>, u32, &'static [u8]);

/// What a watch holds between its looks: the window it watches, and what the looks so far
/// found of its question.
pub(super) struct Watch<'a> {
    pub(super) record: &'a Rc<Record>,
    pub(super) project: &'a ProjectView,
    pub(super) participant: &'a ParticipantView,
    pub(super) pane: &'a Pane,
    pub(super) window: &'a Rc<dyn Window>,
    pub(super) folder: String,
    inside: bool,
    servers_above: Option<String>,
    began: i64,
    /// What the record held at the first look that could read it: a window opened
    /// again on its conversation holds it all from the start, and a look that could
    /// not read it says nothing of growth.
    opened_with: Option<usize>,
    /// The keys pressed into the question it asks now, in all and again into a screen
    /// they did not move (those of the whole watch are the record's, `trust_keys`); and
    /// whether that question is the human's now, its screen unmoved by a key that may
    /// not be pressed again.
    moves: usize,
    asked_keys: usize,
    stuck: bool,
    asks_now: Option<u8>,
    /// The screen the last key was pressed on, how many looks have found it since, and
    /// the key: a key goes into a screen that moved since the last one, so a slow
    /// redraw is never given a second, or, where it may be pressed again, into one
    /// that stayed as it was for [`PRESS_AGAIN_LOOKS`], whose window did not take it.
    pressed_on: Option<PressedOn>,
    /// What the window's folder left out of the project's servers as the watch began.
    pub(super) chosen: ServersChosen,
    /// Whether ConsensFlow pressed into the dialog of new servers, and the servers its
    /// Enter there would use, as the dialog named them.
    pub(super) into_servers: bool,
    pub(super) using: Option<Vec<String>>,
    /// How many looks in a row found the question gone with no prompt on the screen,
    /// and how many found the question.
    gone: u32,
    asking: u32,
    /// Since when the host has shown no screen of a window that asked, and whether
    /// the human was told so: once a watch, so a second time its screen goes unread
    /// in the same watch is told of to nobody.
    unread_since: Option<i64>,
    told_unread: bool,
    /// Whether the human was told of the question of the folder's grants, or of the
    /// dialog of new servers: once its window is past it, the next window that asks it
    /// is told of again.
    pub(super) grants: bool,
    pub(super) servers: bool,
    /// Whether the human was told that a chief's window shows no prompt.
    told_no_prompt: bool,
}

impl<'a> Watch<'a> {
    pub(super) fn new(
        began: i64,
        record: &'a Rc<Record>,
        project: &'a ProjectView,
        participant: &'a ParticipantView,
        pane: &'a Pane,
        window: &'a Rc<dyn Window>,
        trusts: Trusts,
    ) -> Self {
        let Trusts {
            folder,
            inside,
            servers_above,
        } = trusts;
        Self {
            record,
            project,
            participant,
            pane,
            window,
            folder,
            inside,
            servers_above,
            began,
            opened_with: None,
            moves: 0,
            asked_keys: 0,
            stuck: false,
            asks_now: None,
            pressed_on: None,
            chosen: window.servers_chosen(),
            into_servers: false,
            using: None,
            gone: 0,
            asking: 0,
            unread_since: None,
            told_unread: false,
            grants: false,
            servers: false,
            told_no_prompt: false,
        }
    }

    /// Another question than the last look's: its own keys, its own time.
    fn begin_question(&mut self, question: Question) {
        if self.asks_now.replace(dialog(question)) != Some(dialog(question)) {
            self.moves = 0;
            self.asked_keys = 0;
            self.stuck = false;
            self.asking = 0;
            self.pressed_on = None;
        }
    }

    /// The screen the last key went into, as it was: the key is pressed again where
    /// it may be, and the question is the human's where not. Where `lines` is that
    /// screen, how many looks have found it so, and the key.
    fn screen_unmoved(&mut self, lines: &[String]) -> Option<(u32, &'static [u8])> {
        let unmoved = match self.pressed_on.as_mut() {
            Some((on, looks, key)) if *on == lines => {
                *looks += 1;
                Some((*looks, *key))
            }
            _ => None,
        };
        if let Some((looks, key)) = unmoved {
            if looks >= PRESS_AGAIN_LOOKS && !(presses_again(key) && self.asked_keys < MOST_KEYS) {
                self.stuck = true;
            }
        }
        unmoved
    }

    /// The key ConsensFlow presses toward a yes into `question`, where it answers it:
    /// a choice the reader cannot place is pressed nothing, nor a question that is the
    /// human's, nor one whose yes would trust a folder outside the project's.
    fn key_toward_yes(&self, question: Question, accepted: bool) -> Option<&'static [u8]> {
        match question {
            _ if self.stuck => None,
            Question::Toward(key) if self.inside || accepted => Some(key),
            Question::Grants(Some(key)) if self.inside => Some(key),
            Question::Servers(Some(key)) if self.inside && self.servers_above.is_none() => {
                Some(key)
            }
            _ => None,
        }
    }

    /// Whether a key may go into the screen now: `unmoved` as the last key left it, or
    /// a question given its keys already is the human's.
    fn may_press(&mut self, unmoved: Option<(u32, &'static [u8])>) -> bool {
        match unmoved {
            Some((looks, _)) => looks >= PRESS_AGAIN_LOOKS,
            None if self.moves >= MOST_MOVES => {
                self.stuck = true;
                false
            }
            None => true,
        }
    }
}

impl Dispatcher {
    /// One look at the window and what it finds: `Some` once the window is past the
    /// question.
    pub(super) async fn trust_look_and_answer(
        &self,
        w: &mut Watch<'_>,
    ) -> Result<Option<Ended>, EngineError> {
        let length = self.record_length(w.window).await;
        if w.opened_with.is_none() {
            w.opened_with = length;
        }
        let grown = matches!((w.opened_with, length), (Some(from), Some(now)) if now > from);
        let seen = self.trust_look(w.pane, &**w.window).await;
        if !matches!(seen, Seen::Unread) {
            w.unread_since = None;
        }
        self.tell_chief_no_prompt(w, &seen)?;
        match seen {
            Seen::Asks(question, lines) => {
                self.answer_question(w, question, lines).await?;
                Ok(None)
            }
            Seen::Prompt { cleared } => Ok(Some(Ended {
                at_prompt: true,
                cleared,
            })),
            Seen::Other => Ok(self.not_asking(w, grown)),
            Seen::Unread => self.unread(w, grown),
        }
    }

    /// A chief's launch is never given up, and nothing is pasted blind into a window
    /// that may still ask: one that shows neither a question nor a prompt the reader
    /// knows (a harness that draws it otherwise since an update) would hold its first
    /// message for good, so the human hears once (T-629).
    fn tell_chief_no_prompt(&self, w: &mut Watch<'_>, seen: &Seen) -> Result<(), EngineError> {
        let no_prompt = matches!(seen, Seen::Other | Seen::Unread)
            && !w.record.window.borrow().trust.asks()
            && owes_first(w.record);
        if no_prompt
            && w.participant.role == "chief"
            && !w.told_no_prompt
            && self.now() - w.began > NO_PROMPT_MS
        {
            w.told_no_prompt = true;
            self.tell_no_prompt(w.project, w.record)?;
        }
        Ok(())
    }

    /// The window shows its question: answer it by a key where ConsensFlow does, else
    /// tell the human once it has stayed.
    async fn answer_question(
        &self,
        w: &mut Watch<'_>,
        question: Question,
        lines: Vec<String>,
    ) -> Result<(), EngineError> {
        w.record.window.borrow_mut().trust = if matches!(question, Question::Servers(_)) {
            Trusting::AskedServers
        } else {
            Trusting::Asked
        };
        w.gone = 0;
        self.changed();
        w.begin_question(question);
        let unmoved = w.screen_unmoved(&lines);
        // A folder the human said yes to in another window of the
        // project is answered as they answered it.
        let accepted = self
            .trust
            .accepted
            .borrow()
            .contains(&(w.project.id, w.folder.clone()));
        // A choice the reader cannot place is pressed nothing. A question
        // not answered here is said nothing of until it has stayed so: a
        // conversation drawn before its prompt may quote it.
        w.asking += 1;
        if matches!(question, Question::Toward(_)) {
            w.record.window.borrow_mut().trust_sure = true;
        }
        let pressing = w.key_toward_yes(question, accepted);
        // A window inside the project's folder whose conversation quotes the
        // dialog, its cursor and all, before its prompt is read as asking
        // and given a key toward a yes, which goes into its input box,
        // and the human is told ConsensFlow answered (as since round 4).
        if let Some(key) = pressing {
            // The servers' use is told of once the window is past the dialog.
            if w.inside && !matches!(question, Question::Servers(_)) {
                self.tell_yes_pressed(w.project, &w.folder, w.participant, question)?;
            }
            if w.may_press(unmoved) && self.press_toward_trust(w.pane, key).await {
                w.record.window.borrow_mut().trust_keys += 1;
                w.moves += 1;
                w.asked_keys += usize::from(unmoved.is_some());
                if matches!(question, Question::Servers(_)) {
                    w.into_servers = true;
                    if confirms(key) {
                        w.using = Some(w.window.trust_servers(&lines));
                    }
                }
                w.pressed_on = Some((lines, 0, key));
            }
        } else {
            self.tell_trust_unanswered(w, question)?;
        }
        Ok(())
    }

    /// A question ConsensFlow does not answer, once it has stayed on the screen: the
    /// human is to answer it, and is told in the words of its kind.
    fn tell_trust_unanswered(
        &self,
        w: &mut Watch<'_>,
        question: Question,
    ) -> Result<(), EngineError> {
        match question {
            Question::Servers(_) if w.asking >= TELL_LOOKS => {
                // Its servers are not the project's alone, or its choice cannot be
                // placed: the human's.
                w.servers = true;
                self.tell_servers_held(
                    w.project,
                    &w.folder,
                    w.inside,
                    w.servers_above.as_deref(),
                    w.participant,
                )
            }
            Question::Grants(_) if w.asking >= TELL_LOOKS => {
                // Its yes would record a folder outside the project's, or its
                // choice cannot be placed: the human's.
                w.grants = true;
                self.tell_grants_once(w.project, &w.folder, w.inside, w.participant)
            }
            _ if w.asking >= TELL_LOOKS => {
                self.tell_trust_held(w.project, &w.folder, w.inside, w.participant)
            }
            _ => Ok(()),
        }
    }

    /// A screen that shows no question and no prompt.
    fn not_asking(&self, w: &mut Watch<'_>, grown: bool) -> Option<Ended> {
        w.asking = 0;
        // Its record grown past what it held: past the question, whoever
        // answered it (or a process before it writing still).
        if grown {
            return Some(Ended::PAST);
        }
        if w.record.window.borrow().trust.asks() {
            // The question gone for good with no prompt to read (a prompt
            // drawn otherwise than the harness's reader knows): past it.
            w.gone += 1;
            if w.gone >= GONE_LOOKS {
                return Some(Ended::PAST);
            }
            None
        } else {
            self.never_asked_in_time(w)
        }
    }

    /// No screen says nothing of the question: a window that asked is held
    /// until a look reads it past the question, and the human is told once it
    /// was not read for [`UNREAD_MS`]; one that never did is let go after its
    /// time.
    fn unread(&self, w: &mut Watch<'_>, grown: bool) -> Result<Option<Ended>, EngineError> {
        if grown {
            return Ok(Some(Ended::PAST));
        }
        if w.record.window.borrow().trust.asks() {
            let since = *w.unread_since.get_or_insert(self.now());
            let unseen = self.now() - since;
            if unseen >= UNREAD_MS && !w.told_unread {
                w.told_unread = true;
                self.tell_unread(w.project, &w.folder, w.participant, unseen)?;
            }
            Ok(None)
        } else {
            Ok(self.never_asked_in_time(w))
        }
    }

    /// A window that never asked is let go once [`ASKS_WITHIN_MS`] has passed, but not
    /// one still owed its launch's first message, which only its prompt lets go.
    fn never_asked_in_time(&self, w: &Watch<'_>) -> Option<Ended> {
        (self.now() - w.began > ASKS_WITHIN_MS && !owes_first(w.record)).then_some(Ended::PAST)
    }
}
