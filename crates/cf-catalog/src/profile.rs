//! Work tiers and agent profiles: what a model and the road to it are called,
//! and the work its agent suits. `agentProfile` and `modelProfile` are ported
//! branch for branch, and the order of their branches is part of what they
//! answer.

use std::collections::{HashMap, HashSet};

use cf_base::refusal::Refusal;
use cf_proto::agents::{Profile, WorkTier, WorkTierInfo};
use serde_json::Value;

use crate::presets::Preset;
use crate::Catalog;

/// The tiers in the order `WORK_TIERS` lists them, the order the pills show them.
pub const WORK_TIERS: [WorkTier; 4] = [
    WorkTier::Critical,
    WorkTier::Complex,
    WorkTier::Standard,
    WorkTier::Light,
];

/// How `tier` reads: its label and what it is for.
pub const fn work_tier_info(tier: WorkTier) -> WorkTierInfo {
    match tier {
        WorkTier::Critical => WorkTierInfo {
            label: "Critical work",
            description: "Important reviews, architecture, hard problems and important questions. No coding or routine advice.",
        },
        WorkTier::Complex => WorkTierInfo {
            label: "Complex work",
            description: "Demanding implementation, investigation, planning and substantial reviews.",
        },
        WorkTier::Standard => WorkTierInfo {
            label: "Standard work",
            description: "Feature work, tests, research, planning and ordinary reviews.",
        },
        WorkTier::Light => WorkTierInfo {
            label: "Light work",
            description: "Bounded fixes, lookups and routine tasks; verify the model is suitable.",
        },
    }
}

/// `validateWorkTier`: the tier a request names, none when it names none
/// (absent or `null`). Anything else that is not one of the four words is
/// refused.
pub fn validate_work_tier(value: Option<&Value>) -> Result<Option<WorkTier>, Refusal> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(word)) => WORK_TIERS
            .into_iter()
            .find(|tier| tier.as_str() == word)
            .map(Some)
            .ok_or_else(refused),
        Some(_) => Err(refused()),
    }
}

fn refused() -> Refusal {
    Refusal::new(
        "work-tier",
        "Work tier must be critical, complex, standard or light",
    )
}

/// What `agentProfile` reads of an agent: the fields of a roster row or a
/// preset, each as given. Absent and `null` are both none; `""` is some empty
/// text, which is not none to `??` and is falsy to `||`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Settings<'a> {
    /// The CLI by name (`claude`).
    pub harness: Option<&'a str>,
    /// The payload's word for the harness (`claude-code`), read when there is no `harness`.
    pub kind: Option<&'a str>,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    /// Pi's word for the effort.
    pub thinking: Option<&'a str>,
    /// An image agent: JavaScript's truthiness of the row's `designer`.
    pub designer: bool,
    /// The tier the human chose, already validated.
    pub work_tier: Option<WorkTier>,
}

/// The (harness, model) pairs the presets name: a model is the catalog's only
/// on a harness a preset runs it on.
#[derive(Debug, Clone)]
pub(crate) struct Known {
    models: HashMap<String, HashSet<String>>,
}

impl Known {
    /// The pairs of `presets`, each preset's harness as `agentProfile` spells
    /// it: its kind, and `claude` for `claude-code`.
    pub(crate) fn of(presets: &[Preset]) -> Self {
        let mut models: HashMap<String, HashSet<String>> = HashMap::new();
        for preset in presets {
            models
                .entry(harness_of_kind(&preset.kind).to_owned())
                .or_default()
                .insert(preset.model.clone());
        }
        Self { models }
    }

    /// `model` itself, when a preset runs it on `harness`.
    fn named<'m>(&self, harness: Option<&str>, model: Option<&'m str>) -> Option<&'m str> {
        let (harness, model) = harness.zip(model)?;
        self.models.get(harness)?.contains(model).then_some(model)
    }
}

/// A kind as `agentProfile` spells its harness: `claude-code` is `claude`, any
/// other kind is its own word, known to the build or not.
fn harness_of_kind(kind: &str) -> &str {
    if kind == "claude-code" {
        "claude"
    } else {
        kind
    }
}

