//! The catalog (`src/catalog.js`): the ready-made agents of every harness,
//! and the harness each kind belongs to (`src/roster.js`).
//!
//! **One list, derived.** The groups are the presets themselves, reshaped
//! into the manager's vocabulary (kind to harness, thinking and effort to
//! effort); a preset whose kind is no harness the build runs is not offered.

use cf_proto::agents::{CatalogEntry, FoundEntry, Harness};

use crate::presets::Preset;
use crate::profile::Settings;
use crate::Catalog;

/// The harnesses ConsensFlow runs, in the order the page and the roster list
/// them (`HARNESSES`, `src/roster.js`).
pub const HARNESSES: [Harness; 5] = [
    Harness::Claude,
    Harness::Codex,
    Harness::Pi,
    Harness::Opencode,
    Harness::Devin,
];

/// The CLI behind a kind (`harnessForKind`): the store and the roster speak in
/// kinds (`claude-code`), the launcher needs the CLI to find the binary. None
/// for a kind the build does not run (`image`, `kimi`), and for a CLI's own
/// name (`claude`).
pub fn harness_for_kind(kind: &str) -> Option<Harness> {
    HARNESSES.into_iter().find(|harness| harness.kind() == kind)
}

/// The effort levels `harness` accepts (`EFFORTS`, `src/catalog.js`), quoted
/// from its CLI's own help output. `EFFORTS` keys them in `HARNESSES`' order.
pub fn efforts(harness: Harness) -> &'static [&'static str] {
    match harness {
        // claude --help: "Effort level for the current session (low, medium, high, xhigh, max)"
        Harness::Claude => &["low", "medium", "high", "xhigh", "max"],
        // The API enum is none…max; `ultra` is a codex-CLI level above it (verified live).
        Harness::Codex => &["minimal", "low", "medium", "high", "xhigh", "max", "ultra"],
        // pi --help: "Set thinking level: off, minimal, low, medium, high, xhigh, max"
        Harness::Pi => &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        // opencode --help: "provider-specific reasoning effort, e.g., high, max, minimal"
        Harness::Opencode => &["minimal", "low", "medium", "high", "xhigh", "max"],
        // Devin writes the level into its model id (claude-opus-5-5-max): an agent
        // names the family and one of these, and the launch joins them.
        Harness::Devin => &["low", "medium", "high", "xhigh", "max"],
    }
}

/// What one harness lists: its entries, in the order of the presets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub harness: Harness,
    pub entries: Vec<CatalogEntry>,
}

