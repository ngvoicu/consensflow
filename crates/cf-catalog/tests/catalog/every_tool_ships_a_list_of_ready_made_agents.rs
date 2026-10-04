//! The tests under `describe('every tool ships a list of ready-made agents')`
//! in `tests/catalog.test.mjs`.

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
    let mut harnesses: Vec<&str> = Harness::ALL
        .iter()
        .map(|harness| harness.as_str())
        .collect();
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
        assert!(Harness::ALL.contains(&harness));
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