/// The harness an agent runs on as `agentProfile` reads it: the harness it
/// names, else its kind's. None when it names neither.
fn harness_of<'a>(settings: &Settings<'a>) -> Option<&'a str> {
    settings
        .harness
        .or_else(|| settings.kind.map(harness_of_kind))
}

/// What `modelProfile` answers: the profile before its tier.
struct ModelProfile {
    model_key: String,
    model_label: String,
    route_label: String,
    route_note: Option<String>,
}

impl Catalog {
    /// `agentProfile`: what an agent's model and route are called, and the work
    /// it suits. Work tiers express the owner's allocation policy, not
    /// benchmark or price ranks: the tier is read from the model's key (after
    /// the image agent's override) and the effort, and a tier the human chose
    /// comes last.
    pub fn profile(&self, settings: &Settings<'_>) -> Profile {
        let harness = harness_of(settings);
        let known = self.known.named(harness, settings.model).is_some();
        let profile = self.model_profile(settings);
        let effort = if harness == Some("pi") {
            settings.thinking.or(settings.effort)
        } else {
            settings.effort
        };
        let tier = if known && profile.model_key.ends_with("kimi-k3") {
            WorkTier::Complex
        } else if known
            && (matches!(effort, Some("low" | "medium" | "high" | "xhigh" | "max"))
                || (harness == Some("codex") && effort == Some("ultra")))
        {
            match profile.model_key.as_str() {
                "gpt-6-astra" | "claude-fable-5.1" => match effort {
                    Some("max" | "ultra") => WorkTier::Critical,
                    Some("high" | "xhigh") => WorkTier::Complex,
                    Some("medium") => WorkTier::Standard,
                    _ => WorkTier::Light,
                },
                "claude-opus-5.5" => match effort {
                    Some("max") => WorkTier::Critical,
                    Some("low") => WorkTier::Light,
                    _ => WorkTier::Complex,
                },
                "gpt-6.1-sol" => match effort {
                    Some("max" | "ultra") => WorkTier::Complex,
                    Some("low" | "medium") => WorkTier::Light,
                    _ => WorkTier::Standard,
                },
                "claude-sonnet-5.5" => match effort {
                    Some("max") => WorkTier::Complex,
                    Some("low") => WorkTier::Light,
                    _ => WorkTier::Standard,
                },
                // A model the table leaves out is light work at every effort:
                // Haiku 5.5, Terra, Luna.
                _ => WorkTier::Light,
            }
        } else {
            WorkTier::Light
        };
        Profile {
            model_key: profile.model_key,
            model_label: profile.model_label,
            route_label: profile.route_label,
            route_note: profile.route_note,
            work_tier: settings.work_tier.unwrap_or(tier),
        }
    }

