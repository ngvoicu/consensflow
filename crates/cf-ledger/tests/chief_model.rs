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
            model_name: None,
        },
        staff: Vec::new(),
        gate: false,
        wake_up: None,
        specs: None,
        staff_rules: None,
        compaction: None,
        advisor_tool: false,
        prompt_suggestion: false,
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

/// The id a chief's harness lists for its model (`model_name`, which its card says) goes with
/// that model: when the model moves with no pick of the human's, because its harness no longer
/// has it (`chief_gone`) or because the harness's record names the model it ran on
/// (`chief_ran_on`), the name of the old model is not the new one's and is dropped (T-1345).
#[test]
fn a_chief_whose_model_moves_without_a_pick_keeps_no_name_of_the_old_model() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    {
        // Two projects whose Devin chiefs hold the id their launch is given and the id Devin lists.
        let db = Connection::open(&file).unwrap();
        migrate(&db, &MIGRATIONS).unwrap();
        db.execute_batch(
            "INSERT INTO project (id, directory, name, state, created_at, updated_at)
               VALUES (1, '/a', 'a', 'open', 't', 't'), (2, '/b', 'b', 'open', 't', 't');
             INSERT INTO participant (id, project_id, handle, role, roles, harness, model, effort, model_name, created_at)
               VALUES (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, 't'),
                      (2, 1, 'chief', 'chief', '[]', 'devin', 'swe-2-high', 'high', 'swe-2', 't'),
                      (3, 2, 'human', 'human', '[]', NULL, NULL, NULL, NULL, 't'),
                      (4, 2, 'chief', 'chief', '[]', 'devin', 'swe-2-high', 'high', 'swe-2', 't');",
        )
        .unwrap();
    }
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    let name_of = |ledger: &Ledger, project: i64| {
        let chief = chief(ledger, project);
        (chief.model, chief.model_name)
    };
    for project in [1, 2] {
        assert_eq!(
            name_of(&ledger, project),
            (Some("swe-2-high".to_owned()), Some("swe-2".to_owned()))
        );
    }
    ledger
        .chief_gone(chief(&ledger, 1).id, "swe-2-high", Some("adaptive"))
        .unwrap();
    assert_eq!(
        name_of(&ledger, 1),
        (Some("adaptive".to_owned()), None),
        "its harness no longer has the model: the name of the old one goes"
    );
    ledger
        .chief_ran_on(chief(&ledger, 2).id, "swe-2-medium", None)
        .unwrap();
    assert_eq!(
        name_of(&ledger, 2),
        (Some("swe-2-medium".to_owned()), None),
        "the record names another model: the name of the old one goes"
    );
}

/// A chief holds the id its harness lists for its model beside the id it is launched on (T-1345):
/// a new project's, as JSON gives it or typed, and a switch's, which replaces the last. A name goes
/// with a model: a chief on its harness's own default holds none. A name that is no model's name is
/// refused whole, by all three, and leaves no project and the chief as it was.
#[test]
fn a_chief_holds_the_name_its_harness_lists_for_its_model_from_a_new_project_and_a_switch() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = ledger_in(&dir);
    let devin = |model_name: Option<&str>, model: Option<&str>| NewProject {
        chief: NewChief {
            harness: "devin".to_owned(),
            model: model.map(str::to_owned),
            effort: Some("high".to_owned()),
            model_name: model_name.map(str::to_owned),
        },
        ..project("devin", None)
    };
    let name = |ledger: &Ledger, project: i64| {
        let chief = chief(ledger, project);
        (chief.model, chief.model_name)
    };
    // As JSON gives it, and as it is typed.
    let request = NewProject::from_json(&json!({
        "directory": "/work/site", "name": "site",
        "chief": { "harness": "devin", "model": "swe-2-high", "effort": "high", "modelName": "swe-2" },
    }))
    .unwrap();
    assert_eq!(request.chief.model_name.as_deref(), Some("swe-2"));
    let opened = ledger.create_project(&request).unwrap();
    let typed = ledger
        .create_project(&devin(Some("swe-2"), Some("swe-2-high")))
        .unwrap();
    for project in [opened.id, typed.id] {
        assert_eq!(
            name(&ledger, project),
            (Some("swe-2-high".to_owned()), Some("swe-2".to_owned()))
        );
    }
    // A switch as JSON gives it.
    let asked = ChiefSwitch::from_json(&json!({
        "harness": "devin", "model": "swe-2-high", "effort": "high", "modelName": "swe-2",
    }))
    .unwrap();
    assert_eq!(asked.model_name.as_deref(), Some("swe-2"));
    // A name goes with a model: none on the harness's own default.
    let default = ledger.create_project(&devin(Some("swe-2"), None)).unwrap();
    assert_eq!(name(&ledger, default.id), (None, None));
    // A switch replaces it with the new pick's, and with none where the pick has none.
    let switch = |model: &str, model_name: Option<&str>| ChiefSwitch {
        harness: "claude-code".to_owned(),
        model: model.to_owned(),
        effort: None,
        model_name: model_name.map(str::to_owned),
        cut: false,
    };
    ledger
        .switch_chief(
            opened.id,
            &switch("claude-opus-5-5", Some("claude-opus-5-5")),
        )
        .unwrap();
    assert_eq!(
        name(&ledger, opened.id),
        (
            Some("claude-opus-5-5".to_owned()),
            Some("claude-opus-5-5".to_owned())
        )
    );
    ledger
        .switch_chief(opened.id, &switch("claude-sonnet-5-5", None))
        .unwrap();
    assert_eq!(
        name(&ledger, opened.id),
        (Some("claude-sonnet-5-5".to_owned()), None),
        "the name of the model it left does not stay"
    );
    // A name that is no model's name.
    let before = ledger.projects().unwrap().len();
    for bad in ["a\nb", ""] {
        let refused = NewProject::from_json(&json!({
            "directory": "/work/site", "name": "site",
            "chief": { "harness": "devin", "model": "swe-2-high", "modelName": bad },
        }))
        .unwrap_err();
        assert_eq!(refused.code(), Some("invalid-model-name"), "{bad:?}");
        let refused = ledger
            .create_project(&devin(Some(bad), Some("swe-2-high")))
            .unwrap_err();
        assert_eq!(refused.code(), Some("invalid-model-name"), "{bad:?}");
        let refused = ledger
            .switch_chief(opened.id, &switch("claude-opus-5-5", Some(bad)))
            .unwrap_err();
        assert_eq!(refused.code(), Some("invalid-model-name"), "{bad:?}");
    }
    assert_eq!(ledger.projects().unwrap().len(), before);
    assert_eq!(
        name(&ledger, opened.id),
        (Some("claude-sonnet-5-5".to_owned()), None),
        "a refused switch leaves the chief as it was"
    );
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
            model_name: None,
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

/// The rows a ledger from before migration 0028 holds of a chief switched once: a Claude
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
        migrate(&db, &MIGRATIONS[..27]).unwrap();
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
