use std::rc::Rc;

use cf_process::Failed;
use serde_json::json;

use super::*;
use crate::testing::{finished, ChildScript, Ends, ManualTime, ScriptedProcesses};

/// When a test's list is read.
const AT: i64 = 1_793_000_000_000;

/// What Pi 1.4.0 answered `get_available_models` with on 2026-10-28, offline, a few rows of
/// each provider kept (`tools/probes/chief-harness/models.py pi`), and with no login.
const RECORDED: &str = include_str!("../../../tests/fixtures/models/pi-get-available-models.json");
const SIGNED_OUT: &str =
    include_str!("../../../tests/fixtures/models/pi-get-available-models-signed-out.json");

fn recorded() -> ModelListing {
    parse(&serde_json::from_str(RECORDED).unwrap(), AT).unwrap()
}

fn efforts_of(models: &ModelListing, model: &str) -> Vec<String> {
    models.model(model).expect(model).efforts.clone()
}

#[test]
fn each_model_keeps_its_provider_apart_and_launches_as_both_joined() {
    let models = recorded();
    let fable = models.model("openrouter/anthropic/claude-fable-5").unwrap();
    assert_eq!(fable.provider.as_deref(), Some("openrouter"));
    assert_eq!(fable.id, "anthropic/claude-fable-5");
    assert_eq!(
        fable.launch_model(Some("high")),
        "openrouter/anthropic/claude-fable-5"
    );
    assert_eq!(models.models.len(), 20);
    assert!(!models.is_stale());
    assert_eq!(models.how, HOW);
}

#[test]
fn the_levels_are_pi_s_own_rule_of_reasoning_and_the_thinking_map() {
    let models = recorded();
    assert_eq!(efforts_of(&models, "openai/gpt-4"), ["off"], "no reasoning");
    assert_eq!(
        efforts_of(&models, "anthropic/claude-haiku-4-5"),
        ["off", "minimal", "low", "medium", "high"],
        "reasoning and no map: no xhigh or max unless named"
    );
    assert_eq!(
        efforts_of(&models, "kimi-coding/k3"),
        ["low", "high", "max"],
        "a level mapped to null is not taken"
    );
    assert_eq!(
        efforts_of(&models, "anthropic/claude-fable-5"),
        ["minimal", "low", "medium", "high", "xhigh", "max"],
        "off is mapped to null; xhigh and max are named"
    );
    assert_eq!(
        efforts_of(&models, "meta/muse-spark-1.2"),
        ["minimal", "low", "medium", "high", "xhigh"]
    );
}

#[test]
fn a_contributor_model_pi_lists_is_marked_and_never_the_default() {
    let models = recorded();
    let contributor = models.model("meta/muse-spark-1.2-contributor").unwrap();
    assert!(contributor.contributor);
    let default = models.default_model().unwrap();
    assert!(!default.contributor);
}

#[test]
fn pi_with_no_login_lists_no_model_and_says_why() {
    let signed_out = parse(&serde_json::from_str(SIGNED_OUT).unwrap(), AT);
    assert_eq!(
        signed_out,
        Err("Pi lists no model: it holds no login for any provider".to_owned())
    );
    let refused = parse(&json!({ "success": false, "error": "bad" }), AT);
    assert_eq!(
        refused,
        Err("pi refused get_available_models: bad".to_owned())
    );
}

#[test]
fn the_list_is_read_offline_with_nothing_of_pi_s_own() {
    let program = program(PathBuf::from("pi"), &Env::default());
    assert_eq!(
        program.args,
        [
            "--mode",
            "rpc",
            "--no-session",
            "--no-extensions",
            "--no-mcp",
            "--offline"
        ]
    );
    assert_eq!(program.env.text("PI_OFFLINE"), Some("1"));
}

/// A `pi` on the PATH of a throwaway folder, and what Pi's list and its update answer.
fn listed_with(
    update: Option<Result<String, Failed>>,
    fetch: bool,
) -> (Result<ModelListing, String>, Vec<String>) {
    let folder = tempfile::tempdir().unwrap();
    let pi = crate::testing::fake_executable(&folder.path().join("pi"));
    let env = Env::from_vars([("PATH", pi.parent().unwrap().as_os_str())]);
    let processes = Rc::new(ScriptedProcesses::default());
    if let Some(update) = update {
        processes.run_answer("pi update --models", update);
    }
    let answer = json!({ "type": "response", "id": "models", "success": true,
        "data": { "models": [{ "provider": "meta", "id": "spark", "name": "Spark" }] } });
    processes.child(
        "pi",
        ChildScript {
            lines: vec![answer.to_string()],
            ends: Ends::Asked,
        },
    );
    let time = ManualTime::new(0);
    let listed = finished(Box::pin(listed(&env, &*processes, &time, fetch)));
    let ran = processes
        .take_ran()
        .iter()
        .map(|(program, _)| crate::testing::named(program))
        .collect();
    (listed, ran)
}

#[test]
fn only_the_human_s_refresh_fetches_pi_s_catalogs_and_a_failed_fetch_leaves_a_stale_list() {
    let (listed, ran) = listed_with(None, false);
    assert!(!listed.unwrap().is_stale());
    assert!(ran.is_empty(), "nothing fetched: {ran:?}");

    let (listed, ran) = listed_with(Some(Ok(String::new())), true);
    assert!(!listed.unwrap().is_stale());
    assert_eq!(ran, ["pi update --models"]);

    let failed = Failed {
        message: "Command failed: pi update --models\noffline".to_owned(),
        code: Some(1),
        killed: false,
        stdout: String::new(),
    };
    let (listed, _) = listed_with(Some(Err(failed)), true);
    let listed = listed.unwrap();
    assert_eq!(
        listed.freshness,
        cf_proto::models::Freshness::Stale {
            why: "Pi's own catalog: pi update --models failed: offline".to_owned()
        }
    );
    assert_eq!(listed.models.len(), 1, "the list Pi keeps");
}

