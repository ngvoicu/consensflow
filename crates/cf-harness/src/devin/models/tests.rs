use std::rc::Rc;

use cf_process::Failed;
use serde_json::json;

use super::*;
use crate::testing::{finished, ScriptedProcesses};

/// What Devin 3000.11.228 listed on 2026-10-28 for a Pro login, six families of 45 kept
/// (`tools/probes/chief-harness/models.py devin`), and what it said signed out.
const RECORDED: &str = include_str!("../../../tests/fixtures/models/devin-models-list.json");
const SIGNED_OUT: &str =
    include_str!("../../../tests/fixtures/models/devin-models-list-signed-out.json");

fn recorded() -> Models {
    parse(&serde_json::from_str(RECORDED).unwrap()).unwrap()
}

#[test]
fn a_variant_s_effort_is_read_past_the_suffixes_from_its_end() {
    assert_eq!(split("swe-2-high"), ("swe-2".to_owned(), Some("high")));
    assert_eq!(
        split("claude-opus-5-5-medium-fast"),
        ("claude-opus-5-5-fast".to_owned(), Some("medium"))
    );
    assert_eq!(
        split("gpt-6-sol-none-priority"),
        ("gpt-6-sol-priority".to_owned(), Some("none"))
    );
    assert_eq!(split("adaptive"), ("adaptive".to_owned(), None));
    assert_eq!(
        split("max"),
        ("max".to_owned(), None),
        "an effort alone is no model"
    );
    assert_eq!(
        split("swe-2-high-turbo"),
        ("swe-2-high-turbo".to_owned(), None),
        "a suffix it does not know reads as no effort, never a wrong id"
    );
}

#[test]
fn each_model_keeps_the_exact_id_of_each_effort_its_efforts_from_the_lowest() {
    let models = recorded();
    let fast = models.model("claude-opus-5-5-fast").unwrap();
    assert_eq!(fast.efforts, ["low", "medium", "high", "xhigh", "max"]);
    assert_eq!(
        fast.launch_model(Some("medium")),
        "claude-opus-5-5-medium-fast"
    );
    assert_eq!(fast.label, "Claude Opus 5.5 Fast");
    let sol = models.model("gpt-6-sol").unwrap();
    assert_eq!(
        sol.efforts,
        ["none", "low", "medium", "high", "xhigh", "max"]
    );
    assert_eq!(sol.label, "GPT-6 Sol");
    assert_eq!(sol.launch_model(Some("none")), "gpt-6-sol-none");
    let swe = models.model("swe-2").unwrap();
    assert_eq!(swe.efforts, ["medium", "high", "max"]);
    assert_eq!(swe.label, "SWE-2");
    assert_eq!(
        models.model("gpt-6-sol-priority").unwrap().label,
        "GPT-6 Sol Fast"
    );
}

#[test]
fn a_model_with_no_effort_is_its_variant_and_fusion_is_left_out() {
    let models = recorded();
    let adaptive = models.model("adaptive").unwrap();
    assert!(adaptive.efforts.is_empty());
    assert_eq!(adaptive.launch_model(Some("high")), "adaptive");
    assert!(models
        .models
        .iter()
        .all(|model| !model.id.starts_with("fusion")));
    let ids: Vec<&str> = models.models.iter().map(|model| &*model.id).collect();
    assert_eq!(
        ids,
        [
            "adaptive",
            "swe-2",
            "swe-2-lightning",
            "claude-opus-5-5",
            "claude-opus-5-5-fast",
            "gpt-6-sol",
            "gpt-6-sol-priority",
            "kimi-k3"
        ]
    );
    assert!(!models.stale);
    assert_eq!(
        models.default_model().map(|model| &*model.id),
        Some("adaptive")
    );
}

fn devin_run(answer: Result<String, Failed>) -> Result<Models, String> {
    let processes = Rc::new(ScriptedProcesses::default());
    processes.run_answer("devin models list --format json", answer);
    finished(Box::pin(read(
        &*processes,
        program(PathBuf::from("devin"), &Env::default()),
    )))
}

#[test]
fn a_read_runs_the_list_and_a_signed_out_devin_s_words_are_the_reason() {
    assert_eq!(devin_run(Ok(RECORDED.to_owned())).unwrap(), recorded());

    let said: Value = serde_json::from_str(SIGNED_OUT).unwrap();
    let failed = Failed {
        message: format!(
            "Command failed: devin models list --format json\n{}",
            said["stderr"].as_str().unwrap()
        ),
        code: Some(1),
        killed: false,
        stdout: String::new(),
    };
    assert_eq!(
        devin_run(Err(failed)),
        Err("devin models list --format json: Not logged in. Run `devin auth login` to authenticate.".to_owned())
    );
    assert!(devin_run(Ok("{".to_owned())).is_err());
    assert!(devin_run(Ok(json!({ "families": [] }).to_string())).is_err());
}
