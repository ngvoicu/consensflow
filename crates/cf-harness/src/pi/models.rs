//! The models Pi offers and the thinking levels each takes, as its rpc mode lists them
//! (`get_available_models`): one bounded `pi --mode rpc --offline`, no turn (research-02
//! §2: 469 models in about 300 KB, 0.3 s). Offline, Pi answers from its bundled catalog
//! and the copy it keeps, and fetches nothing: a fresh catalog is the human's Refresh,
//! which runs `pi update --models` first ([`UPDATE`]).
//!
//! Pi lists the models of the providers it has set up, whatever sets them up: a login or a
//! provider key, a custom provider in `models.json`, and the providers that its installed
//! packages and its extensions register as they load (HMD-13). So the list is read as a
//! Pi window starts, with the packages and the extensions the human has: Pi's
//! `--no-extensions` also leaves out `settings.json`'s packages, and with them a provider
//! such as this Mac's only one, `@router-for-me/pi-cliproxyapi-provider`. A model is a
//! provider and an id, kept apart (an OpenRouter id holds a `/` of its own): `--model`
//! takes them joined.

use std::path::PathBuf;
use std::time::Duration;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use cf_proto::models::{ModelChoice, ModelListing};
use serde_json::{json, Value};

use super::setup::Setup;
use crate::detect::executable;
use crate::seams::processes::{Limits, Processes, Program};
use crate::seams::Time;
use crate::shared::one_shot::{ask, conversation, BOUND_MS};

/// How the list is read, as the human could run it.
pub(crate) const HOW: &str = "pi --mode rpc --offline: get_available_models";

/// What the human's Refresh runs first: the catalogs fetched again.
pub(crate) const UPDATE: [&str; 2] = ["update", "--models"];

/// How long the catalogs' fetch may take, and how much it may say.
pub(crate) const UPDATE_LIMITS: Limits = Limits {
    timeout: Duration::from_secs(60),
    max_buffer: 1 << 20,
};

/// Pi's thinking levels, from the lowest (`ThinkingLevel` of `pi-ai`).
const LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// How long Pi has to say why it would not start, once a read has found it ended: it says
/// it at once, or it started, and the second start is given up on.
const WHY_LIMITS: Limits = Limits {
    timeout: Duration::from_secs(10),
    max_buffer: 1 << 16,
};

/// The program the list is read from: `pi` at `executable` in rpc mode, offline, with no
/// session and no MCP server, and with the packages and extensions the human has, as a
/// window starts. Offline keeps Pi from installing a package that is missing and from
/// fetching a catalog; it does not stop a package's provider from listing its models
/// from its own cache, or from a server it asks itself (measured on Pi 1.5.0 with the
/// `pi-cliproxyapi-provider` package, offline, with its cache and without).
pub(crate) fn program(executable: PathBuf, env: &Env) -> Program {
    let args = ["--mode", "rpc", "--no-session", "--no-mcp", "--offline"];
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
/// lists no model: then in the words of `setup`, which tells a Pi with no provider set up
/// from one whose providers listed nothing.
pub(crate) fn parse(answer: &Value, at: i64, setup: &Setup) -> Result<ModelListing, String> {
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
        return Err(setup.unlisted());
    }
    Ok(ModelListing::read(models, at, HOW))
}

/// The models `program` lists: one bounded conversation, no turn. A read that fails says what
/// Pi said ([`said`]); one that finds no model says whether Pi has a provider set up
/// ([`Setup`]).
pub(crate) async fn read(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
) -> Result<ModelListing, String> {
    let at = time.wall_ms();
    let (env, again) = (program.env.clone(), program.clone());
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
    .await;
    let answer = match answer {
        Ok(answer) => answer,
        Err(failed) => return Err(said(processes, again, failed).await),
    };
    parse(&answer, at, &Setup::read(&env))
}

/// A failed read, with what Pi said of it. Pi ends at once, with its reason on its error
/// stream, where a package or an extension of the human's fails to load (`Failed to load
/// extension …`), and a read keeps no error stream: so a read that ended before it answered
/// is run once more, to its end, and its last lines are the reason. A read still waiting at
/// its bound says that a package or extension may be what it waits for.
async fn said(processes: &dyn Processes, program: Program, failed: String) -> String {
    if failed.ends_with(ENDED) {
        let Err(ended) = processes.run(program, WHY_LIMITS).await else {
            return failed;
        };
        // `Command failed: <command>\n<error stream>` is a program that ended with a code; a
        // program that did not start, or was given up on, said nothing.
        let said = match (ended.code, ended.killed) {
            (Some(_), false) => ended.message.split_once('\n').map(|(_, said)| said.trim()),
            _ => None,
        };
        return match said.filter(|said| !said.is_empty()) {
            Some(said) => format!("{failed}; Pi said: {}", tail(said)),
            None => failed,
        };
    }
    if failed.contains(WAITED) {
        return format!(
            "{failed}: Pi loads the packages and extensions it has before it lists, and one of \
             them may be waiting for something"
        );
    }
    failed
}

/// What `ask` says of a program that ended first, and what `conversation` says of one that
/// was still talking at its bound.
const ENDED: &str = "ended before it answered";
const WAITED: &str = "did not answer within";

/// The last lines of what Pi said, within a few hundred bytes: the reason is at the end.
fn tail(said: &str) -> String {
    let lines: Vec<&str> = said
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let kept = &lines[lines.len().saturating_sub(4)..];
    let joined = kept.join(" ");
    match joined.char_indices().nth(600) {
        Some((cut, _)) => format!("{}…", &joined[..cut]),
        None => joined,
    }
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

/// Whether `words` are Pi's provider refusing `asked`, a model it does not have: Pi sends
/// an id it does not know, and the provider answers OpenAI's 400 `model_not_found` (1.4.0,
/// CHF-02), Anthropic's 404 `not_found_error`, or, on the `openai-codex` provider, Codex's
/// own "Codex error: The 'X' model is not supported when using Codex with a ChatGPT
/// account" (1.4.0, `gone_launch.py pi`). Pi's own "Model … not found for provider
/// …, using custom model id" is a warning of its offline catalog, which runs the id all
/// the same: never a refusal (Calliope's P1, T-955). A Pi chief's model is `provider/id`
/// and the provider names the id alone, so the id is what the words must name: `asked`
/// split once at its first slash, so that an OpenRouter id keeps its own (Calliope's P1,
/// T-994).
pub(crate) fn gone(words: &str, asked: &str) -> bool {
    (words.contains("model_not_found")
        || words.contains("not_found_error")
        || words.contains("model is not supported"))
        && words.contains(id_of(asked))
}

/// The id of a Pi model `provider/id` as its provider names it: what follows the first
/// slash (`openrouter/anthropic/claude-fable-5` is `anthropic/claude-fable-5`), or the whole
/// of a model named with no provider.
fn id_of(model: &str) -> &str {
    model.split_once('/').map_or(model, |(_, id)| id)
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
