//! The agents the catalog lists and the roster shows, as they cross to the CLI,
//! the API and the page: each shape with its fields in the order the Node code
//! built them, so its JSON reads as it did.

use serde::Serialize;
use serde_json::Value;

/// A CLI ConsensFlow runs agents on, in the order Node's roster listed them.
/// The page, the CLI and the launcher speak these names; the store and the
/// roster speak in kinds (`claude-code`). Kinds the build does not run
/// (`image`, `kimi`) are no harness.
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
    /// Every harness, in the order the page and the roster list them.
    pub const ALL: [Harness; 5] = [
        Harness::Claude,
        Harness::Codex,
        Harness::Pi,
        Harness::Opencode,
        Harness::Devin,
    ];

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

    /// The harness a request names by the CLI's own name (`claude`); none for
    /// any other word, a kind (`claude-code`) among them.
    pub fn from_name(name: &str) -> Option<Harness> {
        Self::ALL
            .into_iter()
            .find(|harness| harness.as_str() == name)
    }

    /// The harness behind a kind: the store and the roster speak in kinds
    /// (`claude-code`), the launcher needs the CLI to find the binary. None for
    /// a kind the build does not run (`image`, `kimi`), and for a CLI's own
    /// name (`claude`).
    pub fn from_kind(kind: &str) -> Option<Harness> {
        Self::ALL.into_iter().find(|harness| harness.kind() == kind)
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

/// How a tier reads on the page and in the chief's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WorkTierInfo {
    pub label: &'static str,
    pub description: &'static str,
}

/// What a model and the road to it are called, and the work its agent suits.
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

/// A ready-made agent as its harness's list shows it.
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

/// An agent of the roster as the page and the CLI list it. A key JavaScript
/// leaves `undefined` is not written by `JSON.stringify`, so a field the row
/// did not have is none and skipped: a row with no `id` has no `name`, one with
/// no `kind` no `harness`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    /// The row's `id`: the agent's handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The CLI's own name (`claude`) for a kind the build runs, else the kind
    /// as the row names it (`kimi`): text, not a [`Harness`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// An image agent; said only when the row's `designer` is `true` itself.
    #[serde(skip_serializing_if = "is_false")]
    pub designer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The tier the human chose for the agent, when they chose one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_tier: Option<WorkTier>,
    /// The effort under the key the row's kind reads (Pi's `thinking`, every
    /// other kind's `effort`); said only when it is not empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// The row's own description, whatever JSON it holds: said when truthy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    /// Provenance, as a row an older build saved names its entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// An agent the human defined: the catalog's have none.
    #[serde(skip_serializing_if = "is_false")]
    pub custom: bool,
    pub profile: Profile,
    /// A kind the build does not run (`kimi`): the row is kept, never launched.
    #[serde(skip_serializing_if = "is_false")]
    pub unsupported: bool,
    /// Kept out of the list the human sees: a Claude or OpenAI model through
    /// Pi or OpenCode, while they keep to their own harnesses. Said by
    /// `listAgents` alone, and last.
    #[serde(skip_serializing_if = "is_false")]
    pub hidden: bool,
}

/// What the human chose about the roster, kept in the file beside their own
/// agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    /// Claude and OpenAI models reached through Pi or OpenCode are hidden.
    pub own_harness_only: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn the_harnesses_are_listed_as_the_roster_lists_them() {
        let names: Vec<_> = Harness::ALL.into_iter().map(Harness::as_str).collect();
        assert_eq!(names, ["claude", "codex", "pi", "opencode", "devin"]);
    }

    #[test]
    fn a_name_or_a_kind_names_its_harness_where_the_build_runs_one() {
        for harness in Harness::ALL {
            assert_eq!(Harness::from_name(harness.as_str()), Some(harness));
            assert_eq!(Harness::from_kind(harness.kind()), Some(harness));
        }
        assert_eq!(Harness::from_kind("claude-code"), Some(Harness::Claude));
        assert_eq!(Harness::from_name("claude-code"), None);
        for word in [
            "image",
            "kimi",
            "",
            "Codex",
            "codex ",
            "claude-code ",
            "constructor",
            "__proto__",
        ] {
            assert_eq!(Harness::from_kind(word), None, "{word:?}");
            assert_eq!(Harness::from_name(word), None, "{word:?}");
        }
        assert_eq!(Harness::from_kind("claude"), None);
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

    fn bare() -> AgentView {
        AgentView {
            name: None,
            harness: None,
            designer: false,
            model: None,
            work_tier: None,
            effort: None,
            description: None,
            preset: None,
            custom: false,
            profile: muse(),
            unsupported: false,
            hidden: false,
        }
    }

    #[test]
    fn a_view_writes_its_fields_in_the_order_the_roster_builds_them() {
        let view = AgentView {
            name: Some("mine".into()),
            harness: Some("kimi".into()),
            designer: true,
            model: Some("moonshot-ai/kimi-k3".into()),
            work_tier: Some(WorkTier::Complex),
            effort: Some("high".into()),
            description: Some(json!("Mine")),
            preset: Some("mine".into()),
            custom: true,
            unsupported: true,
            hidden: true,
            ..bare()
        };
        assert_eq!(
            serde_json::to_string(&view).unwrap(),
            concat!(
                r#"{"name":"mine","harness":"kimi","designer":true,"model":"moonshot-ai/kimi-k3","#,
                r#""workTier":"complex","effort":"high","description":"Mine","preset":"mine","custom":true,"#,
                r#""profile":{"modelKey":"muse-spark-1.3","modelLabel":"Muse Spark 1.3","routeLabel":"OpenCode Zen · Contributor · Free","routeNote":"Prompts and replies may train Meta models.","workTier":"light"},"#,
                r#""unsupported":true,"hidden":true}"#
            )
        );
    }

    #[test]
    fn a_view_of_a_row_with_nothing_to_say_writes_its_profile_alone() {
        // `JSON.stringify` leaves out what is `undefined`: a row with no `id`, `kind` or
        // `model` has no `name`, `harness` or `model`, and the flags are said only when set.
        let written = serde_json::to_string(&bare()).unwrap();
        assert!(
            written.starts_with(r#"{"profile":{"modelKey":"#),
            "{written}"
        );
        assert!(written.ends_with(r#""workTier":"light"}}"#), "{written}");
    }

    #[test]
    fn a_view_keeps_the_description_as_the_json_the_row_held() {
        for description in [json!(5), json!(true), json!([]), json!({}), json!("text")] {
            let view = AgentView {
                description: Some(description.clone()),
                ..bare()
            };
            let written = serde_json::to_value(&view).unwrap();
            assert_eq!(written["description"], description);
        }
    }

    #[test]
    fn preferences_write_the_one_choice_the_roster_keeps() {
        let on = Preferences {
            own_harness_only: true,
        };
        assert_eq!(
            serde_json::to_string(&on).unwrap(),
            r#"{"ownHarnessOnly":true}"#
        );
        let off = Preferences {
            own_harness_only: false,
        };
        assert_eq!(
            serde_json::to_string(&off).unwrap(),
            r#"{"ownHarnessOnly":false}"#
        );
    }
}
