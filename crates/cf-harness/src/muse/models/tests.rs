use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use serde_json::{json, Value};

use super::*;
use crate::seams::processes::{Program, Streams};
use crate::testing::{finished, ChildScript, Ends, ManualTime, ScriptedProcesses};

/// What `muse serve` answered `model/list` with: the account's (`providerCatalog`, recorded
/// 2026-10-27 over the real login), and the bundled one a host answers with no login.
fn recorded(name: &str) -> Value {
    let text = match name {
        "account" => include_str!("../../../tests/fixtures/muse/msp-model-list.json"),
        "bundled" => include_str!("../../../tests/fixtures/muse/msp-model-list-bundled.json"),
        "initialize" => include_str!("../../../tests/fixtures/muse/msp-initialize.json"),
        other => panic!("no fixture {other}"),
    };
    serde_json::from_str(text).unwrap()
}

#[test]
fn the_accounts_models_come_with_the_efforts_each_takes_and_the_contributor_ones_are_marked() {
    let models = parse(&recorded("account")["result"]).unwrap();
    assert!(!models.stale);
    let ids: Vec<&str> = models
        .models
        .iter()
        .map(|model| model.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "muse-wonder-1.0",
            "muse-spark-1.3",
            "muse-spark-1.3-contributor",
            "muse-spark-1.2",
            "muse-spark-1.2-contributor"
        ]
    );
    // The efforts are the model's own: `variants`, in Muse's order, not the tiers it describes.
    let wonder = models.model("muse-wonder-1.0").unwrap();
    assert_eq!(
        wonder.efforts,
        ["minimal", "low", "medium", "high", "xhigh", "max"]
    );
    assert_eq!(wonder.default_effort.as_deref(), Some("high"));
    let old = models.model("muse-spark-1.2").unwrap();
    assert_eq!(old.efforts, ["minimal", "low", "medium", "high", "xhigh"]);
    let contributors: Vec<&str> = models
        .models
        .iter()
        .filter(|model| model.contributor)
        .map(|model| model.id.as_str())
        .collect();
    assert_eq!(
        contributors,
        ["muse-spark-1.3-contributor", "muse-spark-1.2-contributor"]
    );
    assert_eq!(
        warning(models.model("muse-spark-1.3-contributor").unwrap()),
        Some(CONTRIBUTOR_WARNING)
    );
    assert_eq!(warning(wonder), None);
}

#[test]
fn a_contributor_model_is_never_the_default_though_muse_marks_one() {
    // On the account Muse marks `muse-spark-1.3-contributor` its default, and runs it at `max`.
    let models = parse(&recorded("account")["result"]).unwrap();
    let marked = models
        .models
        .iter()
        .find(|model| model.marked_default)
        .unwrap();
    assert_eq!(marked.id, "muse-spark-1.3-contributor");
    // Its twin that Meta does not train on is the default instead.
    assert_eq!(models.default_model().unwrap().id, "muse-spark-1.3");
    // With no twin, the first that is not a contributor model; with only contributor models,
    // none.
    let mut alone = models.clone();
    alone.models.retain(|model| model.id != "muse-spark-1.3");
    assert_eq!(alone.default_model().unwrap().id, "muse-wonder-1.0");
    alone.models.retain(|model| model.contributor);
    assert_eq!(alone.default_model(), None);
    // One Muse marks that is no contributor model stays the default.
    let mut plain = parse(&recorded("account")["result"]).unwrap();
    for model in &mut plain.models {
        model.marked_default = model.id == "muse-wonder-1.0";
    }
    assert_eq!(plain.default_model().unwrap().id, "muse-wonder-1.0");
}

#[test]
fn a_list_a_host_with_no_login_gives_is_muses_bundled_one_and_is_stale() {
    let models = parse(&recorded("bundled")["result"]).unwrap();
    assert!(models.stale);
    assert_eq!(models.models.len(), 1);
    assert_eq!(models.models[0].id, "muse-spark-1.2");
}

