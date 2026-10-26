use std::fs;

use super::*;
use crate::contract::LaunchId;
use crate::testing::{finished, Fakes};

/// A Muse installed where its installer puts it under `home`, its launcher saying it
/// runs `version` (none: no `.muse-version`).
fn installed(home: &Path, version: Option<&str>) -> Env {
    let bin = home.join(".local").join("bin");
    fs::create_dir_all(&bin).unwrap();
    let launcher = bin.join(if cfg!(windows) { "muse.cmd" } else { "muse" });
    fs::write(&launcher, "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
    }
    if let Some(version) = version {
        fs::write(bin.join(".muse-version"), format!("{version}\n")).unwrap();
    }
    Env::from_vars([
        ("HOME", home.to_str().unwrap()),
        ("USERPROFILE", home.to_str().unwrap()),
        ("XDG_DATA_HOME", home.join("data").to_str().unwrap()),
    ])
}

fn adapter(env: &Env) -> MuseAdapter {
    MuseAdapter::new(&Fakes::new(env).services(env, Path::new("")))
}

/// Why a launch on `env` was refused.
fn refused(env: &Env) -> String {
    let id = LaunchId::new("11111111-1111-4111-8111-111111111111").unwrap();
    let launch = Launch {
        id: &id,
        project: 1,
        handle: "muse-1",
        role: "staff",
        directory: "/work",
        resume: None,
        previous_gone: false,
        message: Some("Do it."),
        agent: None,
        instructions: "role text",
        specs: None,
        first_message_ms: 1,
        starts_given_up: 0,
    };
    match finished(adapter(env).prepare(&launch)) {
        Ok(_) => panic!("a Muse window was prepared"),
        Err(reason) => reason,
    }
}

#[test]
fn a_launch_is_refused_in_plain_words_an_older_muse_for_its_version_first() {
    let home = tempfile::tempdir().unwrap();
    let nowhere = Env::from_vars([("HOME", home.path().to_str().unwrap())]);
    assert_eq!(refused(&nowhere), "muse is not installed on this machine");
    let old = installed(home.path(), Some("1.4.12-R6000.1"));
    assert_eq!(
        refused(&old),
        "Muse Code 1.4.13 or newer is required. Update Muse Code before opening a pane."
    );
    for version in [Some("1.4.13-R6996.1"), Some("not a version"), None] {
        let env = installed(home.path(), version);
        if version.is_none() {
            fs::remove_file(home.path().join(".local/bin/.muse-version")).unwrap();
        }
        assert_eq!(refused(&env), NOT_YET, "{version:?}");
    }
    assert_eq!(
        adapter(&nowhere).specmint(Variant::Core).unwrap_err(),
        NOT_YET
    );
}

#[test]
fn a_session_is_resumable_as_its_log_says() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let muse = adapter(&env);
    let resumable = |session| finished(muse.resumable(session));
    assert_eq!(resumable("s1"), Some(false), "no log, and no index");
    let folder = home.path().join("data/muse/sessions/2026/10/26/s1");
    fs::create_dir_all(&folder).unwrap();
    let accepted = r#"{"schema_version":1,"id":"r1","sequence":1,"stream":{"kind":"session","id":"s1"},"payload_type":"runtime.user_intent.accepted","payload":{"intent_id":"t1","surface":"main","semantic_kind":{"kind":"chat"},"refill_blocks":[{"kind":"text","text":"Go."}]}}"#;
    fs::write(folder.join("session.jsonl"), format!("{accepted}\n")).unwrap();
    assert_eq!(resumable("s1"), Some(true));
    // A log there and unreadable, with no index: nothing is told.
    fs::write(folder.join("session.jsonl"), "null\n").unwrap();
    assert_eq!(resumable("s1"), None);
}
