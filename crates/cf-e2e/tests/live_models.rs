//! Live: what each harness lists of its models (CHF-03), read as the daemon reads it
//! ([`Adapter::models`](cf_harness::contract::Adapter::models), through `harness-ask`), with
//! no model turn. It is opt-in (`cargo xtask live models`, `npm run live:models`): it needs
//! each harness named installed and logged in, and is no part of `check` or of CI. It spends
//! no quota: Claude's `initialize`, Codex's `model/list`, Pi's `get_available_models` and
//! `devin models list` run no turn.
//!
//! Each read runs on a throwaway home of the run's own, never the owner's config: Claude in
//! ConsensFlow's own config folder of that home, signed out (its list is the same signed
//! in); Codex in a `CODEX_HOME` holding a copy of `auth.json`; Pi in a
//! `PI_CODING_AGENT_DIR` holding copies of its login, catalog and settings; Devin in a
//! `HOME` holding a copy of its `credentials.toml`. Each copy is made with the mode 600,
//! and the run removes the throwaway when it ends or fails. The owner's login files are
//! hashed before and after: a read that rotated one is a failure.
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

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

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

/// The owner's login file of `harness`, under `home`.
fn login(home: &Path, harness: Harness) -> Option<PathBuf> {
    match harness {
        Harness::Codex => Some(home.join(".codex/auth.json")),
        Harness::Pi => Some(home.join(".pi/agent/auth.json")),
        Harness::Devin => Some(home.join(".local/share/devin/credentials.toml")),
        _ => None,
    }
}

/// A digest of the file at `path`, none where there is none: what a run compares,
/// never what the file holds.
fn digest(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

/// Copies the owner's `from` to `to`, readable by its owner alone.
fn copy_secret(from: &Path, to: &Path) -> Result {
    let fail = |source| Error::File {
        action: "copy a login to",
        path: to.to_owned(),
        source,
    };
    std::fs::create_dir_all(to.parent().unwrap_or(to)).map_err(fail)?;
    std::fs::copy(from, to).map_err(fail)?;
    #[cfg(unix)]
    std::fs::set_permissions(to, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(fail)?;
    Ok(())
}

/// What `harness` is read with: the run's search path and shell, a home of `scratch`'s,
/// ConsensFlow's home in it, and the copies the harness reads its login from.
fn env(harness: Harness, home: &Path, scratch: &Path) -> Result<Value> {
    let mut vars = serde_json::Map::new();
    for name in ["PATH", "USER", "LANG", "TMPDIR", "SHELL"] {
        if let Some(value) = own_var(name) {
            vars.insert(name.to_owned(), Value::String(value));
        }
    }
    let own = scratch.join("home");
    vars.insert("HOME".to_owned(), json!(own));
    vars.insert(
        "CONSENSFLOW_HOME".to_owned(),
        json!(scratch.join("consensflow")),
    );
    match harness {
        Harness::Codex => {
            copy_secret(
                &home.join(".codex/auth.json"),
                &scratch.join("codex/auth.json"),
            )?;
            vars.insert("CODEX_HOME".to_owned(), json!(scratch.join("codex")));
        }
        Harness::Pi => {
            for name in [
                "auth.json",
                "models-store.json",
                "models.json",
                "settings.json",
            ] {
                let from = home.join(".pi/agent").join(name);
                if from.exists() {
                    copy_secret(&from, &scratch.join("pi-agent").join(name))?;
                }
            }
            vars.insert(
                "PI_CODING_AGENT_DIR".to_owned(),
                json!(scratch.join("pi-agent")),
            );
        }
        Harness::Devin => {
            let credentials = ".local/share/devin/credentials.toml";
            copy_secret(&home.join(credentials), &own.join(credentials))?;
            vars.insert("XDG_CONFIG_HOME".to_owned(), json!(own.join(".config")));
            vars.insert("XDG_DATA_HOME".to_owned(), json!(own.join(".local/share")));
        }
        _ => {}
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
    let home = machine::home()?;
    let harnesses = named()?;
    let mut checks = Vec::new();
    for harness in harnesses {
        let scratch = machine::live_folder("models")?.join(format!(
            "{}-{}",
            harness.name(),
            std::process::id()
        ));
        let before = login(&home, harness).map(|file| digest(&file));
        let env = env(harness, &home, &scratch);
        let listed = env.and_then(|env| {
            ask::question(&json!({ "op": "models", "kind": harness.kind(), "env": env }))
        });
        let after = login(&home, harness).map(|file| digest(&file));
        let removed = std::fs::remove_dir_all(&scratch);
        log.say(format!(
            "{}: its throwaway {} removed: {}",
            harness.name(),
            scratch.display(),
            removed.is_ok()
        ));
        checks.push(Check::new(
            harness.name(),
            "the owner's login is the same after the read",
            before == after,
            String::new(),
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
