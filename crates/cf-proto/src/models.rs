//! The models a harness offers, as the harness itself lists them (CHF-03): what the
//! chief's picker, the staff chains' editor (CHN-10) and Muse's pickers (MUS-15) choose
//! from, and what the home ledger keeps of each harness's last read.
//!
//! **A contributor model is never a default:** its provider may train on what it sees
//! (Meta says "your content … may be used for product improvement" of Muse's
//! `-contributor` models, which Pi lists too). [`Models::default_model`] never answers one,
//! and [`warning`] is what a picker shows beside one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What a picker says beside a contributor model.
pub const CONTRIBUTOR_WARNING: &str =
    "Contributor model: Meta may train on what it sees, including your code.";

/// A model a harness offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoice {
    /// The model's id as its harness names it: what `--model` takes, but for Pi, whose
    /// `--model` takes `provider/id` ([`ModelChoice::model`]).
    pub id: String,
    /// The provider the model is reached through, where the harness has several (Pi): its
    /// account, and the first half of what `--model` takes. An OpenRouter id holds a `/`
    /// of its own (`openrouter` and `anthropic/claude-fable-5`), so the two are kept apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// What the harness calls it.
    pub label: String,
    /// The efforts it takes, the harness's own words, from the lowest as the harness orders
    /// them; none where it has no choice of effort.
    pub efforts: Vec<String>,
    /// The effort the harness runs it at where none is given, where its list says one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    /// The model id a launch at an effort passes where the effort is part of the id
    /// (Devin's `model_uid`: `claude-opus-5-5-medium-fast` for `claude-opus-5-5-fast` at
    /// `medium`), by effort. Never joined from the two: the effort may sit before a suffix.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub launch_ids: BTreeMap<String, String>,
    /// The harness's own mark of the model it starts on: never ConsensFlow's choice where
    /// this is a contributor model.
    #[serde(default, skip_serializing_if = "is_false")]
    pub marked_default: bool,
    /// Its provider may train on what it sees.
    #[serde(default, skip_serializing_if = "is_false")]
    pub contributor: bool,
    /// The harness lists it and will not run it (Claude's credit-gated rows).
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl ModelChoice {
    /// A model with no effort and no mark: a contributor model where its id says it is one
    /// (`-contributor` ends it).
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            contributor: id.ends_with("-contributor"),
            id,
            provider: None,
            label: label.into(),
            efforts: Vec::new(),
            default_effort: None,
            launch_ids: BTreeMap::new(),
            marked_default: false,
            disabled: false,
        }
    }

    /// What `--model` takes for this model: `provider/id` where it has a provider, its id
    /// else. The one name a launch, a ledger row and a request use for it.
    pub fn model(&self) -> String {
        match &self.provider {
            Some(provider) => format!("{provider}/{}", self.id),
            None => self.id.clone(),
        }
    }

    /// What `--model` takes for this model at `effort`: the exact id the harness lists for
    /// it, where the effort is part of the id ([`ModelChoice::launch_ids`]); its
    /// [`ModelChoice::model`] else.
    pub fn launch_model(&self, effort: Option<&str>) -> String {
        effort
            .and_then(|effort| self.launch_ids.get(effort))
            .cloned()
            .unwrap_or_else(|| self.model())
    }
}

/// What reading a harness's models answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Models {
    pub models: Vec<ModelChoice>,
    /// The list is not the account's as of now: what the harness ships, or what it kept from
    /// an earlier fetch it could not repeat. Kept, and never a default's source.
    pub stale: bool,
    /// How it was read: the command and the request, as the human could run them.
    pub how: String,
}

impl Models {
    /// The model a harness is started on where nothing names one: the one the harness
    /// marks, unless it is a contributor model, then its twin that is not
    /// (`muse-spark-1.3` for `muse-spark-1.3-contributor`), else the first that is neither
    /// a contributor model nor disabled. None where every model is one of those: no default
    /// is better than one its provider trains on. None too where the list is
    /// [`Models::stale`]: it may name what the account no longer offers, and a window
    /// launched on such a model fails at its first turn (T-816).
    pub fn default_model(&self) -> Option<&ModelChoice> {
        if self.stale {
            return None;
        }
        let usable = |model: &&ModelChoice| !model.contributor && !model.disabled;
        let marked = self.models.iter().find(|model| model.marked_default);
        let twin = marked.filter(|model| model.contributor).and_then(|marked| {
            let twin = marked.id.strip_suffix("-contributor")?;
            self.models
                .iter()
                .find(|model| model.id == twin && model.provider == marked.provider)
        });
        marked
            .filter(usable)
            .or(twin.filter(usable))
            .or_else(|| self.models.iter().find(usable))
    }

    /// The model `model` names ([`ModelChoice::model`]), if listed.
    pub fn model(&self, model: &str) -> Option<&ModelChoice> {
        self.models.iter().find(|listed| listed.model() == model)
    }
}

/// What a picker shows beside `model`: a warning for a contributor model, none else.
pub fn warning(model: &ModelChoice) -> Option<&'static str> {
    model.contributor.then_some(CONTRIBUTOR_WARNING)
}

/// A harness's models as the home ledger keeps them: the last list read, when and how,
/// and what became of the last try.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelList {
    /// The harness, by the CLI's own name (`claude`).
    pub harness: String,
    /// The last list read; none if no read has ever answered.
    pub models: Vec<ModelChoice>,
    /// The list is not the account's as of now: the harness answered from what it ships or
    /// kept ([`Models::stale`]), or the last try failed and this is an earlier read's.
    pub stale: bool,
    /// How the list was read ([`Models::how`]); empty where no read has answered.
    pub how: String,
    /// When the list was read, in milliseconds since the epoch; none where no read has
    /// answered.
    pub read_at: Option<i64>,
    /// When it was last tried.
    pub tried_at: i64,
    /// Why the last try failed (`devin models list: Not logged in…`); none where it answered.
    pub failure: Option<String>,
}

impl ModelList {
    /// What the kept list is now that a try at `at` answered `read`: the answer, kept
    /// whole; or, where it failed, the list kept from before (none where there was none),
    /// marked stale, with the reason.
    pub fn after(
        harness: &str,
        kept: Option<ModelList>,
        at: i64,
        read: Result<Models, String>,
    ) -> Self {
        match read {
            Ok(read) => Self {
                harness: harness.to_owned(),
                models: read.models,
                stale: read.stale,
                how: read.how,
                read_at: Some(at),
                tried_at: at,
                failure: None,
            },
            Err(failure) => {
                let kept = kept.filter(|kept| kept.harness == harness);
                Self {
                    harness: harness.to_owned(),
                    stale: true,
                    tried_at: at,
                    failure: Some(failure),
                    ..kept.unwrap_or(Self {
                        harness: harness.to_owned(),
                        models: Vec::new(),
                        stale: true,
                        how: String::new(),
                        read_at: None,
                        tried_at: at,
                        failure: None,
                    })
                }
            }
        }
    }

    /// The list as a read's answer: what a default is chosen from.
    pub fn models(&self) -> Models {
        Models {
            models: self.models.clone(),
            stale: self.stale,
            how: self.how.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
