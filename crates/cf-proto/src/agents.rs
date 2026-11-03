//! The harnesses ConsensFlow runs and the work tiers a member is given work by, as
//! they cross to the CLI, the API and the page. A member is a harness, a model it
//! lists, an effort and a tier (spec `harness-models`): no catalog of agents, no
//! saved agent of the human's own.

use serde::Serialize;

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

#[cfg(test)]
mod tests {
    use super::*;

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
