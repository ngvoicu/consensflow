//! A chief is a harness and a model (CHF-04): the model the human picked for it is the
//! chief's own row's, never a saved agent's; a Switch chief names the harness and the model
//! on both sides; the model a chief's launch asked for is kept beside it, for the next
//! reopen to tell a move the harness made by itself from the human's (CHF-05). A ledger from
//! before, whose chief names a saved agent and no model, opens as it was.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_ledger::{
    migrate, open_ledger, ChiefSwitch, Ledger, NewChief, NewProject, Options, MIGRATIONS,
};
use cf_proto::ledger::{LastSwitch, ParticipantView, SwitchedFrom};
use rusqlite::Connection;
use serde_json::{json, Value};

/// A Pi model whose id holds a `/` of its own: its provider, then OpenRouter's id.
const PI_MODEL: &str = "openrouter/anthropic/claude-fable-5";

fn project(harness: &str, model: Option<&str>) -> NewProject {
    NewProject {
        directory: "/work/site".to_owned(),
        name: "site".to_owned(),
        chief: NewChief {
            harness: harness.to_owned(),
            model: model.map(str::to_owned),
            effort: None,
        },
        staff: Vec::new(),
        gate: false,
        wake_up: None,
        specs: None,
    }
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).unwrap()
}

fn chief(ledger: &Ledger, project: i64) -> ParticipantView {
    ledger
        .project(project)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .find(|participant| participant.handle == "chief")
        .unwrap()
}

/// The data of the project's `chief.switched` events, oldest first.
fn switches(ledger: &Ledger, project: i64) -> Vec<Value> {
    ledger
        .events(project, 0, 500)
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == "chief.switched")
        .map(|event| event.data)
        .collect()
}

#[test]
fn a_new_chief_is_its_harness_and_the_model_picked_for_it_and_no_saved_agent() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = ledger_in(&dir);
    let project = ledger
        .create_project(&project("pi", Some(PI_MODEL)))
        .unwrap();
    let chief = chief(&ledger, project.id);
    assert_eq!(
        (
            chief.harness.as_deref(),
            chief.model.as_deref(),
            chief.agent
        ),
        (Some("pi"), Some(PI_MODEL), None)
    );
}

#[test]
fn a_new_project_as_json_names_its_chief_by_harness_and_model() {
    let request = NewProject::from_json(&json!({
        "directory": "/work/site",
        "name": "site",
        "chief": { "harness": "pi", "model": PI_MODEL },
    }))
    .unwrap();
    assert_eq!(
        request.chief,
        NewChief {
            harness: "pi".to_owned(),
            model: Some(PI_MODEL.to_owned()),
            effort: None,
        }
    );
}

#[test]
fn a_model_id_is_what_a_harness_names_its_models_and_never_a_flag_or_words() {
    for model in [
        PI_MODEL,
        "~unbiased/pareto-latest",
        "claude-opus-5-5[1m]",
        "gpt-6.1-sol",
        "swe-2",
        "anthropic/claude-haiku-4.5",
    ] {
        assert_eq!(
            cf_ledger::model::require_model_id(model).map_err(|refused| refused.to_string()),
            Ok(()),
            "{model}"
        );
    }
    let long = "m".repeat(257);
    for model in [
        "",
        "-p",
        "--model",
        "two words",
        "a\nb",
        "a\u{7f}",
        long.as_str(),
        "é",
    ] {
        let refused = cf_ledger::model::require_model_id(model).unwrap_err();
        assert_eq!(refused.code(), Some("invalid-model"), "{model:?}");
    }
    let refused = NewProject::from_json(&json!({
        "directory": "/work/site",
        "name": "site",
        "chief": { "harness": "pi", "model": "--help" },
    }))
    .unwrap_err();
    assert_eq!(refused.code(), Some("invalid-model"));
}

