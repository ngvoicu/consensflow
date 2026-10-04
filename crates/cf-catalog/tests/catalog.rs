//! The 23 tests of `tests/catalog.test.mjs`, ported with the catalog: each
//! keeps its sentence, as a name, and its assertions, against the catalog
//! built into the crate.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};

use cf_catalog::{
    efforts, Catalog, CatalogEntry, FoundEntry, Harness, Preset, Settings, WorkTier, HARNESSES,
};

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

mod every_tool_ships_a_list_of_ready_made_agents {
    use super::*;

    #[test]
    fn covers_every_harness_each_with_a_real_list_the_image_agent_among_codex_s() {
        let catalog = catalog();
        let mut offered: Vec<&str> = catalog
            .groups()
            .iter()
            .map(|group| group.harness.as_str())
            .collect();
        offered.sort_unstable();
        let mut harnesses: Vec<&str> = HARNESSES.iter().map(|harness| harness.as_str()).collect();
        harnesses.sort_unstable();
        assert_eq!(offered, harnesses);
        for group in catalog.groups() {
            let least = if group.harness == Harness::Devin {
                1
            } else {
                3
            };
            assert!(
                group.entries.len() >= least,
                "{} needs a real list",
                group.harness.as_str()
            );
        }
        let designers: Vec<(Harness, &str)> = entries(&catalog)
            .filter(|(_, entry)| entry.designer)
            .map(|(harness, entry)| (harness, entry.name.as_str()))
            .collect();
        assert_eq!(designers, [(Harness::Codex, "pygmalion")]);
    }

    #[test]
    fn gives_every_entry_a_name_a_model_and_a_description() {
        let catalog = catalog();
        for (_, entry) in entries(&catalog) {
            assert!(is_agent_name(&entry.name), "{}", entry.name);
            assert!(!entry.model.is_empty());
            assert!(!entry.description.is_empty());
        }
    }

    #[test]
    fn never_repeats_a_name_across_the_whole_catalog() {
        let catalog = catalog();
        let names: Vec<&str> = entries(&catalog)
            .map(|(_, entry)| entry.name.as_str())
            .collect();
        let distinct: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(distinct.len(), names.len());
    }

    #[test]
    fn uses_only_efforts_its_harness_actually_accepts() {
        let catalog = catalog();
        for (harness, entry) in entries(&catalog) {
            let Some(effort) = entry.effort.as_deref() else {
                continue;
            };
            assert!(
                efforts(harness).contains(&effort),
                "{}: {} has no effort {}",
                entry.name,
                harness.as_str(),
                effort
            );
        }
    }

    // An entry with no effort draws a bare harness tag in the roster UI, so a
    // level nobody chose looks exactly like a level the catalog forgot. Where the
    // harness HAS levels, every entry names one — unless it is listed here, and
    // the file says why beside it.
    #[test]
    fn names_an_effort_wherever_its_harness_has_one_or_is_a_listed_exception() {
        // The three models that take no effort parameter at all — see "Effort ceilings"
        // in hosts/lib/presets.js for how that was established.
        let blank_on_purpose: HashSet<&str> = [
            "metis",  // MiniMax M3, on pi
            "mimir",  // MiniMax M3, on opencode
            "triton", // Laguna S 2.1 free, on pi
            "aegir",  // Laguna S 2.1 free, on opencode
            // Added 2026-09-06 with the OpenCode Zen road — same rule, new route.
            // Zen's Nemotron 3 Ultra entry publishes no reasoning options at all,
            // where OpenRouter's does: that is why ymir names `high` and audhumla,
            // the same model on the other road, names nothing.
            "audhumla", // Nemotron 3 Ultra free on OpenCode Zen
            // Devin's SWE-1.6 (2026-10-02): Devin lists it with no levels.
            "hapi",
            "khnum",
            // MiMo V2.6 Pro (2026-09-30): reasoning on or off, no level in any catalog.
            "selene", // on pi
            "idun",   // on opencode
            // Codex Images, among Codex's agents since 2026-10-03: an image agent's
            // window is Codex on its own model, with no effort of its own.
            "pygmalion",
        ]
        .into_iter()
        .collect();
        let catalog = catalog();
        for (harness, entry) in entries(&catalog) {
            if efforts(harness).is_empty() {
                continue;
            }
            assert!(
                entry.effort.is_some() || blank_on_purpose.contains(entry.name.as_str()),
                "{}: {} has effort levels, so this one must name one",
                entry.name,
                harness.as_str()
            );
        }
    }

    // The zoo is the same models through OpenCode: a pair that shares a model
    // must share the level too, or one name means two different agents.
    #[test]
    fn keeps_each_opencode_twin_at_its_pi_twin_s_effort() {
        let catalog = catalog();
        for entry in listed(&catalog, Harness::Opencode) {
            let twins: Vec<&CatalogEntry> = listed(&catalog, Harness::Pi)
                .iter()
                .filter(|p| p.model == entry.model)
                .collect();
            let twin = twins
                .iter()
                .find(|p| p.effort == entry.effort)
                .or(twins.first());
            let Some(twin) = twin else { continue };
            if entry.effort.is_none() {
                continue;
            }
            assert_eq!(
                entry.effort, twin.effort,
                "{} and {} share {} but not the effort",
                entry.name, twin.name, entry.model
            );
        }
    }

