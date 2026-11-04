//! The harnesses ConsensFlow runs and the work tiers a member is given work by, as
//! they cross to the CLI, the API and the page. A member is a harness, a model it
//! lists, an effort and a tier (spec `harness-models`): no catalog of agents, no
//! saved agent of the human's own.

use serde::Serialize;

use crate::cf_word::Shell;

/// A CLI ConsensFlow runs members on, in the order the page lists them.
/// The page, the CLI and the launcher speak these names; the store and the
/// ledger speak in kinds (`claude-code`). Kinds the build does not run
/// (`image`, `kimi`) are no harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    Claude,
    Codex,
    Pi,
    Devin,
    /// Meta's Muse Code (`muse`): detected and read since 3.0.6 (T-703); no
    /// window is opened on it yet.
    Muse,
}

impl Harness {
    /// Every harness, in the order the page lists them.
    pub const ALL: [Harness; 5] = [
        Harness::Claude,
        Harness::Codex,
        Harness::Pi,
        Harness::Devin,
        Harness::Muse,
    ];

    /// The CLI's own name (`claude`), as the page and the launcher say it.
    pub fn as_str(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Pi => "pi",
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

    /// The harness behind a kind: the store and the ledger speak in kinds
    /// (`claude-code`), the launcher needs the CLI to find the binary. None for
    /// a kind the build does not run (`image`, `kimi`), and for a CLI's own
    /// name (`claude`).
    pub fn from_kind(kind: &str) -> Option<Harness> {
        Self::ALL.into_iter().find(|harness| harness.kind() == kind)
    }

    /// The payload's word for it (`claude-code`), as the store and the ledger say it.
    pub fn kind(self) -> &'static str {
        match self {
            Harness::Claude => "claude-code",
            other => other.as_str(),
        }
    }

    /// The shell its window runs a command in, as far as how it runs a program by its path:
    /// Codex's on Windows is PowerShell (T-972, zeewin2), every other is bash-like (Git Bash
    /// on Windows: Claude Code, Pi and Devin). `windows` is where the window runs.
    pub fn shell(self, windows: bool) -> Shell {
        if windows && self == Harness::Codex {
            Shell::PowerShell
        } else {
            Shell::Posix
        }
    }

