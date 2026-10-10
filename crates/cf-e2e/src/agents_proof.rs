//! What a daemon's agents screens must do, asked of it over its API and of the
//! roster file it writes, and nothing else: the catalog it serves, an agent
//! saved by hand with the profile it is shown with, the screens behind the UI
//! token, and the deletion of the agent saved. It uses no crate of the
//! product, so it holds whichever daemon answers: the packaged smoke runs it
//! against the daemon the built app chose, and the `daemon` suite against the
//! daemon from the checkout.
//!
//! [`prove`] is given where the daemon listens (`url`), its UI token and the
//! folder it keeps its roster (`agents.json`) in. The home has no agent of the
//! human's own called `my-maia`; it may hold others. It fails with the check
//! that did not hold.

use std::path::Path;

use serde_json::{json, Value};

use crate::http::{encode_component, origin, Http, Reply};
use crate::{files, Error, Result};

/// What the catalog holds, as the packaged build has it (every agent is in the
/// roster as the catalog has it).
const CATALOG_SIZE: usize = 120;

/// The agent the proof saves by hand, and deletes.
const MINE: &str = "my-maia";

/// What the Agents screen has: the controls the human browses and sorts by.
pub const PRESENT: [&str; 9] = [
    "aria-label=\"Agents\"",
    "Model and reasoning",
    "My own agents",
    "model-summary",
    "model-group",
    "value=\"model-reasoning\" selected",
    "Work tier",
    "tier-pill",
    "Important work only · No coding",
];

/// What the Agents screen no longer has.
pub const GONE: [&str; 15] = [
    "id=\"catalog-section\"",
    "Agent library",
    "Your agents",
    "PM candidate",
    "name=\"tags\"",
    "category-pill",
    "Chief of Staff candidate",
    "name=\"category\"",
    "Name in use",
    "offer__actions",
    "Saved only",
    "Sort by",
    "benchmark",
    "Artificial Analysis",
    "AA ",
];

/// The daemon to hold to the proof.
#[derive(Debug, Clone, Copy)]
pub struct Daemon<'a> {
    /// Where it listens, as its handle line says.
    pub url: &'a str,
    /// Its UI token.
    pub token: &'a str,
    /// The folder it keeps its roster in.
    pub home: &'a Path,
}

/// Fails with the words of `message` when `held` is false.
macro_rules! ensure {
    ($held:expr, $($message:tt)+) => {
        if !$held {
            return Err(Error::Daemon(format!($($message)+)));
        }
    };
}