    #[test]
    fn carries_the_models_verified_live_on_2026_08_21_newest_of_each_family() {
        let catalog = catalog();
        let models: Vec<&str> = entries(&catalog)
            .map(|(_, entry)| entry.model.as_str())
            .collect();
        assert!(models.contains(&"openrouter/z-ai/glm-5.3"));
        assert!(models.contains(&"openrouter/qwen/qwen3.8-max"));
        assert!(models.contains(&"openrouter/moonshotai/kimi-k3"));
        // GPT 6 Astra, probed 2026-09-05: the id answers on codex where `gpt-6`,
        // `gpt-6-sol`, `gpt-6-pro` and `gpt-5.6-pro` are all refused, and its
        // ladder was walked level by level (minimal refused; low..ultra answer).
        assert!(models.contains(&"gpt-6-astra"));
        // Added 2026-08-24, each confirmed present in `pi --list-models` and
        // `opencode models` before it was written down — two free tiers and one
        // unbadged stealth model, on both open-model harnesses. The stealth one
        // ended its testing period on 2026-08-27 (404 naming its own model), so
        // nyx and nott moved to it under its real name: z-ai/glm-5.3-flash,
        // verified that day in OpenRouter's /api/v1/models and by a live one-shot
        // on each CLI — neither harness catalog lists it yet, both run it.
        // Added 2026-09-03, both probed live on each harness at the level the row
        // names. Muse Spark 1.3 needed two probes: the first answered 403 on BOTH
        // harnesses ("18+ age confirmation"), an account attestation no catalog
        // can show, and the rows were held back until a probe answered. See the
        // Gemini 3.8 / Muse Spark paragraph in presets.js.
        for model in [
            "openrouter/z-ai/glm-5.3-flash",
            "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free",
            "openrouter/poolside/laguna-s-2.1:free",
            "openrouter/google/gemini-3.8-flash",
            "openrouter/meta/muse-spark-1.3",
        ] {
            assert!(
                listed(&catalog, Harness::Pi)
                    .iter()
                    .any(|e| e.model == model),
                "pi is missing {model}"
            );
            assert!(
                listed(&catalog, Harness::Opencode)
                    .iter()
                    .any(|e| e.model == model),
                "opencode is missing {model}"
            );
        }
        // Superseded versions must not linger in a curated list — and a retired
        // endpoint is superseded twice over: stealth/ox-alpha answers 404 now.
        for gone in [
            "glm-5.2",
            "qwen3.7",
            "ox-alpha",
            "gemini-3.7",
            "muse-spark-1.2",
        ] {
            assert!(!models.iter().any(|m| m.contains(gone)), "{gone} lingers");
        }
    }

    // Added 2026-09-06. Two new roads to models this catalog already carried, and
    // one new road for GPT 6 Astra. Every id below was probed on the CLI that
    // runs it, at the level its row names, before it was written down.
    #[test]
    fn carries_opencode_zen_only_where_the_account_can_actually_reach_it() {
        // Zen lists 102 models on models.dev and offers 7 through the CLI, because
        // this account has no Zen credential. Only reachable ids belong here, and
        // both of these were probed. pi has no Zen provider at all, so these rows
        // are opencode-only by necessity, not by preference.
        let catalog = catalog();
        let mut zen: Vec<&str> = listed(&catalog, Harness::Opencode)
            .iter()
            .filter(|e| e.model.starts_with("opencode/"))
            .map(|e| e.model.as_str())
            .collect();
        zen.sort_unstable();
        assert_eq!(
            zen,
            [
                "opencode/muse-spark-1.3-contributor-free",
                "opencode/nemotron-3-ultra-free",
            ]
        );
        assert_eq!(
            listed(&catalog, Harness::Pi)
                .iter()
                .filter(|e| e.model.starts_with("opencode/"))
                .count(),
            0,
            "pi has no Zen provider — a Zen row there would never run"
        );
    }

    #[test]
    fn reaches_gpt_6_astra_on_all_three_engines_that_answer_for_it() {
        // codex through the ChatGPT login (astraeus/asteria/celaeno), pi through its own
        // copy of that login, opencode through OpenRouter — three roads, three
        // model strings, so no twin rule couples them. `ultra` stays codex-only:
        // neither of the new roads publishes it.
        let catalog = catalog();
        assert!(listed(&catalog, Harness::Codex)
            .iter()
            .any(|e| e.model == "gpt-6-astra"));
        assert!(listed(&catalog, Harness::Pi)
            .iter()
            .any(|e| e.model == "openai-codex/gpt-6-astra"));
        assert!(listed(&catalog, Harness::Opencode)
            .iter()
            .any(|e| e.model == "openrouter/openai/gpt-6-astra"));
        for harness in [Harness::Pi, Harness::Opencode] {
            let mut levels: Vec<Option<&str>> = listed(&catalog, harness)
                .iter()
                .filter(|e| e.model.contains("gpt-6-astra"))
                .map(|e| e.effort.as_deref())
                .collect();
            levels.sort_unstable();
            assert_eq!(
                levels,
                [
                    Some("high"),
                    Some("low"),
                    Some("max"),
                    Some("medium"),
                    Some("xhigh")
                ],
                "{}: five Astra levels, and no ultra",
                harness.as_str()
            );
        }
    }

