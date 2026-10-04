//! The agents the catalog lists, as they cross to the CLI, the API and the
//! page: each shape with its fields in the order the Node code builds them
//! (`hosts/lib/presets.js`, `src/catalog.js`), so its JSON reads as it did.

use serde::Serialize;

/// A CLI ConsensFlow runs agents on, in the order `HARNESSES`
/// (`src/roster.js`) lists them. The page, the CLI and the launcher speak
/// these names; the store and the roster speak in kinds (`claude-code`).
/// Kinds the build does not run (`image`, `kimi`) are no harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    Claude,
    Codex,
    Pi,
    Opencode,
    Devin,
}

impl Harness {
    /// The CLI's own name (`claude`), as the page and the launcher say it.
    pub fn as_str(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Pi => "pi",
            Harness::Opencode => "opencode",
            Harness::Devin => "devin",
        }
    }

    /// The payload's word for it (`claude-code`), as the store and the roster say it.
    pub fn kind(self) -> &'static str {
        match self {
            Harness::Claude => "claude-code",
            other => other.as_str(),
        }
    }
}

/// The work an agent suits, from the most serious to the lightest: what a
/// task finds an agent by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkTier {
    Critical,
    Complex,
    Standard,
    Light,
}

impl WorkTier {
    /// The tier's own word (`critical`), as the roster saves it.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkTier::Critical => "critical",
            WorkTier::Complex => "complex",
            WorkTier::Standard => "standard",
            WorkTier::Light => "light",
        }
    }
}

/// How a tier reads on the page and in the chief's text (`WORK_TIERS[tier]`,
/// `hosts/lib/presets.js`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WorkTierInfo {
    pub label: &'static str,
    pub description: &'static str,
}

/// What a model and the road to it are called, and the work its agent
/// suits (`agentProfile`, `hosts/lib/presets.js`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// The model's identity (`gpt-6-astra`): one key for it on every road.
    pub model_key: String,
    pub model_label: String,
    /// The road to the model (`Codex login`, `OpenRouter · API`).
    pub route_label: String,
    /// What a person should know of the road's data use, said for Muse's contributor routes alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_note: Option<String>,
    pub work_tier: WorkTier,
}

/// A ready-made agent as its harness's list shows it (`entryFor`, `src/catalog.js`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogEntry {
    /// The preset's name: the agent's handle.
    pub name: String,
    /// An image agent; said only when true.
    #[serde(skip_serializing_if = "is_false")]
    pub designer: bool,
    pub model: String,
    /// The effort the preset names (Pi's `thinking` when it has no `effort`);
    /// said only when it is not empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// The one-line headline (`Claude Code Fable 5.1 MAX`).
    pub description: String,
    /// The preset's own paragraph, for the card that wants it.
    pub detail: String,
    pub profile: Profile,
    /// Provenance, as a row an older build saved names its entry.
    pub preset: String,
}

/// What a lookup by name answers (`catalogEntry`): the entry, then the
/// harness whose list it is in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoundEntry {
    #[serde(flatten)]
    pub entry: CatalogEntry,
    pub harness: Harness,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn muse() -> Profile {
        Profile {
            model_key: "muse-spark-1.3".into(),
            model_label: "Muse Spark 1.3".into(),
            route_label: "OpenCode Zen · Contributor · Free".into(),
            route_note: Some("Prompts and replies may train Meta models.".into()),
            work_tier: WorkTier::Light,
        }
    }

    fn entry() -> CatalogEntry {
        CatalogEntry {
            name: "gefjon".into(),
            designer: false,
            model: "opencode/muse-spark-1.3-contributor-free".into(),
            effort: Some("xhigh".into()),
            description: "OpenCode Zen Muse Spark 1.3 Contributor FREE XHIGH".into(),
            detail: "Collaborative coding and task breakdown.".into(),
            profile: muse(),
            preset: "gefjon".into(),
        }
    }

    #[test]
    fn a_harness_says_its_own_name_and_its_kind() {
        let names = [
            (Harness::Claude, "claude", "claude-code"),
            (Harness::Codex, "codex", "codex"),
            (Harness::Pi, "pi", "pi"),
            (Harness::Opencode, "opencode", "opencode"),
            (Harness::Devin, "devin", "devin"),
        ];
        for (harness, name, kind) in names {
            assert_eq!(harness.as_str(), name);
            assert_eq!(harness.kind(), kind);
            assert_eq!(
                serde_json::to_string(&harness).unwrap(),
                format!("\"{name}\"")
            );
        }
    }

    #[test]
    fn a_work_tier_says_its_own_word() {
        for (tier, word) in [
            (WorkTier::Critical, "critical"),
            (WorkTier::Complex, "complex"),
            (WorkTier::Standard, "standard"),
            (WorkTier::Light, "light"),
        ] {
            assert_eq!(tier.as_str(), word);
            assert_eq!(serde_json::to_string(&tier).unwrap(), format!("\"{word}\""));
        }
    }

    #[test]
    fn a_profile_writes_its_fields_in_the_order_the_catalog_builds_them() {
        assert_eq!(
            serde_json::to_string(&muse()).unwrap(),
            r#"{"modelKey":"muse-spark-1.3","modelLabel":"Muse Spark 1.3","routeLabel":"OpenCode Zen · Contributor · Free","routeNote":"Prompts and replies may train Meta models.","workTier":"light"}"#
        );
    }

    #[test]
    fn a_profile_with_no_route_note_says_none() {
        let plain = Profile {
            route_note: None,
            ..muse()
        };
        let written = serde_json::to_string(&plain).unwrap();
        assert!(!written.contains("routeNote"), "{written}");
        assert!(written
            .ends_with(r#""routeLabel":"OpenCode Zen · Contributor · Free","workTier":"light"}"#));
    }

    #[test]
    fn an_entry_writes_its_fields_in_the_order_the_catalog_builds_them() {
        assert_eq!(
            serde_json::to_string(&entry()).unwrap(),
            concat!(
                r#"{"name":"gefjon","model":"opencode/muse-spark-1.3-contributor-free","effort":"xhigh","#,
                r#""description":"OpenCode Zen Muse Spark 1.3 Contributor FREE XHIGH","#,
                r#""detail":"Collaborative coding and task breakdown.","#,
                r#""profile":{"modelKey":"muse-spark-1.3","modelLabel":"Muse Spark 1.3","routeLabel":"OpenCode Zen · Contributor · Free","routeNote":"Prompts and replies may train Meta models.","workTier":"light"},"#,
                r#""preset":"gefjon"}"#
            )
        );
    }

    #[test]
    fn an_entry_says_designer_after_its_name_only_when_true_and_effort_only_when_named() {
        let image = CatalogEntry {
            designer: true,
            effort: None,
            ..entry()
        };
        let written = serde_json::to_string(&image).unwrap();
        assert!(
            written.starts_with(r#"{"name":"gefjon","designer":true,"model":"#),
            "{written}"
        );
        assert!(!written.contains("\"effort\""), "{written}");
        let plain = serde_json::to_string(&entry()).unwrap();
        assert!(!plain.contains("designer"), "{plain}");
    }

    #[test]
    fn a_lookup_answers_the_entry_with_its_harness_after_the_preset() {
        let found = FoundEntry {
            entry: entry(),
            harness: Harness::Opencode,
        };
        let written = serde_json::to_string(&found).unwrap();
        assert!(
            written.ends_with(r#""preset":"gefjon","harness":"opencode"}"#),
            "{written}"
        );
        assert!(
            written.starts_with(r#"{"name":"gefjon","model":"#),
            "{written}"
        );
    }
}
