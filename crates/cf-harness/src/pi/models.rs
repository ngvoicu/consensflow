//! The models Pi offers and the thinking levels each takes, as its rpc mode lists them
//! (`get_available_models`): one bounded `pi --mode rpc --offline`, no turn (research-02
//! §2: 469 models in about 300 KB, 0.3 s). Offline, Pi answers from its bundled catalog
//! and the copy it keeps, and fetches nothing: a fresh catalog is the human's Refresh,
//! which runs `pi update --models` first ([`UPDATE`]) and then reads the list once, as any
//! read does. A package that fills its catalog a few seconds after Pi starts shows its new
//! models at the next Refresh: holding Pi's session for it took a Refresh past the app's 30 s
//! bridge deadline (the review T-1270, second reading, F6 and F8).
//!
//! Pi lists the models of the providers it has set up, whatever sets them up: a login or a
//! provider key, a custom provider in `models.json`, and the providers that its installed
//! packages and its extensions register as they load (HMD-13). So the list is read as a
//! Pi window starts, with the packages and the extensions the human has: Pi's
//! `--no-extensions` also leaves out `settings.json`'s packages, and with them a provider
//! such as this Mac's only one, `@router-for-me/pi-cliproxyapi-provider`. A model is a
//! provider and an id, kept apart (an OpenRouter id holds a `/` of its own): `--model`
//! takes them joined.
//!
//! **What the page shows is what a Pi window shows its human** (A4-28, T-1321), who opens
//! `/model` seconds after Pi started, not what Pi lists in its first moment: a package's
//! provider answers from its own cache at once and asks its server a moment later (measured
//! on Pi 1.5.0: the cache at 0.3 s, the server's list at 1.8 s, with `--offline` and
//! without: a package decides what it asks for itself). Pi tells of no such moment, so the
//! page's read asks again until the list has stood still ([`SETTLE_MS`]), which is the only
//! confirmation there is (Pi 1.5.0's rpc has no event, state field or command for "my providers
//! have loaded"); a list the wait could not confirm (a refusal, an ask left unanswered, Pi
//! ending, the cap reached while the list still changed) comes back stale, with why, never
//! fresh. A window's launch does not wait ([`default_model`]).

use std::path::PathBuf;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::startup::{StartFailure, StartKind};
use cf_proto::agents::Harness;
use cf_proto::models::{Freshness, ModelChoice, ModelListing, IT_SAID, WORDS_WITHHELD};
use serde_json::{json, Value};

use super::setup::inspect;
use crate::contract::NoDefault;
use crate::detect::executable;
use crate::seams::processes::{Child, Limits, Processes, Program, Streams};
use crate::seams::{arm, Time};
use crate::shared::one_shot::{ask, conversation_as, Said, BOUND_MS};

/// How the list is read, as the human could run it.
pub(crate) const HOW: &str = "pi --mode rpc --offline: get_available_models";

/// What the human's Refresh runs first: the catalogs fetched again.
pub(crate) const UPDATE: [&str; 2] = ["update", "--models"];

/// How long the catalogs' fetch may take, and how much it may say.
pub(crate) const UPDATE_LIMITS: Limits = Limits {
    timeout: Duration::from_secs(60),
    max_buffer: 1 << 20,
};

/// How often a read that waits for Pi's providers asks again.
const POLL_MS: u64 = 250;

/// How long the list must stand still before it is the list: a provider that asks its server
/// on loopback has answered within a second or two (1.5 s of a server's own delay, measured).
/// A provider slower than this shows its models at the next read.
const SETTLE_MS: u64 = 2_500;

/// The most a read waits for the list to stand still, from Pi's first answer, however it
/// changes: the page is waiting, and an extension that never stops changing it is Pi's.
const SETTLE_CAP_MS: u64 = 12_000;

/// How long one more ask is given, once Pi has answered: one that goes unanswered ends the
/// wait, and the list Pi gave is the list.
const ASK_MS: u64 = 5_000;

/// What a read leaves of [`BOUND_MS`] after Pi's first answer: the wait never takes the answer
/// Pi gave from a read that began slowly.
const KEEP_MS: u64 = 3_000;

