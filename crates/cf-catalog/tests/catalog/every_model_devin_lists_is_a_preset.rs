//! Every model Devin lists is a preset (Gabriel, 2026-10-24 and 2026-10-25).
//!
//! `devin-models.json` beside this file is what Devin 3000.11.205 listed on
//! 2026-10-25 (`devin models list --format json`, its ids and family labels;
//! the `/model` dialog showed the same 44 families). Each family is a preset at
//! each of its levels from low to max, and a model with no level is one by its
//! plain id. Left out, as Gabriel decided: the 575 Fusion pairs (a count here,
//! not ids), the none and minimal levels, and the dialog's toggles (fast mode,
//! priority, lightning, a 1M context, thinking), which a preset cannot name: the
//! launch joins a family and its effort, nothing more.

use super::*;
use serde_json::Value;

/// The levels a Devin preset names, the hardest first.
const LEVELS: [&str; 5] = ["max", "xhigh", "high", "medium", "low"];

/// The words of an id the catalog leaves out: a level below low, or a toggle.
const LEFT_OUT: [&str; 7] = [
    "none",
    "minimal",
    "fast",
    "priority",
    "lightning",
    "1m",
    "thinking",
];

/// Devin's families as probed: each label and its ids.
fn devin_lists() -> Vec<(String, Vec<String>)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("catalog")
        .join("devin-models.json");
    let listed: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    listed["families"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|family| family["slug"] != "fusion")
        .map(|family| {
            let ids = family["ids"].as_array().unwrap();
            let ids = ids.iter().map(|id| id.as_str().unwrap().to_owned());
            (family["label"].as_str().unwrap().to_owned(), ids.collect())
        })
        .collect()
}

/// Whether the catalog leaves an id out: a Fusion pair, a level below low, or a toggle.
fn left_out(id: &str) -> bool {
    id.starts_with("fusion-") || id.split('-').any(|part| LEFT_OUT.contains(&part))
}

/// What a Devin preset runs, as Devin names it: the family and its effort, joined.
fn devin_id(entry: &CatalogEntry) -> String {
    entry.effort.as_deref().map_or_else(
        || entry.model.clone(),
        |effort| format!("{}-{effort}", entry.model),
    )
}

#[test]
fn every_family_devin_lists_is_a_preset_at_each_of_its_levels_from_low_to_max_or_by_its_plain_id() {
    let catalog = catalog();
    let devin = listed(&catalog, Harness::Devin);
    let offered: HashSet<String> = devin.iter().map(devin_id).collect();
    let mut listed_ids = HashSet::new();
    for (label, ids) in devin_lists() {
        for id in ids {
            listed_ids.insert(id.clone());
            let taken = !left_out(&id);
            assert_eq!(
                offered.contains(&id),
                taken,
                "{label}: {id} is {}a preset",
                if taken { "" } else { "not " }
            );
        }
    }
    // And nothing on Devin that Devin does not list: SWE-1.6 Slow went (Khnum).
    for entry in devin {
        assert!(
            listed_ids.contains(&devin_id(entry)),
            "{}: Devin lists no {}",
            entry.name,
            devin_id(entry)
        );
    }
    assert_eq!(devin.len(), 151);
}

#[test]
fn each_devin_preset_says_its_family_and_level_and_its_work_in_words_of_its_tier() {
    let catalog = catalog();
    for entry in listed(&catalog, Harness::Devin) {
        let level = entry.effort.as_deref().map(str::to_uppercase);
        let middle = entry.description.strip_prefix("Devin ").map(|rest| {
            level
                .as_deref()
                .and_then(|level| rest.strip_suffix(&format!(" {level}")))
                .unwrap_or(rest)
        });
        assert!(
            middle.is_some_and(|middle| !middle.is_empty()),
            "{}: {}",
            entry.name,
            entry.description
        );
        assert!(is_agent_name(&entry.name), "{}", entry.name);
        assert_eq!(entry.profile.route_label, "Devin account", "{}", entry.name);
    }
}