    /// `modelProfile`: the names of an agent's model and of the road to it.
    fn model_profile(&self, settings: &Settings<'_>) -> ModelProfile {
        let harness = harness_of(settings);
        let model = settings.model;
        let known = self.known.named(harness, model);
        // Devin's own setting (a chief from before every chief had an agent), or an
        // agent off the catalog on whatever id it names.
        if harness == Some("devin") && (known.is_none() || model.is_none_or(str::is_empty)) {
            let named = model.filter(|model| !model.is_empty());
            return ModelProfile {
                model_key: named.unwrap_or("devin-configured").to_owned(),
                model_label: named.unwrap_or("Devin configured model").to_owned(),
                route_label: "Devin account".to_owned(),
                route_note: None,
            };
        }
        // An image agent's window runs on Codex's own model, whatever model it names.
        if settings.designer {
            return ModelProfile {
                model_key: "codex-image".to_owned(),
                model_label: "Codex Images".to_owned(),
                route_label: "Codex login".to_owned(),
                route_note: None,
            };
        }
        // Strip provider paths only AFTER an exact curated model/harness match.
        let key = match known {
            Some(model) => curated_key(model),
            None => model.unwrap_or("default").to_owned(),
        };
        // Contributor/free are reviewed pricing and data-use routes for Muse 1.3.
        let contributor =
            known.filter(|model| key == "muse-spark-1.3" && model.contains("-contributor"));
        let route = match model {
            Some(model) if model.starts_with("openrouter/") => "OpenRouter · API",
            Some(model) if model.starts_with("opencode/") => "OpenCode Zen",
            Some(model) if model.starts_with("openai-codex/") => "Codex subscription",
            Some(model) if model.starts_with("anthropic/") => "Anthropic · API",
            _ => match harness {
                Some("claude") => "Claude Code account",
                Some("codex") => "Codex login",
                Some("devin") => "Devin account",
                Some(other) => other,
                // No kind and no harness: the word JavaScript's string
                // concatenation makes of `undefined`.
                None => "undefined",
            },
        };
        let mut route_label = route.to_owned();
        let mut route_note = None;
        if let Some(model) = contributor {
            route_label.push_str(if model.ends_with("-free") {
                " · Contributor · Free"
            } else {
                " · Contributor"
            });
            route_note = Some("Prompts and replies may train Meta models.".to_owned());
        }
        let label = known
            .and_then(|_| self.model_label(&key))
            .filter(|label| !label.is_empty())
            .or_else(|| model.filter(|model| !model.is_empty()))
            .unwrap_or("Default");
        ModelProfile {
            model_label: label.to_owned(),
            model_key: key,
            route_label,
            route_note,
        }
    }
}

/// The key of a model a preset runs: its last path segment, with the version
/// spelled as the catalog spells it.
fn curated_key(model: &str) -> String {
    let mut key = model
        .rsplit_once('/')
        .map_or(model, |(_, last)| last)
        .to_owned();
    // Anthropic's own ids spell the version with a dash; the key with the dot, as OpenRouter does.
    if let Some(dotted) = claude_version_dotted(&key) {
        key = dotted;
    }
    // Devin spells GPT versions with dashes too (gpt-6-1-sol).
    if let Some(dotted) = gpt_version_dotted(&key) {
        key = dotted;
    }
    // Contributor/free are reviewed pricing and data-use routes for Muse 1.3.
    if let Some(plain) = muse_spark_plain(&key) {
        key = plain;
    }
    key
}

/// What JavaScript's `\d` matches: one ASCII digit and nothing else, where
/// Rust's own `is_numeric` takes the digits of every script.
fn is_digit(text: &str) -> bool {
    matches!(text.as_bytes(), [b'0'..=b'9'])
}

/// `key.replace(/^claude-(fable|haiku|opus|sonnet)-(\d)-(\d)$/, 'claude-$1-$2.$3')`:
/// the rewritten key, none when the pattern does not match.
fn claude_version_dotted(key: &str) -> Option<String> {
    let (family, version) = key.strip_prefix("claude-")?.split_once('-')?;
    let (major, minor) = version.split_once('-')?;
    (matches!(family, "fable" | "haiku" | "opus" | "sonnet") && is_digit(major) && is_digit(minor))
        .then(|| format!("claude-{family}-{major}.{minor}"))
}

/// `key.replace(/^gpt-(\d)-(\d)-([a-z]+)$/, 'gpt-$1.$2-$3')`: the rewritten
/// key, none when the pattern does not match.
fn gpt_version_dotted(key: &str) -> Option<String> {
    let (major, rest) = key.strip_prefix("gpt-")?.split_once('-')?;
    let (minor, family) = rest.split_once('-')?;
    let lowercase = !family.is_empty() && family.bytes().all(|byte| byte.is_ascii_lowercase());
    (is_digit(major) && is_digit(minor) && lowercase)
        .then(|| format!("gpt-{major}.{minor}-{family}"))
}

/// `key.replace(/^muse-spark-1\.3-contributor(?:-free)?$/, 'muse-spark-1.3')`:
/// the rewritten key, none when the pattern does not match.
fn muse_spark_plain(key: &str) -> Option<String> {
    matches!(
        key,
        "muse-spark-1.3-contributor" | "muse-spark-1.3-contributor-free"
    )
    .then(|| "muse-spark-1.3".to_owned())
}

#[cfg(test)]
mod tests;
