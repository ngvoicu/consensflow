use super::*;
use crate::roster::agent_row::AgentRow;
use crate::roster::document::load_document;
use crate::roster::testing::file_with;
use crate::Catalog;
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

#[test]
fn claude_and_openai_models_are_hidden_on_pi_and_opencode_when_the_human_keeps_to_their_own() {
    for (kind, model) in [
        ("pi", "openrouter/anthropic/claude-fable-5.1"),
        ("pi", "openai-codex/gpt-6-astra"),
        ("pi", "openai-codex/gpt-6.1-sol"),
        ("opencode", "openrouter/anthropic/claude-fable-5.1"),
        ("opencode", "openrouter/openai/gpt-6-astra"),
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
        ("devin", "claude-fable-5-1"),
    ] {
        assert!(!hides(ON, &view(kind, model)), "{kind} {model}");
    }
}

#[test]
fn other_makers_models_are_never_hidden_on_pi_or_opencode() {
    for (kind, model) in [
        ("pi", "openrouter/moonshotai/kimi-k3"),
        ("opencode", "opencode/muse-spark-1.3-contributor-free"),
    ] {
        assert!(!hides(ON, &view(kind, model)), "{kind} {model}");
    }
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
