//! What a live test that copies a harness's config folder guards itself with (T-816): that
//! the login in it is kept where a copy cannot carry it, and that the copy is removed when
//! the run ends or fails. Here, and not in the live test's own file, where the kit holds every
//! test to be ignored unless asked for.

use std::path::PathBuf;

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
}