    /// The variables the harness's CLI sets to mark a session of its own, which every
    /// program it starts inherits: who the session is, how to reach it, what it runs on.
    /// A ConsensFlow started from inside such a session (`tauri dev`, a test run, the app
    /// opened from an agent's shell) would hand them to every window: a Claude window
    /// given `CLAUDE_CODE_CHILD_SESSION` saves no transcript, one given the parent's
    /// `CLAUDE_CODE_SESSION_ID` or `CODEX_THREAD_ID` is taken for it, and the messaging
    /// socket and token reach into the parent. Configuration is no part of it
    /// (`CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_USE_BEDROCK`, `CODEX_HOME`, `PI_CODING_AGENT_DIR`).
    ///
    /// Read from the binaries (2026-10-29, T-971): Claude Code 2.1.312's own list of what
    /// it gives a child session; Codex 0.165's sandbox (`codex sandbox -- env`) and the
    /// names its shell's environment is made of; Pi 1.4's `process.env` and its bash
    /// tool's; Devin 3000.11's terminal, whose shell script names and unsets both; Muse 1.4.14's hooks' and tools' (its
    /// session and thread, its log, a Claude-shaped tool call id, its trace) and its
    /// launcher's `MUSE_RELEASE_INFO`. A harness's own switches a launch sets
    /// (`MUSE_NO_AUTO_UPDATE`) are its configuration, and none of this.
    ///
    /// Held live since (`cargo xtask live session-vars`, T-1024, 2026-10-31), each window's
    /// shell read for the names it has: Codex 0.166 gives its shell `CODEX_CI` and
    /// `CODEX_VERSION` too, and sets `CODEX_NETWORK_PROXY_ACTIVE` (with its network proxy),
    /// `CODEX_PERMISSION_PROFILE` and `CODEX_PLUGIN_METRICS_OUTPUT` (a call's own metrics file)
    /// for the programs of its terminals and plugins, each beside `CODEX_TOOL_CALL_ID` and read
    /// as no setting; Muse 1.4.14 gives its shell `MUSE_TOOL_USE_ID`, its tool call's; Devin
    /// 3000.11.228 gives its shell `DEVIN_TRACEPARENT` (its trace, beside `traceparent`),
    /// `AI_AGENT`, and `CHISEL_SESSION_DB`, the path of its own session store, which a Devin
    /// that finds it set takes for its store: a member's Devin would keep its sessions in the
    /// parent's. Devin's terminal script unsets the two it reads, so no shell shows them.
    /// `CODEX_APPLY_PATCH_PRESERVE_LINE_ENDINGS` is a setting Codex hands its patch tool, and
    /// stays.
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
                "CODEX_CI",
                "CODEX_VERSION",
                "CODEX_NETWORK_PROXY_ACTIVE",
                "CODEX_PERMISSION_PROFILE",
                "CODEX_PLUGIN_METRICS_OUTPUT",
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
            Harness::Devin => &[
                "WINDSURF_CASCADE_TERMINAL",
                "DEVIN_DISABLE_HISTEXPAND",
                "DEVIN_TRACEPARENT",
                "CHISEL_SESSION_DB",
                "AI_AGENT",
            ],
            Harness::Muse => &[
                "MUSE_SESSION_ID",
                "MUSE_THREAD_ID",
                "MUSE_CURRENT_SESSION_LOG",
                "MUSE_HUMAN_CONFIRMATION",
                "MUSE_RELEASE_INFO",
                "MUSE_TOOL_USE_ID",
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

/// The variables a terminal sets in the shell of each of its panes to mark that pane as its
/// own, which a program started there hands every program it starts: cmux's (0.65.1, read
/// from its binaries and its harness wrappers, T-1024). Its socket's (`CMUX_SOCKET*`, its
/// password among them) and its hook's program reach into the cmux the app was started
/// from, and its pane's ids make a window that pane: cmux's own `claude` wrapper reports a
/// session's hooks to the pane `CMUX_SURFACE_ID` names, by its socket or the default one.
/// An app started from a cmux terminal gave them to every window.
pub const TERMINAL_SESSION_VARIABLES: [&str; 12] = [
    "CMUX_SOCKET",
    "CMUX_SOCKET_PATH",
    "CMUX_SOCKET_PASSWORD",
    "CMUX_SOCKET_PASSWORD_FILE",
    "CMUX_SOCKET_MODE",
    "CMUX_SOCKET_ENABLE",
    "CMUX_SOCKET_CAPABILITY",
    "CMUX_CLAUDE_HOOK_CMUX_BIN",
    "CMUX_SURFACE_ID",
    "CMUX_WORKSPACE_ID",
    "CMUX_PANEL_ID",
    "CMUX_TAB_ID",
];

/// What no program ConsensFlow starts inherits from the session that started ConsensFlow,
/// unless it is given it itself: a board's window's, which make a program that holds them
/// that board's client ([`cf_base::env::WINDOW_VARIABLES`], T-868), every harness's own
/// ([`Harness::session_variables`], T-971), and a terminal's
/// ([`TERMINAL_SESSION_VARIABLES`], T-1024). One list, which the app takes out of its own
/// environment and of what it takes of a terminal's, the daemon out of its own environment
/// (`cf ui`) and of what it gives a window of a terminal's, and the pane host out of what a
/// pane inherits.
pub fn parent_session_variables() -> impl Iterator<Item = &'static str> {
    cf_base::env::WINDOW_VARIABLES
        .into_iter()
        .chain(
            Harness::ALL
                .into_iter()
                .flat_map(Harness::session_variables)
                .copied(),
        )
        .chain(TERMINAL_SESSION_VARIABLES)
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

/// The work a member is given, from the most serious to the lightest: what a
/// task finds a member by. The human picks it for each member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkTier {
    Critical,
    Complex,
    Standard,
    Light,
}

impl WorkTier {
    /// The tier's own word (`critical`), as the ledger keeps it.
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

impl WorkTier {
    /// The tier a word names (`critical`); none for any other word.
    pub fn from_word(word: &str) -> Option<WorkTier> {
        WORK_TIERS.into_iter().find(|tier| tier.as_str() == word)
    }
}

/// The tiers in the order the pickers and the chief's text list them, the most
/// serious first.
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

/// The words Devin writes after the level in a model's id, each a variant of the family
/// at that level: `claude-opus-5-5-medium-fast`, `gpt-6-sol-high-priority`,
/// `swe-2-max-lightning`, `glm-5-2-max-1m` (`devin models list --format json`, Devin
/// 3000.11.205, T-606).
const DEVIN_VARIANTS: [&str; 5] = ["fast", "priority", "lightning", "1m", "thinking"];

/// The levels Devin writes into a model's id.
const DEVIN_LEVELS: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// A Devin model's id in the parts Devin writes it in: the family, the level, then the
/// variant's words (`claude-opus-5-5-medium-fast` is `claude-opus-5-5`, `medium` and
/// `fast`). The launch puts an agent's effort in as the level (T-609), and the catalog
/// knows a family by the levels its presets run it at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevinModel<'a> {
    /// The family (`claude-opus-5-5`): empty for an id of variant words alone.
    pub family: &'a str,
    /// The level the id names, if it names one.
    pub level: Option<&'a str>,
    /// The variant's words, joined as the id joins them (`medium-fast`'s `fast`): empty
    /// for none.
    pub variant: &'a str,
}

impl<'a> DevinModel<'a> {
    /// The parts of the id `model`.
    pub fn of(model: &'a str) -> Self {
        let words: Vec<&str> = model.split('-').collect();
        // Where the words before `count` of them end in `model`.
        let end = |count: usize| -> usize {
            words[..count].iter().map(|word| word.len()).sum::<usize>() + count.saturating_sub(1)
        };
        let kept = words
            .iter()
            .rposition(|word| !DEVIN_VARIANTS.contains(word))
            .map_or(0, |last| last + 1);
        let variant = if kept == words.len() {
            ""
        } else if kept == 0 {
            model
        } else {
            &model[end(kept) + 1..]
        };
        let level = kept
            .checked_sub(1)
            .map(|last| words[last])
            .filter(|word| DEVIN_LEVELS.contains(word));
        let family = end(kept - usize::from(level.is_some()));
        Self {
            family: &model[..family],
            level,
            variant,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_codex_on_windows_runs_its_commands_in_powershell() {
        for harness in Harness::ALL {
            assert_eq!(
                harness.shell(false),
                Shell::Posix,
                "{harness:?} off Windows"
            );
            let on_windows = if harness == Harness::Codex {
                Shell::PowerShell
            } else {
                Shell::Posix
            };
            assert_eq!(harness.shell(true), on_windows, "{harness:?} on Windows");
        }
    }

    #[test]
    fn a_harness_says_its_own_name_and_its_kind() {
        let names = [
            (Harness::Claude, "claude", "claude-code"),
            (Harness::Codex, "codex", "codex"),
            (Harness::Pi, "pi", "pi"),
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

    /// What each harness's shell showed of its session, live (`cargo xtask live session-vars`,
    /// T-1024), beyond what its binary had been read for: on the list.
    #[test]
    fn the_marks_a_harness_s_shell_showed_live_are_its_own() {
        for (harness, name) in [
            (Harness::Codex, "CODEX_CI"),
            (Harness::Codex, "CODEX_VERSION"),
            (Harness::Codex, "CODEX_NETWORK_PROXY_ACTIVE"),
            (Harness::Codex, "CODEX_PERMISSION_PROFILE"),
            (Harness::Codex, "CODEX_PLUGIN_METRICS_OUTPUT"),
            (Harness::Muse, "MUSE_TOOL_USE_ID"),
            (Harness::Devin, "DEVIN_TRACEPARENT"),
            (Harness::Devin, "CHISEL_SESSION_DB"),
            (Harness::Devin, "AI_AGENT"),
        ] {
            assert!(harness.session_variables().contains(&name), "{name}");
        }
    }

    /// What a harness is configured by is the human's, and reaches every window: only
    /// what marks a session is a session's.
    #[test]
    fn a_harness_configuration_marks_no_session() {
        for name in [
            "CODEX_APPLY_PATCH_PRESERVE_LINE_ENDINGS",
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_SUBAGENT_MODEL",
            "ANTHROPIC_API_KEY",
            "CODEX_HOME",
            "OPENAI_API_KEY",
            "PI_CODING_AGENT_DIR",
            "MUSE_NO_AUTO_UPDATE",
            "MUSE_AUTH_PATH",
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

    /// One list of what a program ConsensFlow starts never inherits: a board's window's, every
    /// harness's session's and a terminal's pane's, and nothing of a harness's or
    /// ConsensFlow's configuration.
    #[test]
    fn a_parent_session_is_a_boards_window_every_harness_session_and_a_terminals_pane() {
        for name in cf_base::env::WINDOW_VARIABLES {
            assert!(is_parent_session_variable(name), "{name}");
        }
        for harness in Harness::ALL {
            for name in harness.session_variables() {
                assert!(is_parent_session_variable(name), "{name}");
            }
        }
        // cmux's, which only a removed harness's throwaway server took out before (T-1024).
        for name in [
            "CMUX_SOCKET",
            "CMUX_SOCKET_PATH",
            "CMUX_SOCKET_PASSWORD",
            "CMUX_CLAUDE_HOOK_CMUX_BIN",
            "CMUX_SURFACE_ID",
            "CMUX_WORKSPACE_ID",
        ] {
            assert!(is_parent_session_variable(name), "{name}");
        }
        for name in [
            "CONSENSFLOW_HOME",
            "CONSENSFLOW_TERMINAL_ENV",
            "CLAUDE_CONFIG_DIR",
            "PATH",
            "CMUX_OTHER",
            "CMUX_",
        ] {
            assert!(!is_parent_session_variable(name), "{name}");
        }
    }

    #[test]
    fn a_devin_model_is_its_family_then_its_level_then_its_variant() {
        let parts = |model| {
            let made = DevinModel::of(model);
            (made.family, made.level, made.variant)
        };
        assert_eq!(parts("swe-2"), ("swe-2", None, ""));
        assert_eq!(
            parts("claude-opus-5-5-max"),
            ("claude-opus-5-5", Some("max"), "")
        );
        assert_eq!(
            parts("claude-opus-5-5-medium-fast"),
            ("claude-opus-5-5", Some("medium"), "fast")
        );
        assert_eq!(parts("glm-5-2-max-1m"), ("glm-5-2", Some("max"), "1m"));
        assert_eq!(
            parts("claude-opus-4-6-thinking"),
            ("claude-opus-4-6", None, "thinking")
        );
        assert_eq!(
            parts("gpt-6-sol-fast-priority"),
            ("gpt-6-sol", None, "fast-priority")
        );
        assert_eq!(parts("fast"), ("", None, "fast"));
        assert_eq!(parts("max-fast"), ("", Some("max"), "fast"));
        assert_eq!(parts(""), ("", None, ""));
    }

    #[test]
    fn the_harnesses_are_listed_as_the_page_lists_them() {
        let names: Vec<_> = Harness::ALL.into_iter().map(Harness::as_str).collect();
        assert_eq!(names, ["claude", "codex", "pi", "devin", "muse"]);
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
    fn the_tiers_go_from_the_most_serious_and_each_word_names_its_own() {
        assert_eq!(
            WORK_TIERS.map(WorkTier::as_str),
            ["critical", "complex", "standard", "light"]
        );
        for tier in WORK_TIERS {
            assert_eq!(WorkTier::from_word(tier.as_str()), Some(tier));
            assert!(!work_tier_info(tier).label.is_empty());
        }
        for word in ["", "Critical", "heavy", " light"] {
            assert_eq!(WorkTier::from_word(word), None, "{word:?}");
        }
    }
}
