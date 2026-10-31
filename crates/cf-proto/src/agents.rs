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
    /// Meta's Muse Code (`muse`): detected and read since 3.0.6 (T-703); no
    /// window is opened on it yet.
    Muse,
}

impl Harness {
    /// Every harness, in the order the page and the roster list them.
    pub const ALL: [Harness; 6] = [
        Harness::Claude,
        Harness::Codex,
        Harness::Pi,
        Harness::Opencode,
        Harness::Devin,
        Harness::Muse,
    ];

    /// The CLI's own name (`claude`), as the page and the launcher say it.
    pub fn as_str(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Pi => "pi",
            Harness::Opencode => "opencode",
            Harness::Devin => "devin",
            Harness::Muse => "muse",
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

    /// The variables the harness's CLI sets to mark a session of its own, which every
    /// program it starts inherits: who the session is, how to reach it, what it runs on.
    /// A ConsensFlow started from inside such a session (`tauri dev`, a test run, the app
    /// opened from an agent's shell) would hand them to every window: a Claude window
    /// given `CLAUDE_CODE_CHILD_SESSION` saves no transcript, one given the parent's
    /// `CLAUDE_CODE_SESSION_ID` or `CODEX_THREAD_ID` is taken for it, and the messaging
    /// socket and token reach into the parent. Configuration is no part of it
    /// (`CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_USE_BEDROCK`, `CODEX_HOME`, `PI_CODING_AGENT_DIR`),
    /// nor is OpenCode's `AGENT=1`, true of every window, and too common a name.
    ///
    /// Read from the binaries (2026-10-29, T-971): Claude Code 2.1.312's own list of what
    /// it gives a child session; Codex 0.165's sandbox (`codex sandbox -- env`) and the
    /// names its shell's environment is made of; Pi 1.4's `process.env` and its bash
    /// tool's; OpenCode 1.19's `process.env` and its terminal's; Devin 3000.11's terminal,
    /// whose shell script names and unsets both; Muse 1.4.14's hooks' and tools' (its
    /// session and thread, its log, a Claude-shaped tool call id, its trace) and its
    /// launcher's `MUSE_RELEASE_INFO`. A harness's own switches a launch sets
    /// (`MUSE_NO_AUTO_UPDATE`) are its configuration, and none of this.
    pub fn session_variables(self) -> &'static [&'static str] {
        match self {
            Harness::Claude => &[
                "CLAUDECODE",
                "CLAUDE_CODE_ENTRYPOINT",
                "CLAUDE_CODE_SESSION_ID",
                "CLAUDE_CODE_CHILD_SESSION",
                "CLAUDE_CODE_SESSION_ATTENDED",
                "CLAUDE_CODE_MESSAGING_SOCKET",
                "CLAUDE_CODE_MESSAGING_TOKEN",
                "CLAUDE_CODE_BRIDGE_SESSION_ID",
                "CLAUDE_CODE_EXECPATH",
                "CLAUDE_PID",
                "CLAUDE_EFFORT",
                "AI_AGENT",
                "TRACEPARENT",
            ],
            Harness::Codex => &[
                "CODEX_SANDBOX",
                "CODEX_SANDBOX_NETWORK_DISABLED",
                "CODEX_THREAD_ID",
                "CODEX_SESSION_ID",
                "CODEX_TOOL_CALL_ID",
            ],
            Harness::Pi => &[
                "PI_CODING_AGENT",
                "PI_SESSION_ID",
                "PI_SESSION_FILE",
                "PI_PROVIDER",
                "PI_MODEL",
                "PI_REASONING_LEVEL",
                "AI_AGENT",
            ],
            Harness::Opencode => &["OPENCODE", "OPENCODE_PID", "OPENCODE_TERMINAL"],
            Harness::Devin => &["WINDSURF_CASCADE_TERMINAL", "DEVIN_DISABLE_HISTEXPAND"],
            Harness::Muse => &[
                "MUSE_SESSION_ID",
                "MUSE_THREAD_ID",
                "MUSE_CURRENT_SESSION_LOG",
                "MUSE_HUMAN_CONFIRMATION",
                "MUSE_RELEASE_INFO",
                "CLAUDE_CODE_TOOL_USE_ID",
                "TRACEPARENT",
                "TRACESTATE",
            ],
        }
    }
}

/// Whether `name` marks a session of some harness's own ([`Harness::session_variables`]),
/// as the system compares names: on Windows in any case.
pub fn is_session_variable(name: &str) -> bool {
    Harness::ALL
        .into_iter()
        .flat_map(Harness::session_variables)
        .any(|marked| same_name(marked, name))
}

