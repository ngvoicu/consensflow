//! What the live test of the harnesses' model lists asks (`models`, CHF-03): the models a
//! harness lists, read as the daemon reads them ([`crate::contract::Adapter::models`]),
//! with the machine's own programs and no model turn.

use cf_proto::agents::Harness;
use cf_proto::models::{warning, Models};
use serde_json::{json, Value};

use super::{env, harness, text};
use crate::seams::{SystemProcesses, SystemTime};

/// The models the harness `kind` names lists, with the environment `env` names; `fetch`
/// is the human's Refresh.
pub(super) fn models(question: &Value) -> Result<Value, String> {
    let harness = harness(text(question, "kind")?)?;
    let env = env(question)?;
    let fetch = question.get("fetch") == Some(&Value::Bool(true));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|failed| failed.to_string())?;
    let processes = SystemProcesses::new(env.clone());
    let time = SystemTime;
    let listed: Result<Models, String> = runtime.block_on(async {
        match harness {
            Harness::Claude => crate::claude::models::listed(&env, &processes, &time).await,
            Harness::Codex => crate::codex::models::listed(&env, &processes, &time).await,
            Harness::Pi => crate::pi::models::listed(&env, &processes, &time, fetch).await,
            Harness::Devin => crate::devin::models::listed(&env, &processes).await,
            Harness::Opencode => Err("ConsensFlow cannot read this harness's models".to_owned()),
        }
    });
    processes.end_all();
    let listed = listed?;
    let warnings: serde_json::Map<String, Value> = listed
        .models
        .iter()
        .filter_map(|model| Some((model.model(), json!(warning(model)?))))
        .collect();
    Ok(json!({
        "models": listed.models,
        "stale": listed.stale,
        "how": listed.how,
        "default": listed.default_model().map(cf_proto::models::ModelChoice::model),
        "warnings": warnings,
    }))
}
