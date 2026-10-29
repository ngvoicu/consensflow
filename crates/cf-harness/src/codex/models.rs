//! The models Codex offers and the efforts each takes, as its app-server lists them
//! (`model/list`): one bounded `codex app-server` over stdio, `initialize`, `initialized`,
//! then each page of `model/list`, no turn (research-02 §1: 0.1–1.5 s).
//!
//! It runs on the human's own Codex home, the account the windows use. Signed in, the
//! app-server fetches the account's list and keeps it in `models_cache.json`, which it
//! answers from while the copy is fresh; signed out, or with no network and no copy, it
//! answers from the list it ships, with nothing in the answer to say so. So a list is
//! stale where the home holds no copy, or one fetched more than [`FRESH_MS`] before the
//! read: an app-server that could have fetched it again would have.

use std::path::PathBuf;

use cf_base::env::Env;
use cf_base::path;
use cf_proto::agents::Harness;
use cf_proto::models::{ModelChoice, ModelListing};
use serde_json::{json, Value};

use super::paths::codex_home;
use crate::detect::executable;
use crate::seams::processes::{Processes, Program};
use crate::seams::Time;
use crate::shared::one_shot::{ask, conversation, BOUND_MS};

/// How the list is read, as the human could run it.
pub const HOW: &str = "codex app-server: model/list";

/// How old the app-server's copy of the account's list may be and the list still be the
/// account's: an hour, well past the few minutes it keeps a copy before it fetches again
/// (a second read three seconds after the first answered from the first's copy).
pub const FRESH_MS: i64 = 3_600_000;

/// The most pages of `model/list` read: one held every model (7 to 12) on 0.165.
const PAGES: usize = 10;

/// The program the list is read from: `codex app-server` at `executable`, on the home
/// `env` names.
pub fn program(executable: PathBuf, env: &Env) -> Program {
    Program {
        executable,
        args: vec!["app-server".to_owned()],
        cwd: None,
        env: env.clone(),
    }
}

/// What the pages of `model/list` list, the hidden models left out. Stale where `fetched`
/// (when the app-server's copy was fetched, if it kept one) is not within [`FRESH_MS`] of
/// `at`. Fails where it lists no model.
pub fn parse(pages: &[Value], fetched: Option<i64>, at: i64) -> Result<ModelListing, String> {
    let text = |row: &Value, name: &str| row.get(name).and_then(Value::as_str).map(str::to_owned);
    let models: Vec<ModelChoice> = pages
        .iter()
        .flat_map(|page| {
            page.get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|row| row.get("hidden") != Some(&Value::Bool(true)))
        .filter_map(|row| {
            let id = text(row, "model")
                .or_else(|| text(row, "id"))
                .filter(|id| !id.is_empty())?;
            let efforts = row
                .get("supportedReasoningEfforts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|effort| text(effort, "reasoningEffort"))
                .collect();
            Some(ModelChoice {
                label: text(row, "displayName").unwrap_or_else(|| id.clone()),
                efforts,
                default_effort: text(row, "defaultReasoningEffort"),
                marked_default: row.get("isDefault") == Some(&Value::Bool(true)),
                ..ModelChoice::new(id.clone(), "")
            })
        })
        .collect();
    if models.is_empty() {
        return Err("Codex lists no model".to_owned());
    }
    let listing = ModelListing::read(models, at, HOW);
    Ok(match fetched {
        None => listing.stale("the list Codex ships: it holds no copy of the account's"),
        Some(fetched) if at - fetched > FRESH_MS => {
            listing.stale("Codex's copy of the account's list, which it could not fetch again")
        }
        Some(_) => listing,
    })
}

/// When the app-server's copy in `cache` (`models_cache.json`) was fetched: none where
/// there is none, or it says no time.
pub fn fetched_at(cache: &str) -> Option<i64> {
    let cache: Value = serde_json::from_str(cache).ok()?;
    let at: jiff::Timestamp = cache.get("fetched_at")?.as_str()?.parse().ok()?;
    Some(at.as_millisecond())
}

/// The pages of `model/list` that `program` answers: one bounded conversation, no turn.
pub async fn pages(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
) -> Result<Vec<Value>, String> {
    const WHAT: &str = "codex app-server";
    conversation(processes, time, program, WHAT, BOUND_MS, async |child| {
        let initialize = json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": { "name": "consensflow", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true },
            },
        });
        result(
            ask(child, WHAT, &initialize, |line| line["id"] == 1).await?,
            "initialize",
        )?;
        // A notification: no id, no answer.
        child
            .write_line(&json!({ "method": "initialized" }).to_string())
            .await?;
        let mut pages = Vec::new();
        let mut cursor = Value::Null;
        for id in 2..2 + PAGES {
            let list = json!({
                "id": id,
                "method": "model/list",
                "params": { "cursor": cursor, "includeHidden": false },
            });
            let page = result(
                ask(child, WHAT, &list, |line| line["id"] == id).await?,
                "model/list",
            )?;
            cursor = page.get("nextCursor").cloned().unwrap_or(Value::Null);
            pages.push(page);
            if !cursor.is_string() {
                break;
            }
        }
        Ok(pages)
    })
    .await
}

/// The `result` of an answer to `method`, or the app-server's refusal.
fn result(answer: Value, method: &str) -> Result<Value, String> {
    if let Some(error) = answer.get("error") {
        let said = error["message"].as_str().unwrap_or("no reason given");
        return Err(format!("codex app-server refused {method}: {said}"));
    }
    Ok(answer.get("result").cloned().unwrap_or(Value::Null))
}

/// The models the Codex installed here lists, on the human's own Codex home: stale where
/// the app-server answered from no fresh copy of the account's list.
pub(crate) async fn listed(
    env: &Env,
    processes: &dyn Processes,
    time: &dyn Time,
) -> Result<ModelListing, String> {
    let executable = executable(Harness::Codex, env)?;
    let at = time.wall_ms();
    let pages = pages(processes, time, program(PathBuf::from(executable), env)).await?;
    let cache = path::join(&[&codex_home(env)?, "models_cache.json"]);
    let fetched = std::fs::read_to_string(cache)
        .ok()
        .as_deref()
        .and_then(fetched_at);
    parse(&pages, fetched, at)
}

/// Whether `words` are Codex refusing a model it does not have: a launch on one exits with
/// "The 'X' model is not supported when using Codex with a ChatGPT account" (0.165.0,
/// CHF-02), and an API's account is told `model_not_found`.
pub(crate) fn gone(words: &str) -> bool {
    words.contains("model is not supported") || words.contains("model_not_found")
}

/// The model Codex starts on where none is named: the one its list marks, never a
/// contributor model (`ModelListing::default_model`); left to it where the list cannot be
/// read, as Codex's own default is no contributor model (CHF-06).
pub(crate) async fn default_model(
    env: &Env,
    processes: &dyn Processes,
    time: &dyn Time,
) -> Result<Option<String>, String> {
    Ok(listed(env, processes, time)
        .await
        .ok()
        .as_ref()
        .and_then(ModelListing::default_model)
        .map(ModelChoice::model))
}

#[cfg(test)]
mod tests;
