//! What a live test that copies a harness's config folder guards itself with (T-816): that
//! the login in it is kept where a copy cannot carry it, and that the copy is removed when
//! the run ends or fails. Here, and not in the live test's own file, where the kit holds every
//! test to be ignored unless asked for.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Whether every login Muse's `auth.json` names is kept in the keychain (`providers.<name>.storage`
/// is `keychain`). Only that key is read, and never printed. A login held in a file is a
/// secret the copy of the config would carry, and Muse refreshes an expiring token it finds,
/// which rotates the one the human's own login depends on: nothing is copied then.
pub fn login_in_keychain(auth: &str) -> std::result::Result<(), String> {
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
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn at(folder: PathBuf) -> Self {
        Self(folder)
    }
}

/// What a run copies of a harness's folders: a file as it is, with the mode it had (a login
/// file is 0600, and `files::copy` writes fresh bytes at the process's default, 0644: T-841),
/// in folders only the owner enters.
pub fn copy_kept(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(folder) = to.parent() {
        owner_only_folders(folder)?;
    }
    // `std::fs::copy` opens the target with the source's mode and copies its permission bits.
    std::fs::copy(from, to).map(|_| ())
}

/// `folder` and the folders above it that are not there yet made, readable by their owner alone
/// (where the system has modes).
fn owner_only_folders(folder: &Path) -> io::Result<()> {
    let mut made = std::fs::DirBuilder::new();
    made.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut made, 0o700);
    made.create(folder)
}

/// The folder `from` copied to `to` the same way, the files under it too.
pub fn copy_folder_kept(from: &Path, to: &Path) -> io::Result<()> {
    for entry in std::fs::read_dir(from)?.flatten() {
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy_folder_kept(&source, &target)?;
        } else {
            copy_kept(&source, &target)?;
        }
    }
    Ok(())
}

/// What a run of Muse's reads copies of the human's folders into `scratch`: the login file
/// (its secret is in the keychain, which the copy does not carry) and the model catalog cache,
/// and nothing else of the config (the settings and their backup, what it trusts) or of the data
/// (its sessions): Muse lists the account's models from the cache and the login alone.
pub fn copy_muse_into(home: &Path, scratch: &Path) -> io::Result<()> {
    copy_kept(
        &home.join(".config/muse/auth.json"),
        &scratch.join("config/muse/auth.json"),
    )?;
    copy_folder_kept(
        &home.join(".local/share/muse/model-catalog"),
        &scratch.join("data/muse/model-catalog"),
    )
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // The folder this run made, by its own name: nothing else is removed.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_held_anywhere_but_the_keychain_is_not_copied() {
        // Key names only, as `auth.json` has them: `providers.<name>.storage` says where the secret is.
        let kept = r#"{"schema_version":2,"providers":{"meta":{"storage":"keychain","mechanism":"oauth"}}}"#;
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

    #[cfg(unix)]
    #[test]
    fn a_file_is_copied_with_the_mode_it_had_into_folders_only_its_owner_enters() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("auth.json");
        std::fs::write(&source, "{}").unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600)).unwrap();
        let target = root.path().join("scratch/config/muse/auth.json");
        copy_kept(&source, &target).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "{}");
        assert_eq!(mode(&target), 0o600, "a login file stays the owner's alone");
        for folder in ["scratch", "scratch/config", "scratch/config/muse"] {
            assert_eq!(mode(&root.path().join(folder)), 0o700, "{folder}");
        }
        // A file that was readable stays so, and a folder copies file by file.
        let catalog = root.path().join("model-catalog");
        std::fs::create_dir_all(catalog.join("inner")).unwrap();
        std::fs::write(catalog.join("rows.json"), "[]").unwrap();
        std::fs::write(catalog.join("inner/more.json"), "[]").unwrap();
        std::fs::set_permissions(
            catalog.join("rows.json"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let copied = root.path().join("scratch/data/model-catalog");
        copy_folder_kept(&catalog, &copied).unwrap();
        assert_eq!(mode(&copied.join("rows.json")), 0o644);
        assert!(copied.join("inner/more.json").is_file());
    }

    #[test]
    fn a_run_of_muse_copies_the_login_file_and_the_model_catalog_and_nothing_else_of_the_config() {
        let root = tempfile::tempdir().unwrap();
        let (home, scratch) = (root.path().join("home"), root.path().join("scratch"));
        let config = home.join(".config/muse");
        let catalog = home.join(".local/share/muse/model-catalog");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&catalog).unwrap();
        // The files of the human's folder: the login, the settings and their backup, what it
        // trusts, a lock; and the one cache.
        for name in [
            "auth.json",
            "settings.json",
            "settings.json.bak",
            "trust.json",
            ".auth.json.lock",
        ] {
            std::fs::write(config.join(name), "{}").unwrap();
        }
        std::fs::write(catalog.join("6d657461__p746268.json"), "{}").unwrap();
        // Something else of the data folder, a session, which is the human's.
        std::fs::create_dir_all(home.join(".local/share/muse/sessions")).unwrap();
        std::fs::write(home.join(".local/share/muse/sessions/one.jsonl"), "{}").unwrap();
        copy_muse_into(&home, &scratch).unwrap();
        let mut found: Vec<String> = Vec::new();
        let mut folders = vec![scratch.clone()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                if entry.path().is_dir() {
                    folders.push(entry.path());
                } else {
                    let name = entry.path().strip_prefix(&scratch).unwrap().to_owned();
                    found.push(name.display().to_string());
                }
            }
        }
        found.sort();
        assert_eq!(
            found,
            [
                "config/muse/auth.json",
                "data/muse/model-catalog/6d657461__p746268.json"
            ]
        );
    }
}