#[test]
fn a_hidden_model_a_contributor_by_description_and_a_list_with_no_model_are_read_as_they_say() {
    let result = json!({ "source": "providerCatalog", "models": [
        { "modelId": "muse-image-1.0", "displayLabel": "Muse Image", "visibility": "hidden" },
        { "modelId": "muse-x-1", "variants": [],
          "reasoningEffortVariants": [{ "tier": "low" }, { "tier": "high" }],
          "description": "Your content may be used for product improvement." },
        { "modelId": "" },
    ] });
    let models = parse(&result).unwrap();
    assert_eq!(models.models.len(), 1);
    let model = &models.models[0];
    assert_eq!(
        (model.id.as_str(), model.label.as_str()),
        ("muse-x-1", "muse-x-1")
    );
    assert_eq!(model.efforts, ["low", "high"]);
    assert!(model.contributor, "said so by its description, not its id");
    assert!(parse(&json!({ "models": [] })).is_err());
    assert!(parse(&json!({})).is_err());
}

fn program() -> Program {
    Program {
        executable: PathBuf::from("/home/jane/.local/bin/muse"),
        args: super::msp::serve_args(),
        cwd: None,
        env: Env::default(),
    }
}

/// The lines a host writes for the three requests: its `initialize`, then the list.
fn host(processes: &ScriptedProcesses, lines: Vec<Value>) {
    processes.child(
        "muse",
        ChildScript {
            lines: lines.iter().map(Value::to_string).collect(),
            ends: Ends::Itself,
        },
    );
}

#[test]
fn the_list_is_asked_of_a_host_after_its_initialize_and_the_host_is_ended() {
    let processes = Rc::new(ScriptedProcesses::default());
    let time = Rc::new(ManualTime::new(0));
    let mut list = recorded("account");
    list["id"] = json!(2);
    // A notification and a reply to no request come between: passed over.
    host(
        &processes,
        vec![
            recorded("initialize"),
            json!({ "jsonrpc": "2.0", "method": "model/changed", "params": {} }),
            json!({ "jsonrpc": "2.0", "id": 99, "result": {} }),
            list,
        ],
    );
    let models = finished(Box::pin(read(&*processes, &*time, program()))).unwrap();
    assert_eq!(models.models.len(), 5);
    let written: Vec<Value> = processes
        .take_written()
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let methods: Vec<(&str, Option<u64>)> = written
        .iter()
        .map(|sent| (sent["method"].as_str().unwrap(), sent["id"].as_u64()))
        .collect();
    assert_eq!(
        methods,
        [
            ("initialize", Some(1)),
            ("initialized", None),
            ("model/list", Some(2))
        ]
    );
    let spawned = processes.take_spawned();
    assert_eq!(
        spawned[0].0.args,
        ["serve", "--provider", "meta", "--no-session-log"]
    );
    // Its input and output are this side's, a line at a time: on `Quiet` it takes no input
    // (the first live read, 2026-10-27, failed so where this scripted host could not).
    assert_eq!(spawned[0].1, Streams::Lines);
}

#[test]
fn a_host_that_refuses_ends_or_says_nothing_is_a_reason_and_no_list() {
    let time = Rc::new(ManualTime::new(0));
    // It refuses `model/list`.
    let processes = ScriptedProcesses::default();
    host(
        &processes,
        vec![
            recorded("initialize"),
            json!({ "jsonrpc": "2.0", "id": 2, "error": { "code": -32000, "message": "not logged in" } }),
        ],
    );
    assert_eq!(
        finished(Box::pin(read(&processes, &*time, program()))).unwrap_err(),
        "muse serve refused model/list: not logged in"
    );
    // It ends before it answers.
    let processes = ScriptedProcesses::default();
    host(&processes, vec![recorded("initialize")]);
    assert_eq!(
        finished(Box::pin(read(&processes, &*time, program()))).unwrap_err(),
        "muse serve ended before it answered model/list"
    );
    // It does not start.
    let processes = ScriptedProcesses::default();
    assert!(finished(Box::pin(read(&processes, &*time, program())))
        .unwrap_err()
        .contains("ENOENT"));
}
