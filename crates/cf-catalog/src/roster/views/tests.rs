use super::*;
use cf_proto::agents::WorkTier;
use serde_json::json;

fn row(text: &str) -> AgentRow {
    AgentRow::from_value(serde_json::from_str(text).unwrap()).unwrap()
}

fn view_of(text: &str) -> AgentView {
    Catalog::bundled().unwrap().view(&row(text)).unwrap()
}

fn written(view: &AgentView) -> String {
    serde_json::to_string(view).unwrap()
}

fn refusal_of(text: &str) -> Refusal {
    Catalog::bundled().unwrap().view(&row(text)).unwrap_err()
}

/// The text of a custom row of the human's, on this kind and model.
fn custom(kind: &str, model: &str, more: &str) -> String {
    format!(r#"{{"id":"mine","kind":"{kind}","model":"{model}"{more}}}"#)
}

#[test]
fn a_catalog_row_is_viewed_as_the_catalog_lists_it() {
    let view = view_of(
        r#"{"id":"thoth","name":"Thoth","kind":"devin","model":"claude-fable-5-1","effort":"max","description":"Devin Claude Fable 5.1 MAX","preset":"thoth"}"#,
    );
    assert_eq!(
        written(&view),
        concat!(
            r#"{"name":"thoth","harness":"devin","model":"claude-fable-5-1","effort":"max","#,
            r#""description":"Devin Claude Fable 5.1 MAX","preset":"thoth","#,
            r#""profile":{"modelKey":"claude-fable-5.1","modelLabel":"Claude Fable 5.1","routeLabel":"Devin account","workTier":"critical"}}"#
        )
    );
}

#[test]
fn a_view_says_everything_a_row_has_in_the_order_the_roster_builds_it() {
    let view = view_of(
        r#"{"id":"mine","kind":"kimi","designer":true,"model":"m","workTier":"light","effort":"high","description":"d","preset":"p","custom":true}"#,
    );
    assert_eq!(
        written(&view),
        concat!(
            r#"{"name":"mine","harness":"kimi","designer":true,"model":"m","workTier":"light","#,
            r#""effort":"high","description":"d","preset":"p","custom":true,"#,
            r#""profile":{"modelKey":"codex-image","modelLabel":"Codex Images","routeLabel":"Codex login","workTier":"light"},"#,
            r#""unsupported":true}"#
        )
    );
}

#[test]
fn a_view_of_an_empty_row_has_its_profile_and_says_it_is_unsupported() {
    // `JSON.stringify` leaves out the keys that are `undefined`.
    assert_eq!(
        written(&view_of("{}")),
        concat!(
            r#"{"profile":{"modelKey":"default","modelLabel":"Default","routeLabel":"undefined","workTier":"light"},"#,
            r#""unsupported":true}"#
        )
    );
}

#[test]
fn a_row_with_no_id_has_no_name_and_one_with_no_model_has_no_model() {
    let no_id = view_of(r#"{"kind":"codex","model":"m"}"#);
    assert_eq!((no_id.name, no_id.model.as_deref()), (None, Some("m")));
    let no_model = view_of(r#"{"id":"a","kind":"codex"}"#);
    assert_eq!(
        (no_model.name.as_deref(), no_model.model),
        (Some("a"), None)
    );
}

#[test]
fn each_kind_the_build_runs_is_a_harness_named_as_the_cli_is() {
    for (kind, harness) in [
        ("claude-code", "claude"),
        ("codex", "codex"),
        ("pi", "pi"),
        ("opencode", "opencode"),
        ("devin", "devin"),
    ] {
        let view = view_of(&custom(kind, "m", ""));
        assert_eq!(view.harness.as_deref(), Some(harness), "{kind}");
        assert!(!view.unsupported, "{kind}");
    }
}

#[test]
fn a_kind_the_build_does_not_run_is_text_not_a_harness_and_is_unsupported() {
    let view = view_of(r#"{"id":"old-kimi","kind":"kimi","model":"moonshot-ai/kimi-k3"}"#);
    assert_eq!(view.harness.as_deref(), Some("kimi"));
    assert!(view.unsupported);
    assert_eq!(
        view.profile.route_label, "kimi",
        "the profile reads the same kind"
    );
    let written = written(&view);
    assert!(
        written.starts_with(
            r#"{"name":"old-kimi","harness":"kimi","model":"moonshot-ai/kimi-k3","profile":"#
        ),
        "{written}"
    );
    assert!(written.ends_with(r#""unsupported":true}"#), "{written}");
}

#[test]
fn a_row_with_no_kind_has_no_harness_and_is_unsupported() {
    let view = view_of(r#"{"id":"a","model":"m"}"#);
    assert_eq!(view.harness, None);
    assert!(view.unsupported);
    assert_eq!(view.profile.route_label, "undefined");
    assert!(!written(&view).contains("\"harness\""));
}

#[test]
fn a_designer_is_said_for_true_alone_and_the_profile_reads_any_truthy_value() {
    for truthy in [r#""no""#, "[]", "{}", "1", r#""0""#] {
        let view = view_of(&custom("codex", "m", &format!(r#","designer":{truthy}"#)));
        assert!(!view.designer, "{truthy}: the view says true alone");
        assert_eq!(
            view.profile.model_key, "codex-image",
            "{truthy}: the profile reads it"
        );
        assert!(!written(&view).contains("designer"), "{truthy}");
    }
    let view = view_of(&custom("codex", "m", r#","designer":true"#));
    assert!(view.designer);
    assert_eq!(view.profile.model_key, "codex-image");
    for falsy in ["false", "0", r#""""#, "null"] {
        let view = view_of(&custom("codex", "m", &format!(r#","designer":{falsy}"#)));
        assert!(!view.designer, "{falsy}");
        assert_eq!(view.profile.model_key, "m", "{falsy}");
    }
}

#[test]
fn the_view_names_the_harness_of_the_kind_and_the_profile_reads_the_harness_of_the_row() {
    let view = view_of(&custom("claude-code", "m", r#","harness":"codex""#));
    assert_eq!(view.harness.as_deref(), Some("claude"));
    assert_eq!(view.profile.route_label, "Codex login");
    let plain = view_of(&custom("claude-code", "m", ""));
    assert_eq!(plain.profile.route_label, "Claude Code account");
}

#[test]
fn on_pi_the_view_reads_thinking_alone_and_the_profile_thinking_before_effort() {
    let sol = "openai-codex/gpt-6.1-sol";
    let cases = [
        // (the row's effort fields, what the view says, the tier the profile reads)
        (
            r#","thinking":null,"effort":"max""#,
            None,
            WorkTier::Complex,
        ),
        (r#","effort":"max""#, None, WorkTier::Complex),
        (
            r#","thinking":"low","effort":"max""#,
            Some("low"),
            WorkTier::Light,
        ),
        (
            r#","thinking":"max","effort":"low""#,
            Some("max"),
            WorkTier::Complex,
        ),
        (r#","thinking":"max""#, Some("max"), WorkTier::Complex),
    ];
    for (fields, effort, tier) in cases {
        let view = view_of(&custom("pi", sol, fields));
        assert_eq!(view.effort.as_deref(), effort, "{fields}");
        assert_eq!(view.profile.work_tier, tier, "{fields}");
    }
}

#[test]
fn off_pi_the_view_reads_effort_alone() {
    let sol = "gpt-6.1-sol";
    let view = view_of(&custom("codex", sol, r#","thinking":"max""#));
    assert_eq!(view.effort, None);
    assert_eq!(
        view.profile.work_tier,
        WorkTier::Light,
        "no effort, no tier above light"
    );
    let view = view_of(&custom("codex", sol, r#","effort":"max","thinking":"low""#));
    assert_eq!(view.effort.as_deref(), Some("max"));
    assert_eq!(view.profile.work_tier, WorkTier::Complex);
}

#[test]
fn an_empty_effort_or_preset_is_not_said_and_null_is_as_good_as_absent() {
    for fields in [
        r#","effort":"""#,
        r#","effort":null"#,
        r#","preset":"""#,
        r#","preset":null"#,
    ] {
        let view = view_of(&custom("codex", "m", fields));
        assert_eq!((view.effort, view.preset), (None, None), "{fields}");
    }
    let view = view_of(&custom("pi", "m", r#","thinking":"","effort":"high""#));
    assert_eq!(
        view.effort, None,
        "empty text is falsy, and Pi reads thinking alone"
    );
}

#[test]
fn a_description_that_is_truthy_is_kept_as_the_json_it_is() {
    for description in [
        json!(5),
        json!(true),
        json!([]),
        json!({}),
        json!("text"),
        json!([0]),
    ] {
        let view = view_of(&custom(
            "codex",
            "m",
            &format!(r#","description":{description}"#),
        ));
        assert_eq!(view.description, Some(description.clone()), "{description}");
    }
    for falsy in ["0", "false", r#""""#, "null"] {
        let view = view_of(&custom("codex", "m", &format!(r#","description":{falsy}"#)));
        assert_eq!(view.description, None, "{falsy}");
    }
    assert_eq!(view_of(&custom("codex", "m", "")).description, None);
}

#[test]
fn a_row_is_custom_when_its_custom_field_is_truthy() {
    for truthy in ["true", r#""yes""#, "1", "[]"] {
        let view = view_of(&custom("codex", "m", &format!(r#","custom":{truthy}"#)));
        assert!(view.custom, "{truthy}");
    }
    for falsy in ["false", "0", r#""""#, "null"] {
        let view = view_of(&custom("codex", "m", &format!(r#","custom":{falsy}"#)));
        assert!(!view.custom, "{falsy}");
    }
    assert!(!view_of(&custom("codex", "m", "")).custom);
}

#[test]
fn a_work_tier_the_row_names_is_said_and_the_profile_takes_it() {
    for (word, tier) in [
        ("critical", WorkTier::Critical),
        ("complex", WorkTier::Complex),
        ("standard", WorkTier::Standard),
        ("light", WorkTier::Light),
    ] {
        let view = view_of(&custom("codex", "m", &format!(r#","workTier":"{word}""#)));
        assert_eq!(view.work_tier, Some(tier), "{word}");
        assert_eq!(view.profile.work_tier, tier, "{word}");
    }
    let none = view_of(&custom("codex", "m", r#","workTier":null"#));
    assert_eq!(none.work_tier, None);
    assert_eq!(none.profile.work_tier, WorkTier::Light);
}

#[test]
fn a_work_tier_outside_the_four_fails_the_view_with_the_tier_sentence() {
    for tier in [
        r#""huge""#,
        "7",
        r#""""#,
        "true",
        "[]",
        "{}",
        r#""Critical""#,
    ] {
        let refusal = refusal_of(&custom("codex", "m", &format!(r#","workTier":{tier}"#)));
        assert_eq!(
            refusal.message, "Work tier must be critical, complex, standard or light",
            "{tier}"
        );
        assert_eq!((refusal.code, refusal.status), ("work-tier", 400), "{tier}");
    }
}
