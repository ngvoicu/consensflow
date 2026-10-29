//! What the live test of Muse's reads asks (`muse_models`, `muse_usage`): the models its
//! host lists and what `muse usage --json` says of the account, each as the daemon would read
//! them, on the machine's own login. No model turn is run, and no allowance spent.

use cf_proto::agents::Harness;
use serde_json::{json, Value};

use super::env;
use crate::detect::harness_path;
use crate::muse::{models, usage};
use crate::seams::{SystemProcesses, SystemTime};

/// The runtime these asks run in, and where Muse is.
fn muse(
    question: &Value,
) -> Result<
    (
        tokio::runtime::Runtime,
        std::path::PathBuf,
        cf_base::env::Env,
    ),
    String,
> {
    let env = env(question)?;
    let found = harness_path(Harness::Muse, &env).ok_or("Muse Code is not installed")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|failed| failed.to_string())?;
    Ok((runtime, found, env))
}

/// The models Muse lists.
pub(super) fn models(question: &Value) -> Result<Value, String> {
    let (runtime, executable, env) = muse(question)?;
    let processes = SystemProcesses::new(env.clone());
    let listed = runtime.block_on(models::read(
        &processes,
        &SystemTime,
        models::program(executable, &env),
    ))?;
    processes.end_all();
    let rows: Vec<Value> = listed
        .models
        .iter()
        .map(|model| {
            json!({
                "id": model.id,
                "label": model.label,
                "efforts": model.efforts,
                "defaultEffort": model.default_effort,
                "contributor": model.contributor,
                "markedDefault": model.marked_default,
            })
        })
        .collect();
    let warnings: serde_json::Map<String, Value> = listed
        .models
        .iter()
        .filter_map(|model| Some((model.id.clone(), json!(models::warning(model)?))))
        .collect();
    Ok(json!({
        "stale": listed.stale,
        "models": rows,
        "default": listed.default_model().map(|model| model.id.clone()),
        "warnings": warnings,
    }))
}

/// What `muse usage --json` says.
pub(super) fn usage(question: &Value) -> Result<Value, String> {
    let (runtime, executable, env) = muse(question)?;
    let processes = SystemProcesses::new(env.clone());
    let said = runtime.block_on(usage::read(
        &processes,
        &SystemTime,
        usage::program(executable, &env),
    ))?;
    processes.end_all();
    serde_json::to_value(said).map_err(|failed| failed.to_string())
}
