//! Work tiers and agent profiles (`hosts/lib/presets.js`, from `MODEL_LABELS`
//! on): what a model and the road to it are called, and the work its agent
//! suits. `agentProfile` and `modelProfile` are ported branch for branch, and
//! the order of their branches is part of what they answer.

use std::collections::{HashMap, HashSet};

use cf_base::refusal::Refusal;
use cf_proto::agents::{Profile, WorkTier};
use serde::Serialize;
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

/// How a tier reads on the page and in the chief's text (`WORK_TIERS[tier]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WorkTierInfo {
    pub label: &'static str,
    pub description: &'static str,
}

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

/// `key.replace(/^claude-(fable|opus|sonnet)-(\d)-(\d)$/, 'claude-$1-$2.$3')`:
/// the rewritten key, none when the pattern does not match.
fn claude_version_dotted(key: &str) -> Option<String> {
    let (family, version) = key.strip_prefix("claude-")?.split_once('-')?;
    let (major, minor) = version.split_once('-')?;
    (matches!(family, "fable" | "opus" | "sonnet") && is_digit(major) && is_digit(minor))
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
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn catalog() -> Catalog {
        Catalog::bundled().unwrap()
    }

    fn preset(name: &str, kind: &str, model: &str, effort: Option<&str>) -> Preset {
        Preset {
            preset: name.to_owned(),
            id: name.to_owned(),
            name: name.to_owned(),
            label: name.to_owned(),
            description: name.to_owned(),
            kind: kind.to_owned(),
            model: model.to_owned(),
            effort: effort.map(str::to_owned),
            thinking: None,
            designer: false,
        }
    }

    #[test]
    fn a_digit_is_one_ascii_digit() {
        for digit in ["0", "5", "9"] {
            assert!(is_digit(digit), "{digit}");
        }
        for not in ["", "55", "a", "-", "٥", "５", "5\n", " 5"] {
            assert!(!is_digit(not), "{not:?}");
        }
    }

    #[test]
    fn the_dash_in_an_anthropic_version_becomes_a_dot_where_the_pattern_matched() {
        for (key, dotted) in [
            ("claude-fable-5-1", "claude-fable-5.1"),
            ("claude-opus-5-5", "claude-opus-5.5"),
            ("claude-sonnet-5-5", "claude-sonnet-5.5"),
            ("claude-sonnet-0-9", "claude-sonnet-0.9"),
        ] {
            assert_eq!(claude_version_dotted(key).as_deref(), Some(dotted), "{key}");
        }
        for key in [
            "claude-fable-15-1",
            "claude-fable-5-10",
            "claude-fable-٥-1",
            "claude-fable-5-٥",
            "claude-haiku-4-5",
            "claude-Fable-5-1",
            "Claude-fable-5-1",
            "claude-fable-5-1-x",
            "claude-fable-5-1\n",
            "claude-fable-5",
            "claude-fable--1",
            "claude-fable-5.1",
            "claude--5-1",
            "claude-5-1",
            "xclaude-fable-5-1",
            "openrouter/anthropic/claude-fable-5-1",
            "",
        ] {
            assert_eq!(claude_version_dotted(key), None, "{key:?}");
        }
    }

    #[test]
    fn the_dashes_in_a_gpt_version_become_a_dot_where_the_pattern_matched() {
        for (key, dotted) in [
            ("gpt-6-1-sol", "gpt-6.1-sol"),
            ("gpt-5-6-terra", "gpt-5.6-terra"),
            ("gpt-0-0-x", "gpt-0.0-x"),
        ] {
            assert_eq!(gpt_version_dotted(key).as_deref(), Some(dotted), "{key}");
        }
        for key in [
            "gpt-6-astra",
            "gpt-6-1-Sol",
            "gpt-6-1-sol2",
            "gpt-6-1-sol-x",
            "gpt-6-1-so_l",
            "gpt-6-1-",
            "gpt-6-1",
            "gpt-6-10-sol",
            "gpt-16-1-sol",
            "gpt-٦-1-sol",
            "gpt-6-1-sól",
            "GPT-6-1-sol",
            "gpt-6-1-sol\n",
            "gpt-6.1-sol",
            "xgpt-6-1-sol",
            "",
        ] {
            assert_eq!(gpt_version_dotted(key), None, "{key:?}");
        }
    }

    #[test]
    fn the_contributor_routes_of_muse_spark_become_its_key_and_nothing_else_does() {
        for key in [
            "muse-spark-1.3-contributor",
            "muse-spark-1.3-contributor-free",
        ] {
            assert_eq!(
                muse_spark_plain(key).as_deref(),
                Some("muse-spark-1.3"),
                "{key}"
            );
        }
        for key in [
            "muse-spark-1.3",
            "muse-spark-1.3-contributor-paid",
            "muse-spark-1.3-contributor-free-",
            "muse-spark-1.3-contributor-",
            "muse-spark-1x3-contributor",
            "muse-spark-1.2-contributor",
            "Muse-spark-1.3-contributor",
            "muse-spark-1.3-contributor\n",
            "opencode/muse-spark-1.3-contributor-free",
        ] {
            assert_eq!(muse_spark_plain(key), None, "{key:?}");
        }
    }

    #[test]
    fn a_key_is_the_last_segment_with_its_version_in_the_catalog_s_spelling() {
        assert_eq!(curated_key("gpt-6-astra"), "gpt-6-astra");
        assert_eq!(curated_key("openrouter/openai/gpt-6.1-sol"), "gpt-6.1-sol");
        assert_eq!(curated_key("gpt-6-1-sol"), "gpt-6.1-sol");
        assert_eq!(curated_key("claude-opus-5-5"), "claude-opus-5.5");
        assert_eq!(
            curated_key("openrouter/anthropic/claude-fable-5.1"),
            "claude-fable-5.1"
        );
        assert_eq!(
            curated_key("opencode/muse-spark-1.3-contributor-free"),
            "muse-spark-1.3"
        );
        assert_eq!(curated_key("a/b/"), "");
        assert_eq!(curated_key("/x"), "x");
    }

    #[test]
    fn a_work_tier_is_one_of_four_words_or_none() {
        assert_eq!(validate_work_tier(None), Ok(None));
        assert_eq!(validate_work_tier(Some(&json!(null))), Ok(None));
        for tier in WORK_TIERS {
            assert_eq!(
                validate_work_tier(Some(&json!(tier.as_str()))),
                Ok(Some(tier))
            );
        }
    }

    #[test]
    fn any_other_work_tier_is_refused_with_the_one_sentence() {
        for refused in [
            json!("huge"),
            json!(""),
            json!("Critical"),
            json!(" critical"),
            json!("critical "),
            json!("constructor"),
            json!("__proto__"),
            json!("hasOwnProperty"),
            json!(7),
            json!(0),
            json!(true),
            json!(false),
            json!([]),
            json!(["critical"]),
            json!({}),
            json!({ "critical": true }),
        ] {
            let refusal = validate_work_tier(Some(&refused)).unwrap_err();
            assert_eq!(
                refusal.message, "Work tier must be critical, complex, standard or light",
                "{refused}"
            );
            assert_eq!(
                (refusal.code, refusal.status),
                ("work-tier", 400),
                "{refused}"
            );
        }
    }

    #[test]
    fn the_tiers_read_as_the_page_shows_them() {
        let labels: Vec<_> = WORK_TIERS
            .into_iter()
            .map(|tier| work_tier_info(tier).label)
            .collect();
        assert_eq!(
            labels,
            [
                "Critical work",
                "Complex work",
                "Standard work",
                "Light work"
            ]
        );
        assert_eq!(
            serde_json::to_string(&work_tier_info(WorkTier::Light)).unwrap(),
            r#"{"label":"Light work","description":"Bounded fixes, lookups and routine tasks; verify the model is suitable."}"#
        );
    }

    #[test]
    fn devin_s_own_setting_comes_before_the_designer_and_the_key() {
        let catalog = catalog();
        let configured = |model| {
            catalog.profile(&Settings {
                harness: Some("devin"),
                model,
                designer: true,
                ..Settings::default()
            })
        };
        for model in [None, Some("")] {
            let profile = configured(model);
            assert_eq!(profile.model_key, "devin-configured");
            assert_eq!(profile.model_label, "Devin configured model");
            assert_eq!(profile.route_label, "Devin account");
        }
        // Off the catalog, a Devin agent is called by whatever id it names.
        let named = configured(Some("some-model"));
        assert_eq!(
            (named.model_key.as_str(), named.model_label.as_str()),
            ("some-model", "some-model")
        );
        assert_eq!(named.route_label, "Devin account");
        // On the catalog, the designer is first.
        let image = configured(Some("claude-opus-5-5"));
        assert_eq!(image.model_key, "codex-image");
        assert_eq!(image.route_label, "Codex login");
    }

    #[test]
    fn an_image_agent_is_codex_images_whatever_model_it_names() {
        let catalog = catalog();
        for settings in [
            Settings {
                kind: Some("codex"),
                model: Some("gpt-6-astra"),
                effort: Some("max"),
                ..Settings::default()
            },
            Settings {
                kind: Some("image"),
                model: Some("anything"),
                ..Settings::default()
            },
            Settings::default(),
        ] {
            let profile = catalog.profile(&Settings {
                designer: true,
                ..settings
            });
            assert_eq!(
                (
                    profile.model_key.as_str(),
                    profile.model_label.as_str(),
                    profile.route_label.as_str(),
                    profile.route_note,
                    profile.work_tier
                ),
                (
                    "codex-image",
                    "Codex Images",
                    "Codex login",
                    None,
                    WorkTier::Light
                )
            );
        }
    }

    #[test]
    fn the_tier_reads_the_key_after_the_designer_override() {
        let catalog = catalog();
        let astra = Settings {
            kind: Some("codex"),
            model: Some("gpt-6-astra"),
            effort: Some("max"),
            ..Settings::default()
        };
        assert_eq!(catalog.profile(&astra).work_tier, WorkTier::Critical);
        let image = catalog.profile(&Settings {
            designer: true,
            ..astra
        });
        assert_eq!(image.work_tier, WorkTier::Light);
    }

    #[test]
    fn a_provider_path_is_stripped_only_for_a_known_pair() {
        let catalog = catalog();
        let on_pi = |model| {
            catalog.profile(&Settings {
                kind: Some("pi"),
                model: Some(model),
                effort: Some("high"),
                ..Settings::default()
            })
        };
        let curated = on_pi("openrouter/anthropic/claude-fable-5.1");
        assert_eq!(curated.model_key, "claude-fable-5.1");
        assert_eq!(curated.model_label, "Claude Fable 5.1");
        assert_eq!(curated.route_label, "OpenRouter · API");
        // Off the catalog, the whole id is the key and the label.
        let custom = on_pi("openrouter/anthropic/claude-x");
        assert_eq!(custom.model_key, "openrouter/anthropic/claude-x");
        assert_eq!(custom.model_label, "openrouter/anthropic/claude-x");
        assert_eq!(custom.route_label, "OpenRouter · API");
        // A curated id on a harness that has no preset for it is off the catalog too.
        let elsewhere = catalog.profile(&Settings {
            kind: Some("codex"),
            model: Some("openrouter/anthropic/claude-fable-5.1"),
            ..Settings::default()
        });
        assert_eq!(elsewhere.model_key, "openrouter/anthropic/claude-fable-5.1");
    }

    #[test]
    fn a_kind_and_a_harness_are_two_spellings_the_known_pairs_tell_apart() {
        let catalog = catalog();
        // `claude` as a kind is the harness `claude`: Opus on it is a preset's.
        let by_kind = catalog.profile(&Settings {
            kind: Some("claude"),
            model: Some("claude-opus-5-5"),
            effort: Some("max"),
            ..Settings::default()
        });
        assert_eq!(by_kind.model_key, "claude-opus-5.5");
        assert_eq!(by_kind.work_tier, WorkTier::Critical);
        // `claude-code` as a harness is no harness: nothing is known on it.
        let by_harness = catalog.profile(&Settings {
            harness: Some("claude-code"),
            model: Some("claude-opus-5-5"),
            effort: Some("max"),
            ..Settings::default()
        });
        assert_eq!(by_harness.model_key, "claude-opus-5-5");
        assert_eq!(by_harness.model_label, "claude-opus-5-5");
        assert_eq!(by_harness.route_label, "claude-code");
        assert_eq!(by_harness.work_tier, WorkTier::Light);
        // As a kind it is `claude`, and the harness, when there is one, wins over the kind.
        let by_claude_code = catalog.profile(&Settings {
            kind: Some("claude-code"),
            harness: Some("codex"),
            model: Some("claude-opus-5-5"),
            ..Settings::default()
        });
        assert_eq!(by_claude_code.route_label, "Codex login");
        assert_eq!(by_claude_code.model_key, "claude-opus-5-5");
    }

    #[test]
    fn kimi_is_complex_before_any_effort_rule_is_read() {
        let catalog = catalog();
        for effort in [None, Some(""), Some("low"), Some("bogus"), Some("max")] {
            let profile = catalog.profile(&Settings {
                kind: Some("opencode"),
                model: Some("openrouter/moonshotai/kimi-k3"),
                effort,
                ..Settings::default()
            });
            assert_eq!(profile.model_key, "kimi-k3");
            assert_eq!(profile.work_tier, WorkTier::Complex, "{effort:?}");
        }
        // Off the catalog the same name earns nothing.
        let unknown = catalog.profile(&Settings {
            kind: Some("opencode"),
            model: Some("openrouter/moonshotai/kimi-k3-preview/kimi-k3"),
            effort: Some("max"),
            ..Settings::default()
        });
        assert_eq!(unknown.work_tier, WorkTier::Light);
    }

    #[test]
    fn a_tier_the_human_chose_comes_last_of_all() {
        let catalog = catalog();
        let astra = Settings {
            kind: Some("codex"),
            model: Some("gpt-6-astra"),
            effort: Some("max"),
            ..Settings::default()
        };
        for tier in WORK_TIERS {
            let chosen = catalog.profile(&Settings {
                work_tier: Some(tier),
                ..astra
            });
            assert_eq!(chosen.work_tier, tier);
            let image = catalog.profile(&Settings {
                work_tier: Some(tier),
                designer: true,
                ..astra
            });
            assert_eq!(image.work_tier, tier);
        }
        let custom = catalog.profile(&Settings {
            kind: Some("codex"),
            model: Some("custom"),
            work_tier: Some(WorkTier::Critical),
            ..Settings::default()
        });
        assert_eq!(custom.work_tier, WorkTier::Critical);
        let devin = catalog.profile(&Settings {
            harness: Some("devin"),
            work_tier: Some(WorkTier::Standard),
            ..Settings::default()
        });
        assert_eq!(devin.work_tier, WorkTier::Standard);
    }

    #[test]
    fn pi_reads_its_thinking_before_its_effort_and_an_empty_thinking_is_still_a_reading() {
        let catalog = catalog();
        let sol = |thinking, effort| {
            catalog
                .profile(&Settings {
                    kind: Some("pi"),
                    model: Some("openai-codex/gpt-6.1-sol"),
                    thinking,
                    effort,
                    ..Settings::default()
                })
                .work_tier
        };
        assert_eq!(sol(Some("max"), Some("low")), WorkTier::Complex);
        assert_eq!(sol(None, Some("max")), WorkTier::Complex);
        assert_eq!(sol(Some("low"), Some("max")), WorkTier::Light);
        // `thinking ?? effort`: an empty thinking is not none, so it hides the effort.
        assert_eq!(sol(Some(""), Some("max")), WorkTier::Light);
        // Other harnesses read the effort alone.
        let on_codex = catalog.profile(&Settings {
            kind: Some("codex"),
            model: Some("gpt-6.1-sol"),
            thinking: Some("low"),
            effort: Some("max"),
            ..Settings::default()
        });
        assert_eq!(on_codex.work_tier, WorkTier::Complex);
    }

    #[test]
    fn an_agent_with_no_kind_and_no_harness_has_the_route_undefined() {
        let catalog = catalog();
        let nameless = catalog.profile(&Settings {
            model: Some("acme-x"),
            effort: Some("high"),
            ..Settings::default()
        });
        assert_eq!(nameless.route_label, "undefined");
        assert_eq!(nameless.model_key, "acme-x");
        // An empty word is a word: its route is empty, not `undefined`.
        for settings in [
            Settings {
                kind: Some(""),
                ..Settings::default()
            },
            Settings {
                harness: Some(""),
                ..Settings::default()
            },
        ] {
            assert_eq!(catalog.profile(&settings).route_label, "");
        }
        // Another harness's route is its own word.
        let image = catalog.profile(&Settings {
            kind: Some("image"),
            model: Some("gpt-image-2"),
            ..Settings::default()
        });
        assert_eq!(image.route_label, "image");
    }

    #[test]
    fn an_empty_model_keeps_its_empty_key_and_is_labelled_the_default() {
        let catalog = catalog();
        let empty = catalog.profile(&Settings {
            kind: Some("codex"),
            model: Some(""),
            ..Settings::default()
        });
        assert_eq!(
            (empty.model_key.as_str(), empty.model_label.as_str()),
            ("", "Default")
        );
        let none = catalog.profile(&Settings {
            kind: Some("codex"),
            ..Settings::default()
        });
        assert_eq!(
            (none.model_key.as_str(), none.model_label.as_str()),
            ("default", "Default")
        );
        assert_eq!(none.route_label, "Codex login");
    }

    #[test]
    fn the_prefix_of_a_model_names_its_road_before_its_harness_does() {
        let catalog = catalog();
        for (model, route) in [
            ("openrouter/x/y", "OpenRouter · API"),
            ("opencode/y", "OpenCode Zen"),
            ("openai-codex/y", "Codex subscription"),
            ("anthropic/y", "Anthropic · API"),
        ] {
            let profile = catalog.profile(&Settings {
                kind: Some("claude-code"),
                model: Some(model),
                ..Settings::default()
            });
            assert_eq!(profile.route_label, route, "{model}");
        }
        for (kind, route) in [
            ("claude-code", "Claude Code account"),
            ("codex", "Codex login"),
            ("devin", "Devin account"),
            ("pi", "pi"),
            ("opencode", "opencode"),
        ] {
            let profile = catalog.profile(&Settings {
                kind: Some(kind),
                model: Some("model-x"),
                ..Settings::default()
            });
            assert_eq!(profile.route_label, route, "{kind}");
        }
    }

    #[test]
    fn muse_s_contributor_routes_say_so_with_a_note_and_the_free_one_says_free() {
        let presets = vec![
            preset(
                "free",
                "opencode",
                "opencode/muse-spark-1.3-contributor-free",
                Some("high"),
            ),
            preset(
                "paid",
                "opencode",
                "opencode/muse-spark-1.3-contributor",
                Some("high"),
            ),
            preset(
                "plain",
                "pi",
                "openrouter/meta/muse-spark-1.3",
                Some("high"),
            ),
        ];
        let catalog = Catalog::new(
            presets,
            BTreeMap::from([("muse-spark-1.3".to_owned(), "Muse Spark 1.3".to_owned())]),
        );
        let on = |model, kind| {
            catalog.profile(&Settings {
                kind: Some(kind),
                model: Some(model),
                effort: Some("high"),
                ..Settings::default()
            })
        };
        let free = on("opencode/muse-spark-1.3-contributor-free", "opencode");
        assert_eq!(free.route_label, "OpenCode Zen · Contributor · Free");
        assert_eq!(
            free.route_note.as_deref(),
            Some("Prompts and replies may train Meta models.")
        );
        assert_eq!(
            (free.model_key.as_str(), free.model_label.as_str()),
            ("muse-spark-1.3", "Muse Spark 1.3")
        );
        let paid = on("opencode/muse-spark-1.3-contributor", "opencode");
        assert_eq!(paid.route_label, "OpenCode Zen · Contributor");
        assert_eq!(
            paid.route_note.as_deref(),
            Some("Prompts and replies may train Meta models.")
        );
        let plain = on("openrouter/meta/muse-spark-1.3", "pi");
        assert_eq!(plain.route_label, "OpenRouter · API");
        assert_eq!(plain.route_note, None);
        // Not a preset's pair: the model is called by its id and the route has no note.
        let unlisted = on("opencode/muse-spark-1.3-contributor-free", "pi");
        assert_eq!(
            unlisted.model_key,
            "opencode/muse-spark-1.3-contributor-free"
        );
        assert_eq!(unlisted.route_note, None);
    }

    #[test]
    fn a_label_that_is_empty_is_no_label() {
        let presets = vec![preset("blank", "codex", "gpt-x", Some("high"))];
        let labels = BTreeMap::from([("gpt-x".to_owned(), String::new())]);
        let catalog = Catalog::new(presets, labels);
        let profile = catalog.profile(&Settings {
            kind: Some("codex"),
            model: Some("gpt-x"),
            ..Settings::default()
        });
        assert_eq!(profile.model_label, "gpt-x");
    }
}
