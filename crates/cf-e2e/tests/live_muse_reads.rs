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
//! the human's own folder is proved untouched. The copy is of `auth.json` and the model
//! catalog cache alone (T-841; not the settings, their backup or the trust file), the login
//! in a private file, and made only where every login in `auth.json` is kept in the keychain (its
//! `storage` key, read and never printed): a login held in a file would be carried by the copy,
//! and a refresh from it could rotate a token the human's own login depends on. The run says
//! where the copy is, and removes it when it ends or fails (T-816).
//! A run on a data folder with no catalog cache
//! lists only Muse's bundled model, which is why the cache is copied.
//!
//! The run says counts and kinds, never an account's name, a file's words or a model's price.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cf_e2e::door::source::Source;
use cf_e2e::live::guard::{copy_muse_into, login_in_keychain, Scratch};
use cf_e2e::live::{ask, machine, Log};
use cf_e2e::process::own_var;
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// A folder or file standing for the owner's, as a test makes one: read for `Guard`, never written.
fn owned(path: impl AsRef<std::path::Path>) -> cf_e2e::door::source::Source {
    cf_e2e::door::source::Account::this().named(path, cf_e2e::door::source::Purpose::Guard)
}

/// Compare every non-directory entry by metadata only, without following links: a source of the
/// owner's is walked and each entry of it stated, nothing opened.
fn tree(root: &Source) -> Result<BTreeMap<Source, cf_base::file::Stat>> {
    let at = |source: &Source| PathBuf::from(source.name());
    let meta = root.lstat().map_err(|source| Error::File {
        action: "stat",
        path: at(root),
        source,
    })?;
    if meta.file_type().is_symlink() {
        return Err(Error::File {
            action: "list",
            path: at(root),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "folder is a symbolic link",
            ),
        });
    }
    let mut found = BTreeMap::new();
    let entries = root.walk().map_err(|source| Error::File {
        action: "list",
        path: at(root),
        source,
    })?;
    for entry in entries {
        if entry.lstat().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let stat = entry.stat().map_err(|source| Error::File {
            action: "stat",
            path: at(&entry),
            source,
        })?;
        found.insert(entry, stat);
    }
    Ok(found)
}

/// The text of Muse's `auth.json` in `config`, read through its source and held in memory only.
fn login_text(config: &Source) -> Result<String> {
    let auth = config.join("auth.json");
    auth.read_to_string().map_err(|source| Error::File {
        action: "read",
        path: PathBuf::from(auth.name()),
        source,
    })
}

/// The paths that are not as they were between two looks, by name alone: never a content.
fn changed_paths(
    before: &BTreeMap<Source, cf_base::file::Stat>,
    after: &BTreeMap<Source, cf_base::file::Stat>,
) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .map(Source::name)
        .collect()
}