/// Pi's thinking levels, from the lowest (`ThinkingLevel` of `pi-ai`).
const LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

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

/// Why a `get_available_models` answer lists no model.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unlisted {
    /// Pi said so: it refused the request.
    Said(String),
    /// Pi answered with no model: no provider is set up, or the providers it has listed none.
    /// Which of the two is told from Pi's folder ([`super::setup::Setup`]), read only then.
    NoModel,
}

/// What a `get_available_models` answer lists, read at `at`. Fails where Pi refused, or
/// lists no model ([`Unlisted`]).
pub(crate) fn parse(answer: &Value, at: i64) -> Result<ModelListing, Unlisted> {
    if answer.get("success") == Some(&Value::Bool(false)) {
        // What Pi gives as the reason is Pi's words, which an extension of the human's can
        // write: they stand after the mark, whole, for whoever tells them to screen.
        return Err(Unlisted::Said(
            match answer["error"].as_str().map(str::trim) {
                Some(said) if !said.is_empty() => {
                    format!("pi refused get_available_models{IT_SAID}{said}")
                }
                _ => "pi refused get_available_models: no reason given".to_owned(),
            },
        ));
    }
    let rows = answer["data"]["models"]
        .as_array()
        .ok_or_else(|| Unlisted::Said("Pi's answer has no models".to_owned()))?;
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
        return Err(Unlisted::NoModel);
    }
    Ok(ModelListing::read(models, at, HOW))
}

/// Whether a read waits for Pi's providers to load: the page's read does, as a window shows its
/// human a list some seconds after Pi started; a window's launch, which asks for a default
/// model, does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Settle {
    No,
    WaitForProviders,
}

/// The request for `models`: `models` the first time, `models-2`, `models-3` after.
fn request(asks: u32) -> (String, Value) {
    let id = if asks == 1 {
        "models".to_owned()
    } else {
        format!("models-{asks}")
    };
    let line = json!({ "id": id, "type": "get_available_models" });
    (id, line)
}

/// The models an answer lists, as Pi gave them: what a later answer is compared with.
fn listed_in(answer: &Value) -> &Value {
    &answer["data"]["models"]
}

/// How long the wait for the list to stand still may last, `spent` milliseconds into a read
/// that has [`BOUND_MS`]: [`SETTLE_CAP_MS`], less than what is left of the bound.
fn wait_allowed(spent: u64) -> u64 {
    SETTLE_CAP_MS.min(BOUND_MS.saturating_sub(spent).saturating_sub(KEEP_MS))
}

/// Why a list was not confirmed, in the words a picker shows beside it (it is what a Pi window
/// would still have been loading): said of the list Pi gave, which stays, stale.
const REFUSED: &str = "Pi was still loading its models: it refused to list them a second time";
const NO_ANSWER: &str = "Pi was still loading its models: it did not answer a second ask in time";
const ENDED: &str = "Pi was still loading its models: it ended before it answered a second ask";
const CHANGING: &str =
    "Pi was still loading its models: its list was still changing when the wait ended";
const LATE: &str = "Pi was still loading its models: its first answer came too late to look again";

/// What a wait for Pi's providers came to: the last answer Pi gave, and why it is not a confirmed
/// list, if it is not.
struct Settled {
    answer: Value,
    unconfirmed: Option<&'static str>,
}

