use super::*;
use std::collections::BTreeMap;

#[test]
fn devin_s_own_setting_comes_before_the_designer_and_the_key() {
    let catalog = catalog();
    let configured = |model| {
        catalog.profile(&Settings {
            harness: Some("devin"),
            model,
            designer: true,
            ..Settings::default()
        })
    };
    for model in [None, Some("")] {
        let profile = configured(model);
        assert_eq!(profile.model_key, "devin-configured");
        assert_eq!(profile.model_label, "Devin configured model");
        assert_eq!(profile.route_label, "Devin account");
    }
    // Off the catalog, a Devin agent is called by whatever id it names.
    let named = configured(Some("some-model"));
    assert_eq!(
        (named.model_key.as_str(), named.model_label.as_str()),
        ("some-model", "some-model")
    );
    assert_eq!(named.route_label, "Devin account");
    // On the catalog, the designer is first.
    let image = configured(Some("claude-opus-5-5"));
    assert_eq!(image.model_key, "codex-image");
    assert_eq!(image.route_label, "Codex login");
}

#[test]
fn an_image_agent_is_codex_images_whatever_model_it_names() {
    let catalog = catalog();
    for settings in [
        Settings {
            kind: Some("codex"),
            model: Some("gpt-6-astra"),
            effort: Some("max"),
            ..Settings::default()
        },
        Settings {
            kind: Some("image"),
            model: Some("anything"),
            ..Settings::default()
        },
        Settings::default(),
    ] {
        let profile = catalog.profile(&Settings {
            designer: true,
            ..settings
        });
        assert_eq!(
            (
                profile.model_key.as_str(),
                profile.model_label.as_str(),
                profile.route_label.as_str(),
                profile.route_note,
                profile.work_tier
            ),
            (
                "codex-image",
                "Codex Images",
                "Codex login",
                None,
                WorkTier::Light
            )
        );
    }
}

#[test]
fn the_tier_reads_the_key_after_the_designer_override() {
    let catalog = catalog();
    let astra = Settings {
        kind: Some("codex"),
        model: Some("gpt-6-astra"),
        effort: Some("max"),
        ..Settings::default()
    };
    assert_eq!(catalog.profile(&astra).work_tier, WorkTier::Critical);
    let image = catalog.profile(&Settings {
        designer: true,
        ..astra
    });
    assert_eq!(image.work_tier, WorkTier::Light);
}

#[test]
fn a_provider_path_is_stripped_only_for_a_known_pair() {
    let catalog = catalog();
    let on_pi = |model| {
        catalog.profile(&Settings {
            kind: Some("pi"),
            model: Some(model),
            effort: Some("high"),
            ..Settings::default()
        })
    };
    let curated = on_pi("openrouter/anthropic/claude-fable-5.1");
    assert_eq!(curated.model_key, "claude-fable-5.1");
    assert_eq!(curated.model_label, "Claude Fable 5.1");
    assert_eq!(curated.route_label, "OpenRouter · API");
    // Off the catalog, the whole id is the key and the label.
    let custom = on_pi("openrouter/anthropic/claude-x");
    assert_eq!(custom.model_key, "openrouter/anthropic/claude-x");
    assert_eq!(custom.model_label, "openrouter/anthropic/claude-x");
    assert_eq!(custom.route_label, "OpenRouter · API");
    // A curated id on a harness that has no preset for it is off the catalog too.
    let elsewhere = catalog.profile(&Settings {
        kind: Some("codex"),
        model: Some("openrouter/anthropic/claude-fable-5.1"),
        ..Settings::default()
    });
    assert_eq!(elsewhere.model_key, "openrouter/anthropic/claude-fable-5.1");
}

#[test]
fn a_kind_and_a_harness_are_two_spellings_the_known_pairs_tell_apart() {
    let catalog = catalog();
    // `claude` as a kind is the harness `claude`: Opus on it is a preset's.
    let by_kind = catalog.profile(&Settings {
        kind: Some("claude"),
        model: Some("claude-opus-5-5"),
        effort: Some("max"),
        ..Settings::default()
    });
    assert_eq!(by_kind.model_key, "claude-opus-5.5");
    assert_eq!(by_kind.work_tier, WorkTier::Critical);
    // `claude-code` as a harness is no harness: nothing is known on it.
    let by_harness = catalog.profile(&Settings {
        harness: Some("claude-code"),
        model: Some("claude-opus-5-5"),
        effort: Some("max"),
        ..Settings::default()
    });
    assert_eq!(by_harness.model_key, "claude-opus-5-5");
    assert_eq!(by_harness.model_label, "claude-opus-5-5");
    assert_eq!(by_harness.route_label, "claude-code");
    assert_eq!(by_harness.work_tier, WorkTier::Light);
    // As a kind it is `claude`, and the harness, when there is one, wins over the kind.
    let by_claude_code = catalog.profile(&Settings {
        kind: Some("claude-code"),
        harness: Some("codex"),
        model: Some("claude-opus-5-5"),
        ..Settings::default()
    });
    assert_eq!(by_claude_code.route_label, "Codex login");
    assert_eq!(by_claude_code.model_key, "claude-opus-5-5");
}

