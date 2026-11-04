//! What a chief's window runs on (CHF-05): a chief is a harness and a model, never a
//! saved agent. A reopen goes on with the model, and the effort or thinking, the
//! harness's own record says the conversation ran on last, so that a model the human
//! switched to in the window is kept; where the record names none (a conversation with no
//! answer yet, a fresh one), the model picked for the chief. A model the harness moved to
//! by itself is never taken (MUS-24): the reopen asks again for what the last launch
//! asked for.
//!
//! A model its harness no longer has (CHF-06): where a chief's launch fails, or its turn
//! fails, in the words its harness says it with ([`Adapter::model_gone`]), the chief opens
//! again at once on the harness's own default model, never a contributor model
//! ([`Adapter::default_model`]), which becomes the model picked for it, and the human is
//! told once, by name, what it starts on and what was gone. Where the harness has no such
//! model, the chief stays closed until the human switches it, and is told why.

use std::rc::Rc;

use cf_harness::contract::{Adapter, Agent, Used};
use cf_ledger::{NewNote, ParticipantView, ProjectView};

use crate::deliveries::Delivering;
use crate::dispatcher::Dispatcher;
use crate::handoff::name_of;
use crate::record::Record;
use crate::seams::EngineError;
use crate::windows::Relaunch;

/// The model, and the effort or thinking, a chief's launch asks its harness for; none of
/// each leaves it to the harness's own default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ChiefRuns {
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) thinking: Option<String>,
}

impl ChiefRuns {
    /// What the record said, where the harness did not move to it by itself and it is no
    /// model a launch found gone; else what the last launch asked for, or the model picked
    /// for the chief where it asked for none. A model found gone is never asked for again.
    pub(crate) fn of(chief: &ParticipantView, used: Option<Used>) -> Self {
        let gone = chief.gone_model.as_deref();
        let kept =
            |model: &Option<String>| model.clone().filter(|model| Some(model.as_str()) != gone);
        let picked = Self::picked(chief);
        let picked = || Self {
            model: kept(&chief.model),
            ..picked
        };
        match used {
            Some(used) if used.by_harness || Some(used.model.as_str()) == gone => Self {
                model: kept(&chief.launched_model).or_else(|| kept(&chief.model)),
                ..Self::picked(chief)
            },
            Some(used) => Self {
                model: Some(used.model),
                effort: used.effort,
                thinking: used.thinking,
            },
            None => picked(),
        }
    }

    /// What a participant was picked to run on: its model and its effort as its harness
    /// takes the effort (harness-models): Pi's as its thinking, none for Devin, whose model
    /// id holds the effort (`claude-opus-5-5-high`), and the effort for every other.
    pub(crate) fn picked(participant: &ParticipantView) -> Self {
        let effort = participant.effort.clone();
        let (effort, thinking) = match participant.harness.as_deref() {
            Some("pi") => (None, effort),
            Some("devin") => (None, None),
            _ => (effort, None),
        };
        Self {
            model: participant.model.clone(),
            effort,
            thinking,
        }
    }

    /// The agent a chief's launch is given: none where it names nothing, which leaves the
    /// model to the harness's own default.
    pub(crate) fn agent(&self) -> Option<Agent<'_>> {
        (*self != Self::default()).then(|| self.member_agent(false))
    }

    /// The agent a member's launch is given, an image designer's among them: a model it
    /// names none of is its harness's own default.
    pub(crate) fn member_agent(&self, designer: bool) -> Agent<'_> {
        Agent {
            model: self.model.as_deref(),
            effort: self.effort.as_deref(),
            thinking: self.thinking.as_deref(),
            designer,
        }
    }
}

impl Dispatcher {
    /// What the chief's window opens on: on its conversation `resume`, what the harness's
    /// record says it ran on last; else the model picked for it.
    pub(crate) async fn chief_runs(
        &self,
        chief: &ParticipantView,
        adapter: &dyn Adapter,
        resume: Option<&str>,
    ) -> ChiefRuns {
        let used = match resume {
            Some(session) => {
                adapter
                    .last_used(session, chief.launched_model.as_deref())
                    .await
            }
            None => None,
        };
        ChiefRuns::of(chief, used)
    }

    /// Whether the chief that needs an edit (`chief`, T-1202) has a model now: the one its
    /// harness's record names its conversation ran on last, kept for it with its effort and
    /// the mark taken off. A record read once and naming none is not read again (`record`'s
    /// `no_model_in_record`) until the human switches the chief, which takes the mark off.
    pub(crate) async fn chief_model_from_record(
        &self,
        chief: &ParticipantView,
        record: &Rc<Record>,
    ) -> Result<bool, EngineError> {
        if record.window.borrow().no_model_in_record {
            return Ok(false);
        }
        let session = self
            .conversation_to_go_on_with(chief)
            .await?
            .and_then(|conversation| conversation.native_session);
        let adapter = self
            .seams
            .adapters
            .adapter(chief.harness.as_deref().unwrap_or_default());
        let used = match (session, adapter) {
            (Some(session), Some(adapter)) => {
                adapter
                    .last_used(&session, chief.launched_model.as_deref())
                    .await
            }
            _ => None,
        };
        let Some(used) = used else {
            record.window.borrow_mut().no_model_in_record = true;
            self.changed();
            return Ok(false);
        };
        let effort = used.effort.as_deref().or(used.thinking.as_deref());
        self.seams
            .ledger
            .borrow_mut()
            .chief_ran_on(chief.id, &used.model, effort)?;
        self.changed();
        Ok(true)
    }