/// Pi's answer to `get_available_models`, asked again until what it lists has stood still for
/// [`SETTLE_MS`], every ask answered: that list is **confirmed**. Pi has no signal that its
/// extensions and providers have loaded (Pi 1.5.0's rpc: no event, no state field, no command),
/// so standing still is the only confirmation there is. The wait ends otherwise at the first
/// ask Pi refuses, leaves unanswered or ends on, or where [`wait_allowed`] runs out while the
/// list still changes: the last list Pi gave then comes back with why it is unconfirmed, for the
/// read to mark it stale. An answer that lists no model is waited on like any other.
async fn settled(child: &dyn Child, time: &dyn Time, first: Value, spent: u64) -> Settled {
    // A refusal of the first ask is the read's failure, not a list to confirm.
    if first.get("success") != Some(&Value::Bool(true)) {
        return Settled {
            answer: first,
            unconfirmed: None,
        };
    }
    let allowed = wait_allowed(spent);
    if allowed == 0 {
        return Settled {
            answer: first,
            unconfirmed: Some(LATE),
        };
    }
    // By the clock, not by the waits counted: each ask takes its own time on top of them.
    let until = time.wall_ms() + i64::try_from(allowed).unwrap_or(0);
    let (mut latest, mut still, mut asks) = (first, 0, 1);
    while still < SETTLE_MS {
        if time.wall_ms() >= until {
            return Settled {
                answer: latest,
                unconfirmed: Some(CHANGING),
            };
        }
        time.sleep(Duration::from_millis(POLL_MS)).await;
        still += POLL_MS;
        asks += 1;
        let (id, line) = request(asks);
        let asked = ask(child, "pi --mode rpc", &line, |answer| {
            answer["type"] == "response" && answer["id"] == id.as_str()
        });
        // An ask is given what the wait has left (a quarter of a second at the least), so that
        // the last of them ends within what [`KEEP_MS`] keeps of the read's bound.
        let left = u64::try_from(until - time.wall_ms()).unwrap_or(0);
        let next = match arm(time, ASK_MS.min(left).max(POLL_MS)).bound(asked).await {
            None => return unconfirmed(latest, NO_ANSWER),
            Some(Err(_)) => return unconfirmed(latest, ENDED),
            Some(Ok(next)) if next.get("success") != Some(&Value::Bool(true)) => {
                return unconfirmed(latest, REFUSED)
            }
            Some(Ok(next)) => next,
        };
        if listed_in(&next) != listed_in(&latest) {
            still = 0;
        }
        latest = next;
    }
    Settled {
        answer: latest,
        unconfirmed: None,
    }
}

fn unconfirmed(answer: Value, why: &'static str) -> Settled {
    Settled {
        answer,
        unconfirmed: Some(why),
    }
}

/// The models `program` lists: one bounded conversation, no turn, one start of Pi.
///
/// A read that fails says what Pi said (its error stream and its exit code are kept from the
/// one start, and nothing is started again to find out); one that waited for Pi in vain says
/// which packages and extensions it was loading. One that finds no model says whether Pi has a
/// provider set up ([`super::setup::Setup`]), reading its folder only then, and on a thread of its own
/// ([`inspect`]). Where `settle` says so, the list is the one Pi has once its providers have
/// loaded ([`settled`]); where that was not confirmed the list comes back stale, with why.
pub(crate) async fn read(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
    settle: Settle,
) -> Result<ModelListing, Said> {
    let at = time.wall_ms();
    let env = program.env.clone();
    let (id, asked) = request(1);
    let (answer, unconfirmed) = match conversation_as(
        processes,
        time,
        program,
        Streams::Conversation,
        "pi --mode rpc",
        BOUND_MS,
        async |child| {
            let first = ask(child, "pi --mode rpc", &asked, |line| {
                line["type"] == "response" && line["id"] == id.as_str()
            })
            .await?;
            Ok(match settle {
                Settle::No => (first, None),
                Settle::WaitForProviders => {
                    let spent = u64::try_from(time.wall_ms() - at).unwrap_or(0);
                    let Settled {
                        answer,
                        unconfirmed,
                    } = settled(child, time, first, spent).await;
                    (answer, unconfirmed)
                }
            })
        },
    )
    .await
    {
        Ok(read) => read,
        Err(failed) => return Err(waiting_on(failed, time, &env).await),
    };
    match parse(&answer, at) {
        Ok(listing) => Ok(match unconfirmed {
            Some(why) => listing.stale(why),
            None => listing,
        }),
        // Pi started, and said no: its words are Pi's, for the page; a note says only that
        // it ended with an error.
        Err(Unlisted::Said(why)) => Err(Said {
            words: why,
            failure: Some(StartFailure::of(StartKind::Failed)),
        }),
        Err(Unlisted::NoModel) => {
            let unlisted = inspect(time, &env).await.unlisted();
            Err(Said::from(match unconfirmed {
                Some(why) => format!("{why}; {unlisted}"),
                None => unlisted,
            }))
        }
    }
}

