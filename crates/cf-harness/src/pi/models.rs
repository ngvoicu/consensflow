//! The models Pi offers and the thinking levels each takes, as its rpc mode lists them
//! (`get_available_models`): one bounded `pi --mode rpc --offline`, no turn (research-02
//! §2: 469 models in about 300 KB, 0.3 s). Offline, Pi answers from its bundled catalog
//! and the copy it keeps, and fetches nothing: a fresh catalog is the human's Refresh,
//! which runs `pi update --models` first ([`UPDATE`]).
//!
//! Pi lists only the providers it holds a login for, so a model is a provider and an id,
//! kept apart (an OpenRouter id holds a `/` of its own): `--model` takes them joined.

use std::path::PathBuf;
use std::time::Duration;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use cf_proto::models::{ModelChoice, ModelListing};
use serde_json::{json, Value};

use crate::detect::executable;
use crate::seams::processes::{Limits, Processes, Program};
use crate::seams::Time;
use crate::shared::one_shot::{ask, conversation, BOUND_MS};

/// How the list is read, as the human could run it.
pub const HOW: &str = "pi --mode rpc --offline: get_available_models";

/// What the human's Refresh runs first: the catalogs fetched again.
pub const UPDATE: [&str; 2] = ["update", "--models"];

/// How long the catalogs' fetch may take, and how much it may say.
pub const UPDATE_LIMITS: Limits = Limits {
    timeout: Duration::from_secs(60),
    max_buffer: 1 << 20,
};

/// Pi's thinking levels, from the lowest (`ThinkingLevel` of `pi-ai`).
const LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// The program the list is read from: `pi` at `executable` in rpc mode, offline, with no
/// session, extension or MCP server of its own.
pub fn program(executable: PathBuf, env: &Env) -> Program {
    let args = [
        "--mode",
        "rpc",
        "--no-session",
        "--no-extensions",
        "--no-mcp",
        "--offline",
    ];
    Program {
        executable,
        args: args.map(str::to_owned).to_vec(),
        cwd: None,
        env: env.clone().with_var("PI_OFFLINE", "1"),
    }
}

/// The levels `model` takes, by Pi's own rule (`getSupportedThinkingLevels`): `off` alone
/// for a model with no reasoning; else each level its `thinkingLevelMap` does not map to
/// null, `xhigh` and `max` only where the map names them.
fn levels(model: &Value) -> Vec<String> {
    if model.get("reasoning") != Some(&Value::Bool(true)) {
        return vec!["off".to_owned()];
    }
    let map = model.get("thinkingLevelMap").and_then(Value::as_object);
    LEVELS
        .into_iter()
        .filter(|level| match map.and_then(|map| map.get(*level)) {
            Some(Value::Null) => false,
            Some(_) => true,
            None => !matches!(*level, "xhigh" | "max"),
        })
        .map(str::to_owned)
        .collect()
}

/// What a `get_available_models` answer lists, read at `at`. Fails where Pi refused, or
/// lists no model (it holds no login for any provider).
pub fn parse(answer: &Value, at: i64) -> Result<ModelListing, String> {
    if answer.get("success") == Some(&Value::Bool(false)) {
        let said = answer["error"].as_str().unwrap_or("no reason given");
        return Err(format!("pi refused get_available_models: {said}"));
    }
    let rows = answer["data"]["models"]
        .as_array()
        .ok_or("Pi's answer has no models")?;
    let text = |row: &Value, name: &str| row.get(name).and_then(Value::as_str).map(str::to_owned);
    let models: Vec<ModelChoice> = rows
        .iter()
        .filter_map(|row| {
            let id = text(row, "id").filter(|id| !id.is_empty())?;
            let provider = text(row, "provider").filter(|provider| !provider.is_empty())?;
            Some(ModelChoice {
                provider: Some(provider),
                label: text(row, "name").unwrap_or_else(|| id.clone()),
                efforts: levels(row),
                ..ModelChoice::new(id.clone(), "")
            })
        })
        .collect();
    if models.is_empty() {
        return Err("Pi lists no model: it holds no login for any provider".to_owned());
    }
    Ok(ModelListing::read(models, at, HOW))
}

/// The models `program` lists: one bounded conversation, no turn.
pub async fn read(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
) -> Result<ModelListing, String> {
    let at = time.wall_ms();
    let request = json!({ "id": "models", "type": "get_available_models" });
    let answer = conversation(
        processes,
        time,
        program,
        "pi --mode rpc",
        BOUND_MS,
        async |child| {
            ask(child, "pi --mode rpc", &request, |line| {
                line["type"] == "response" && line["id"] == "models"
            })
            .await
        },
    )
    .await?;
    parse(&answer, at)
}

/// The models the Pi installed here lists. Where `fetch` (the human's Refresh), its
/// catalogs are fetched first ([`UPDATE`]): a fetch that fails leaves the list it keeps,
/// which is then stale, with why.
pub(crate) async fn listed(
    env: &Env,
    processes: &dyn Processes,
    time: &dyn Time,
    fetch: bool,
) -> Result<ModelListing, String> {
    let executable = PathBuf::from(executable(Harness::Pi, env)?);
    let mut fetched = Ok(());
    if fetch {
        let update = Program {
            executable: executable.clone(),
            args: UPDATE.map(str::to_owned).to_vec(),
            cwd: None,
            env: env.clone(),
        };
        fetched = processes.run(update, UPDATE_LIMITS).await.map(|_| ());
    }
    let listing = read(processes, time, program(executable, env)).await?;
    Ok(match fetched {
        Ok(()) => listing,
        Err(failed) => listing.stale(format!(
            "Pi's own catalog: pi update --models failed: {}",
            failed.message.lines().last().unwrap_or_default()
        )),
    })
}

/// Whether `words` are Pi's provider refusing a model it does not have: Pi sends an id it
/// does not know, and the provider answers 400 `model_not_found` (1.4.0, CHF-02). Pi's own
/// "Model … not found for provider …, using custom model id" is a warning of its offline
/// catalog, which runs the id all the same: never a refusal (Calliope's P1, T-955).
pub(crate) fn gone(words: &str) -> bool {
    words.contains("model_not_found")
}

/// The model Pi starts on where none is named: its settings' default
/// (`defaultProvider`/`defaultModel` in its folder's `settings.json`), what its own launch
/// with no `--model` runs; where that is a contributor model, or there is none, the first
/// its list offers that is not (`ModelListing::default_model`). Never a contributor model
/// (CHF-06, MUS-15): where none can be had, why.
pub(crate) async fn default_model(
    env: &Env,
    processes: &dyn Processes,
    time: &dyn Time,
) -> Result<Option<String>, String> {
    let settings = super::paths::agent_dir(env)
        .ok()
        .and_then(|dir| std::fs::read_to_string(cf_base::path::join(&[&dir, "settings.json"])).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let named = settings.as_ref().and_then(|settings| {
        let provider = settings["defaultProvider"].as_str()?;
        let model = settings["defaultModel"].as_str()?;
        Some(format!("{provider}/{model}"))
    });
    if let Some(named) = named.filter(|named| !cf_proto::models::is_contributor(named)) {
        return Ok(Some(named));
    }
    let listing = listed(env, processes, time, false).await?;
    listing
        .default_model()
        .map(|model| Some(model.model()))
        .ok_or_else(|| "Pi offers no model that is not a contributor model".to_owned())
}

#[cfg(test)]
mod tests;
