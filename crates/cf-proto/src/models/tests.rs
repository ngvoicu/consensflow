use super::*;

fn listed(models: Vec<ModelChoice>) -> ModelListing {
    ModelListing::read(models, 10, "a test")
}

fn marked(id: &str) -> ModelChoice {
    ModelChoice {
        marked_default: true,
        ..ModelChoice::new(id, id)
    }
}

#[test]
fn a_contributor_model_is_known_by_its_id_and_warned_of() {
    let contributor = ModelChoice::new("muse-spark-1.3-contributor", "Spark");
    let plain = ModelChoice::new("muse-spark-1.3", "Spark");
    assert!(contributor.contributor);
    assert!(!plain.contributor);
    assert_eq!(warning(&contributor), Some(CONTRIBUTOR_WARNING));
    assert_eq!(warning(&plain), None);
}

#[test]
fn the_marked_model_is_the_default_unless_it_is_a_contributor_model_then_its_twin() {
    let models = listed(vec![
        ModelChoice::new("a", "A"),
        marked("b"),
        ModelChoice::new("c", "C"),
    ]);
    assert_eq!(models.default_model().map(|model| &*model.id), Some("b"));

    let models = listed(vec![
        ModelChoice::new("muse-wonder-1.0", "Wonder"),
        marked("muse-spark-1.3-contributor"),
        ModelChoice::new("muse-spark-1.3", "Spark"),
    ]);
    assert_eq!(
        models.default_model().map(|model| &*model.id),
        Some("muse-spark-1.3")
    );
}

#[test]
fn with_no_twin_the_default_is_the_first_model_that_is_no_contributor_and_not_disabled() {
    let disabled = ModelChoice {
        disabled: true,
        ..ModelChoice::new("fable", "Fable")
    };
    let models = listed(vec![
        ModelChoice::new("meta-1-contributor", "One"),
        disabled,
        marked("meta-2-contributor"),
        ModelChoice::new("plain", "Plain"),
    ]);
    assert_eq!(
        models.default_model().map(|model| &*model.id),
        Some("plain")
    );
}

#[test]
fn a_list_of_contributor_models_alone_has_no_default() {
    let models = listed(vec![
        marked("a-contributor"),
        ModelChoice::new("b-contributor", "B"),
    ]);
    assert_eq!(models.default_model(), None);
}

#[test]
fn a_stale_list_gives_no_default_whatever_it_marks() {
    let models = listed(vec![
        marked("gpt-6.1-sol"),
        ModelChoice::new("gpt-5.5", "5.5"),
    ])
    .stale("the list Codex ships");
    assert_eq!(models.default_model(), None);
    assert_eq!(
        models.freshness,
        Freshness::Stale {
            why: "the list Codex ships".to_owned()
        }
    );
}

#[test]
fn a_twin_is_looked_for_with_the_provider_of_the_marked_model() {
    let on = |provider: &str, id: &str, mark: bool| ModelChoice {
        provider: Some(provider.to_owned()),
        marked_default: mark,
        ..ModelChoice::new(id, id)
    };
    let models = listed(vec![
        on("other", "spark", false),
        on("meta", "spark-contributor", true),
        on("meta", "wonder", false),
        on("meta", "spark", false),
    ]);
    assert_eq!(
        models.default_model().map(ModelChoice::model),
        Some("meta/spark".to_owned())
    );
}

#[test]
fn pi_s_provider_and_id_are_joined_for_the_launch_at_the_first_slash_only() {
    let model = ModelChoice {
        provider: Some("openrouter".to_owned()),
        ..ModelChoice::new("anthropic/claude-fable-5", "Fable")
    };
    assert_eq!(model.model(), "openrouter/anthropic/claude-fable-5");
    assert_eq!(model.launch_model(Some("high")), model.model());
    let models = listed(vec![model]);
    assert!(models
        .model("openrouter/anthropic/claude-fable-5")
        .is_some());
    assert!(models.model("anthropic/claude-fable-5").is_none());
}

#[test]
fn an_effort_inside_the_id_launches_the_exact_id_listed_never_a_join() {
    let model = ModelChoice {
        efforts: vec!["low".to_owned(), "medium".to_owned()],
        launch_ids: BTreeMap::from([
            ("low".to_owned(), "claude-opus-5-5-low-fast".to_owned()),
            (
                "medium".to_owned(),
                "claude-opus-5-5-medium-fast".to_owned(),
            ),
        ]),
        ..ModelChoice::new("claude-opus-5-5-fast", "Claude Opus 5.5 Fast")
    };
    assert_eq!(
        model.launch_model(Some("medium")),
        "claude-opus-5-5-medium-fast"
    );
    assert_eq!(model.launch_model(None), "claude-opus-5-5-fast");
}

#[test]
fn a_failed_read_lists_no_model_and_says_why() {
    let listing = ModelListing::failed("Not logged in");
    assert!(listing.models.is_empty());
    assert_eq!((listing.read_at, listing.is_stale()), (None, true));
    assert_eq!(
        listing.freshness,
        Freshness::Stale {
            why: "Not logged in".to_owned()
        }
    );
    assert_eq!(listing.default_model(), None, "stale: no default");
}

#[test]
fn a_listing_s_json_says_its_freshness_by_state() {
    let fresh = serde_json::to_value(listed(Vec::new())).unwrap();
    assert_eq!(
        fresh,
        serde_json::json!({ "models": [], "readAt": 10, "freshness": { "state": "fresh" }, "how": "a test" })
    );
    let stale = serde_json::to_value(listed(Vec::new()).stale("offline")).unwrap();
    assert_eq!(
        stale["freshness"],
        serde_json::json!({ "state": "stale", "why": "offline" })
    );
}

#[test]
fn the_ledger_s_json_says_only_what_a_model_has() {
    let json = serde_json::to_value(ModelChoice::new("haiku", "Haiku")).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "id": "haiku", "label": "Haiku", "efforts": [] })
    );
    let back: ModelChoice = serde_json::from_value(json).unwrap();
    assert_eq!(back, ModelChoice::new("haiku", "Haiku"));
}