#[test]
fn kimi_is_complex_before_any_effort_rule_is_read() {
    let catalog = catalog();
    for effort in [None, Some(""), Some("low"), Some("bogus"), Some("max")] {
        let profile = catalog.profile(&Settings {
            kind: Some("opencode"),
            model: Some("openrouter/moonshotai/kimi-k3"),
            effort,
            ..Settings::default()
        });
        assert_eq!(profile.model_key, "kimi-k3");
        assert_eq!(profile.work_tier, WorkTier::Complex, "{effort:?}");
    }
    // Off the catalog the same name earns nothing.
    let unknown = catalog.profile(&Settings {
        kind: Some("opencode"),
        model: Some("openrouter/moonshotai/kimi-k3-preview/kimi-k3"),
        effort: Some("max"),
        ..Settings::default()
    });
    assert_eq!(unknown.work_tier, WorkTier::Light);
}

#[test]
fn a_tier_the_human_chose_comes_last_of_all() {
    let catalog = catalog();
    let astra = Settings {
        kind: Some("codex"),
        model: Some("gpt-6-astra"),
        effort: Some("max"),
        ..Settings::default()
    };
    for tier in WORK_TIERS {
        let chosen = catalog.profile(&Settings {
            work_tier: Some(tier),
            ..astra
        });
        assert_eq!(chosen.work_tier, tier);
        let image = catalog.profile(&Settings {
            work_tier: Some(tier),
            designer: true,
            ..astra
        });
        assert_eq!(image.work_tier, tier);
    }
    let custom = catalog.profile(&Settings {
        kind: Some("codex"),
        model: Some("custom"),
        work_tier: Some(WorkTier::Critical),
        ..Settings::default()
    });
    assert_eq!(custom.work_tier, WorkTier::Critical);
    let devin = catalog.profile(&Settings {
        harness: Some("devin"),
        work_tier: Some(WorkTier::Standard),
        ..Settings::default()
    });
    assert_eq!(devin.work_tier, WorkTier::Standard);
}

#[test]
fn pi_reads_its_thinking_before_its_effort_and_an_empty_thinking_is_still_a_reading() {
    let catalog = catalog();
    let sol = |thinking, effort| {
        catalog
            .profile(&Settings {
                kind: Some("pi"),
                model: Some("openai-codex/gpt-6.1-sol"),
                thinking,
                effort,
                ..Settings::default()
            })
            .work_tier
    };
    assert_eq!(sol(Some("max"), Some("low")), WorkTier::Complex);
    assert_eq!(sol(None, Some("max")), WorkTier::Complex);
    assert_eq!(sol(Some("low"), Some("max")), WorkTier::Light);
    // `thinking ?? effort`: an empty thinking is not none, so it hides the effort.
    assert_eq!(sol(Some(""), Some("max")), WorkTier::Light);
    // Other harnesses read the effort alone.
    let on_codex = catalog.profile(&Settings {
        kind: Some("codex"),
        model: Some("gpt-6.1-sol"),
        thinking: Some("low"),
        effort: Some("max"),
        ..Settings::default()
    });
    assert_eq!(on_codex.work_tier, WorkTier::Complex);
}

#[test]
fn an_agent_with_no_kind_and_no_harness_has_the_route_undefined() {
    let catalog = catalog();
    let nameless = catalog.profile(&Settings {
        model: Some("acme-x"),
        effort: Some("high"),
        ..Settings::default()
    });
    assert_eq!(nameless.route_label, "undefined");
    assert_eq!(nameless.model_key, "acme-x");
    // An empty word is a word: its route is empty, not `undefined`.
    for settings in [
        Settings {
            kind: Some(""),
            ..Settings::default()
        },
        Settings {
            harness: Some(""),
            ..Settings::default()
        },
    ] {
        assert_eq!(catalog.profile(&settings).route_label, "");
    }
    // Another harness's route is its own word.
    let image = catalog.profile(&Settings {
        kind: Some("image"),
        model: Some("gpt-image-2"),
        ..Settings::default()
    });
    assert_eq!(image.route_label, "image");
}

