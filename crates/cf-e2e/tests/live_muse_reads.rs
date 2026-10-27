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
//! the human's own folder is proved untouched. The copy is made only where every login in
//! `auth.json` is kept in the keychain (its `storage` key, read and never printed): a login
//! held in a file would be carried by the copy, and a refresh from it could rotate a token
//! the human's own login depends on. The copy is removed when the run ends or fails (T-816).
//! A run on a data folder with no catalog cache
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

/// Whether every login Muse's `auth.json` names is kept in the keychain (`providers.<name>.storage`
/// is `keychain`). Only that key is read, and never printed. A login held in a file is a
/// secret the copy of the config would carry, and Muse refreshes an expiring token it finds,
/// which rotates the one the human's own login depends on: nothing is copied then.
fn login_in_keychain(auth: &str) -> std::result::Result<(), String> {
    let refused = |why: &str| {
        Err(format!(
            "{why}: the config of Muse is not copied, for a copy of a login that is held anywhere but the keychain could be refreshed and rotate the token the human's own login depends on"
        ))
    };
    let Ok(found) = serde_json::from_str::<Value>(auth) else {
        return refused("Muse's auth.json is not JSON");
    };
    let providers = found
        .get("providers")
        .and_then(Value::as_object)
        .filter(|providers| !providers.is_empty());
    let Some(providers) = providers else {
        return refused("Muse's auth.json names no login");
    };
    if providers
        .values()
        .all(|login| login.get("storage").and_then(Value::as_str) == Some("keychain"))
    {
        Ok(())
    } else {
        refused("a login of Muse's is not kept in the keychain")
    }
}

/// The scratch folder of a run, removed when it is dropped: whether the run ended or failed on an
/// assertion, a copy of the human's Muse config is never left in the temporary folder.
struct Scratch(PathBuf);

impl Scratch {
    fn at(folder: PathBuf) -> Self {
        Self(folder)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // The folder this run made, by its own name: nothing else is removed.
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
    // Where the login is kept decides whether a copy of the config is safe to make.
    let auth = files::read_string(&config.join("auth.json"))?;
    login_in_keychain(&auth).map_err(Error::Daemon)?;
    let scratch =
        Scratch::at(std::env::temp_dir().join(format!("muse-reads-{}", std::process::id())));
    copy_tree(&config, &scratch.0.join("config/muse"))?;
    copy_tree(
        &home.join(".local/share/muse/model-catalog"),
        &scratch.0.join("data/muse/model-catalog"),
    )?;
    let env = env(&scratch.0);

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

    drop(scratch);
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

#[test]
fn a_login_held_anywhere_but_the_keychain_is_not_copied() {
    // Key names only, as `auth.json` has them: `providers.<name>.storage` says where the secret is.
    let kept =
        r#"{"schema_version":2,"providers":{"meta":{"storage":"keychain","mechanism":"oauth"}}}"#;
    assert_eq!(login_in_keychain(kept), Ok(()));
    for held in [
        r#"{"providers":{"meta":{"storage":"keychain_fallback_file"}}}"#,
        r#"{"providers":{"meta":{"storage":"keychain"},"other":{"storage":"file"}}}"#,
        r#"{"providers":{"meta":{"mechanism":"oauth"}}}"#,
        r#"{"providers":{}}"#,
        r#"{}"#,
        "not json",
    ] {
        let refused = login_in_keychain(held).expect_err(held);
        assert!(refused.contains("not copied"), "{refused}");
        assert!(
            !refused.contains("fallback_file") && !refused.contains("file\""),
            "{refused}"
        );
    }
}

#[test]
fn the_scratch_folder_goes_when_the_run_fails_as_when_it_ends() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("muse-reads-1");
    let made = || {
        std::fs::create_dir_all(folder.join("config/muse")).unwrap();
        std::fs::write(folder.join("config/muse/auth.json"), "{}").unwrap();
    };
    made();
    drop(Scratch::at(folder.clone()));
    assert!(!folder.exists(), "removed at the end");
    made();
    let failing = std::panic::catch_unwind(|| {
        let _scratch = Scratch::at(folder.clone());
        panic!("an assertion of the run failed");
    });
    assert!(failing.is_err());
    assert!(!folder.exists(), "removed on a failed assertion too");
}