impl Catalog {
    /// `CATALOG`: each harness with its entries, the harnesses in the order
    /// they first appear among the presets, which is not `HARNESSES`' order.
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// `catalogEntry`: the entry named `name` with the harness that lists it;
    /// the first group holding the name wins.
    pub fn entry(&self, name: &str) -> Option<FoundEntry> {
        self.groups.iter().find_map(|group| {
            group
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| FoundEntry {
                    entry: entry.clone(),
                    harness: group.harness,
                })
        })
    }

    /// The presets grouped by harness, a harness at its first preset.
    pub(crate) fn grouped(&self) -> Vec<Group> {
        let mut groups: Vec<Group> = Vec::new();
        for preset in &self.presets {
            let Some(harness) = harness_for_kind(&preset.kind) else {
                continue;
            };
            let entry = self.entry_for(preset, harness);
            match groups.iter_mut().find(|group| group.harness == harness) {
                Some(group) => group.entries.push(entry),
                None => groups.push(Group {
                    harness,
                    entries: vec![entry],
                }),
            }
        }
        groups
    }

    /// `entryFor`: a preset as its harness's list shows it.
    fn entry_for(&self, preset: &Preset, harness: Harness) -> CatalogEntry {
        // What the profile is given: the preset's effort, else Pi's thinking.
        // The profile's own reading on Pi is the other way round (`thinking`
        // first), and it is given no `thinking`: the two readings meet here.
        let effort = preset.effort.as_deref().or(preset.thinking.as_deref());
        CatalogEntry {
            name: preset.preset.clone(),
            designer: preset.designer,
            model: preset.model.clone(),
            effort: effort
                .filter(|effort| !effort.is_empty())
                .map(str::to_owned),
            // `label` is the one-line headline ("Claude Code Fable 5.1 MAX"); the
            // preset's own prose is kept alongside for the card that wants it.
            description: preset.label.clone(),
            detail: preset.description.clone(),
            profile: self.profile(&Settings {
                harness: Some(harness.as_str()),
                model: Some(&preset.model),
                effort,
                designer: preset.designer,
                ..Settings::default()
            }),
            // Provenance, as a row an older build saved names its entry: the roster
            // reads such a copy as the catalog's own agent.
            preset: preset.preset.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_proto::agents::WorkTier;
    use std::collections::BTreeMap;

    fn preset(name: &str, kind: &str, model: &str) -> Preset {
        Preset {
            preset: name.to_owned(),
            id: name.to_owned(),
            name: name.to_owned(),
            label: format!("{name} headline"),
            description: format!("{name} paragraph"),
            kind: kind.to_owned(),
            model: model.to_owned(),
            effort: None,
            thinking: None,
            designer: false,
        }
    }

    fn catalog_of(presets: Vec<Preset>) -> Catalog {
        Catalog::new(presets, BTreeMap::new())
    }

    #[test]
    fn the_harnesses_are_listed_as_the_roster_lists_them() {
        let names: Vec<_> = HARNESSES.into_iter().map(Harness::as_str).collect();
        assert_eq!(names, ["claude", "codex", "pi", "opencode", "devin"]);
    }

    #[test]
    fn a_kind_names_its_harness_where_the_build_runs_one() {
        for harness in HARNESSES {
            assert_eq!(harness_for_kind(harness.kind()), Some(harness));
        }
        assert_eq!(harness_for_kind("claude-code"), Some(Harness::Claude));
        for kind in [
            "image",
            "kimi",
            "claude",
            "",
            "Codex",
            "codex ",
            "claude-code ",
            "constructor",
            "__proto__",
        ] {
            assert_eq!(harness_for_kind(kind), None, "{kind:?}");
        }
    }

    #[test]
    fn every_harness_accepts_some_efforts_and_the_codex_cli_one_more() {
        for harness in HARNESSES {
            assert!(!efforts(harness).is_empty(), "{}", harness.as_str());
        }
        assert!(efforts(Harness::Codex).contains(&"ultra"));
        for harness in [
            Harness::Claude,
            Harness::Pi,
            Harness::Opencode,
            Harness::Devin,
        ] {
            assert!(!efforts(harness).contains(&"ultra"), "{}", harness.as_str());
        }
        assert!(efforts(Harness::Pi).contains(&"off"));
    }

    #[test]
    fn the_groups_come_in_the_order_their_harnesses_first_appear_among_the_presets() {
        let catalog = catalog_of(vec![
            preset("a", "pi", "m1"),
            preset("b", "codex", "m2"),
            preset("c", "pi", "m3"),
            preset("d", "claude-code", "m4"),
            preset("e", "codex", "m5"),
            preset("f", "devin", "m6"),
        ]);
        let shape: Vec<(Harness, Vec<&str>)> = catalog
            .groups()
            .iter()
            .map(|group| {
                (
                    group.harness,
                    group
                        .entries
                        .iter()
                        .map(|entry| entry.name.as_str())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                (Harness::Pi, vec!["a", "c"]),
                (Harness::Codex, vec!["b", "e"]),
                (Harness::Claude, vec!["d"]),
                (Harness::Devin, vec!["f"]),
            ]
        );
    }

    #[test]
    fn a_preset_whose_kind_is_no_harness_is_left_out_of_the_groups() {
        let catalog = catalog_of(vec![
            preset("kim", "kimi", "kimi-k3"),
            preset("nova", "codex", "gpt-6-astra"),
            preset("painter", "image", "gpt-image-2"),
            preset("claude", "claude", "claude-opus-5-5"),
        ]);
        assert_eq!(catalog.presets().len(), 4);
        let listed: Vec<_> = catalog
            .groups()
            .iter()
            .flat_map(|group| group.entries.iter().map(|entry| entry.name.as_str()))
            .collect();
        assert_eq!(listed, ["nova"]);
        for name in ["kim", "painter", "claude"] {
            assert_eq!(catalog.entry(name), None, "{name}");
        }
        assert_eq!(catalog_of(vec![preset("kim", "kimi", "m")]).groups(), []);
    }

    #[test]
    fn the_first_group_holding_a_name_wins_not_the_first_preset() {
        // Pi's group comes first, by its first preset; the first preset named
        // `twin` is Codex's, which a lookup over the presets would answer.
        let catalog = catalog_of(vec![
            preset("seed", "pi", "m0"),
            preset("twin", "codex", "m1"),
            preset("twin", "pi", "m2"),
        ]);
        let found = catalog.entry("twin").unwrap();
        assert_eq!(found.harness, Harness::Pi);
        assert_eq!(found.entry.model, "m2");
    }

    #[test]
    fn a_lookup_answers_the_entry_and_its_harness_and_matches_the_name_exactly() {
        let catalog = Catalog::bundled().unwrap();
        let zeus = catalog.entry("zeus").unwrap();
        assert_eq!(zeus.harness, Harness::Claude);
        assert_eq!(zeus.entry.model, "claude-opus-5-5");
        for name in ["nobody", "", "@zeus", "Zeus", "zeus ", "ZEUS"] {
            assert_eq!(catalog.entry(name), None, "{name:?}");
        }
    }

    #[test]
    fn an_entry_headlines_with_the_label_and_keeps_the_paragraph_as_its_detail() {
        let catalog = catalog_of(vec![preset("nova", "codex", "gpt-6-astra")]);
        let entry = &catalog.groups()[0].entries[0];
        assert_eq!(entry.description, "nova headline");
        assert_eq!(entry.detail, "nova paragraph");
        assert_eq!(
            (entry.name.as_str(), entry.preset.as_str()),
            ("nova", "nova")
        );
    }

    #[test]
    fn an_entry_shows_the_effort_before_the_thinking_and_the_profile_reads_them_the_other_way() {
        let mut both = preset("both", "pi", "openai-codex/gpt-6.1-sol");
        both.effort = Some("low".to_owned());
        both.thinking = Some("max".to_owned());
        let mut thinking = preset("thinking", "pi", "openai-codex/gpt-6.1-sol");
        thinking.thinking = Some("max".to_owned());
        let catalog = catalog_of(vec![both, thinking]);
        let entries = &catalog.groups()[0].entries;
        // The entry shows `effort ?? thinking`, and the profile is given what it shows ...
        assert_eq!(entries[0].effort.as_deref(), Some("low"));
        assert_eq!(entries[0].profile.work_tier, WorkTier::Light);
        assert_eq!(entries[1].effort.as_deref(), Some("max"));
        assert_eq!(entries[1].profile.work_tier, WorkTier::Complex);
        // ... where an agent's own settings on Pi read `thinking ?? effort`.
        let own = catalog.profile(&Settings {
            kind: Some("pi"),
            model: Some("openai-codex/gpt-6.1-sol"),
            effort: Some("low"),
            thinking: Some("max"),
            ..Settings::default()
        });
        assert_eq!(own.work_tier, WorkTier::Complex);
    }

    #[test]
    fn an_empty_effort_is_left_out_of_the_entry_but_the_profile_is_still_given_it() {
        let mut empty = preset("empty", "pi", "openai-codex/gpt-6.1-sol");
        empty.effort = Some(String::new());
        empty.thinking = Some("max".to_owned());
        let catalog = catalog_of(vec![empty]);
        let entry = &catalog.groups()[0].entries[0];
        // `'' ?? 'max'` is `''`: not nullish, so it hides the thinking, and it is falsy.
        assert_eq!(entry.effort, None);
        assert_eq!(entry.profile.work_tier, WorkTier::Light);
    }

    #[test]
    fn an_image_preset_is_a_designer_entry_on_codex_images() {
        let mut image = preset("painter", "codex", "codex-image");
        image.designer = true;
        let catalog = catalog_of(vec![image]);
        let entry = &catalog.groups()[0].entries[0];
        assert!(entry.designer);
        assert_eq!(entry.profile.model_key, "codex-image");
        assert_eq!(entry.profile.route_label, "Codex login");
        assert_eq!(catalog.entry("painter").unwrap().harness, Harness::Codex);
    }
}