/// Holds the daemon to what its agents screens must do.
pub fn prove(daemon: Daemon<'_>) -> Result<()> {
    let Daemon { url, token, home } = daemon;
    let origin = origin(url);
    let http = Http::new();
    // `bearer` is the token the request brings, none to bring none.
    let ask =
        |path: &str, method: &str, body: Option<Value>, bearer: Option<&str>| -> Result<Reply> {
            http.send(method, &format!("{origin}{path}"), bearer, body.as_ref())
        };
    let agents = || -> Result<Value> {
        let answer = ask("/api/agents", "GET", None, Some(token))?;
        ensure!(answer.status == 200, "GET /api/agents: {}", answer.status);
        answer.json()
    };
    let stored = || -> Result<Value> {
        let roster: Value = serde_json::from_str(&files::read_string(&home.join("agents.json"))?)
            .map_err(|source| Error::Json {
            text: "the roster file".to_owned(),
            source,
        })?;
        Ok(roster["agents"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["id"] == MINE))
            .cloned()
            .unwrap_or(Value::Null))
    };
    let listed = |answer: &Value| answer["agents"].as_array().cloned().unwrap_or_default();

    // The screens and their API are the app's, behind its UI token: with none,
    // or another, they say so.
    let another = format!("{token}x");
    for path in ["/", "/harnesses", "/api/agents"] {
        for (what, bearer) in [
            ("no token", None),
            ("another token", Some(another.as_str())),
        ] {
            let refused = ask(path, "GET", None, bearer)?;
            ensure!(
                refused.status == 401,
                "{path} with {what}: {}",
                refused.status
            );
            let said = refused.json()?;
            ensure!(
                said == json!({ "error": "unauthorized" }),
                "{path} with {what}: {said}"
            );
        }
    }

    // The catalog: every agent in the roster, as the catalog has it.
    let first = agents()?;
    let first_agents = listed(&first);
    let catalog = first_agents
        .iter()
        .filter(|agent| agent["custom"] != true)
        .count();
    ensure!(
        catalog == CATALOG_SIZE,
        "packaged preset count: {catalog}, not {CATALOG_SIZE}"
    );
    let model = first_agents
        .iter()
        .find(|agent| agent["name"] == "pygmalion")
        .map(|agent| agent["model"].clone());
    ensure!(
        model == Some(json!("codex-image")),
        "pygmalion's model is {}, not 'codex-image'",
        model.unwrap_or(Value::Null)
    );
    ensure!(
        !first_agents.iter().any(|agent| agent["name"] == MINE),
        "{MINE} is not yet"
    );

    // An agent saved by hand: the file keeps what was said, and the screen
    // shows it with its profile.
    let added = ask(
        "/api/agents",
        "POST",
        Some(json!({ "name": MINE, "harness": "codex", "model": "gpt-6-astra", "effort": "low" })),
        Some(token),
    )?;
    ensure!(
        added.status == 201,
        "POST /api/agents: {} {}",
        added.status,
        added.text
    );
    let row = stored()?;
    ensure!(
        !row.is_null(),
        "the roster file holds no {MINE} once it is saved"
    );
    let kept = (
        row["effort"].clone(),
        row["model"].clone(),
        row.get("profile").is_some(),
    );
    ensure!(
        kept == (json!("low"), json!("gpt-6-astra"), false),
        "the file keeps [effort, model, has a profile] as {} {} {}, not 'low' 'gpt-6-astra' false",
        kept.0,
        kept.1,
        kept.2
    );
    let after_adding = agents()?;
    let after_adding_agents = listed(&after_adding);
    let mine = after_adding_agents
        .iter()
        .find(|agent| agent["name"] == MINE)
        .cloned()
        .unwrap_or(Value::Null);
    let shown = (
        mine["effort"].clone(),
        mine["custom"].clone(),
        mine["profile"]["workTier"].clone(),
    );
    ensure!(
        shown == (json!("low"), json!(true), json!("light")),
        "the screen shows [effort, custom, work tier] as {} {} {}, not 'low' true 'light'",
        shown.0,
        shown.1,
        shown.2
    );
    ensure!(
        after_adding_agents.len() == first_agents.len() + 1,
        "{} agents listed after one was saved, not {}",
        after_adding_agents.len(),
        first_agents.len() + 1
    );

    // The screens, opened as the app opens them: the token in the address, or
    // as the bearer.
    let page = ask("/", "GET", None, Some(token))?;
    ensure!(page.status == 200, "the Agents screen: {}", page.status);
    ensure!(
        page.content_type
            .as_deref()
            .is_some_and(|kind| kind.starts_with("text/html")),
        "the Agents screen is {:?}, not text/html",
        page.content_type
    );
    for text in PRESENT {
        ensure!(page.text.contains(text), "the Agents screen lacks {text}");
    }
    for text in GONE {
        ensure!(!page.text.contains(text), "gone: {text}");
    }
    let framed = ask(
        &format!("/?token={}", encode_component(token)),
        "GET",
        None,
        None,
    )?;
    ensure!(
        framed.status == 200,
        "the Agents screen at its framed address: {}",
        framed.status
    );
    ensure!(
        framed.text.contains("aria-label=\"Agents\""),
        "the Agents screen at its framed address is not the Agents screen"
    );
    let harnesses = ask(
        &format!("/harnesses?token={}", encode_component(token)),
        "GET",
        None,
        None,
    )?;
    ensure!(
        harnesses.status == 200,
        "the Harnesses screen at its framed address: {}",
        harnesses.status
    );
    ensure!(
        harnesses
            .text
            .contains("<title>ConsensFlow Harnesses</title>"),
        "the Harnesses screen at its framed address is not the Harnesses screen"
    );

    // The deletion: a catalog agent is not the human's to delete, the agent
    // saved is.
    let catalog_delete = ask("/api/agents/maia", "DELETE", None, Some(token))?;
    ensure!(
        catalog_delete.status == 400,
        "DELETE of a catalog agent: {}, not 400",
        catalog_delete.status
    );
    let own_delete = ask(&format!("/api/agents/{MINE}"), "DELETE", None, Some(token))?;
    ensure!(
        own_delete.status == 204,
        "DELETE of {MINE}: {}, not 204",
        own_delete.status
    );
    let after = agents()?;
    let after_agents = listed(&after);
    ensure!(
        after_agents.len() == first_agents.len(),
        "{} agents listed after one was deleted, not {}",
        after_agents.len(),
        first_agents.len()
    );
    ensure!(
        !after_agents.iter().any(|agent| agent["name"] == MINE),
        "{MINE} is still listed after it was deleted"
    );
    ensure!(
        after.get("catalog").is_none(),
        "the answer still holds a catalog of its own"
    );
    ensure!(stored()?.is_null(), "the file no longer holds it");
    Ok(())
}