#[test]
fn a_switch_names_the_harness_and_the_model_it_leaves_and_the_ones_it_takes() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = ledger_in(&dir);
    let project = ledger
        .create_project(&project("claude-code", Some("claude-opus-5-5")))
        .unwrap();
    let switch = ChiefSwitch::from_json(&json!({
        "harness": "pi",
        "model": PI_MODEL,
        "cut": true,
    }))
    .unwrap();
    ledger.switch_chief(project.id, &switch).unwrap();
    let chief = chief(&ledger, project.id);
    assert_eq!(
        (
            chief.harness.as_deref(),
            chief.model.as_deref(),
            chief.agent
        ),
        (Some("pi"), Some(PI_MODEL), None)
    );
    assert_eq!(
        ledger.last_switch(project.id).unwrap(),
        Some(LastSwitch {
            from: SwitchedFrom {
                harness: "claude-code".to_owned(),
                model: Some("claude-opus-5-5".to_owned()),
            },
            cut: true,
        })
    );
    assert_eq!(
        switches(&ledger, project.id),
        vec![json!({
            "from": { "harness": "claude-code", "model": "claude-opus-5-5" },
            "to": { "harness": "pi", "model": PI_MODEL },
            "cut": true,
        })]
    );
}

#[test]
fn a_switch_needs_a_model_the_way_a_harness_names_one() {
    for (switch, code) in [
        (json!({ "harness": "pi" }), "invalid-model"),
        (json!({ "harness": "pi", "model": null }), "invalid-model"),
        (json!({ "harness": "pi", "model": "-x" }), "invalid-model"),
        (
            json!({ "harness": "nope", "model": "m" }),
            "invalid-harness",
        ),
    ] {
        let refused = ChiefSwitch::from_json(&switch).unwrap_err();
        assert_eq!(refused.code(), Some(code), "{switch}");
    }
}

#[test]
fn the_model_a_chiefs_launch_asked_for_is_kept_apart_from_the_one_picked_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = ledger_in(&dir);
    let project = ledger
        .create_project(&project("claude-code", Some("claude-opus-5-5")))
        .unwrap();
    let id = chief(&ledger, project.id).id;
    assert_eq!(chief(&ledger, project.id).launched_model, None);
    ledger
        .chief_launched(id, Some("claude-sonnet-5-5"))
        .unwrap();
    let after = chief(&ledger, project.id);
    assert_eq!(
        (after.model.as_deref(), after.launched_model.as_deref()),
        (Some("claude-opus-5-5"), Some("claude-sonnet-5-5"))
    );
    // What its card says it runs on: the model its window ran on where it is not the pick.
    assert_eq!(after.ran_on.as_deref(), Some("claude-sonnet-5-5"));
    ledger.chief_launched(id, Some("claude-opus-5-5")).unwrap();
    assert_eq!(
        chief(&ledger, project.id).ran_on,
        None,
        "the pick: nothing more said"
    );
    // A launch on the harness's own default asks for none.
    ledger.chief_launched(id, None).unwrap();
    assert_eq!(chief(&ledger, project.id).launched_model, None);
    // A Switch chief starts a fresh conversation, whose first launch has asked for nothing.
    ledger
        .chief_launched(id, Some("claude-sonnet-5-5"))
        .unwrap();
    let switch =
        ChiefSwitch::from_json(&json!({ "harness": "codex", "model": "gpt-6-luna" })).unwrap();
    ledger.switch_chief(project.id, &switch).unwrap();
    assert_eq!(chief(&ledger, project.id).launched_model, None);
}

#[test]
fn a_gone_model_is_kept_apart_and_the_harnesss_default_becomes_the_chiefs_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = ledger_in(&dir);
    let project = ledger
        .create_project(&project("codex", Some("gpt-gone-9")))
        .unwrap();
    let id = chief(&ledger, project.id).id;
    ledger.chief_launched(id, Some("gpt-gone-9")).unwrap();
    ledger
        .chief_gone(id, "gpt-gone-9", Some("gpt-6-luna"))
        .unwrap();
    let after = chief(&ledger, project.id);
    assert_eq!(
        (
            after.model.as_deref(),
            after.gone_model.as_deref(),
            after.launched_model
        ),
        (Some("gpt-6-luna"), Some("gpt-gone-9"), None)
    );
    // Left to the harness's own default: no model of the chief's.
    ledger.chief_gone(id, "gpt-6-luna", None).unwrap();
    assert_eq!(chief(&ledger, project.id).model, None);
    let refused = ledger
        .chief_gone(id, "gpt-6-luna", Some("not a model"))
        .unwrap_err();
    assert_eq!(refused.code(), Some("invalid-model"));
    // A Switch chief forgets it: the model picked then is the human's word.
    let switch =
        ChiefSwitch::from_json(&json!({ "harness": "codex", "model": "gpt-gone-9" })).unwrap();
    ledger.switch_chief(project.id, &switch).unwrap();
    assert_eq!(chief(&ledger, project.id).gone_model, None);
}