#[test]
fn devin_s_models_take_the_tiers_gabriel_decided() {
    // max, xhigh, high, medium, low; a model not named here is light work at
    // every level (SWE-2 among them, as before), and so is one with no level.
    let flagship = ["critical", "complex", "complex", "standard", "light"];
    let sonnet = ["complex", "standard", "standard", "standard", "light"];
    let sol = ["complex", "standard", "standard", "light", "light"];
    let table: HashMap<&str, [&str; 5]> = [
        // From the table the catalog had.
        ("claude-fable-5-1", flagship),
        ("gpt-6-astra", flagship),
        // Opus 5.5 at medium is standard work now (2026-10-24).
        ("claude-opus-5-5", flagship),
        ("claude-sonnet-5-5", sonnet),
        ("gpt-6-1-sol", sol),
        ("kimi-k3", ["complex"; 5]),
        // Proposed by the chief, decided by Gabriel (2026-10-25).
        ("claude-5-fable", sonnet),
        ("claude-opus-5", sonnet),
        ("claude-opus-4-8", sol),
        ("claude-sonnet-5", sol),
        ("muse-wonder-1-0", sol),
        (
            "gpt-6-sol",
            ["standard", "light", "light", "light", "light"],
        ),
        (
            "gemini-4-argon",
            ["light", "light", "complex", "standard", "light"],
        ),
    ]
    .into_iter()
    .collect();
    for entry in listed(&catalog(), Harness::Devin) {
        let expected = entry.effort.as_deref().map_or("light", |effort| {
            let at = LEVELS.iter().position(|level| *level == effort).unwrap();
            table
                .get(entry.model.as_str())
                .map_or("light", |tiers| tiers[at])
        });
        assert_eq!(
            entry.profile.work_tier.as_str(),
            expected,
            "{} ({})",
            entry.name,
            devin_id(entry)
        );
    }
}

#[test]
fn kimi_k3_1_pro_is_complex_at_max_standard_at_high_and_light_at_low_where_kimi_k3_stays_complex() {
    // A Kimi K3.1 Pro of the human's own on Devin, which lists none, is light work off the catalog.
    assert_eq!(
        catalog()
            .profile(&Settings {
                harness: Some("devin"),
                model: Some("kimi-k3.1-pro"),
                effort: Some("max"),
                ..Settings::default()
            })
            .work_tier,
        WorkTier::Light
    );
    // Devin lists no K3.1 Pro (3000.11.205); Pi reaches it through OpenRouter,
    // at low, high and max (2026-10-25). OpenCode lists it too, and gains no
    // preset: it leaves ConsensFlow (Gabriel, 2026-10-25).
    let catalog = catalog();
    for (name, harness, level, tier) in [
        ("themis", Harness::Pi, "max", WorkTier::Complex),
        ("crius", Harness::Pi, "high", WorkTier::Standard),
        ("coeus", Harness::Pi, "low", WorkTier::Light),
    ] {
        let entry = found(&catalog, name);
        assert_eq!(entry.harness, harness, "{name}");
        assert_eq!(
            entry.entry.model, "openrouter/moonshotai/kimi-k3.1-pro",
            "{name}"
        );
        assert_eq!(entry.entry.effort.as_deref(), Some(level), "{name}");
        assert_eq!(entry.entry.profile.model_label, "Kimi K3.1 Pro", "{name}");
        assert_eq!(entry.entry.profile.work_tier, tier, "{name}");
    }
    // Kimi K3 is complex work whatever its level, on every harness that carries it.
    for name in ["endymion", "mani", "iabet", "imentet", "imsety"] {
        let entry = found(&catalog, name);
        assert_eq!(entry.entry.profile.model_key, "kimi-k3", "{name}");
        assert_eq!(entry.entry.profile.work_tier, WorkTier::Complex, "{name}");
    }
    for harness in [Harness::Devin, Harness::Opencode] {
        assert!(listed(&catalog, harness)
            .iter()
            .all(|entry| !entry.model.contains("k3.1") && !entry.model.contains("k3-1")));
    }
}

#[test]
fn gemini_4_argon_is_complex_at_high_standard_at_medium_and_light_at_low_on_devin_and_pi() {
    let catalog = catalog();
    for (names, harness, model) in [
        (["heka", "heqet", "hesat"], Harness::Devin, "gemini-4-argon"),
        (
            ["hecate", "dione", "tyche"],
            Harness::Pi,
            "openrouter/google/gemini-4-argon",
        ),
    ] {
        for (name, (level, tier)) in names.into_iter().zip([
            ("high", WorkTier::Complex),
            ("medium", WorkTier::Standard),
            ("low", WorkTier::Light),
        ]) {
            let entry = found(&catalog, name);
            assert_eq!(entry.harness, harness, "{name}");
            assert_eq!(entry.entry.model, model, "{name}");
            assert_eq!(entry.entry.effort.as_deref(), Some(level), "{name}");
            assert_eq!(entry.entry.profile.model_key, "gemini-4-argon", "{name}");
            assert_eq!(entry.entry.profile.model_label, "Gemini 4 Argon", "{name}");
            assert_eq!(entry.entry.profile.work_tier, tier, "{name}");
        }
    }
}
