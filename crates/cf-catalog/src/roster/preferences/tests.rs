use super::*;
use crate::roster::agent_row::AgentRow;
use crate::roster::document::load_document;
use crate::roster::testing::file_with;
use crate::Catalog;
use cf_base::js;
use serde_json::json;
use tempfile::tempdir;

fn document(text: &str) -> Document {
    let (_home, path) = file_with(text.as_bytes());
    load_document(&path).unwrap()
}

fn chosen(text: &str) -> bool {
    preferences_of(&document(text)).own_harness_only
}

/// The view of a row on this kind and model.
fn view(kind: &str, model: &str) -> AgentView {
    let row = format!(r#"{{"id":"mine","kind":"{kind}","model":"{model}"}}"#);
    let row = AgentRow::from_value(serde_json::from_str(&row).unwrap()).unwrap();
    Catalog::bundled().unwrap().view(&row).unwrap()
}

const ON: Preferences = Preferences {
    own_harness_only: true,
};
const OFF: Preferences = Preferences {
    own_harness_only: false,
};

#[test]
fn a_roster_with_no_file_keeps_the_choice_off() {
    let home = tempdir().unwrap();
    let document = load_document(&home.path().join("agents.json")).unwrap();
    assert_eq!(preferences_of(&document), OFF);
}

#[test]
fn a_file_with_no_preferences_keeps_the_choice_off() {
    assert!(!chosen("{}"));
    assert!(!chosen(r#"{"preferences":{}}"#));
    assert!(!chosen(r#"{"preferences":{"colour":"green"}}"#));
    assert!(
        !chosen(r#"{"preferences":{"specs":true}}"#),
        "specs for every project are no choice about the roster"
    );
}

#[test]
fn the_choice_is_on_when_the_file_says_true() {
    assert!(chosen(r#"{"preferences":{"ownHarnessOnly":true}}"#));
    assert!(chosen(
        r#"{"agents":[],"preferences":{"extra":1,"ownHarnessOnly":true}}"#
    ));
}

#[test]
fn only_true_itself_turns_it_on() {
    for other in [
        "false",
        r#""true""#,
        "1",
        "[true]",
        "null",
        "{}",
        r#""yes""#,
    ] {
        assert!(
            !chosen(&format!(
                r#"{{"preferences":{{"ownHarnessOnly":{other}}}}}"#
            )),
            "{other}"
        );
    }
}

#[test]
fn preferences_that_are_no_object_say_nothing() {
    for other in ["null", "true", r#""ownHarnessOnly""#, "[]", "[true]", "5"] {
        assert!(!chosen(&format!(r#"{{"preferences":{other}}}"#)), "{other}");
    }
}

/// The preferences a file keeps after `patch` was set on `text`, and the file's own words.
fn kept_after(text: &str, patch: Value) -> (Result<Preferences, String>, Value) {
    let (_home, path) = file_with(text.as_bytes());
    let set = set_preferences(&path, Some(&patch)).map_err(|refusal| refusal.message);
    let kept: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    (set, kept["preferences"].clone())
}

#[test]
fn the_choice_set_leaves_the_app_s_specs_as_the_file_has_them_and_keeps_what_a_later_build_kept() {
    let file = r#"{"preferences":{"colour":"green","ownHarnessOnly":false,"specs":true},"specsDefaults":{"folder":"docs/specs","variant":"tdd-html"}}"#;
    let (_home, path) = file_with(file.as_bytes());
    assert_eq!(
        set_preferences(&path, Some(&json!({ "ownHarnessOnly": true }))),
        Ok(ON)
    );
    let after: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    // Its own keys first, as Node and 3.0.4 wrote them; a later build's after them, as they were.
    assert_eq!(
        js::stringify(&after["preferences"]),
        r#"{"ownHarnessOnly":true,"specs":true,"colour":"green"}"#
    );
    assert_eq!(
        after["specsDefaults"],
        json!({ "folder": "docs/specs", "variant": "tdd-html" })
    );
    // A patch that names none changes nothing, and still writes the whole set.
    let (set, kept) = kept_after(r#"{"preferences":{"specs":true}}"#, json!({}));
    assert_eq!(set, Ok(OFF));
    assert_eq!(kept, json!({ "ownHarnessOnly": false, "specs": true }));
    // A file that never knew of specs is written as it always was.
    let (_, kept) = kept_after("{}", json!({ "ownHarnessOnly": true }));
    assert_eq!(kept, json!({ "ownHarnessOnly": true }));
}

#[test]
fn a_switch_that_is_none_and_a_choice_that_is_unknown_are_refused_as_before_and_leave_the_file() {
    let file = r#"{"preferences":{"ownHarnessOnly":true,"specs":true}}"#;
    for patch in [
        json!({ "ownHarnessOnly": "yes" }),
        json!({ "ownHarnessOnly": null }),
        json!({ "ownHarnessOnly": 1 }),
    ] {
        let (set, kept) = kept_after(file, patch.clone());
        assert_eq!(
            set,
            Err("ownHarnessOnly is on or off".to_owned()),
            "{patch}"
        );
        assert_eq!(
            kept,
            json!({ "ownHarnessOnly": true, "specs": true }),
            "{patch}: the file is as it was"
        );
    }
    let (set, _) = kept_after("{}", json!({ "ownHarnessOnly": true, "colour": true }));
    assert_eq!(set, Err("no preference named colour".to_owned()));
}

#[test]
fn the_app_s_specs_are_no_choice_about_the_roster_and_are_refused_by_name_leaving_the_file() {
    let file = r#"{"preferences":{"ownHarnessOnly":false,"specs":true}}"#;
    for (patch, key) in [
        (json!({ "specs": false }), "specs"),
        (json!({ "specsFolder": "docs" }), "specsFolder"),
        (json!({ "specsVariant": "tdd" }), "specsVariant"),
        (json!({ "ownHarnessOnly": true, "specs": false }), "specs"),
    ] {
        let (set, kept) = kept_after(file, patch.clone());
        assert_eq!(set, Err(format!("no preference named {key}")), "{patch}");
        assert_eq!(
            kept,
            json!({ "ownHarnessOnly": false, "specs": true }),
            "{patch}: the file is as it was"
        );
    }
}

#[test]
fn claude_and_openai_models_are_hidden_on_pi_opencode_and_devin_when_the_human_keeps_to_their_own()
{
    for (kind, model) in [
        ("pi", "openrouter/anthropic/claude-fable-5.1"),
        ("pi", "openai-codex/gpt-6-astra"),
        ("pi", "openai-codex/gpt-6.1-sol"),
        ("opencode", "openrouter/anthropic/claude-fable-5.1"),
        ("opencode", "openrouter/openai/gpt-6-astra"),
        // Devin reaches them too (the owner, 2026-10-25).
        ("devin", "claude-fable-5-1"),
        ("devin", "claude-opus-5-5"),
        ("devin", "gpt-6-astra"),
        ("devin", "gpt-6-1-sol"),
    ] {
        let view = view(kind, model);
        assert!(hides(ON, &view), "{kind} {model}");
        assert!(!hides(OFF, &view), "{kind} {model}: not unless they ask");
    }
}

#[test]
fn nothing_is_hidden_on_the_harnesses_that_run_those_models_themselves() {
    for (kind, model) in [
        ("claude-code", "claude-fable-5-1"),
        ("codex", "gpt-6-astra"),
    ] {
        assert!(!hides(ON, &view(kind, model)), "{kind} {model}");
    }
}

#[test]
fn other_makers_models_are_never_hidden_on_pi_opencode_or_devin() {
    for (kind, model) in [
        ("pi", "openrouter/moonshotai/kimi-k3"),
        ("opencode", "opencode/muse-spark-1.3-contributor-free"),
        ("devin", "swe-2"),
        ("devin", "swe-1-6"),
        ("devin", "gemini-4-argon"),
    ] {
        assert!(!hides(ON, &view(kind, model)), "{kind} {model}");
    }
}

/// Of Devin's agents, the box hides exactly the Claude and OpenAI ones: every
/// family Devin names "Claude …" or "GPT…" in its list, and none other.
#[test]
fn on_devin_the_box_hides_every_claude_and_openai_agent_and_nothing_else() {
    let catalog = Catalog::bundled().unwrap();
    let devin = catalog
        .groups()
        .iter()
        .find(|group| group.harness == crate::Harness::Devin)
        .unwrap();
    let mut hidden = 0;
    for entry in &devin.entries {
        let row = format!(
            r#"{{"id":"{}","kind":"devin","model":"{}"}}"#,
            entry.name, entry.model
        );
        let view = catalog
            .view(&AgentRow::from_value(serde_json::from_str(&row).unwrap()).unwrap())
            .unwrap();
        let label = entry.profile.model_label.as_str();
        let theirs = label.starts_with("Claude ") || label.starts_with("GPT-");
        assert_eq!(hides(ON, &view), theirs, "{} ({label})", entry.name);
        hidden += usize::from(theirs);
    }
    assert!(hidden > 0 && hidden < devin.entries.len());
}

#[test]
fn a_model_off_the_catalog_is_hidden_by_the_key_it_is_given_under() {
    // A model the catalog does not run is keyed as it is written: its
    // provider path is not stripped, so only a bare `claude-` or `gpt-` name counts.
    assert!(hides(ON, &view("pi", "claude-x")));
    assert!(hides(ON, &view("opencode", "gpt-9")));
    assert!(!hides(ON, &view("pi", "openrouter/anthropic/claude-x")));
}

#[test]
fn the_prefix_is_exact_to_the_letter_and_the_dash() {
    for model in [
        "claudex-1",
        "claude",
        "gpt4",
        "gpt",
        "GPT-6",
        "Claude-1",
        "xgpt-1",
    ] {
        assert!(!hides(ON, &view("pi", model)), "{model}");
    }
}

#[test]
fn the_harness_that_counts_is_the_views_and_a_row_with_none_is_never_hidden() {
    let mut view = view("codex", "claude-x");
    assert!(!hides(ON, &view), "Codex is not relayed");
    view.harness = Some("pi".to_owned());
    assert!(hides(ON, &view));
    view.harness = None;
    assert!(!hides(ON, &view));
}
