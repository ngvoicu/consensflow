//! Live: what each harness lists of its models (CHF-03), read as the daemon reads it
//! ([`Adapter::models`](cf_harness::contract::Adapter::models), through `harness-ask`), with
//! no model turn. It is opt-in (`cargo xtask live models`, `npm run live:models`): it needs
//! each harness named installed and logged in, and is no part of `check` or of CI. It spends
//! no quota: Claude's `initialize`, Codex's `model/list`, Pi's `get_available_models` and
//! `devin models list` run no turn.
//!
//! Each read runs on the home the kit gives every live window of the harness
//! ([`machine::installed`]: Claude Code on a config folder of the run's own, Codex, Pi and
//! Devin on a home of the run's own, each with a copy of the login), never the owner's, and
//! the owner's setup and login are looked at after it as a rig's end looks: a read that
//! changed one, or rotated a login, is a failure.
//!
//! It checks, for each harness, that the list is the account's (not stale), names models
//! with a label, a contributor model only with its warning and never as the default, and
//! a default where the list has a model that is neither. The run says ids and counts,
//! never a login.
//!
//! ```text
//! cargo xtask live models                 every harness installed
//! cargo xtask live models claude devin    some
//! ```

use std::path::Path;

use cf_e2e::live::machine::Installed;
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, ask, machine, Harness, Log};
use cf_e2e::process::own_var;
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// The harnesses this ConsensFlow reads the models of.
const READ: [Harness; 4] = [Harness::Claude, Harness::Codex, Harness::Pi, Harness::Devin];

/// The harnesses the run names, every one read where it names none.
fn named() -> Result<Vec<Harness>> {
    let words = live::list("harness");
    if words.is_empty() {
        return Ok(READ.to_vec());
    }
    words
        .iter()
        .map(|word| {
            Harness::from_name(word)
                .filter(|harness| READ.contains(harness))
                .ok_or_else(|| {
                    Error::Daemon(format!("no harness whose models are read is called {word}"))
                })
        })
        .collect()
}

/// What `harness` is read with: the run's search path and shell, ConsensFlow's home in
/// `scratch`, and the variables of the home the kit gives the harness's windows.
fn env(installed: &Installed, scratch: &Path) -> Result<Value> {
    let mut vars = serde_json::Map::new();
    for name in ["PATH", "USER", "LANG", "TMPDIR", "SHELL", "HOME"] {
        if let Some(value) = own_var(name) {
            vars.insert(name.to_owned(), Value::String(value));
        }
    }
    vars.insert(
        "CONSENSFLOW_HOME".to_owned(),
        json!(scratch.join("consensflow")),
    );
    for (name, value) in installed.home_vars()? {
        vars.insert(name, Value::String(value));
    }
    Ok(Value::Object(vars))
}

/// What one harness's list came to.
fn checks_of(harness: Harness, listed: &Value, log: &Log) -> Vec<Check> {
    let who = harness.name();
    let listing = &listed["listing"];
    let models = listing["models"].as_array().cloned().unwrap_or_default();
    let freshness = &listing["freshness"];
    let ids: Vec<String> = models
        .iter()
        .map(|model| match model["provider"].as_str() {
            Some(provider) => format!("{provider}/{}", live::text(&model["id"])),
            None => live::text(&model["id"]),
        })
        .collect();
    log.say(format!(
        "{who}: {} models by {}, freshness {}, default {}; first: {}",
        ids.len(),
        live::text(&listing["how"]),
        freshness,
        listed["default"],
        ids.iter().take(6).cloned().collect::<Vec<_>>().join(", ")
    ));
    let contributors: Vec<&String> = models
        .iter()
        .zip(&ids)
        .filter(|(model, _)| model["contributor"] == true)
        .map(|(_, id)| id)
        .collect();
    let warned = contributors
        .iter()
        .all(|id| listed["warnings"].get(id.as_str()).is_some());
    let default = listed["default"].as_str();
    let usable = models
        .iter()
        .any(|model| model["contributor"] != true && model["disabled"] != true);
    vec![
        Check::new(
            who,
            "lists models",
            !models.is_empty(),
            format!("{} models", ids.len()),
        ),
        Check::new(
            who,
            "the account's list, not stale",
            freshness["state"] == "fresh",
            format!("{freshness}"),
        ),
        Check::new(
            who,
            "every model has a label",
            models
                .iter()
                .all(|model| !live::text(&model["label"]).is_empty()),
            String::new(),
        ),
        Check::new(
            who,
            "a contributor model has its warning",
            warned,
            format!("{} contributor models", contributors.len()),
        ),
        Check::new(
            who,
            "a default where one can be, never a contributor model",
            default.is_some() == usable
                && default.is_none_or(|id| !contributors.iter().any(|c| *c == id)),
            format!("default {}", default.unwrap_or("none")),
        ),
    ]
}

#[test]
#[ignore = "opt-in: reads each harness's model list as the daemon does, no model turn; cargo xtask live models (npm run live:models)"]
fn each_harness_lists_its_models_with_no_model_turn_and_no_login_of_the_owner_s_changed() -> Result
{
    // No window runs here, and no model: a `--models` would be read by nothing.
    let log = Log::own_agents("live models", "it reads what each harness lists")?;
    ask::prepare()?;
    let harnesses = named()?;
    let mut checks = Vec::new();
    for harness in harnesses {
        let made = cf_e2e::door::made_in(
            &machine::live_folder("models")?,
            &format!("{}-", harness.name()),
        )
        .map_err(|source| Error::File {
            action: "make a folder in",
            path: machine::live_folder("models").unwrap_or_default(),
            source,
        })?;
        let scratch = made.hand_on();
        let installed = machine::installed(harness)?;
        let listed = env(&installed, &scratch).and_then(|env| {
            ask::question(&json!({ "op": "models", "kind": harness.kind(), "env": env }))
        });
        let kept = installed.finish_homes();
        drop(installed);
        let removed = cf_e2e::door::Door::of_run().remove_made(&scratch);
        log.say(format!(
            "{}: its throwaway {} removed: {}",
            harness.name(),
            scratch.display(),
            removed.is_gone()
        ));
        checks.push(Check::new(
            harness.name(),
            "the owner's setup and login are as they were after the read",
            kept.is_ok(),
            kept.err().map(|said| said.to_string()).unwrap_or_default(),
        ));
        match listed {
            Ok(listed) => checks.extend(checks_of(harness, &listed, &log)),
            Err(failed) => checks.push(Check::failed(
                harness.name(),
                "lists models",
                failed.to_string(),
            )),
        }
    }
    report::conclude(&log, "what each harness's list came to", &checks)
}
