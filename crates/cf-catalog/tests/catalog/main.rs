//! The 26 tests of `tests/catalog.test.mjs`, ported with the catalog: each
//! keeps its sentence, as a name, and its assertions, against the catalog
//! built into the crate. A `describe` block of the JS is a module here, in a
//! file of its own; the tests outside one are in this file.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};

use cf_catalog::{efforts, Catalog, CatalogEntry, FoundEntry, Harness, Preset, Settings, WorkTier};

mod catalog_presentation_follows_actual_model_and_effort;
mod claude_haiku_5_5_is_light_work_on_claude_code_alone;
mod every_tool_ships_a_list_of_ready_made_agents;

fn catalog() -> Catalog {
    Catalog::bundled().unwrap()
}

/// Every entry of the catalog with the harness that lists it, harness by harness.
fn entries(catalog: &Catalog) -> impl Iterator<Item = (Harness, &CatalogEntry)> {
    catalog.groups().iter().flat_map(|group| {
        group
            .entries
            .iter()
            .map(move |entry| (group.harness, entry))
    })
}

/// What one harness lists (`CATALOG.pi`); none for a harness with no group.
fn listed(catalog: &Catalog, harness: Harness) -> &[CatalogEntry] {
    catalog
        .groups()
        .iter()
        .find(|group| group.harness == harness)
        .map_or(&[], |group| group.entries.as_slice())
}

/// The entry `catalogEntry(name)` finds, which the test says is there.
fn found(catalog: &Catalog, name: &str) -> FoundEntry {
    catalog
        .entry(name)
        .unwrap_or_else(|| panic!("{name} is in the catalog"))
}

/// The agent a lookup found, as its own settings: the harness, the model, the
/// effort and the designer flag, nothing of its name or its profile.
fn settings_of(found: &FoundEntry) -> Settings<'_> {
    Settings {
        harness: Some(found.harness.as_str()),
        model: Some(&found.entry.model),
        effort: found.entry.effort.as_deref(),
        designer: found.entry.designer,
        ..Settings::default()
    }
}

/// `/^[a-z][a-z0-9-]*$/`.
fn is_agent_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[test]
fn ships_nothing_for_kimi_code_which_consensflow_does_not_run() {
    // Kimi is no harness of the build: it has no group, no efforts, and no preset of its kind.
    let catalog = catalog();
    assert!(Harness::ALL
        .iter()
        .all(|harness| harness.as_str() != "kimi"));
    assert!(catalog
        .groups()
        .iter()
        .all(|group| group.harness.as_str() != "kimi"));
    assert_eq!(Harness::from_kind("kimi"), None);
    assert!(catalog.presets().iter().all(|preset| preset.kind != "kimi"));
    assert_eq!(catalog.entry("ilmarinen"), None);
}

#[test]
fn ships_all_compatible_low_medium_choices_with_stable_identities_and_pi_openrouter_routing() {
    let catalog = catalog();
    let matrix = [
        (Harness::Codex, "gpt-6.1-sol", "hemera", "phaethon"),
        (Harness::Pi, "openai-codex/gpt-6.1-sol", "leto", "asterope"),
        (
            Harness::Opencode,
            "openrouter/openai/gpt-6.1-sol",
            "arvakr",
            "alsvidr",
        ),
        (Harness::Codex, "gpt-6-astra", "electra", "maia"),
        (Harness::Pi, "openai-codex/gpt-6-astra", "alcyone", "merope"),
        (
            Harness::Opencode,
            "openrouter/openai/gpt-6-astra",
            "dagr",
            "skirnir",
        ),
        (Harness::Claude, "claude-fable-5-1", "terpsichore", "thalia"),
        (
            Harness::Pi,
            "openrouter/anthropic/claude-fable-5.1",
            "musaeus",
            "erato",
        ),
        (
            Harness::Opencode,
            "openrouter/anthropic/claude-fable-5.1",
            "suttung",
            "kvasir",
        ),
    ];
    for (harness, model, low, medium) in matrix {
        for (name, effort) in [(low, "low"), (medium, "medium")] {
            let entry = found(&catalog, name);
            assert_eq!(entry.harness, harness, "{name}");
            assert_eq!(entry.entry.model, model, "{name}");
            assert_eq!(entry.entry.effort.as_deref(), Some(effort), "{name}");
        }
    }
    assert_eq!(entries(&catalog).count(), 120);
    for name in ["orpheus", "linus", "erato", "kronos", "atlas"] {
        let entry = found(&catalog, name);
        assert!(
            entry.entry.model.starts_with("openrouter/anthropic/"),
            "{name}"
        );
        assert_eq!(
            entry.entry.profile.route_label, "OpenRouter · API",
            "{name}"
        );
    }
    assert!(!listed(&catalog, Harness::Claude)
        .iter()
        .any(|e| e.model.contains("astra")));
    assert!(!listed(&catalog, Harness::Codex)
        .iter()
        .any(|e| e.model.contains("fable")));
}