#[test]
fn an_empty_model_keeps_its_empty_key_and_is_labelled_the_default() {
    let catalog = catalog();
    let empty = catalog.profile(&Settings {
        kind: Some("codex"),
        model: Some(""),
        ..Settings::default()
    });
    assert_eq!(
        (empty.model_key.as_str(), empty.model_label.as_str()),
        ("", "Default")
    );
    let none = catalog.profile(&Settings {
        kind: Some("codex"),
        ..Settings::default()
    });
    assert_eq!(
        (none.model_key.as_str(), none.model_label.as_str()),
        ("default", "Default")
    );
    assert_eq!(none.route_label, "Codex login");
}

#[test]
fn the_prefix_of_a_model_names_its_road_before_its_harness_does() {
    let catalog = catalog();
    for (model, route) in [
        ("openrouter/x/y", "OpenRouter · API"),
        ("opencode/y", "OpenCode Zen"),
        ("openai-codex/y", "Codex subscription"),
        ("anthropic/y", "Anthropic · API"),
    ] {
        let profile = catalog.profile(&Settings {
            kind: Some("claude-code"),
            model: Some(model),
            ..Settings::default()
        });
        assert_eq!(profile.route_label, route, "{model}");
    }
    for (kind, route) in [
        ("claude-code", "Claude Code account"),
        ("codex", "Codex login"),
        ("devin", "Devin account"),
        ("pi", "pi"),
        ("opencode", "opencode"),
    ] {
        let profile = catalog.profile(&Settings {
            kind: Some(kind),
            model: Some("model-x"),
            ..Settings::default()
        });
        assert_eq!(profile.route_label, route, "{kind}");
    }
}

#[test]
fn muse_s_contributor_routes_say_so_with_a_note_and_the_free_one_says_free() {
    let presets = vec![
        preset(
            "free",
            "opencode",
            "opencode/muse-spark-1.3-contributor-free",
            Some("high"),
        ),
        preset(
            "paid",
            "opencode",
            "opencode/muse-spark-1.3-contributor",
            Some("high"),
        ),
        preset(
            "plain",
            "pi",
            "openrouter/meta/muse-spark-1.3",
            Some("high"),
        ),
    ];
    let catalog = Catalog::new(
        presets,
        BTreeMap::from([("muse-spark-1.3".to_owned(), "Muse Spark 1.3".to_owned())]),
    );
    let on = |model, kind| {
        catalog.profile(&Settings {
            kind: Some(kind),
            model: Some(model),
            effort: Some("high"),
            ..Settings::default()
        })
    };
    let free = on("opencode/muse-spark-1.3-contributor-free", "opencode");
    assert_eq!(free.route_label, "OpenCode Zen · Contributor · Free");
    assert_eq!(
        free.route_note.as_deref(),
        Some("Prompts and replies may train Meta models.")
    );
    assert_eq!(
        (free.model_key.as_str(), free.model_label.as_str()),
        ("muse-spark-1.3", "Muse Spark 1.3")
    );
    let paid = on("opencode/muse-spark-1.3-contributor", "opencode");
    assert_eq!(paid.route_label, "OpenCode Zen · Contributor");
    assert_eq!(
        paid.route_note.as_deref(),
        Some("Prompts and replies may train Meta models.")
    );
    let plain = on("openrouter/meta/muse-spark-1.3", "pi");
    assert_eq!(plain.route_label, "OpenRouter · API");
    assert_eq!(plain.route_note, None);
    // Not a preset's pair: the model is called by its id and the route has no note.
    let unlisted = on("opencode/muse-spark-1.3-contributor-free", "pi");
    assert_eq!(
        unlisted.model_key,
        "opencode/muse-spark-1.3-contributor-free"
    );
    assert_eq!(unlisted.route_note, None);
}

#[test]
fn a_label_that_is_empty_is_no_label() {
    let presets = vec![preset("blank", "codex", "gpt-x", Some("high"))];
    let labels = BTreeMap::from([("gpt-x".to_owned(), String::new())]);
    let catalog = Catalog::new(presets, labels);
    let profile = catalog.profile(&Settings {
        kind: Some("codex"),
        model: Some("gpt-x"),
        ..Settings::default()
    });
    assert_eq!(profile.model_label, "gpt-x");
}
