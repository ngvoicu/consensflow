//! The models Claude Code offers and the efforts each takes, as its SDK's `initialize`
//! control request lists them (`models[]`, the Agent SDK's `supportedModels()`): one
//! bounded `claude -p` over stream-json, no user turn, so no model request and no tokens
//! (research-02 §4: 0.4–3.4 s, the same list signed in, signed out and offline).
//!
//! It runs in a config folder of ConsensFlow's own, never the human's `~/.claude`: an
//! `initialize` may write its config, and the list is the same without a login. So the
//! human's own `availableModels` and an organisation's picker policy are not applied.
//!
//! Each row's id is its `resolvedModel` (`claude-opus-5-5`), what a launch passes: an
//! alias (`opus`) moves with Claude's releases. The `default` row is a pointer, not a
//! model: it is left out, and the model it resolves to is the one marked default.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_proto::agents::Harness;
use cf_proto::models::{ModelChoice, ModelListing};
use serde_json::{json, Value};

use crate::contract::NoDefault;
use crate::detect::executable;
use crate::seams::processes::{Processes, Program};
use crate::seams::Time;
use crate::shared::one_shot::{ask, conversation, BOUND_MS};

/// How the list is read, as the human could run it.
pub(crate) const HOW: &str =
    "claude -p --input-format stream-json: the SDK's initialize control request";

/// The program the list is read from: `claude` at `executable`, in the config folder
/// `config`, with nothing it need not fetch.
pub(crate) fn program(executable: PathBuf, env: &Env, config: &Path) -> Program {
    let args = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--strict-mcp-config",
        "--no-chrome",
        "--no-session-persistence",
    ];
    Program {
        executable,
        args: args.map(str::to_owned).to_vec(),
        cwd: Some(config.to_path_buf()),
        env: env
            .clone()
            .with_var("CLAUDE_CONFIG_DIR", &config.to_string_lossy())
            .with_var("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
    }
}

/// What the `initialize` answer's `response` lists, read at `at`. Fails where it lists no
/// model.
pub(crate) fn parse(response: &Value, at: i64) -> Result<ModelListing, String> {
    let rows = response
        .get("models")
        .and_then(Value::as_array)
        .ok_or("Claude Code's initialize answer has no models")?;
    let text = |row: &Value, name: &str| row.get(name).and_then(Value::as_str).map(str::to_owned);
    let pointed = rows
        .iter()
        .find(|row| text(row, "value").as_deref() == Some("default"))
        .and_then(|row| text(row, "resolvedModel"));
    let mut models: Vec<ModelChoice> = Vec::new();
    for row in rows {
        let Some(value) = text(row, "value").filter(|value| value != "default") else {
            continue;
        };
        let id = text(row, "resolvedModel").unwrap_or(value);
        if models.iter().any(|model| model.id == id) {
            continue;
        }
        let efforts = if row.get("supportsEffort") == Some(&Value::Bool(false)) {
            Vec::new()
        } else {
            words(row.get("supportedEffortLevels"))
        };
        models.push(ModelChoice {
            label: text(row, "displayName").unwrap_or_else(|| id.clone()),
            efforts,
            marked_default: pointed.as_deref() == Some(id.as_str()),
            disabled: row.get("disabled") == Some(&Value::Bool(true)),
            ..ModelChoice::new(id.clone(), "")
        });
    }
    if models.is_empty() {
        return Err("Claude Code lists no model".to_owned());
    }
    Ok(ModelListing::read(models, at, HOW))
}

/// The strings of a list.
pub(crate) fn words(list: Option<&Value>) -> Vec<String> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|word| word.as_str().map(str::to_owned))
        .collect()
}

/// The models `program` lists: one bounded conversation, no turn.
pub(crate) async fn read(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
) -> Result<ModelListing, String> {
    let at = time.wall_ms();
    let request = json!({
        "type": "control_request",
        "request_id": "models",
        "request": { "subtype": "initialize" },
    });
    let answer = conversation(
        processes,
        time,
        program,
        "claude -p",
        BOUND_MS,
        async |child| {
            ask(child, "claude -p", &request, |line| {
                line["type"] == "control_response" && line["response"]["request_id"] == "models"
            })
            .await
        },
    )
    .await?;
    let response = &answer["response"];
    if response["subtype"] == "error" {
        let said = response["error"].as_str().unwrap_or("no reason given");
        return Err(format!("claude -p refused initialize: {said}"));
    }
    parse(&response["response"], at)
}

/// The models the Claude Code installed here lists, read in ConsensFlow's own config
/// folder for it (`model-lists/claude` in its home), never the human's.
pub(crate) async fn listed(
    env: &Env,
    processes: &dyn Processes,
    time: &dyn Time,
) -> Result<ModelListing, String> {
    let executable = executable(Harness::Claude, env)?;
    let config = config_root(env)
        .ok_or("ConsensFlow has no home to read Claude Code's models in")?
        .join("model-lists")
        .join("claude");
    std::fs::create_dir_all(&config)
        .map_err(|error| format!("{} could not be made: {error}", config.display()))?;
    read(
        processes,
        time,
        program(PathBuf::from(executable), env, &config),
    )
    .await
}

/// Whether `words` are Claude Code refusing `asked` as a model it does not have: a launch
/// on one exits with `[claude-code:unrecognized_model]` (2.1.312, CHF-02), and its window
/// says there is an issue with the selected model, the whole id named in both.
pub(crate) fn gone(words: &str, asked: &str) -> bool {
    (words.contains("unrecognized_model") || words.contains("issue with the selected model"))
        && words.contains(asked)
}

/// The model Claude Code starts on where none is named: the one its list marks, never a
/// contributor model (`ModelListing::default_model`); left to it where the list cannot be
/// read, as Claude Code's own default is no contributor model (CHF-06).
pub(crate) async fn default_model(
    env: &Env,
    processes: &dyn Processes,
    time: &dyn Time,
) -> Result<Option<String>, NoDefault> {
    Ok(listed(env, processes, time)
        .await
        .ok()
        .as_ref()
        .and_then(ModelListing::default_model)
        .map(ModelChoice::model))
}

#[cfg(test)]
mod tests;
