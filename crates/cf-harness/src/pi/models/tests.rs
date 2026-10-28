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
