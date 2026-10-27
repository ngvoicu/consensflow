//! Live: what Muse Code says without a model turn (MUS-14, MUS-15): the models its host
//! lists, and the allowance `muse usage --json` reports, read as the daemon reads them. It is
//! opt-in (`cargo xtask live muse-reads`, `npm run live:muse-reads`): it needs Muse installed
//! and logged in, and is ignored unless asked for and is no part of `check` or of CI. It
//! spends no allowance: `model/list` and `muse usage` run no model turn, and neither needs an
//! allowance (a host with no login lists only what Muse bundles, which is said and fails
//! here: it is not the account's list).
//!
//! It checks, and fails where Muse no longer does so:
//! - the list is the account's (`providerCatalog`), not the bundled one, and holds a model
//!   that is no contributor model;
//! - every model whose id ends `-contributor` is marked one and carries a warning, and the
//!   default a member is given is none of them, though Muse marks one its own default;
//! - each model names the efforts it takes;
//! - `muse usage --json` answers: a quota (a count of the window, or an account with no
//!   allowance), or nothing known;
//! - neither read changed a file of the human's `~/.config/muse`.
//!
//! Muse rewrites its `auth.json` itself on any command that uses the login (its
//! `user_avatar_url`, a signed link that expires: seen 2026-10-26 by T-705 and 2026-10-27
//! here), so the reads run on a throwaway copy of the config and of the model catalog cache
//! (`XDG_CONFIG_HOME`, `XDG_DATA_HOME`), the login being the keychain's, which is global, and
//! the human's own folder is proved untouched. A run on a data folder with no catalog cache
//! lists only Muse's bundled model, which is why the cache is copied.
//!
//! The run says counts and kinds, never an account's name, a file's words or a model's price.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use cf_e2e::live::{ask, machine, Log};
use cf_e2e::process::own_var;
use cf_e2e::{files, Error, Result};
use serde_json::{json, Value};

/// Every file under `root` with a digest of its bytes: what a run changed is said by name,
/// never by what the files hold (Muse's config names its account).
fn tree(root: &Path) -> Result<BTreeMap<PathBuf, u64>> {
    let mut found = BTreeMap::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let mut hasher = DefaultHasher::new();
                files::read(&path)?.hash(&mut hasher);
                found.insert(path, hasher.finish());
            }
        }
    }
    Ok(found)
}

/// A failure of the file or folder `path`, as the kit says one.
fn failed(action: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.to_owned();
    move |source| Error::File {
        action,
        path,
        source,
    }
}

/// The folder `from` copied to `to`, with what is under it.
fn copy_tree(from: &Path, to: &Path) -> Result {
    for entry in std::fs::read_dir(from)
        .map_err(failed("list", from))?
        .flatten()
    {
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            files::copy(&source, &target)?;
        }
    }
    Ok(())
}

/// What Muse is given to run in: the run's own home, shell and search path, and its folders
/// (config, data, cache, state) those of `scratch`. No key of an account's is passed.
fn env(scratch: &Path) -> Value {
    let mut vars = serde_json::Map::new();
    for name in ["HOME", "PATH", "USER", "LANG", "TMPDIR", "SHELL"] {
        if let Some(value) = own_var(name) {
            vars.insert(name.to_owned(), Value::String(value));
        }
    }
    for (name, folder) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_STATE_HOME", "state"),
    ] {
        vars.insert(name.to_owned(), json!(scratch.join(folder)));
    }
    Value::Object(vars)
}

#[test]
#[ignore = "opt-in: reads Muse's model list and usage, no model turn; cargo xtask live muse-reads (npm run live:muse-reads)"]
fn muse_lists_the_accounts_models_and_says_its_allowance_with_no_model_turn() -> Result {
    // No window runs here, and no model: a `--models` would be read by nothing.
    let _log = Log::own_agents("live muse-reads", "it reads what Muse says of itself")?;
    ask::prepare()?;
    let home = machine::home()?;
    let config = home.join(".config/muse");
    let before = tree(&config)?;
    let scratch = std::env::temp_dir().join(format!("muse-reads-{}", std::process::id()));
    copy_tree(&config, &scratch.join("config/muse"))?;
    copy_tree(
        &home.join(".local/share/muse/model-catalog"),
        &scratch.join("data/muse/model-catalog"),
    )?;
    let env = env(&scratch);

    let listed = ask::question(&json!({ "op": "muse_models", "env": env }))?;
    assert_eq!(
        listed["stale"], false,
        "Muse listed its bundled models, not the account's: is it logged in?"
    );
    let models = listed["models"].as_array().unwrap();
    assert!(!models.is_empty(), "no model listed");
    let contributors: Vec<&str> = models
        .iter()
        .filter(|model| model["contributor"] == true)
        .map(|model| model["id"].as_str().unwrap())
        .collect();
    for model in models {
        let id = model["id"].as_str().unwrap();
        assert_eq!(
            model["contributor"] == true,
            id.ends_with("-contributor"),
            "{id}: contributor by its id"
        );
        assert!(
            !model["efforts"].as_array().unwrap().is_empty(),
            "{id} names no effort"
        );
        assert_eq!(
            listed["warnings"].get(id).is_some(),
            contributors.contains(&id),
            "{id}: a warning exactly where it is a contributor model"
        );
    }
    let default = listed["default"]
        .as_str()
        .expect("a model that is no contributor's");
    assert!(
        !contributors.contains(&default),
        "the default {default} is a contributor model"
    );
    eprintln!(
        "muse-reads: {} models, {} contributor, default {default}",
        models.len(),
        contributors.len()
    );

    let usage = ask::question(&json!({ "op": "muse_usage", "env": env }))?;
    eprintln!(
        "muse-reads: usage says {}",
        usage
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("nothing known")
    );

    std::fs::remove_dir_all(&scratch).map_err(failed("remove", &scratch))?;
    let after = tree(&config)?;
    let changed: Vec<&PathBuf> = before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .collect();
    assert!(
        changed.is_empty(),
        "a read changed ~/.config/muse: {changed:?}"
    );
    Ok(())
}
