//! The models Pi offers and the thinking levels each takes, as its rpc mode lists them
//! (`get_available_models`): one bounded `pi --mode rpc --offline`, no turn (research-02
//! §2: 469 models in about 300 KB, 0.3 s). Offline, Pi answers from its bundled catalog
//! and the copy it keeps, and fetches nothing: a fresh catalog is the human's Refresh,
//! which runs `pi update --models` first ([`UPDATE`]), and then holds Pi's session open for
//! [`REFRESH_HOLD`] and asks again, for what the providers of its packages fetch ([`read`]).
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
use cf_proto::models::{Freshness, ModelChoice, ModelListing};
use serde_json::{json, Value};

use super::setup::Setup;
use crate::detect::executable;
use crate::seams::processes::{Limits, Processes, Program, Streams};
use crate::seams::Time;
use crate::shared::one_shot::{ask, conversation_as, BOUND_MS};

/// How the list is read, as the human could run it.
pub(crate) const HOW: &str = "pi --mode rpc --offline: get_available_models";

/// How a Refresh's list is read: asked again once Pi's packages have had [`REFRESH_HOLD`].
pub(crate) const HOW_REFRESHED: &str =
    "pi --mode rpc --offline: get_available_models, asked again after the packages' 10 s";

/// What the human's Refresh runs first: the catalogs fetched again.
pub(crate) const UPDATE: [&str; 2] = ["update", "--models"];

/// How long the catalogs' fetch may take, and how much it may say.
pub(crate) const UPDATE_LIMITS: Limits = Limits {
    timeout: Duration::from_secs(60),
    max_buffer: 1 << 20,
};

/// Pi's thinking levels, from the lowest (`ThinkingLevel` of `pi-ai`).
const LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// How long a Refresh keeps Pi's session open after its first answer, and asks again
/// (the review T-1270, F6). A provider of a package lists what it holds at once and fetches
/// its catalog after: the installed CLIProxyAPI package answers its cache, asks its server in
/// the background, and lists what comes (on Pi 1.5.0, held 8 s past its answer, its cache file
/// was rewritten). `pi update --models` does not load a package's providers, so a read that
/// stops at the first answer would give the old catalog, again and again, as a fresh one.
const REFRESH_HOLD: Duration = Duration::from_secs(10);

/// How long the second question of a Refresh has to be answered: Pi answers it from what it
/// holds, so that a Pi that is there answers at once.
const AGAIN_MS: u64 = 5_000;

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
/// lists no model: then in the words `unlisted` gives, which tell a Pi with no provider set
/// up from one whose providers listed nothing. They are asked for only then: a Pi that lists
/// models has its folder left alone.
pub(crate) fn parse(
    answer: &Value,
    at: i64,
    unlisted: impl FnOnce() -> String,
) -> Result<ModelListing, String> {
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
        return Err(unlisted());
    }
    Ok(ModelListing::read(models, at, HOW))
}

/// The models `program` lists: one bounded conversation, no turn, one start of Pi.
///
/// Where `hold` (a Refresh), Pi's session is kept open for it after the first answer, and
/// `get_available_models` is asked again: the second answer is the list, as the providers of
/// Pi's packages have it after they fetched. Where Pi does not answer the second time, the
/// first answer stands, stale, for why.
///
/// A read that fails says what Pi said (its error stream and its exit code are kept from the
/// one start, and nothing is started again to find out); one that waited for Pi in vain says
/// which packages and extensions it was loading. One that finds no model says whether Pi has a
/// provider set up ([`Setup`]), reading its folder only then.
pub(crate) async fn read(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
    hold: Option<Duration>,
) -> Result<ModelListing, String> {
    let mut at = time.wall_ms();
    let env = program.env.clone();
    let asked = |id: &str| json!({ "id": id, "type": "get_available_models" });
    let answers =
        |id: &'static str| move |line: &Value| line["type"] == "response" && line["id"] == id;
    let hold_ms = hold.map_or(0, |hold| u64::try_from(hold.as_millis()).unwrap_or(0));
    // The first answer has the bound of any read; a Refresh adds its hold and its second ask.
    let bound = BOUND_MS + hold.map_or(0, |_| hold_ms + AGAIN_MS);
    let (answer, again) = conversation_as(
        processes,
        time,
        program,
        Streams::Conversation,
        "pi --mode rpc",
        bound,
        async |child| {
            let first = ask(child, "pi --mode rpc", &asked("models"), answers("models")).await?;
            let Some(hold) = hold else {
                return Ok((first, None));
            };
            time.sleep(hold).await;
            let second = match crate::seams::arm(time, AGAIN_MS)
                .bound(ask(
                    child,
                    "pi --mode rpc",
                    &asked("models-again"),
                    answers("models-again"),
                ))
                .await
            {
                Some(second) => second,
                None => Err("pi --mode rpc did not answer within 5 s".to_owned()),
            };
            Ok((first, Some(second)))
        },
    )
    .await
    .map_err(|failed| waiting_on(failed, &env))?;
    let unlisted = || Setup::read(&env).unlisted();
    match again {
        None => parse(&answer, at, unlisted),
        Some(Ok(second)) => {
            at = time.wall_ms();
            let mut listing = parse(&second, at, unlisted)?;
            HOW_REFRESHED.clone_into(&mut listing.how);
            Ok(listing)
        }
        Some(Err(why)) => Ok(parse(&answer, at, unlisted)?.stale(format!(
            "Pi did not list again after the {} s hold, so the list is its first answer, which \
             a package's provider may give from its cache: {why}",
            hold_ms / 1000
        ))),
    }
}

/// A failed read, and where Pi was given up on, what it was loading: the packages and
/// extensions of its folder, by name, for one of them may be what it waited for.
fn waiting_on(failed: String, env: &Env) -> String {
    if !failed.contains("did not answer within") {
        return failed;
    }
    let loading = Setup::read(env)
        .loading()
        .map_or_else(String::new, |loading| format!(" ({loading})"));
    format!(
        "{failed}. Pi loads the packages and extensions it has before it lists, and one of \
         them may be waiting for something{loading}"
    )
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
    let listing = read(
        processes,
        time,
        program(executable, env),
        fetch.then_some(REFRESH_HOLD),
    )
    .await?;
    Ok(match fetched {
        Ok(()) => listing,
        Err(failed) => {
            let why = format!(
                "Pi's own catalog: pi update --models failed: {}",
                failed.message.lines().last().unwrap_or_default()
            );
            // A list already stale for why Pi did not list again is stale for both.
            match &listing.freshness {
                Freshness::Stale { why: earlier } => {
                    let both = format!("{earlier}; {why}");
                    listing.stale(both)
                }
                Freshness::Fresh => listing.stale(why),
            }
        }
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