#[test]
fn gemini_3_1_pro_preview_is_retired_on_every_harness_while_gemini_3_8_flash_remains() {
    let catalog = catalog();
    assert_eq!(catalog.entry("helios"), None);
    assert_eq!(catalog.entry("heimdall"), None);
    assert!(!entries(&catalog).any(|(_, p)| p.model.contains("gemini-3.1-pro")));
    assert_eq!(
        found(&catalog, "nike").entry.model,
        "openrouter/google/gemini-3.8-flash"
    );
    assert_eq!(
        found(&catalog, "sif").entry.model,
        "openrouter/google/gemini-3.8-flash"
    );
}

#[test]
fn muse_contributor_variants_share_model_identity_while_retaining_route_terms_and_execution_ids() {
    let catalog = catalog();
    for name in ["eos", "logi", "gefjon"] {
        let p = found(&catalog, name);
        assert_eq!(p.entry.profile.model_key, "muse-spark-1.3");
        assert_eq!(p.entry.profile.model_label, "Muse Spark 1.3");
        if name == "gefjon" {
            assert!(p.entry.model.contains("contributor"));
            assert!(p.entry.profile.route_label.contains("Contributor"));
            assert_eq!(
                p.entry.profile.route_note.as_deref(),
                Some("Prompts and replies may train Meta models.")
            );
        } else {
            assert_eq!(p.entry.profile.route_note, None);
        }
    }
    assert_eq!(
        found(&catalog, "gefjon").entry.profile.route_label,
        "OpenCode Zen · Contributor · Free"
    );
}

#[test]
fn offers_devin_s_swe_1_6_by_name_plain_and_slow_and_no_agent_on_devin_s_own_setting() {
    // 2026-10-02, the owner: no agent runs on a harness's own default model.
    let catalog = catalog();
    assert!(listed(&catalog, Harness::Devin)
        .iter()
        .all(|entry| entry.model != "default"));
    for (name, model, label) in [
        ("hapi", "swe-1-6", "SWE-1.6"),
        ("khnum", "swe-1-6-slow", "SWE-1.6 Slow"),
    ] {
        let entry = found(&catalog, name);
        assert_eq!(entry.harness, Harness::Devin, "{name}");
        assert_eq!(entry.entry.model, model, "{name}");
        // Devin lists SWE-1.6 with no levels: the row names none.
        assert_eq!(entry.entry.effort, None, "{name}");
        assert_eq!(entry.entry.profile.model_label, label, "{name}");
        assert_eq!(entry.entry.profile.route_label, "Devin account", "{name}");
    }
    // No model at all is a chief from before every chief had an agent: Devin's own setting.
    let own = catalog.profile(&Settings {
        harness: Some("devin"),
        ..Settings::default()
    });
    assert_eq!(own.model_label, "Devin configured model");
    // The levels Devin writes into its model ids (claude-opus-5-5-max), for
    // an agent that names a family that has them.
    assert_eq!(
        efforts(Harness::Devin),
        ["low", "medium", "high", "xhigh", "max"]
    );
}

