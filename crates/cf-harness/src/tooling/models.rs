//! What the live tests of the harnesses' models ask: the models a harness lists (`models`,
//! CHF-03), read as the daemon reads them, and the model a conversation last ran on
//! (`last_used`, CHF-05), read as a chief's reopen reads it; both through the adapter the
//! registry builds ([`crate::launch::adapter`]), with the machine's own programs and no
//! model turn.

use std::path::Path;
use std::rc::Rc;

use cf_proto::models::{warning, ModelChoice};
use serde_json::{json, Value};

use super::{env, harness, text};
use crate::launch::adapter;
use crate::seams::{SystemProcesses, SystemTime};
use crate::testing::Fakes;

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
    // The adapter's services: the machine's programs and clock; nothing else is used by a read.
    let processes = Rc::new(SystemProcesses::new(env.clone()));
    let mut services = Fakes::new(&env).services(&env, Path::new("."));
    services.processes = Rc::clone(&processes) as _;
    services.time = Rc::new(SystemTime);
    let listed = runtime.block_on(adapter(harness, &services).models(fetch));
    processes.end_all();
    let listing = listed?;
    let warnings: serde_json::Map<String, Value> = listing
        .models
        .iter()
        .filter_map(|model| Some((model.model(), json!(warning(model)?))))
        .collect();
    Ok(json!({
        "listing": listing,
        "default": listing.default_model().map(ModelChoice::model),
        "warnings": warnings,
    }))
}

/// What the conversation `session` of the harness `kind` names last ran on, with the
/// environment `env` names, as a chief's reopen reads it; `asked` is the model its last
/// launch asked for. Null where the record says nothing.
pub(super) fn last_used(question: &Value) -> Result<Value, String> {
    let harness = harness(text(question, "kind")?)?;
    let env = env(question)?;
    let session = text(question, "session")?;
    let asked = question.get("asked").and_then(Value::as_str);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|failed| failed.to_string())?;
    let services = Fakes::new(&env).services(&env, Path::new("."));
    let used = runtime.block_on(adapter(harness, &services).last_used(session, asked));
    Ok(used.map_or(Value::Null, |used| {
        json!({
            "model": used.model,
            "effort": used.effort,
            "thinking": used.thinking,
            "byHarness": used.by_harness,
        })
    }))
}