/// What no program ConsensFlow starts inherits from the session that started ConsensFlow,
/// unless it is given it itself: a board's window's, which make a program that holds them
/// that board's client ([`cf_base::env::WINDOW_VARIABLES`], T-868), and every harness's own
/// ([`Harness::session_variables`], T-971). One list, which the app takes out of its own
/// environment and of what it takes of a terminal's, the daemon out of what it gives a window
/// of a terminal's, and the pane host out of what a pane inherits.
pub fn parent_session_variables() -> impl Iterator<Item = &'static str> {
    cf_base::env::WINDOW_VARIABLES.into_iter().chain(
        Harness::ALL
            .into_iter()
            .flat_map(Harness::session_variables)
            .copied(),
    )
}

/// Whether `name` is one of the [`parent_session_variables`], as the system compares names.
pub fn is_parent_session_variable(name: &str) -> bool {
    parent_session_variables().any(|marked| same_name(marked, name))
}

/// Whether two variables' names are one name: on Windows in any case.
fn same_name(one: &str, other: &str) -> bool {
    if cfg!(windows) {
        one.eq_ignore_ascii_case(other)
    } else {
        one == other
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
/// agents, as the Agents screen and the CLI read it. What they say of specs for
/// every project is kept in the same place of the file and is no choice about
/// the roster: the page alone says it (`cf_proto::specs::AppSpecs`).
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
            (Harness::Muse, "muse", "muse"),
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

    /// Every harness the build runs names what marks a session of its own (T-971): a
    /// harness added with none fails here until its CLI is read for them.
    #[test]
    fn every_harness_names_the_variables_that_mark_its_own_session() {
        for harness in Harness::ALL {
            let names = harness.session_variables();
            assert!(!names.is_empty(), "{} names none", harness.as_str());
            for name in names {
                assert!(
                    !name.is_empty()
                        && name.bytes().all(|byte| byte.is_ascii_uppercase()
                            || byte.is_ascii_digit()
                            || byte == b'_'),
                    "{}: {name:?} is no variable's name",
                    harness.as_str()
                );
                assert!(is_session_variable(name), "{name}");
            }
        }
    }

    /// What a harness is configured by is the human's, and reaches every window: only
    /// what marks a session is a session's.
    #[test]
    fn a_harness_configuration_marks_no_session() {
        for name in [
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_SUBAGENT_MODEL",
            "ANTHROPIC_API_KEY",
            "CODEX_HOME",
            "OPENAI_API_KEY",
            "PI_CODING_AGENT_DIR",
            "MUSE_NO_AUTO_UPDATE",
            "MUSE_AUTH_PATH",
            "OPENCODE_CONFIG",
            "AGENT",
            "CONSENSFLOW_HOME",
            "PATH",
            "HOME",
            "TERM",
            "",
        ] {
            assert!(!is_session_variable(name), "{name}");
        }
        assert_eq!(is_session_variable("claudecode"), cfg!(windows));
    }

    /// One list of what a program ConsensFlow starts never inherits: a board's window's and
    /// every harness's session's, and nothing of a harness's or ConsensFlow's configuration.
    #[test]
    fn a_parent_session_is_a_boards_window_and_every_harness_session() {
        for name in cf_base::env::WINDOW_VARIABLES {
            assert!(is_parent_session_variable(name), "{name}");
        }
        for harness in Harness::ALL {
            for name in harness.session_variables() {
                assert!(is_parent_session_variable(name), "{name}");
            }
        }
        for name in [
            "CONSENSFLOW_HOME",
            "CONSENSFLOW_TERMINAL_ENV",
            "CLAUDE_CONFIG_DIR",
            "PATH",
        ] {
            assert!(!is_parent_session_variable(name), "{name}");
        }
    }

    #[test]
    fn the_harnesses_are_listed_as_the_roster_lists_them() {
        let names: Vec<_> = Harness::ALL.into_iter().map(Harness::as_str).collect();
        assert_eq!(
            names,
            ["claude", "codex", "pi", "opencode", "devin", "muse"]
        );
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
    fn preferences_write_the_one_choice_about_the_roster_and_nothing_of_specs() {
        for (own_harness_only, written) in [
            (true, r#"{"ownHarnessOnly":true}"#),
            (false, r#"{"ownHarnessOnly":false}"#),
        ] {
            let preferences = Preferences { own_harness_only };
            assert_eq!(serde_json::to_string(&preferences).unwrap(), written);
        }
    }
}