/// What Muse is given to run in: a home of the run's own, the shell and search path, and its folders
/// (config, data, cache, state) those of `scratch`. No key of an account's is passed.
fn env(scratch: &Path, home: &Path) -> Value {
    let mut vars = serde_json::Map::new();
    vars.insert("HOME".to_owned(), json!(home));
    for name in ["PATH", "USER", "LANG", "TMPDIR", "SHELL"] {
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
    let log = Log::own_agents("live muse-reads", "it reads what Muse says of itself")?;
    ask::prepare()?;
    let home = machine::home()?;
    // The owner's Muse folder is a source: listed, stated and read through it, never by a path.
    let config = owned(&home).join(".config/muse");
    let before = tree(&config)?;
    // Where the login is kept decides whether a copy of the config is safe to make.
    let auth = login_text(&config)?;
    login_in_keychain(&auth).map_err(Error::Daemon)?;
    let scratch = Scratch::made("muse-reads-").map_err(|source| Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })?;
    // What the run copies, and nothing else of Muse's folders: the login file (its keychain
    // entry is the secret, which the copy does not carry) and the model catalog cache. The login
    // is private; the cache keeps its mode. Both are in a folder only its owner enters. It is removed when the run ends or fails.
    eprintln!(
        "muse-reads: copying ~/.config/muse/auth.json and ~/.local/share/muse/model-catalog, and nothing else, into {} (removed when the run ends)",
        scratch.path().display()
    );
    copy_muse_into(owned(&home), scratch.path(), &auth).map_err(|source| Error::File {
        action: "copy the login and the model catalog of",
        path: home.join(".config/muse"),
        source,
    })?;
    // The question's homes are the run's, as every start's are: the home is a folder below the
    // scratch, and never the caller's own.
    let run_home = scratch.path().join("home");
    cf_e2e::door::make_dir(&run_home)?;
    let env = env(scratch.path(), &run_home);

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
    let changed = changed_paths(&before, &after);
    assert!(
        changed.is_empty(),
        "a read changed ~/.config/muse (compared by size, modification time and file identity only): {changed:?}"
    );
    log.passed();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case's reads of the owner's folder, on a decoy owner: the folder is listed and stated,
    /// its login read, and a change is named by its path and never by what it holds.
    #[test]
    fn the_case_reads_the_owners_muse_folder_through_a_source_and_names_a_change_by_its_path() {
        let root = cf_e2e::door::made("t-").unwrap();
        let config = root.path().join(".config/muse");
        cf_e2e::door::make_dir(&config).unwrap();
        cf_e2e::door::write(config.join("auth.json"), "decoy login words").unwrap();
        cf_e2e::door::write(config.join("settings.json"), "one").unwrap();
        let source = owned(root.path()).join(".config/muse");
        assert_eq!(login_text(&source).unwrap(), "decoy login words");
        let before = tree(&source).unwrap();
        assert_eq!(before.len(), 2);
        assert!(changed_paths(&before, &tree(&source).unwrap()).is_empty());
        cf_e2e::door::write(config.join("settings.json"), "changed, and longer").unwrap();
        let after = tree(&source).unwrap();
        let named = changed_paths(&before, &after);
        assert!(
            named.iter().all(|name| name.ends_with("settings.json")),
            "{named:?}"
        );
        assert!(!named.is_empty());
        assert!(!format!("{named:?}").contains("longer"));
        assert!(login_text(&owned(root.path().join("none"))).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn t1744_review_muse_tree_sees_a_fifo_without_opening_it() {
        let root = cf_e2e::door::made("t-").unwrap();
        let fifo = root.path().join("unrecognized-login");
        nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).unwrap();
        let seen = tree(&owned(root.path())).expect("a FIFO can be stated without being opened");
        assert!(
            seen.contains_key(&owned(&fifo)),
            "the FIFO was not compared"
        );
    }

    #[cfg(unix)]
    #[test]
    fn t1744_review_muse_tree_refuses_an_unreadable_folder() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = cf_e2e::door::made("t-").unwrap();
        let nested = root.path().join("nested");
        cf_e2e::door::make_dir(&nested).unwrap();
        cf_e2e::door::write(nested.join("unknown"), "synthetic").unwrap();
        cf_e2e::door::set_permissions(&nested, std::fs::Permissions::from_mode(0o000)).unwrap();
        let seen = tree(&owned(root.path()));
        cf_e2e::door::set_permissions(&nested, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(seen.is_err(), "an unreadable folder was silently skipped");
    }

    #[cfg(unix)]
    #[test]
    fn t1744_review_muse_tree_refuses_a_linked_root() {
        let root = cf_e2e::door::made("t-").unwrap();
        let actual = root.path().join("actual");
        let linked = root.path().join("linked");
        cf_e2e::door::make_dir(&actual).unwrap();
        cf_e2e::door::write(actual.join("unknown"), "synthetic").unwrap();
        cf_e2e::door::symlink(&actual, &linked).unwrap();
        assert!(
            tree(&owned(&linked)).is_err(),
            "a linked root looked like an empty tree"
        );
    }

    #[cfg(unix)]
    #[test]
    fn t1744_review_muse_tree_does_not_open_other_files() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = cf_e2e::door::made("t-").unwrap();
        let unknown = root.path().join("other-name.json");
        let nested = root.path().join("nested");
        cf_e2e::door::make_dir(&nested).unwrap();
        let nested_unknown = nested.join("another-name");
        for file in [&unknown, &nested_unknown] {
            cf_e2e::door::write(file, "synthetic").unwrap();
            cf_e2e::door::set_permissions(file, std::fs::Permissions::from_mode(0o000)).unwrap();
        }
        let seen =
            tree(&owned(root.path())).expect("metadata does not need to open an unknown file");
        assert!(seen.contains_key(&owned(&unknown)));
        assert!(seen.contains_key(&owned(&nested_unknown)));
    }

    #[test]
    fn t1744_muse_owner_tree_does_not_read_login_files() {
        let root = cf_e2e::door::made("t-").unwrap();
        for name in ["auth.json", "auth.json.bak", "settings.json"] {
            cf_e2e::door::write(root.path().join(name), "synthetic").unwrap();
        }
        let observed = tree(&owned(root.path())).unwrap();
        for name in ["auth.json", "auth.json.bak", "settings.json"] {
            assert_eq!(
                observed.get(&owned(root.path().join(name))),
                Some(&cf_base::file::stat(&root.path().join(name)).unwrap())
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn kit_round3_muse_fingerprint_does_not_follow_links_out() {
        let root = cf_e2e::door::made("t-").unwrap();
        let foreign = cf_e2e::door::made("t-").unwrap();
        cf_e2e::door::write(foreign.path().join("login"), "fabricated refresh token").unwrap();
        cf_e2e::door::symlink(foreign.path().join("login"), root.path().join("leak")).unwrap();
        let found = tree(&owned(root.path())).unwrap();
        assert!(
            !found.contains_key(&owned(root.path().join("leak"))),
            "a linked-out login was fingerprinted"
        );
    }
}