    /// The chief's launch asked for `runs`: kept for the next reopen to compare the record
    /// with.
    pub(crate) fn chief_launched(
        &self,
        chief: &ParticipantView,
        runs: &ChiefRuns,
    ) -> Result<(), EngineError> {
        self.seams
            .ledger
            .borrow_mut()
            .chief_launched(chief.id, runs.model.as_deref())?;
        Ok(())
    }
}

impl Dispatcher {
    /// The model the chief's last launch asked for, where `words` are its harness saying it
    /// no longer has it, by name: every harness's refusal names the model it refused, so
    /// words of a gone model that name another, or none, are no word of the chief's. None
    /// where the launch asked for none. `words` are the record's own of a failed turn, or the
    /// screen of a launch that ended with a code of failure before it came up, never a
    /// screen else: a chief's screen may show anything (it grepped for the words), and only
    /// a refusal at launch has no record to say it in (Calliope's P1, T-955).
    pub(crate) fn gone_model(
        &self,
        chief: &ParticipantView,
        words: &str,
    ) -> Result<Option<String>, EngineError> {
        let now = self.known_project(chief.project_id)?;
        let Some(asked) = now
            .participants
            .iter()
            .find(|participant| participant.id == chief.id)
            .and_then(|participant| participant.launched_model.clone())
        else {
            return Ok(None);
        };
        let harness = chief.harness.as_deref().unwrap_or_default();
        let Some(adapter) = self.seams.adapters.adapter(harness) else {
            return Ok(None);
        };
        Ok(adapter.model_gone(words, &asked).then_some(asked))
    }

    /// The chief opens again at once, its model `gone`: its next launch starts on its
    /// harness's default ([`Dispatcher::start_on_default`]).
    pub(crate) fn reopen_without(&self, record: &Record, gone: String) {
        let mut window = record.window.borrow_mut();
        let failures = window.relaunch.map_or(0, |relaunch| relaunch.failures);
        window.gone = Some(gone);
        window.relaunch = Some(Relaunch {
            failures,
            at: self.now(),
            reopens: true,
        });
        drop(window);
        self.changed();
    }

    /// The chief as its launch runs it: where its last launch found its model gone, on its
    /// harness's default model from now on, which the ledger keeps as the chief's, and the
    /// human told once; none where the harness has no such model, the chief then held
    /// ([`held_for_no_default`]), what it was to receive given back.
    pub(crate) async fn start_on_default(
        &self,
        record: &Rc<Record>,
        project: &ProjectView,
        chief: &ParticipantView,
        adapter: &dyn Adapter,
        delivering: Option<&Delivering>,
    ) -> Result<Option<ParticipantView>, EngineError> {
        let Some(gone) = record.window.borrow_mut().gone.take() else {
            // Held since a launch before this one, a restart's or a Resume's included: no
            // window, and the human was told then.
            if held_for_no_default(chief) {
                if let Some(delivering) = delivering {
                    self.give_back(
                        delivering.message,
                        "the chief's model is gone and its harness has no other",
                    )?;
                }
                return Ok(None);
            }
            return Ok(Some(chief.clone()));
        };
        let harness = name_of(chief.harness.as_deref().unwrap_or_default());
        // A default that is the very model found gone is no other to start on: the chief is
        // held, and told so (Calliope's P2b, T-994).
        let started = match adapter.default_model().await {
            Ok(Some(model)) if model == gone => {
                Err("its default is the model that is gone".to_owned())
            }
            started => started,
        };
        let body = match &started {
            Ok(Some(model)) => format!(
                "The chief's model, {gone}, is no longer available in {harness}: the chief starts on {model}, {harness}'s default model, instead. Switch the chief to pick another."
            ),
            Ok(None) => format!(
                "The chief's model, {gone}, is no longer available in {harness}: the chief starts on {harness}'s own default model instead. Switch the chief to pick another."
            ),
            Err(why) => format!(
                "The chief's model, {gone}, is no longer available in {harness}, and ConsensFlow found no other to start it on ({why}): switch the chief to another model."
            ),
        };
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body,
            },
        )?;
        let Ok(model) = started else {
            // Held in the ledger, so that a restart or a Resume holds it still and tells the
            // human nothing more: the model picked stays the human's (never none, which is
            // the harness's own default), found gone (Calliope's P2, T-955).
            self.seams
                .ledger
                .borrow_mut()
                .chief_gone(chief.id, &gone, Some(&gone))?;
            if let Some(delivering) = delivering {
                self.give_back(
                    delivering.message,
                    "the chief's model is gone and its harness has no other",
                )?;
            }
            self.changed();
            return Ok(None);
        };
        self.seams
            .ledger
            .borrow_mut()
            .chief_gone(chief.id, &gone, model.as_deref())?;
        self.changed();
        Ok(Some(ParticipantView {
            model,
            launched_model: None,
            gone_model: Some(gone),
            ..chief.clone()
        }))
    }
}

/// Whether `chief` is held for want of a model to start it on: its model found gone, and
/// its harness with no other ([`Dispatcher::start_on_default`]), as the ledger keeps it, its
/// model and the gone one the same. It stays closed until the human switches it (CHF-06).
pub(crate) fn held_for_no_default(chief: &ParticipantView) -> bool {
    chief.gone_model.is_some() && chief.gone_model == chief.model
}

#[cfg(test)]
mod tests;
