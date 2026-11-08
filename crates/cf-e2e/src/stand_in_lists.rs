//! What the rig's stand-in harnesses list of their models when a daemon reads them (A4-28,
//! T-1329). The daemon checks every model a case seats against the list its harness gives, and
//! a stand-in that listed nothing had a bypass in the daemon's own code to stand in for it;
//! now the stand-ins answer, as the harnesses do, and a model nobody lists is refused.
//!
//! A stand-in lists `fake`, `fake-2` and `fake-chief`, the models the rig's roster seats its
//! stand-ins on, and what the roster of the case in the rig's folder (`stand-ins.json`, the
//! case's own and never the daemon's) seats on its harness: a
//! member of the stand-in harness is named by its model, so it runs on the name of its agent.
//! A model of the roster that names an effort (`gpt-6-astra` at `medium`) lists it, and the rest
//! list none. None of them is marked as the harness's default: a launch that names no model is
//! left to run on the harness's own, as before.

use std::path::Path;

use serde_json::{json, Value};

/// The models the roster of a rig seats its stand-ins on.
pub const STAND_IN_MODELS: [&str; 3] = ["fake", "fake-2", "fake-chief"];

/// The stand-in Codex's model the cases name without writing it in the roster: `maia`, GPT-6
/// Astra at `medium` (`rig::project::stand_in`). Codex lists it at the efforts a case may pick.
pub const CODEX_ASTRA: &str = "gpt-6-astra";

/// The file of the roster, in the rig's folder beside the home.
pub const ROSTER: &str = "stand-ins.json";

/// One model a stand-in lists, and the efforts it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub id: String,
    pub efforts: Vec<String>,
}

/// What the stand-in of `kind` (the harness's kind: `claude-code`, `codex`, `devin`, `pi`)
/// lists, in the rig whose daemon home is `consensflow_home` (`CONSENSFLOW_HOME`, the folder
/// `consensflow` beside the roster).
pub fn listed(kind: &str, consensflow_home: &Path) -> Vec<Listed> {
    let roster = consensflow_home
        .parent()
        .map(|rig| rig.join(ROSTER))
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    of_roster(kind, roster.as_ref())
}

/// [`listed`] for the roster `roster` (none where the rig has written none).
pub fn of_roster(kind: &str, roster: Option<&Value>) -> Vec<Listed> {
    let mut models: Vec<Listed> = STAND_IN_MODELS
        .iter()
        .map(|id| Listed {
            id: (*id).to_owned(),
            efforts: Vec::new(),
        })
        .collect();
    if kind == "codex" {
        models.push(Listed {
            id: CODEX_ASTRA.to_owned(),
            efforts: ["low", "medium", "high"].map(str::to_owned).to_vec(),
        });
    }
    let rows = roster
        .and_then(|roster| roster.get("agents"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| row["kind"] == kind && row["designer"] != true);
    for row in rows {
        let efforts: Vec<String> = ["effort", "thinking"]
            .into_iter()
            .find_map(|key| row.get(key).and_then(Value::as_str))
            .map(str::to_owned)
            .into_iter()
            .collect();
        // The model the row runs on is the chief's pick; a member on the stand-in's own model
        // is asked for by the name of its agent.
        let model = row["model"].as_str();
        let named = row["id"]
            .as_str()
            .filter(|_| model.is_none_or(|model| STAND_IN_MODELS.contains(&model)));
        for id in model.into_iter().chain(named) {
            match models.iter_mut().find(|listed| listed.id == id) {
                Some(listed) => {
                    for effort in &efforts {
                        if !listed.efforts.contains(effort) {
                            listed.efforts.push(effort.clone());
                        }
                    }
                }
                None => models.push(Listed {
                    id: id.to_owned(),
                    efforts: efforts.clone(),
                }),
            }
        }
    }
    models
}

/// The answer of `claude -p --input-format stream-json` to the `initialize` control request
/// `request` (`request_id`, which the answer repeats): the SDK's `models[]`.
pub fn claude_answer(request_id: &str, models: &[Listed]) -> Value {
    let rows: Vec<Value> = models
        .iter()
        .map(|model| {
            json!({
                "value": model.id,
                "displayName": model.id,
                "resolvedModel": model.id,
                "supportsEffort": !model.efforts.is_empty(),
                "supportedEffortLevels": model.efforts,
            })
        })
        .collect();
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": { "models": rows },
        },
    })
}