/// The rows a ledger from before migration 0026 holds of a chief switched once: a Claude
/// Code chief on no agent, switched to a Codex one on a saved agent (`chief-switch.sql`'s).
const OLD_CHIEF: &str = "
  INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
    VALUES (1, '/work/site', 'site', 'open', 0, '2026-09-19T10:00:01.000Z', '2026-09-19T10:00:01.000Z');
  INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, created_at,
      switched_from_harness, switched_from_agent, switched_from_cut)
    VALUES (1, 1, 'human', 'human', '[]', NULL, NULL, '2026-09-19T10:00:02.000Z', NULL, NULL, 0),
           (2, 1, 'chief', 'chief', '[]', 'eval-codex-worker', 'codex', '2026-09-19T10:00:03.000Z',
            'claude-code', NULL, 0);
  INSERT INTO event (id, project_id, at, kind, data) VALUES (1, 1, '2026-09-19T10:00:09.000Z',
    'chief.switched', '{\"from\":{\"harness\":\"claude-code\",\"agent\":null},\"to\":{\"harness\":\"codex\",\"agent\":\"eval-codex-worker\"},\"cut\":false}');
";

#[test]
fn a_ledger_whose_chief_names_a_saved_agent_and_no_model_opens_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    {
        let db = Connection::open(&file).unwrap();
        migrate(&db, &MIGRATIONS[..25]).unwrap();
        db.execute_batch(OLD_CHIEF).unwrap();
    }
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    let chief = chief(&ledger, 1);
    // Its row as it was: its agent kept, no model, nothing launched yet; its reopen reads
    // its record, never the roster (CHF-04).
    assert_eq!(
        (
            chief.harness.as_deref(),
            chief.agent.as_deref(),
            chief.model,
            chief.launched_model,
            chief.gone_model
        ),
        (Some("codex"), Some("eval-codex-worker"), None, None, None)
    );
    assert_eq!(
        ledger.last_switch(1).unwrap(),
        Some(LastSwitch {
            from: SwitchedFrom {
                harness: "claude-code".to_owned(),
                model: None,
            },
            cut: false,
        })
    );
    // The next switch writes the new shape beside the old event, which stays as it was.
    let switch =
        ChiefSwitch::from_json(&json!({ "harness": "claude-code", "model": "claude-opus-5-5" }))
            .unwrap();
    ledger.switch_chief(1, &switch).unwrap();
    assert_eq!(
        switches(&ledger, 1),
        vec![
            json!({
                "from": { "harness": "claude-code", "agent": null },
                "to": { "harness": "codex", "agent": "eval-codex-worker" },
                "cut": false,
            }),
            // An old chief's switch keeps the saved agent it ran on, as the event and the
            // handoff named it before (Calliope's P3, T-955).
            json!({
                "from": { "harness": "codex", "model": null, "agent": "eval-codex-worker" },
                "to": { "harness": "claude-code", "model": "claude-opus-5-5" },
                "cut": false,
            }),
        ]
    );
    let after = self::chief(&ledger, 1);
    assert_eq!(
        (after.agent, after.model.as_deref()),
        (None, Some("claude-opus-5-5"))
    );
    // The handoff says "from Codex (eval-codex-worker)", as it did.
    assert_eq!(
        ledger.last_switch(1).unwrap(),
        Some(LastSwitch {
            from: SwitchedFrom {
                harness: "codex".to_owned(),
                model: Some("eval-codex-worker".to_owned()),
            },
            cut: false,
        })
    );
}
