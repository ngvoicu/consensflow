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
    Ok(env_with(&own_var, installed.home_vars()?, scratch))
}

/// [`env`], where the machine's variables are `lookup`'s and the home's are `home`.
fn env_with(
    lookup: &dyn Fn(&str) -> Option<String>,
    home: Vec<(String, String)>,
    scratch: &Path,
) -> Value {
    let mut vars = serde_json::Map::new();
    // `SystemRoot` and the system's other variables, which Codex on Windows needs to give a fresh
    // list and a default (T-1937); a machine without them has none to give.
    for name in [
        "PATH",
        "USER",
        "LANG",
        "TMPDIR",
        "SHELL",
        "SystemRoot",
        "WINDIR",
        "ComSpec",
        "PATHEXT",
    ] {
        if let Some(value) = lookup(name) {
            vars.insert(name.to_owned(), Value::String(value));
        }
    }
    vars.insert(
        "CONSENSFLOW_HOME".to_owned(),
        json!(scratch.join("consensflow")),
    );
    // A home of the run's own for every variable a program finds one by, then what the harness's
    // plan says over it: a plan that names only its own folder (Codex's `CODEX_HOME`) leaves the
    // read with no `HOME` of the caller's (K3 review T-1978, 2).
    for (name, value) in cf_e2e::standin_process::home_vars(&scratch.join("home")) {
        vars.insert(name.to_owned(), json!(value));
    }
    for (name, value) in home {
        vars.insert(name, Value::String(value));
    }
    Value::Object(vars)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A harness whose plan names only its own folder (Codex's `CODEX_HOME`) is read on the run's
    /// own home for the rest, never the caller's `HOME`, profile, AppData or XDG folders.
    #[test]
    fn a_read_whose_plan_names_only_its_own_folder_has_no_home_of_the_callers() {
        let machine = |name: &str| match name {
            "HOME" | "USERPROFILE" | "APPDATA" | "LOCALAPPDATA" | "XDG_CONFIG_HOME"
            | "HOMEDRIVE" | "HOMEPATH" => Some(format!("/the-callers/{name}")),
            "PATH" => Some("p".to_owned()),
            _ => None,
        };
        let plan = vec![("CODEX_HOME".to_owned(), "/scratch/home/.codex".to_owned())];
        let env = env_with(&machine, plan, Path::new("/scratch"));
        let slashed = |name: &str| env[name].as_str().unwrap_or("").replace('\\', "/");
        assert_eq!(slashed("HOME"), "/scratch/home");
        assert_eq!(slashed("USERPROFILE"), "/scratch/home");
        assert_eq!(slashed("XDG_CONFIG_HOME"), "/scratch/home/.config");
        assert_eq!(slashed("CODEX_HOME"), "/scratch/home/.codex");
        assert!(
            !env.to_string().contains("the-callers"),
            "a variable of the caller's reached the read: {env}"
        );
    }

    /// T-1937: a Windows Codex given no `SystemRoot` gets a stale model list with no default
    /// (0.170.0 and 0.171.0 alike); with it, a fresh one. The read is given the system's own
    /// variables, and the run's home still wins over the machine's.
    #[test]
    fn a_models_read_is_given_the_windows_system_variables_the_machine_has() {
        let machine = |name: &str| match name {
            "SystemRoot" => Some(r"C:\Windows".to_owned()),
            "WINDIR" => Some(r"C:\Windows".to_owned()),
            "ComSpec" => Some(r"C:\Windows\system32\cmd.exe".to_owned()),
            "PATHEXT" => Some(".EXE;.CMD".to_owned()),
            "PATH" => Some("p".to_owned()),
            "HOME" => Some("the-callers-home".to_owned()),
            _ => None,
        };
        let home = vec![("HOME".to_owned(), "the-runs-home".to_owned())];
        let env = env_with(&machine, home, Path::new("/scratch"));
        for name in ["SystemRoot", "WINDIR", "ComSpec", "PATHEXT"] {
            assert!(env[name].is_string(), "{name} was left out: {env}");
        }
        assert_eq!(env["HOME"], "the-runs-home");
        assert!(
            env["TEMP"].is_null(),
            "a variable the machine lacks is not made up"
        );
    }
}