/// The `result` of `codex app-server` to `model/list`: one page.
pub fn codex_page(models: &[Listed]) -> Value {
    let rows: Vec<Value> = models
        .iter()
        .map(|model| {
            let efforts: Vec<Value> = model
                .efforts
                .iter()
                .map(|effort| json!({ "reasoningEffort": effort }))
                .collect();
            json!({
                "id": model.id,
                "model": model.id,
                "displayName": model.id,
                "hidden": false,
                "supportedReasoningEfforts": efforts,
            })
        })
        .collect();
    json!({ "data": rows, "nextCursor": null })
}

/// What `devin models list --format json` prints: a family for each model, a variant for each
/// of its efforts (`<model>-<effort>`, Devin's id for it) or the model itself where it has none.
pub fn devin_families(models: &[Listed]) -> Value {
    let families: Vec<Value> = models
        .iter()
        .map(|model| {
            let variants: Vec<Value> = if model.efforts.is_empty() {
                vec![json!({ "model_uid": model.id, "label": model.id })]
            } else {
                model
                    .efforts
                    .iter()
                    .map(|effort| {
                        json!({
                            "model_uid": format!("{}-{effort}", model.id),
                            "label": format!("{} {effort}", model.id),
                        })
                    })
                    .collect()
            };
            json!({ "family_uid": model.id, "slug": model.id, "variants": variants })
        })
        .collect();
    json!({ "families": families })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(models: &[Listed]) -> Vec<&str> {
        models.iter().map(|model| model.id.as_str()).collect()
    }

    #[test]
    fn a_rig_with_no_roster_lists_the_models_the_stand_ins_run_on() {
        assert_eq!(
            ids(&of_roster("claude-code", None)),
            ["fake", "fake-2", "fake-chief"]
        );
        // Codex also lists the model of `maia`, whom cases name without writing her down.
        assert_eq!(
            ids(&of_roster("codex", None)),
            ["fake", "fake-2", "fake-chief", "gpt-6-astra"]
        );
    }

    #[test]
    fn a_member_on_the_stand_ins_model_runs_on_the_name_of_its_agent_and_a_real_model_on_itself() {
        let roster = json!({ "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "mybuilder", "kind": "claude-code", "model": "fake" },
            { "id": "maia", "kind": "codex", "model": "gpt-6-astra", "effort": "medium" },
            { "id": "zed", "kind": "codex", "model": "fake" },
            { "id": "drawer", "kind": "codex", "designer": true },
        ]});
        assert_eq!(
            ids(&of_roster("claude-code", Some(&roster))),
            ["fake", "fake-2", "fake-chief", "chief", "mybuilder"]
        );
        let codex = of_roster("codex", Some(&roster));
        assert_eq!(
            ids(&codex),
            ["fake", "fake-2", "fake-chief", "gpt-6-astra", "zed"]
        );
        let astra = codex.iter().find(|model| model.id == "gpt-6-astra").unwrap();
        assert_eq!(astra.efforts, ["low", "medium", "high"]);
        assert!(
            of_roster("devin", Some(&roster))
                .iter()
                .all(|model| model.efforts.is_empty()),
            "the roster of another harness gives it nothing"
        );
    }

    #[test]
    fn each_stand_in_answers_in_its_harnesss_shape() {
        let models = [Listed {
            id: "swe-2".to_owned(),
            efforts: vec!["low".to_owned(), "high".to_owned()],
        }];
        let claude = claude_answer("models", &models);
        assert_eq!(claude["response"]["request_id"], "models");
        assert_eq!(
            claude["response"]["response"]["models"][0]["supportedEffortLevels"],
            json!(["low", "high"])
        );
        let codex = codex_page(&models);
        assert_eq!(codex["data"][0]["model"], "swe-2");
        assert_eq!(
            codex["data"][0]["supportedReasoningEfforts"][1]["reasoningEffort"],
            "high"
        );
        let devin = devin_families(&models);
        assert_eq!(
            devin["families"][0]["variants"][1]["model_uid"],
            "swe-2-high"
        );
    }
}