/// Pi's default model, from a folder of its own whose `settings.json` says `settings`, a
/// `pi` on the PATH whose list offers `listed`.
fn default_with(
    settings: Option<Value>,
    listed: &[(&str, &str)],
) -> Result<Option<String>, String> {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("bin")).unwrap();
    let pi = crate::testing::fake_executable(&folder.path().join("bin").join("pi"));
    let agent = folder.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    if let Some(settings) = settings {
        std::fs::write(agent.join("settings.json"), settings.to_string()).unwrap();
    }
    let env = Env::from_vars([
        ("PATH", pi.parent().unwrap().as_os_str()),
        ("PI_CODING_AGENT_DIR", agent.as_os_str()),
    ]);
    let processes = Rc::new(ScriptedProcesses::default());
    let models: Vec<Value> = listed
        .iter()
        .map(|(provider, id)| json!({ "provider": provider, "id": id, "name": id }))
        .collect();
    let answer = json!({ "type": "response", "id": "models", "success": true,
        "data": { "models": models } });
    processes.child(
        "pi",
        ChildScript {
            lines: vec![answer.to_string()],
            ends: Ends::Asked,
        },
    );
    let time = ManualTime::new(0);
    finished(Box::pin(default_model(&env, &*processes, &time)))
}

#[test]
fn pi_starts_on_its_settings_default_and_never_on_a_contributor_model() {
    let settings = |provider: &str, model: &str| {
        Some(json!({ "defaultProvider": provider, "defaultModel": model }))
    };
    let listed = [
        ("meta", "muse-spark-1.3-contributor"),
        ("openai", "gpt-6-luna"),
    ];
    assert_eq!(
        default_with(settings("openrouter", "anthropic/claude-fable-5"), &listed),
        Ok(Some("openrouter/anthropic/claude-fable-5".to_owned()))
    );
    // A contributor default is never the one: the first its list offers that is not.
    assert_eq!(
        default_with(settings("meta", "muse-spark-1.3-contributor"), &listed),
        Ok(Some("openai/gpt-6-luna".to_owned()))
    );
    // No default said: the same.
    assert_eq!(
        default_with(None, &listed),
        Ok(Some("openai/gpt-6-luna".to_owned()))
    );
    // Nothing but contributor models: none, and why.
    assert_eq!(
        default_with(None, &[("meta", "muse-spark-1.3-contributor")]),
        Err("Pi offers no model that is not a contributor model".to_owned())
    );
}

#[test]
fn a_model_pi_does_not_have_is_said_in_its_providers_words_and_its_catalogs_warning_is_none() {
    // Pi's own warning of an id its offline catalog does not know: it runs it all the same,
    // and a model the catalog has not caught up with yet works (Calliope's P1, T-955).
    assert!(!gone(
        "Model 'gpt-gone-9' not found for provider 'openai'. Using custom model id.",
        "openai/gpt-gone-9"
    ));
    // The provider's refusal, as OpenAI said it on CHF-02's probe.
    assert!(gone(
        r#"400 {"error":{"code":"model_not_found","message":"The model `gpt-gone-9` does not exist"}}"#,
        "openai/gpt-gone-9"
    ));
    assert!(!gone(
        "429 rate limited: try again later",
        "openai/gpt-gone-9"
    ));
}

/// A Pi chief's model is `provider/id`, and the provider names the id alone: the words are
/// held to the id after the first slash, so an OpenRouter id keeps its own slash, and
/// Anthropic's `not_found_error` is a refusal as OpenAI's `model_not_found` is (Calliope's
/// P1, T-994).
#[test]
fn a_pi_refusal_names_the_id_after_its_provider_split_once_at_the_first_slash() {
    let anthropic = r#"404 {"type":"error","error":{"type":"not_found_error","message":"model: claude-fable-5"}}"#;
    assert!(gone(anthropic, "anthropic/claude-fable-5"));
    assert!(
        !gone(anthropic, "anthropic/claude-opus-5-5"),
        "another model"
    );
    // OpenRouter's id holds a slash of its own, which its refusal names whole.
    let openrouter = r#"404 {"error":{"code":"model_not_found","message":"No endpoints found for anthropic/claude-fable-5"}}"#;
    assert!(gone(openrouter, "openrouter/anthropic/claude-fable-5"));
    assert!(
        !gone(
            r#"404 {"error":{"code":"model_not_found","message":"No endpoints found for claude-fable-5"}}"#,
            "openrouter/anthropic/claude-fable-5"
        ),
        "split once: OpenRouter's id is not its last segment"
    );
    // A model with no provider is its own id.
    assert!(gone(anthropic, "claude-fable-5"));
}

/// Pi 1.4.0's windows on a model their provider lacks stay up, and the first turn fails in
/// the record's `errorMessage` (`gone_launch.py pi`, 2026-10-31): the `openai-codex`
/// provider in Codex's own words, with no `model_not_found`, and Anthropic's 404
/// `not_found_error`. Both refuse the model asked for, by its id alone.
#[test]
fn a_pi_window_on_a_model_its_provider_lacks_fails_in_that_providers_words() {
    let codex = "Codex error: The 'gpt-gone-9' model is not supported when using Codex with a ChatGPT account.";
    assert!(gone(codex, "openai-codex/gpt-gone-9"));
    assert!(!gone(codex, "openai-codex/gpt-6-luna"));
    let anthropic = r#"404 {"type":"error","error":{"type":"not_found_error","message":"model: claude-gone-9"},"request_id":"req_0"}"#;
    assert!(gone(anthropic, "anthropic/claude-gone-9"));
}