    #[test]
    fn is_the_payload_presets_and_nothing_else_one_list_not_two() {
        // A second hand-written list is how `nike` came to mean GPT-5.6-luna in the
        // app and Gemini 3.7 Flash in the harness. The catalog is now derived, so
        // the two can no longer disagree.
        let catalog = catalog();
        let by_name: HashMap<&str, &Preset> = catalog
            .presets()
            .iter()
            .map(|preset| (preset.preset.as_str(), preset))
            .collect();

        for (harness, entry) in entries(&catalog) {
            let preset = by_name
                .get(entry.name.as_str())
                .unwrap_or_else(|| panic!("{} exists as a preset", entry.name));
            assert_eq!(
                entry.model, preset.model,
                "{}: model matches the harness",
                entry.name
            );
            assert_eq!(
                entry.effort.as_deref(),
                preset.effort.as_deref().or(preset.thinking.as_deref()),
                "{}: effort",
                entry.name
            );
            assert_eq!(
                entry.preset, entry.name,
                "{}: records its provenance",
                entry.name
            );
            assert!(HARNESSES.contains(&harness));
        }

        // Every preset is offered, the image agent among Codex's.
        let offered = entries(&catalog).count();
        assert_eq!(
            offered,
            catalog.presets().len(),
            "every preset is offered, image included"
        );
    }

    #[test]
    fn finds_an_entry_by_name_whatever_tool_it_belongs_to() {
        let catalog = catalog();
        let entry = found(&catalog, "zeus");
        assert_eq!(entry.harness, Harness::Claude);
        assert_eq!(entry.entry.model, "claude-opus-5-5");
        assert_eq!(catalog.entry("nobody"), None);
    }

    #[test]
    fn records_the_effort_levels_each_cli_accepts_as_its_own_help_states_them() {
        assert_eq!(
            efforts(Harness::Claude),
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert!(efforts(Harness::Codex).contains(&"ultra"));
        assert!(efforts(Harness::Pi).contains(&"off"));
    }

    #[test]
    fn names_no_ultra_preset_ultra_stays_a_level_the_cli_takes_not_a_row_the_catalog_ships() {
        // Sol stepped down from ultra to max by the user's decision (2026-09-06):
        // a deliberate seat below the proven ceiling. The effort-ceilings audit
        // must not "fix" it back.
        let catalog = catalog();
        let ultras: Vec<&CatalogEntry> = entries(&catalog)
            .map(|(_, entry)| entry)
            .filter(|entry| entry.effort.as_deref() == Some("ultra"))
            .collect();
        assert!(ultras.is_empty());
        let hyperion = found(&catalog, "hyperion");
        assert_eq!(hyperion.entry.effort.as_deref(), Some("max"));
        assert_eq!(hyperion.entry.description, "Codex GPT 6.1 Sol MAX");
    }
}

#[test]
fn ships_nothing_for_kimi_code_which_consensflow_does_not_run() {
    // Kimi is no harness of the build: it has no group, no efforts, and no preset of its kind.
    let catalog = catalog();
    assert!(HARNESSES.iter().all(|harness| harness.as_str() != "kimi"));
    assert!(catalog
        .groups()
        .iter()
        .all(|group| group.harness.as_str() != "kimi"));
    assert_eq!(cf_catalog::harness_for_kind("kimi"), None);
    assert!(catalog.presets().iter().all(|preset| preset.kind != "kimi"));
    assert_eq!(catalog.entry("ilmarinen"), None);
}

mod catalog_presentation_follows_actual_model_and_effort {
    use super::*;

    #[test]
    fn gives_every_curated_entry_an_explicit_model_route_and_practical_description() {
        let catalog = catalog();
        for (_, entry) in entries(&catalog) {
            assert!(!entry.profile.model_key.is_empty(), "{}", entry.name);
            assert!(!entry.profile.model_label.is_empty(), "{}", entry.name);
            assert!(!entry.profile.route_label.is_empty(), "{}", entry.name);
            let written = serde_json::to_value(&entry.profile).unwrap();
            assert!(written.get("categories").is_none(), "no role pills");
        }
    }

    #[test]
    fn unifies_reviewed_provider_aliases_while_keeping_model_snapshots_distinct() {
        let catalog = catalog();
        let key = |name: &str| found(&catalog, name).entry.profile.model_key;
        assert_eq!(key("astraeus"), "gpt-6-astra");
        assert_eq!(key("phosphoros"), "gpt-6-astra");
        assert_eq!(key("aurvandil"), "gpt-6-astra");
        assert_eq!(key("logi"), key("gefjon"));
        assert_ne!(key("freya"), key("hades"));
    }
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
    assert_eq!(entries(&catalog).count(), 119);
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