/// A failed read, and where Pi was given up on, what it was loading: the packages and
/// extensions of its folder, by name, for one of them may be what it waited for. That is
/// ConsensFlow's description, so it stands before [`IT_SAID`], where Pi's own words, which
/// come last, begin.
async fn waiting_on(failed: Said, time: &dyn Time, env: &Env) -> Said {
    let Said { words, failure } = failed;
    let (head, said) = match words.split_once(IT_SAID) {
        Some((head, said)) => (head, Some(said)),
        None => (words.as_str(), None),
    };
    if !head.contains("did not answer within") {
        return Said { words, failure };
    }
    let loading = inspect(time, env)
        .await
        .loading()
        .map_or_else(String::new, |loading| format!(" ({loading})"));
    let head = format!(
        "{head}. Pi loads the packages and extensions it has before it lists, and one of \
         them may be waiting for something{loading}"
    );
    let words = match said {
        Some(said) => format!("{head}{IT_SAID}{said}"),
        None => head,
    };
    Said { words, failure }
}

/// The most of what a failed `pi update --models` said that is handed on, as a read of Pi
/// hands on no more than cf-process keeps of its error stream: more than this is withheld.
const UPDATE_SAID_BYTES: usize = 4 << 10;

/// Why `pi update --models` failed, in words: its exit code and, after [`IT_SAID`], everything
/// its error stream held (the message is `Command failed: <cmd>\n<what it wrote>`), whole, for
/// whoever tells it to screen it before they cut it. A program that never started has only
/// the system's words, which are ConsensFlow's to say. What it wrote past [`UPDATE_SAID_BYTES`]
/// is not handed on at all, and [`WORDS_WITHHELD`] stands in its place: a cut to its end can take
/// the label of a value and leave the value (T-1276, `long-password-continuation`), as the end
/// of a read's error stream does.
fn update_failure(failed: &cf_process::Failed) -> String {
    let code = failed
        .code
        .map_or_else(String::new, |code| format!(" (exit {code})"));
    let Some((_, said)) = failed.message.split_once('\n') else {
        return format!("failed{code}: {}", failed.message.trim());
    };
    let said = said.trim();
    if said.is_empty() {
        format!("failed{code}")
    } else if said.len() > UPDATE_SAID_BYTES {
        format!("failed{code}{IT_SAID}{WORDS_WITHHELD}")
    } else {
        format!("failed{code}{IT_SAID}{said}")
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
    settle: Settle,
) -> Result<ModelListing, Said> {
    let executable = PathBuf::from(executable(Harness::Pi, env).map_err(|words| Said {
        words,
        failure: Some(StartFailure::of(StartKind::NotFound)),
    })?);
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
    let listing = read(processes, time, program(executable, env), settle).await?;
    Ok(match fetched {
        Ok(()) => listing,
        Err(failed) => {
            let catalog = format!(
                "Pi's own catalog: pi update --models {}",
                update_failure(&failed)
            );
            // ConsensFlow's own words about the list stand before the mark where Pi's begin.
            match &listing.freshness {
                Freshness::Stale { why } => {
                    let why = format!("{why}; {catalog}");
                    listing.stale(why)
                }
                Freshness::Fresh => listing.stale(catalog),
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
) -> Result<Option<String>, NoDefault> {
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
    // What a read that failed said is Pi's, for the page: the chief's note says the kind of
    // failure and its code (T-1328).
    let listing = listed(env, processes, time, false, Settle::No)
        .await
        .map_err(|said| NoDefault::Unread(said.failure))?;
    listing
        .default_model()
        .map(|model| Some(model.model()))
        .ok_or(NoDefault::NoneOffered(
            "Pi offers no model that is not a contributor model",
        ))
}

#[cfg(test)]
mod tests;
