use std::fs;
use std::path::Path;

use cf_base::env::Env;
use serde_json::{json, Value};

use super::*;
use crate::contract::{Agent, LaunchId};
use crate::testing::{finished, Fakes};

const LAUNCH: &str = "11111111-1111-4111-8111-111111111111";

/// A Muse installed where its installer puts it under `home`, its launcher saying it
/// runs `version` (none: no `.muse-version`), the binary it names beside it.
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
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        fs::write(bin.join(format!("muse-bin-{version}{suffix}")), "binary").unwrap();
    }
    Env::from_vars([
        ("HOME", home.to_str().unwrap()),
        ("USERPROFILE", home.to_str().unwrap()),
        ("XDG_DATA_HOME", home.join("data").to_str().unwrap()),
        ("XDG_CONFIG_HOME", home.join("config").to_str().unwrap()),
        (
            "CONSENSFLOW_HOME",
            home.join("consensflow").to_str().unwrap(),
        ),
    ])
}

fn adapter(env: &Env) -> MuseAdapter {
    MuseAdapter::new(&Fakes::new(env).services(env, Path::new("")))
}

/// A worker's launch on `muse-spark-1.3` at `minimal`, on `resume` where given.
fn launch<'a>(id: &'a LaunchId, resume: Option<&'a str>) -> Launch<'a> {
    Launch {
        id,
        project: 1,
        handle: "muse-1",
        role: "worker",
        directory: "/work",
        resume,
        previous_gone: true,
        message: Some("Do it."),
        agent: Some(Agent {
            model: Some("muse-spark-1.3"),
            effort: Some("minimal"),
            thinking: None,
            designer: false,
        }),
        instructions: "# ConsensFlow worker",
        specs: None,
        first_message_ms: 1,
        starts_given_up: 0,
    }
}

fn prepared(env: &Env, resume: Option<&str>) -> Result<Prepared, String> {
    let id = LaunchId::new(LAUNCH).unwrap();
    finished(adapter(env).prepare(&launch(&id, resume)))
}

/// The hooks a command line gives Muse with `-c hooks=`.
fn hooks_of(argv: &[String]) -> Value {
    let at = argv.iter().position(|word| word == "-c").unwrap();
    serde_json::from_str(argv[at + 1].strip_prefix("hooks=").unwrap()).unwrap()
}

#[test]
fn a_launch_is_refused_in_plain_words_where_muse_is_missing_or_older_than_consensflow_reads() {
    let home = tempfile::tempdir().unwrap();
    let nowhere = Env::from_vars([("HOME", home.path().to_str().unwrap())]);
    assert_eq!(
        prepared(&nowhere, None).err().unwrap(),
        "muse is not installed on this machine"
    );
    let old = installed(home.path(), Some("1.4.12-R6000.1"));
    assert_eq!(
        prepared(&old, None).err().unwrap(),
        "Muse Code 1.4.13 or newer is required. Update Muse Code before opening a pane."
    );
}

#[test]
fn a_fresh_window_runs_the_pinned_binary_in_full_permission_with_its_hook_and_its_first_message_last(
) {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let prepared = prepared(&env, None).unwrap();
    let argv = &prepared.argv;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let binary = home
        .path()
        .join(".local")
        .join("bin")
        .join(format!("muse-bin-1.4.13-R6996.1{suffix}"));
    assert_eq!(argv[0], binary.to_string_lossy());
    assert_eq!(
        argv[3..],
        [
            "--model",
            "muse-spark-1.3",
            "--reasoning-effort",
            "minimal",
            "--yolo",
            "--contributor-mode",
            "off",
            "Do it."
        ]
    );
    assert_eq!(
        prepared.env,
        [("MUSE_NO_AUTO_UPDATE".to_owned(), "1".to_owned())]
    );
    assert_eq!(prepared.drop_env, ["META_API_KEY"]);
    assert_eq!(prepared.native_session, None);
    // The hook: the window's own `cf`, the role written for the launch, and the file it
    // writes the window's sessions in, both in the launch's folder.
    let hooks = hooks_of(argv);
    let command = hooks["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let folder = home
        .path()
        .join("consensflow")
        .join("integrations")
        .join("muse")
        .join(LAUNCH);
    let role = [
        "role",
        ".claude",
        "skills",
        "consensflow-worker",
        "SKILL.md",
    ]
    .iter()
    .fold(folder.clone(), |path, part| path.join(part));
    assert!(command.contains(" hook muse-session "), "{command}");
    assert!(
        command.contains(&format!("'{}'", role.display())),
        "{command}"
    );
    assert!(
        command.ends_with(&format!("'{}'", folder.join("sessions.jsonl").display())),
        "{command}"
    );
    assert_eq!(fs::read_to_string(role).unwrap(), "# ConsensFlow worker");
    assert_eq!(
        hooks.as_object().unwrap().len(),
        1,
        "no hooks of the owner's"
    );
}

#[test]
fn a_resumed_window_opens_on_its_session_and_takes_its_first_message_once_it_is_up() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let session = "01a17bc9-e758-77f3-ac8d-a725edf373ae";
    let prepared = prepared(&env, Some(session)).unwrap();
    assert_eq!(prepared.argv[3..5], ["resume", session]);
    assert!(
        !prepared.argv.iter().any(|word| word == "Do it."),
        "{:?}",
        prepared.argv
    );
    assert_eq!(prepared.native_session.as_deref(), Some(session));
    let id = LaunchId::new(LAUNCH).unwrap();
    let muse = adapter(&env);
    assert!(muse.first_message_once_up(&launch(&id, Some(session))));
    assert!(!muse.first_message_once_up(&launch(&id, None)));
}

#[test]
fn the_owner_s_hooks_are_kept_for_the_run_and_their_settings_are_never_written() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let settings = home.path().join("config/muse/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let owner = json!({
        "schema_version": 1,
        "hooks": { "Stop": [{ "matcher": "", "hooks": [{ "type": "command", "command": "owner-stop" }] }] },
    });
    fs::write(&settings, owner.to_string()).unwrap();
    let prepared = prepared(&env, None).unwrap();
    let hooks = hooks_of(&prepared.argv);
    assert_eq!(hooks["Stop"], owner["hooks"]["Stop"]);
    assert_eq!(hooks["SessionStart"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read_to_string(&settings).unwrap(), owner.to_string());
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

#[test]
fn its_servers_start_with_the_window_on_a_mac_and_its_box_is_cleared_with_control_c() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let expected = if cfg!(target_os = "macos") {
        Servers::AtLaunch
    } else {
        Servers::Unknown
    };
    assert_eq!(adapter(&env).servers(), expected);
    let windows = Env::from_vars([("OS", "Windows_NT"), ("HOME", "/h")]);
    assert_eq!(adapter(&windows).servers(), Servers::Unknown);
    assert_eq!(adapter(&env).input(), Input::Pasted { clear: &[0x03] });
    assert!(
        !adapter(&env).keeps_first_instructions(),
        "the hook says the role at each start"
    );
}

#[test]
fn a_specmint_muse_has_installed_is_used_where_it_is_and_consensflow_s_own_copy_otherwise() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let muse = adapter(&env);
    assert!(matches!(
        muse.specmint(Variant::Core).unwrap(),
        Skill::Own(_)
    ));
}