#[test]
fn assigns_four_work_tiers_by_model_and_effort_across_routes_without_agent_name_rules() {
    let catalog = catalog();
    for (name, tier) in [
        ("astraeus", "critical"),
        ("calliope", "critical"),
        ("phosphoros", "critical"),
        ("zeus", "critical"),
        ("asteria", "complex"),
        ("celaeno", "complex"),
        ("taygete", "complex"),
        ("vidar", "complex"),
        ("clio", "complex"),
        ("endymion", "complex"),
        ("apollo", "complex"),
        ("kronos", "complex"),
        // Sol and Sonnet at max are complex work (Gabriel, 2026-09-30), and so
        // is Opus 5.5 at medium and high, beside Sonnet at max (2026-10-03).
        ("hyperion", "complex"),
        ("hermod", "complex"),
        ("artemis", "complex"),
        ("poseidon", "complex"),
        ("thalia", "standard"),
        ("maia", "standard"),
        ("phoebus", "standard"),
        ("theia", "standard"),
        ("diana", "light"),
        ("electra", "light"),
        ("pygmalion", "light"),
        ("hemera", "light"),
        ("phaethon", "light"),
        ("asterope", "light"),
        ("huginn", "light"),
    ] {
        let entry = found(&catalog, name);
        assert_eq!(entry.entry.profile.work_tier.as_str(), tier, "{name}");
        // The agent under another name: a tier is read from the model and the effort alone.
        assert_eq!(
            catalog.profile(&settings_of(&entry)).work_tier.as_str(),
            tier,
            "{name}"
        );
    }
    for (harness, entry) in entries(&catalog) {
        let twin = entries(&catalog)
            .map(|(_, p)| p)
            .find(|p| p.profile.model_key == entry.profile.model_key && p.effort == entry.effort)
            .unwrap();
        assert_eq!(
            entry.profile.work_tier,
            twin.profile.work_tier,
            "{}{}",
            harness.as_str(),
            entry.name
        );
    }
    // Sonnet 5.5 below max, as an agent of your own on Claude Code: xhigh, high
    // and medium are standard work, low is light.
    for (effort, tier) in [
        ("xhigh", "standard"),
        ("high", "standard"),
        ("medium", "standard"),
        ("low", "light"),
    ] {
        let sonnet = catalog.profile(&Settings {
            harness: Some("claude"),
            model: Some("claude-sonnet-5-5"),
            effort: Some(effort),
            ..Settings::default()
        });
        assert_eq!(sonnet.work_tier.as_str(), tier, "Sonnet 5.5 {effort}");
    }
    // Opus 5.5, as an agent of your own on Claude Code: max is critical work,
    // low light, and every level between them complex.
    for (effort, tier) in [
        ("max", "critical"),
        ("xhigh", "complex"),
        ("high", "complex"),
        ("medium", "complex"),
        ("low", "light"),
    ] {
        let opus = catalog.profile(&Settings {
            harness: Some("claude"),
            model: Some("claude-opus-5-5"),
            effort: Some(effort),
            ..Settings::default()
        });
        assert_eq!(opus.work_tier.as_str(), tier, "Opus 5.5 {effort}");
    }
    // Codex's ultra sits above max: Sol there is complex work too.
    assert_eq!(
        catalog
            .profile(&Settings {
                harness: Some("codex"),
                model: Some("gpt-6.1-sol"),
                effort: Some("ultra"),
                ..Settings::default()
            })
            .work_tier,
        WorkTier::Complex
    );
    let custom = catalog.profile(&Settings {
        harness: Some("codex"),
        model: Some("custom"),
        work_tier: Some(WorkTier::Critical),
        ..Settings::default()
    });
    assert_eq!(custom.work_tier, WorkTier::Critical);
}

#[test]
fn carries_gpt_6_1_sol_wherever_sol_was_sonnet_5_5_and_mimo_v2_6_pro_through_openrouter_on_pi_and_opencode(
) {
    // 2026-09-30: every Sol preset moved to 6.1 (probed on Codex 0.159.2, Pi 0.99.1,
    // OpenCode 1.18.33), Sonnet to 5.5, and MiMo V2.6 Pro joined.
    let catalog = catalog();
    // `/gpt-[\d.]+-sol$/`: "gpt-", digits and dots, and "-sol" to the end.
    let is_sol = |model: &str| {
        model
            .strip_suffix("-sol")
            .and_then(|stem| stem.rfind("gpt-").map(|at| &stem[at + "gpt-".len()..]))
            .is_some_and(|version| {
                !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.')
            })
    };
    let sol: Vec<&CatalogEntry> = entries(&catalog)
        .map(|(_, entry)| entry)
        .filter(|entry| is_sol(&entry.model))
        .collect();
    assert_eq!(sol.len(), 13);
    for entry in sol {
        assert!(entry.model.ends_with("gpt-6.1-sol"), "{}", entry.name);
        assert_eq!(entry.profile.model_label, "GPT-6.1 Sol", "{}", entry.name);
        assert!(
            entry.description.contains(" GPT 6.1 Sol "),
            "{}",
            entry.name
        );
    }
    // Sonnet 5.5 at max, xhigh and medium, each probed on Claude Code 2.1.286.
    for (name, effort, label, tier) in [
        ("hermod", "max", "MAX", "complex"),
        ("forseti", "xhigh", "XHIGH", "standard"),
        ("ullr", "medium", "MEDIUM", "standard"),
    ] {
        let entry = found(&catalog, name);
        assert_eq!(
            (
                entry.harness,
                entry.entry.model.as_str(),
                entry.entry.effort.as_deref(),
                entry.entry.description.as_str(),
                entry.entry.profile.model_label.as_str(),
            ),
            (
                Harness::Claude,
                "claude-sonnet-5-5",
                Some(effort),
                format!("Claude Code Sonnet 5.5 {label}").as_str(),
                "Claude Sonnet 5.5",
            ),
            "{name}"
        );
        assert_eq!(entry.entry.profile.work_tier.as_str(), tier, "{name}");
    }
    for (name, harness) in [("selene", Harness::Pi), ("idun", Harness::Opencode)] {
        let entry = found(&catalog, name);
        assert_eq!(entry.harness, harness);
        assert_eq!(entry.entry.model, "openrouter/xiaomi/mimo-v2.6-pro");
        // MiMo takes reasoning on or off, no level: its presets name none.
        assert_eq!(entry.entry.effort, None, "{name}");
        assert_eq!(entry.entry.profile.model_label, "MiMo V2.6 Pro");
        assert_eq!(entry.entry.profile.route_label, "OpenRouter · API");
    }
}

#[test]
fn offers_devin_s_flagship_models_as_presets_each_on_its_twin_s_tier() {
    // 2026-10-01: Devin lists 54 model families; the catalog carries the ladders
    // Claude Code and Codex carry (Fable 5.1, Opus 5.5, Sonnet 5.5, GPT-6 Astra,
    // GPT-6.1 Sol) and Devin's own SWE-2. Devin writes the level into the model
    // id: a row names family and effort, and the launch joins them. Every one
    // answered "Upgrade to Pro" on a free plan; Devin says so itself, so a row
    // carries no note about plans (the owner's call).
    let catalog = catalog();
    let devin: Vec<&CatalogEntry> = listed(&catalog, Harness::Devin)
        .iter()
        .filter(|entry| entry.effort.is_some())
        .collect();
    assert_eq!(devin.len(), 25);
    let twins: Vec<&CatalogEntry> = listed(&catalog, Harness::Claude)
        .iter()
        .chain(listed(&catalog, Harness::Codex))
        .collect();
    for entry in devin {
        let level = entry.effort.as_deref().unwrap();
        assert!(
            ["low", "medium", "high", "xhigh", "max"].contains(&level),
            "{}",
            entry.name
        );
        // `^Devin .+ LEVEL$`: the words around at least one character.
        let middle = entry
            .description
            .strip_prefix("Devin ")
            .and_then(|rest| rest.strip_suffix(format!(" {}", level.to_uppercase()).as_str()));
        assert!(
            middle.is_some_and(|middle| {
                !middle.is_empty() && !middle.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
            }),
            "{}",
            entry.name
        );
        assert_eq!(entry.profile.route_label, "Devin account", "{}", entry.name);
        assert_eq!(entry.profile.route_note, None, "{}", entry.name);
        if entry.model == "swe-2" {
            assert_eq!(entry.profile.model_label, "SWE-2", "{}", entry.name);
            continue;
        }
        let twin = twins
            .iter()
            .find(|t| {
                t.profile.model_key == entry.profile.model_key && t.effort.as_deref() == Some(level)
            })
            .unwrap_or_else(|| {
                panic!(
                    "{}: {} has a Claude Code or Codex twin",
                    entry.name, entry.model
                )
            });
        assert_eq!(
            entry.profile.model_label, twin.profile.model_label,
            "{}",
            entry.name
        );
        assert_eq!(
            entry.profile.work_tier, twin.profile.work_tier,
            "{} and {}",
            entry.name, twin.name
        );
    }
    let osiris = found(&catalog, "osiris");
    assert_eq!(
        (osiris.entry.model.as_str(), osiris.entry.profile.work_tier),
        ("claude-opus-5-5", WorkTier::Critical)
    );
    let ra = found(&catalog, "ra");
    assert_eq!(
        (ra.entry.model.as_str(), ra.entry.effort.as_deref()),
        ("gpt-6-1-sol", Some("max"))
    );
}
