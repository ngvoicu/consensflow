//! The reds of K3's doors, part 1 (T-2007): each case the design's readings (T-2000, T-2002)
//! and the reviews before them (T-1978, T-1997) found, held by the judge, the seal and the
//! window's and the helper's doors. The seal's cases run in memory, on a decoy account whose
//! every variable names a folder of another name; the judge's on folders this test makes.

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use serde_json::json;

use super::account::TestRoot;
use super::place::Act;
use super::spawn::{self, Judges, Program};
use super::{judge, made};

fn vars(list: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    list.iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

/// A run's homes, all below `home`, as the door gives them.
fn run_homes(home: &str) -> Vec<(OsString, OsString)> {
    crate::standin_process::home_vars(Path::new(home))
        .into_iter()
        .filter(|(name, _)| !spawn::PROFILE_HALVES.contains(name))
        .map(|(name, value)| (OsString::from(name), value.into_os_string()))
        .collect()
}

/// The seal with every folder below `/kit/run` the run's, and `/owner` the account's stores.
fn sealed_in_memory(
    env: Vec<(OsString, OsString)>,
    windows: bool,
) -> Result<Vec<(OsString, OsString)>, String> {
    let is_runs = |path: &Path| {
        let text = path.to_string_lossy().replace('\\', "/").to_lowercase();
        text.starts_with("/kit/run") || text.starts_with("k:/kit/run")
    };
    let in_protected = |path: &Path| path.to_string_lossy().starts_with("/owner");
    spawn::seal(
        env,
        &Judges {
            windows,
            is_runs: &is_runs,
            in_protected: &in_protected,
            kit: &[],
        },
    )
}

// ---- Correction 1: a Windows profile is judged joined. ----

#[test]
fn a_windows_profile_is_judged_as_its_two_halves_joined_against_the_profile() {
    let mut env = run_homes("K:\\kit\\run\\home");
    env.retain(|(name, _)| name != "USERPROFILE" && name != "HOME");
    env.extend(vars(&[
        ("HOME", "K:\\kit\\run\\home"),
        ("USERPROFILE", "K:\\kit\\run\\home"),
        ("HOMEDRIVE", "K:"),
        ("HOMEPATH", "\\kit\\run\\home"),
    ]));
    // Each half alone is no folder of the run's: `K:` and `\kit\run\home` are refused as homes,
    // the pair joined is the profile.
    assert!(
        sealed_in_memory(env.clone(), true).is_ok(),
        "{:?}",
        sealed_in_memory(env.clone(), true)
    );
    let mut elsewhere = env.clone();
    elsewhere.retain(|(name, _)| name != "HOMEPATH");
    elsewhere.push(("HOMEPATH".into(), "\\Users\\owner".into()));
    let refused = sealed_in_memory(elsewhere, true).unwrap_err();
    assert!(refused.contains("HOMEDRIVE and HOMEPATH"), "{refused}");
    let mut half = env;
    half.retain(|(name, _)| name != "HOMEDRIVE");
    assert!(sealed_in_memory(half, true).is_err());
}

// ---- Correction 2: the seal emits the canonical home names. ----

#[test]
fn a_home_spelled_in_another_case_is_refused_on_unix_and_named_as_itself_on_windows() {
    let mut env = run_homes("/kit/run/home");
    env.retain(|(name, _)| name != "HOME");
    env.push(("home".into(), "/kit/run/home".into()));
    let refused = sealed_in_memory(env.clone(), false).unwrap_err();
    assert!(refused.contains("spelled in another case"), "{refused}");
    let mut windows = run_homes("K:\\kit\\run\\home");
    windows.retain(|(name, _)| name != "HOME");
    windows.push(("home".into(), "K:\\kit\\run\\home".into()));
    let sealed = sealed_in_memory(windows, true).unwrap();
    assert!(sealed.iter().any(|(name, _)| name == "HOME"));
    assert!(!sealed.iter().any(|(name, _)| name == "home"));
}

#[test]
fn a_name_given_twice_in_different_case_is_refused_everywhere() {
    let mut env = run_homes("/kit/run/home");
    env.push(("appdata".into(), "/owner/AppData/Roaming".into()));
    for windows in [false, true] {
        let refused = sealed_in_memory(env.clone(), windows).unwrap_err();
        assert!(refused.contains("given twice"), "{refused}");
    }
}

#[test]
fn every_home_must_be_there_and_the_runs_and_no_other_variable_names_a_store() {
    let env = run_homes("/kit/run/home");
    assert!(sealed_in_memory(env.clone(), false).is_ok());
    for name in spawn::HOME_VARS {
        let mut without = env.clone();
        without.retain(|(held, _)| held != name);
        let refused = sealed_in_memory(without, false).unwrap_err();
        assert!(refused.contains(name), "{name}: {refused}");
        let mut owners = env.clone();
        owners.retain(|(held, _)| held != name);
        owners.push((name.into(), "/owner/elsewhere".into()));
        assert!(sealed_in_memory(owners, false).is_err(), "{name}");
    }
    let mut named = env.clone();
    named.push(("GIT_CONFIG_GLOBAL".into(), "/owner/.codex/config".into()));
    assert!(sealed_in_memory(named, false)
        .unwrap_err()
        .contains("GIT_CONFIG_GLOBAL"));
    // The search path may name a store: a harness's own program folder is on it.
    let mut path = env;
    path.push(("PATH".into(), "/owner/.pi/agent/bin:/usr/bin".into()));
    assert!(sealed_in_memory(path, false).is_ok());
}

#[test]
fn the_kits_own_variables_are_the_doors_and_no_callers() {
    let env = run_homes("/kit/run/home");
    let is_runs = |path: &Path| path.starts_with("/kit/run");
    let in_protected = |_: &Path| false;
    let kit = [(
        super::account::PROTECTED_FILE.to_owned(),
        "/kit/run/roots.json".to_owned(),
    )];
    let judges = Judges {
        windows: false,
        is_runs: &is_runs,
        in_protected: &in_protected,
        kit: &kit,
    };
    let mut changed = env.clone();
    changed.push((super::account::PROTECTED_FILE.into(), "/elsewhere".into()));
    assert!(spawn::seal(changed, &judges).is_err());
    assert!(spawn::seal(env.clone(), &judges).is_err(), "taken away");
    let mut given = env;
    given.push((super::MADE_ROOTS.into(), "/owner".into()));
    let none = Judges { kit: &[], ..judges };
    assert!(spawn::seal(given, &none).is_err(), "a caller's MADE_ROOTS");
}

// ---- Correction 3: a window is judged by its program and its update switches. ----

#[test]
fn a_harness_program_is_known_by_the_file_it_resolves_to() {
    let made = Path::new("/kit/run/bin");
    assert_eq!(
        spawn::program_of(
            &made.join("x"),
            Some(Path::new("/opt/muse/muse-bin-1.4.14")),
            false,
            false
        ),
        Program::Harness("muse")
    );
    assert_eq!(
        spawn::program_of(
            Path::new("/usr/local/bin/cc"),
            Some(Path::new("/Users/o/.local/share/claude/versions/2.1.330")),
            false,
            false
        ),
        Program::Harness("claude")
    );
    assert_eq!(
        spawn::program_of(Path::new("/o/.pi/agent/bin/pi"), None, true, false),
        Program::Harness("pi")
    );
    assert_eq!(
        spawn::program_of(Path::new("/usr/bin/security"), None, false, false),
        Program::Keychain
    );
    assert_eq!(
        spawn::program_of(Path::new("/o/.cargo/bin/cargo"), None, false, false),
        Program::Cargo
    );
    // A stand-in the run made, named for a harness, is the run's own.
    assert_eq!(
        spawn::program_of(&made.join("claude"), None, false, true),
        Program::Plain
    );
}

#[test]
fn a_harness_window_without_its_update_switch_is_refused() {
    let env = vars(&[("PI_OFFLINE", "1"), ("DISABLE_AUTOUPDATER", "1")]);
    assert!(spawn::has_update_switches(&Program::Harness("pi"), &env).is_ok());
    let refused = spawn::has_update_switches(&Program::Harness("pi"), &[]).unwrap_err();
    assert!(refused.contains("PI_OFFLINE"), "{refused}");
    assert!(spawn::has_update_switches(&Program::Harness("claude"), &env).is_err());
}

#[test]
fn a_window_asked_with_another_home_or_with_a_home_taken_away_is_not_opened() {
    let root = made("k3-window-").unwrap();
    let home = root.path().join("home");
    let host: Vec<(OsString, OsString)> = run_homes(&home.to_string_lossy())
        .into_iter()
        .chain(vars(&[("PATH", "/usr/bin:/bin")]))
        .chain(
            super::Door::child_vars()
                .into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        )
        .collect();
    let decoy = root.path().join("decoy-appdata");
    let _held = TestRoot::add(&decoy);
    let program = "/bin/sh";
    let fine = json!({ "id": "p-1", "argv": [program], "env": {}, "dropEnv": [] });
    assert_eq!(super::window_admitted(&host, &fine), Ok(()));
    let decoy_text = decoy.to_string_lossy();
    for (case, body) in [
        // Another APPDATA, spelled in lower case: on Unix a spelling in another case of a home;
        // on Windows the decoy, which is no folder of the run's.
        (
            "appdata",
            json!({ "id": "p-2", "argv": [program], "env": { "appdata": decoy_text } }),
        ),
        (
            "APPDATA",
            json!({ "id": "p-3", "argv": [program], "env": { "APPDATA": decoy_text } }),
        ),
        (
            "HOME dropped",
            json!({ "id": "p-4", "argv": [program], "dropEnv": ["HOME"] }),
        ),
        (
            "the kit's own dropped",
            json!({ "id": "p-5", "argv": [program], "dropEnv": [super::account::PROTECTED_FILE] }),
        ),
        (
            "the Keychain",
            json!({ "id": "p-6", "argv": ["/usr/bin/security"] }),
        ),
    ] {
        assert!(
            super::window_admitted(&host, &body).is_err(),
            "{case}: the window was admitted"
        );
    }
}

/// A pane host's environment, as the door gives it: the run's homes below `home`, a search path
/// and the door's own variables.
fn window_host(home: &Path) -> Vec<(OsString, OsString)> {
    run_homes(&home.to_string_lossy())
        .into_iter()
        .chain(vars(&[("PATH", "/usr/bin:/bin")]))
        .chain(
            super::Door::child_vars()
                .into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        )
        .collect()
}

/// `harness`'s update switches, as a window's `env`.
fn switches_of(harness: &str) -> serde_json::Value {
    spawn::update_switches(harness)
        .iter()
        .map(|(name, value)| ((*name).to_owned(), json!(value)))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// T-2086 C1: a window that starts a harness's program without the switches that keep it from
/// updating itself is refused, through the window's own door, and opened with them.
#[test]
fn a_window_that_starts_a_harness_without_its_update_switches_is_not_opened() {
    let root = made("k3-switches-").unwrap();
    let host = window_host(&root.path().join("home"));
    let muse = root.path().join("opt").join("muse-bin-1.4.14");
    let bare = json!({ "id": "p-1", "argv": [muse], "env": {} });
    let refused = super::window_admitted(&host, &bare).unwrap_err();
    assert!(refused.contains("MUSE"), "{refused}");
    let switched = json!({ "id": "p-2", "argv": [muse], "env": switches_of("muse") });
    assert_eq!(super::window_admitted(&host, &switched), Ok(()));
}

/// T-2086 C2: a live Claude window starts the rig's shim (`claude`, a file the run made), which
/// starts what its variable names (`CF_TEST_HARNESS`; on Windows the program written in it), the
/// kit's wrapper (`claude-isolated`, made by the run too), which starts the real Claude Code. The
/// window's door sees through both: the window is Claude's, and needs its switches. A shim that
/// starts a stand-in is a plain program.
#[test]
fn a_window_started_through_a_rigs_shim_and_the_kits_wrapper_is_asked_for_the_harnesss_switches() {
    let root = made("k3-wrapper-").unwrap();
    let host = window_host(&root.path().join("home"));
    let wrapper = root.path().join("claude-isolated");
    super::write_executable(&wrapper, "#!/bin/sh\nexec /the/claude \"$@\"\n").unwrap();
    super::register_wrapper(&wrapper, "claude");
    let stand_in = root.path().join("fake_agent");
    super::write_executable(&stand_in, "#!/bin/sh\n").unwrap();
    let shim = root.path().join("claude");
    super::write_executable(&shim, "#!/bin/sh\nexec \"$CF_TEST_HARNESS\" \"$@\"\n").unwrap();
    super::register_shim(&shim, Some("CF_TEST_HARNESS"), &stand_in);

    let wrapped = json!({ "id": "p-1", "argv": [wrapper], "env": {} });
    let refused = super::window_admitted(&host, &wrapped).unwrap_err();
    assert!(refused.contains("DISABLE_AUTOUPDATER"), "{refused}");
    let switched = json!({ "id": "p-2", "argv": [wrapper], "env": switches_of("claude") });
    assert_eq!(super::window_admitted(&host, &switched), Ok(()));

    // The shim, on a stand-in: plain. On the wrapper its variable names: Claude's.
    let on_stand_in = json!({ "id": "p-3", "argv": [shim], "env": {} });
    assert_eq!(super::window_admitted(&host, &on_stand_in), Ok(()));
    let mut env = serde_json::Map::new();
    env.insert("CF_TEST_HARNESS".into(), json!(wrapper));
    let on_wrapper = json!({ "id": "p-4", "argv": [shim], "env": env.clone() });
    assert!(
        super::window_admitted(&host, &on_wrapper).is_err(),
        "the shim's wrapper was not seen"
    );
    for (name, value) in switches_of("claude").as_object().unwrap() {
        env.insert(name.clone(), value.clone());
    }
    let switched = json!({ "id": "p-5", "argv": [shim], "env": env });
    assert_eq!(super::window_admitted(&host, &switched), Ok(()));
}

/// The environment a pane host on Windows gives a window, in its builder's order (T-2086 B1):
/// the host's own, every value the account's registry stores over it, the request's `env` over
/// that, and its `dropEnv` taken out last. Names compare in any case, as Windows' do.
fn as_the_host_builds_it(
    host: &[(OsString, OsString)],
    stored: &[(&str, &str)],
    body: &serde_json::Value,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = host
        .iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    let put = |env: &mut Vec<(String, String)>, name: &str, value: &str| {
        env.retain(|(held, _)| !held.eq_ignore_ascii_case(name));
        env.push((name.to_owned(), value.to_owned()));
    };
    for (name, value) in stored {
        put(&mut env, name, value);
    }
    for (name, value) in body["env"].as_object().into_iter().flatten() {
        put(&mut env, name, value.as_str().unwrap());
    }
    for name in body["dropEnv"].as_array().into_iter().flatten() {
        env.retain(|(held, _)| !held.eq_ignore_ascii_case(name.as_str().unwrap()));
    }
    let mut env: Vec<_> = env
        .into_iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .collect();
    env.sort();
    env
}

/// T-2086 B1: on Windows the pane host puts the account's stored variables (`setx`'s) over its own
/// environment, which the seal never saw. The window the door sends names each stored variable:
/// the judged value where the judged environment has it (a request's value wins over the
/// registry's), and taken out where it has not, so the window's environment is the one judged,
/// whatever the registry holds.
#[test]
fn a_window_is_sent_so_that_no_stored_variable_of_the_account_reaches_it() {
    let root = made("k3-stored-").unwrap();
    let home = root.path().join("home");
    let temp = root.path().join("temp");
    let host: Vec<(OsString, OsString)> = run_homes(&home.to_string_lossy())
        .into_iter()
        .chain(vars(&[
            ("PATH", "/usr/bin:/bin"),
            ("TEMP", &temp.to_string_lossy()),
        ]))
        .chain(
            super::Door::child_vars()
                .into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        )
        .collect();
    // What `setx` leaves in HKCU\Environment: a harness's home, the temp folder, the search
    // path, and a variable the host has not.
    let stored = [
        ("CODEX_HOME", "C:\\Users\\owner\\.codex"),
        ("temp", "C:\\Users\\owner\\AppData\\Local\\Temp"),
        ("Path", "C:\\Users\\owner\\.local\\bin"),
        ("OneDrive", "C:\\Users\\owner\\OneDrive"),
    ];
    let names: Vec<OsString> = stored
        .iter()
        .map(|(name, _)| OsString::from(*name))
        .collect();
    let body =
        json!({ "id": "p-1", "argv": ["/bin/sh"], "env": { "CF_ASKED": "1" }, "dropEnv": [] });
    let judged = spawn::pane_environment(&host, &body, true).unwrap();
    let sent = super::window_to_send(&host, &body, &names).unwrap();
    assert_eq!(
        as_the_host_builds_it(&host, &stored, &sent),
        as_the_host_builds_it(&judged, &[], &json!({})),
        "{sent}"
    );
    // A stored name the request takes out stays out.
    let dropping = json!({ "id": "p-2", "argv": ["/bin/sh"], "dropEnv": ["TEMP"] });
    let sent = super::window_to_send(&host, &dropping, &names).unwrap();
    let built = as_the_host_builds_it(&host, &stored, &sent);
    assert!(!built.iter().any(|(name, _)| name == "temp"), "{built:?}");
    // With nothing stored, the request is sent as it was asked.
    assert_eq!(super::window_to_send(&host, &body, &[]).unwrap(), body);
}

#[cfg(unix)]
#[test]
fn a_daemons_window_that_breaks_the_seal_is_answered_as_refused_and_never_reaches_the_host() {
    use crate::rig::route_tests::sinks_over;
    let root = made("k3-relay-").unwrap();
    let home = root.path().join("home");
    let host_env: Vec<(OsString, OsString)> = run_homes(&home.to_string_lossy())
        .into_iter()
        .chain(vars(&[("PATH", "/usr/bin:/bin")]))
        .collect();
    let (shared, host, to_host, daemon, to_daemon) = sinks_over(host_env);
    let open = json!({ "v": 1, "id": "n-1", "kind": "req", "op": "pane.open",
        "body": { "id": "p1-chief", "argv": ["/bin/sh"], "dropEnv": ["HOME"] } });
    shared.route_daemon(&open.to_string(), &host, &daemon);
    let answer = to_daemon
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the daemon was not answered");
    assert!(answer.contains("\"ok\":false"), "{answer}");
    assert!(answer.contains("the rig refused the window"), "{answer}");
    assert!(
        to_host
            .recv_timeout(std::time::Duration::from_millis(300))
            .is_err(),
        "the pane host was asked"
    );
    assert_eq!(shared.refused_count(), 1);
}

/// The names an account's registry stores, which the Mac has none of.
#[cfg(unix)]
fn registry_names() -> std::result::Result<Vec<OsString>, String> {
    Ok(vec![OsString::from("REGISTRY_ONLY")])
}

/// A registry that cannot be read.
#[cfg(unix)]
fn registry_unreadable() -> std::result::Result<Vec<OsString>, String> {
    Err("the registry key Environment cannot be opened (error 5)".to_owned())
}

/// T-2119 B1-b: the routers a window passes through name every variable the account's registry
/// stores in what the pane host is sent, for the window the rig opens (`Rig::open_pane`) and for
/// one the daemon asks (the relay); and a registry that cannot be read opens no window (B1-a).
#[cfg(unix)]
#[test]
fn the_relay_and_the_rigs_open_hand_the_registrys_names_on_and_a_registry_that_fails_opens_nothing()
{
    use crate::rig::route_tests::sinks_over;
    let root = made("k3-registry-").unwrap();
    let home = root.path().join("home");
    let host_env = || -> Vec<(OsString, OsString)> {
        run_homes(&home.to_string_lossy())
            .into_iter()
            .chain(vars(&[("PATH", "/usr/bin:/bin")]))
            .collect()
    };
    let body = json!({ "id": "p1-chief", "argv": ["/bin/sh"], "dropEnv": [] });
    let (shared, host, to_host, daemon, to_daemon) = sinks_over(host_env());
    let shared = shared.naming(registry_names);
    // The window the rig opens.
    let sent = shared.window_to_send(&body).unwrap();
    assert!(
        sent["dropEnv"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "REGISTRY_ONLY"),
        "{sent}"
    );
    // The window the daemon asks.
    let open = json!({ "v": 1, "id": "n-1", "kind": "req", "op": "pane.open", "body": body });
    shared.route_daemon(&open.to_string(), &host, &daemon);
    let relayed = to_host
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the pane host was sent no window");
    assert!(relayed.contains("REGISTRY_ONLY"), "{relayed}");
    // A registry that cannot be read.
    let shared = shared.naming(registry_unreadable);
    assert!(shared
        .window_to_send(&body)
        .unwrap_err()
        .contains("error 5"));
    shared.route_daemon(&open.to_string(), &host, &daemon);
    let answer = to_daemon
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the daemon was not answered");
    assert!(answer.contains("\"ok\":false"), "{answer}");
    assert!(
        to_host
            .recv_timeout(std::time::Duration::from_millis(300))
            .is_err(),
        "the pane host was asked"
    );
}

// ---- The helper's question. ----

#[test]
fn a_question_whose_environment_names_a_home_of_the_account_is_not_asked() {
    let root = made("k3-question-").unwrap();
    let decoy = root.path().join("owner-appdata");
    let _held = TestRoot::add(&decoy);
    let home = root.path().join("home");
    let mut fine = json!({ "op": "models", "kind": "codex", "env": { "HOME": home } });
    crate::live::ask::question_sealed(&mut fine).unwrap();
    // Made whole: every home below the one it named.
    assert_eq!(
        fine["env"]["CODEX_HOME"],
        json!(home.join(".codex").to_string_lossy())
    );
    let mut owners =
        json!({ "op": "models", "kind": "codex", "env": { "HOME": home, "APPDATA": decoy } });
    assert!(crate::live::ask::question_sealed(&mut owners).is_err());
    let mut elsewhere =
        json!({ "op": "models", "kind": "codex", "env": { "HOME": "/not/the/runs" } });
    assert!(crate::live::ask::question_sealed(&mut elsewhere).is_err());
}

/// T-2086 N5: a question starts the installed harness to read something of it, and carries the
/// switches that keep every harness from updating or installing anything itself, over what its
/// caller said.
#[test]
fn a_question_is_asked_with_every_harnesss_update_switches_over_what_its_caller_said() {
    let root = made("k3-question-switches-").unwrap();
    let home = root.path().join("home");
    let mut body = json!({
        "op": "models", "kind": "pi", "env": { "HOME": home, "PI_OFFLINE": "0" }
    });
    crate::live::ask::question_sealed(&mut body).unwrap();
    for (name, value) in [
        ("PI_OFFLINE", "1"),
        ("DISABLE_AUTOUPDATER", "1"),
        ("DISABLE_UPDATES", "1"),
        ("MUSE_NO_AUTO_UPDATE", "1"),
    ] {
        assert_eq!(body["env"][name], json!(value), "{name}");
    }
}

// ---- The judge. ----

#[test]
fn nothing_is_made_in_a_store_that_is_not_there_yet() {
    let root = made("k3-absent-").unwrap();
    let store = root.path().join("owner").join(".codex");
    let _held = TestRoot::add(&store);
    assert!(super::made_in(&store.join("tmp"), "h-").is_err());
    assert!(!store.exists(), "the absent store was made");
    // In another case, on a volume that compares names in any case.
    let other = root.path().join("owner").join(".CODEX").join("tmp");
    let any_case = super::place::any_case_at(root.path());
    assert_eq!(super::made_in(&other, "h-").is_err(), any_case);
    if any_case {
        assert!(!root.path().join("owner").exists());
    }
}

#[test]
fn a_tree_a_protected_root_leads_into_is_not_removed_or_moved() {
    let root = made("k3-tree-").unwrap();
    let tree = root.path().join("tree");
    let inner = tree.join("inner");
    super::make_dir(&inner).unwrap();
    // A root of the account's that leads into the run's tree: a variable that names it.
    let _held = TestRoot::add(&inner);
    assert!(judge(&tree, Act::Remove).is_err());
    assert!(judge(&tree, Act::Rename).is_err());
    assert!(!super::Door::of_run().remove_in_made(&tree).is_gone());
    assert!(inner.is_dir());
    assert!(super::rename(&tree, root.path().join("moved")).is_err());
    assert!(inner.is_dir());
}

#[test]
fn a_place_that_cannot_be_resolved_but_for_its_absence_is_refused() {
    let root = made("k3-unresolved-").unwrap();
    let file = root.path().join("file");
    super::write(&file, "a file").unwrap();
    // Through a file: not absent, and no place.
    assert!(judge(&file.join("below"), Act::Write).is_err());
    // A `..` among the absent names, where the system follows it after the links (Unix); Windows
    // settles a `..` by the path's words, as the door's absolute path does.
    #[cfg(unix)]
    assert!(judge(&root.path().join("absent/../x"), Act::Write).is_err());
}

/// T-2086 C3: a place below a folder that cannot be read cannot be resolved, and is refused: it
/// is not taken for absent, which would judge it by the folder above as below a folder the run
/// made.
#[cfg(unix)]
#[test]
fn a_place_below_a_folder_that_cannot_be_read_is_refused() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = made("k3-unreadable-").unwrap();
    let locked = root.path().join("locked");
    fs::create_dir_all(locked.join("child")).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let judged = judge(&locked.join("child").join("file"), Act::Write);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    let refused = judged.unwrap_err().to_string();
    assert!(refused.contains("cannot be"), "{refused}");
}

#[cfg(unix)]
#[test]
fn a_protected_link_that_changes_where_it_leads_is_judged_where_it_now_leads() {
    let root = made("k3-moving-link-").unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    super::make_dir(&first).unwrap();
    super::make_dir(&second).unwrap();
    let link = root.path().join("owner-store");
    std::os::unix::fs::symlink(&first, &link).unwrap();
    let _held = TestRoot::add(&link);
    assert!(judge(&first.join("x"), Act::Write).is_err());
    assert!(judge(&second.join("x"), Act::Write).is_ok());
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&second, &link).unwrap();
    assert!(judge(&second.join("x"), Act::Write).is_err());
    assert!(judge(&first.join("x"), Act::Write).is_ok());
}

#[cfg(unix)]
#[test]
fn an_allocation_whose_place_leads_into_a_protected_root_makes_nothing() {
    let root = made("k3-allocation-").unwrap();
    let store = root.path().join("owner-store");
    super::make_dir(&store).unwrap();
    let place = root.path().join("live");
    std::os::unix::fs::symlink(&store, &place).unwrap();
    let _held = TestRoot::add(&store);
    assert!(super::made_in(&place.join("projects"), "case-").is_err());
    assert!(fs::read_dir(&store).unwrap().next().is_none());
}

#[test]
fn a_named_file_or_folder_is_made_new_and_only_it_is_the_runs() {
    let elsewhere = crate::standin_process::unowned_tempdir();
    let audit = elsewhere.path().join("reports").join("audit.txt");
    drop(super::made_file(&audit).unwrap());
    assert!(super::made_file(&audit).is_err(), "made twice");
    // The folder made on the way is no folder of the run's.
    assert!(super::writable(&elsewhere.path().join("reports").join("other"), false).is_err());
    assert!(super::writable(&audit, false).is_ok());
    // The same for a folder a caller names.
    let export = elsewhere.path().join("exports").join("one");
    super::made_dir(&export).unwrap();
    assert!(super::made_dir(&export).is_err());
    assert!(super::writable(&export.join("evidence.json"), false).is_ok());
    assert!(super::writable(&elsewhere.path().join("exports").join("two"), true).is_err());
}

#[test]
fn a_file_the_run_made_and_another_put_at_its_path_since_is_not_the_runs() {
    let elsewhere = crate::standin_process::unowned_tempdir();
    let report = elsewhere.path().join("rig-left.txt");
    drop(super::made_file(&report).unwrap());
    assert!(super::write(&report, "the run's").is_ok());
    fs::remove_file(&report).unwrap();
    // Another file at the same path, while the first's number is not free to be taken again.
    let _held = fs::File::create(elsewhere.path().join("held")).unwrap();
    fs::write(&report, "another's").unwrap();
    assert!(super::write(&report, "over it").is_err());
    assert_eq!(fs::read_to_string(&report).unwrap(), "another's");
}

#[test]
fn the_roots_file_and_the_folders_it_is_in_are_protected_for_the_runs_life() {
    let (file, _) = super::account::own_file().unwrap();
    let folder = file.parent().unwrap().to_path_buf();
    assert!(super::write(&file, "[]").is_err());
    assert!(super::rename(&file, folder.join("moved.json")).is_err());
    assert!(judge(&folder, Act::Remove).is_err());
    assert!(judge(&folder, Act::Rename).is_err());
    assert!(!super::Door::of_run().remove_made(&folder).is_gone());
    assert!(file.is_file());
}

// ---- Correction 6, the children's audit appends. ----

#[test]
fn a_child_adds_to_the_audit_file_the_run_made_and_to_no_file_put_at_its_path_since() {
    let run = made("k3-audit-").unwrap();
    let file = run.path().join("audit.txt");
    let first = super::Door::asked(Some("1".as_ref()), Some(file.as_os_str()), None).unwrap();
    let identity = super::identity::of_path(&file).unwrap();
    let child = super::Door::asked_of(
        Some("1".as_ref()),
        Some(file.as_os_str()),
        Some("1".as_ref()),
        Some(identity.as_ref()),
    );
    assert!(child.is_ok());
    // Another file at the path: a child told the run's file refuses it.
    fs::rename(&file, run.path().join("moved.txt")).unwrap();
    fs::write(&file, "another's").unwrap();
    let refused = super::Door::asked_of(
        Some("1".as_ref()),
        Some(file.as_os_str()),
        Some("1".as_ref()),
        Some(identity.as_ref()),
    )
    .unwrap_err();
    assert!(refused.contains("not the one the run made"), "{refused}");
    // The run's own door adds nothing to the other file: it stops, as a door whose file cannot be
    // written does.
    let stopped = std::panic::catch_unwind(|| first.note("not the run's file"));
    assert!(stopped.is_err());
    assert_eq!(fs::read_to_string(&file).unwrap(), "another's");
}

// ---- The judge opens nothing of a file it judges (round 2 of T-2007). ----

/// A whole-file write lock of `file`'s, as SQLite takes its locks (`fcntl`): whether it was
/// taken.
#[cfg(unix)]
fn write_lock(file: &fs::File) -> bool {
    use std::os::fd::AsRawFd;
    // SAFETY: the structure is plain data, zeroed, then set for a whole-file write lock.
    #[allow(unsafe_code)]
    let mut lock: nix::libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = nix::libc::F_WRLCK as _;
    lock.l_whence = nix::libc::SEEK_SET as _;
    nix::fcntl::fcntl(file.as_raw_fd(), nix::fcntl::FcntlArg::F_SETLK(&lock)).is_ok()
}

/// The stand-in Devin's store crashed under load (SIGBUS, `Bus error: 10`) when the judge looked
/// at its WAL and shared-memory files: an open and a close of a file by the judge (its probe of
/// the volume's case, a made file's identity) drops every `fcntl` lock its process holds on that
/// file, SQLite's among them; another connection then takes the store for unused and resets the
/// shared memory the first has mapped. The judge reads what a file is without opening it, and a
/// lock its process holds on the file it judges is held after.
#[cfg(unix)]
#[test]
fn the_judge_releases_no_lock_its_process_holds_on_a_file_it_judges() {
    const CHILD: &str = "CF_T2007_LOCK_CHILD";
    if let Some(file) = crate::process::own_var(CHILD) {
        let open = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(file)
            .unwrap();
        assert!(!write_lock(&open), "the parent's lock was gone");
        return;
    }
    let root = made("k3-lock-").unwrap();
    let file = root.path().join("sessions.db-shm");
    super::write(&file, "shared memory").unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&file)
        .unwrap();
    assert!(write_lock(&held));
    for act in [Act::Write, Act::Remove, Act::Rename] {
        let _ = judge(&file, act);
    }
    let _ = super::writable(&file, false);
    let ran = crate::process::case_of(
        std::env::current_exe().unwrap(),
        "door::k3_tests::the_judge_releases_no_lock_its_process_holds_on_a_file_it_judges",
        std::time::Duration::from_secs(60),
    )
    .var(CHILD, &file)
    .run()
    .unwrap();
    assert_eq!(ran.code, Some(0), "{ran}");
    drop(held);
}

// ---- T-2219: the guards T-2086 found unplanted, each held by a test of its own. ----

/// The judge's ground is by the act (design section 3): a name a caller gives is made new and never
/// over what is there; a write or a rename goes below a folder this run made; a folder is made
/// there or is one this run made; a removal takes a folder this run made or what is below one; an
/// allocation asks nothing more.
#[test]
fn the_judges_ground_is_by_the_act() {
    let mine = made("k3-ground-").unwrap();
    let theirs = crate::standin_process::unowned_tempdir();
    let below_mine = mine.path().join("inner");
    let below_theirs = theirs.path().join("inner");
    crate::door::make_dir(&below_mine).unwrap();
    crate::standin_process::foreign_dir(&below_theirs);
    crate::standin_process::foreign_write(theirs.path().join("there.txt"), "theirs");
    crate::door::write(mine.path().join("here.txt"), "mine").unwrap();
    // A name: new anywhere nothing is, never over what is there.
    assert!(judge(&theirs.path().join("new.txt"), Act::Name).is_ok());
    assert!(judge(&mine.path().join("new.txt"), Act::Name).is_ok());
    assert!(judge(&theirs.path().join("there.txt"), Act::Name).is_err());
    assert!(judge(&mine.path().join("here.txt"), Act::Name).is_err());
    // A write and a rename: below a folder this run made, or a file it made, and nowhere else.
    for act in [Act::Write, Act::Rename] {
        assert!(judge(&mine.path().join("here.txt"), act).is_ok(), "{act:?}");
        assert!(judge(&below_mine.join("new.txt"), act).is_ok(), "{act:?}");
        assert!(
            judge(&theirs.path().join("there.txt"), act).is_err(),
            "{act:?}"
        );
        assert!(
            judge(&below_theirs.join("new.txt"), act).is_err(),
            "{act:?}"
        );
    }
    // A folder: a folder this run made, or below one.
    assert!(judge(mine.path(), Act::Folder).is_ok());
    assert!(judge(&below_mine.join("deeper"), Act::Folder).is_ok());
    assert!(judge(&below_theirs.join("deeper"), Act::Folder).is_err());
    assert!(judge(theirs.path(), Act::Folder).is_err());
    // A removal: a folder this run made, or what is below one.
    assert!(judge(mine.path(), Act::Remove).is_ok());
    assert!(judge(&below_mine, Act::Remove).is_ok());
    assert!(judge(&mine.path().join("here.txt"), Act::Remove).is_ok());
    assert!(judge(&below_theirs, Act::Remove).is_err());
    assert!(judge(&theirs.path().join("there.txt"), Act::Remove).is_err());
    // An allocation asks no more than that it is no store's.
    assert!(judge(&theirs.path().join("allocated"), Act::Allocate).is_ok());
}

/// A write through a last link goes to what it leads to, so it is refused whatever it leads to; a
/// removal and a rename take the link itself, and an allocation is judged by the place.
#[cfg(unix)]
#[test]
fn a_write_through_a_last_link_is_refused_and_a_removal_of_the_link_is_not() {
    let mine = made("k3-link-").unwrap();
    let inside = mine.path().join("inside.txt");
    crate::door::write(&inside, "mine").unwrap();
    let link = mine.path().join("link");
    std::os::unix::fs::symlink(&inside, &link).unwrap();
    // The link leads to a file this run made, below a folder it made: still no write goes through it.
    for act in [Act::Write, Act::Name, Act::Folder] {
        let refused = judge(&link, act).unwrap_err();
        assert!(
            refused.to_string().contains("is a link"),
            "{act:?}: {refused}"
        );
    }
    assert!(judge(&link, Act::Remove).is_ok());
    assert!(judge(&link, Act::Rename).is_ok());
}

/// A name below a file is no place, whatever the system calls its absence.
#[test]
fn a_name_below_a_file_is_no_place() {
    let mine = made("k3-below-file-").unwrap();
    let file = mine.path().join("a.txt");
    crate::door::write(&file, "mine").unwrap();
    for act in [
        Act::Write,
        Act::Name,
        Act::Folder,
        Act::Allocate,
        Act::Remove,
    ] {
        // "is a file" where the system calls the path absent, "cannot be resolved" where it says
        // "not a directory": no place either way.
        let refused = judge(&file.join("child"), act).unwrap_err().to_string();
        assert!(
            refused.contains("is a file") || refused.contains("cannot be resolved"),
            "{act:?}: {refused}"
        );
    }
    assert!(super::place::resolve(&file.join("deeper").join("child")).is_err());
}

/// `program_of`, each way a program is known: by a Muse binary's name, by Claude's `versions`
/// folder, by a store of the table, by the Keychain's utility, by cargo, by the harness's own name
/// as the last net (`gemini` is agy's), with an extension or in another case, and not where the run
/// made the file.
#[test]
fn a_program_is_known_by_each_way_the_door_knows_one() {
    let plain = |program: &str, real: Option<&str>, in_store: bool, made_by_run: bool| {
        spawn::program_of(
            Path::new(program),
            real.map(Path::new),
            in_store,
            made_by_run,
        )
    };
    // Muse's `muse-bin-<version>`, whatever the program is called that leads to it.
    assert_eq!(
        plain(
            "/usr/local/bin/m",
            Some("/opt/muse/muse-bin-2.0"),
            false,
            false
        ),
        Program::Harness("muse")
    );
    assert_eq!(
        plain("/opt/muse/muse-bin-2.0", None, false, false),
        Program::Harness("muse")
    );
    // Claude's versions folder is Claude's only where its parent is `claude`.
    assert_eq!(
        plain(
            "/x/cc",
            Some("/h/.local/share/claude/versions/2.1.0"),
            false,
            false
        ),
        Program::Harness("claude")
    );
    assert_eq!(
        plain(
            "/x/cc",
            Some("/h/.local/share/other/versions/2.1.0"),
            false,
            false
        ),
        Program::Plain
    );
    assert_eq!(
        plain("/x/cc", Some("/h/.local/share/claude/bin/cc"), false, false),
        Program::Plain
    );
    // A file in a store of the table is the harness it is named for, or a harness not named.
    assert_eq!(
        plain("/h/.pi/agent/bin/pi", None, true, false),
        Program::Harness("pi")
    );
    assert_eq!(
        plain(
            "/h/.local/share/devin/cli/_versions/3000/devin",
            None,
            true,
            false
        ),
        Program::Harness("devin")
    );
    assert_eq!(
        plain("/h/.pi/agent/bin/helper", None, true, false),
        Program::Harness("a harness")
    );
    // A file the run made is plain, whatever its name.
    for name in [
        "claude", "codex", "pi", "devin", "muse", "agy", "cargo", "security",
    ] {
        assert_eq!(
            plain(&format!("/run/bin/{name}"), None, false, true),
            Program::Plain,
            "{name}"
        );
    }
    // The last net: the name, by the program as given or by the file it resolves to, in any case
    // and with its extension.
    for (given, real, expected) in [
        ("claude", None, Program::Harness("claude")),
        ("/usr/bin/CODEX.EXE", None, Program::Harness("codex")),
        ("x", Some("/opt/bin/Pi.cmd"), Program::Harness("pi")),
        ("/usr/bin/gemini", None, Program::Harness("agy")),
        ("/usr/bin/agy", None, Program::Harness("agy")),
        ("/usr/bin/muse", None, Program::Harness("muse")),
        ("/usr/bin/devin", None, Program::Harness("devin")),
        ("/usr/local/bin/cargo", None, Program::Cargo),
        ("cargo.exe", None, Program::Cargo),
        ("/usr/bin/security", None, Program::Keychain),
        ("security", None, Program::Keychain),
        ("/opt/tools/security", None, Program::Keychain),
        ("/usr/bin/git", None, Program::Plain),
        ("/usr/bin/codexx", None, Program::Plain),
    ] {
        assert_eq!(
            plain(given, real, false, false),
            expected,
            "{given} {real:?}"
        );
    }
}

/// A window whose program is the Keychain's utility, or cargo, is refused each by itself: only
/// their doors start them.
#[test]
fn a_window_whose_program_is_the_keychains_utility_or_cargo_is_not_opened() {
    let root = made("k3-window-doors-").unwrap();
    let host = window_host(&root.path().join("home"));
    let keychain =
        json!({ "id": "p-1", "argv": ["/usr/bin/security", "find-generic-password"], "env": {} });
    let refused = super::window_admitted(&host, &keychain).unwrap_err();
    assert!(refused.contains("Keychain"), "{refused}");
    let cargo = json!({ "id": "p-2", "argv": ["/opt/x/cargo", "build"], "env": {} });
    let refused = super::window_admitted(&host, &cargo).unwrap_err();
    assert!(refused.contains("cargo"), "{refused}");
    let git = json!({ "id": "p-3", "argv": ["/usr/bin/git", "status"], "env": {} });
    assert_eq!(super::window_admitted(&host, &git), Ok(()));
}

/// The seal refuses each of the door's own variables a caller gives or changes, and one profile
/// half given without the other.
#[test]
fn the_seal_refuses_a_callers_roots_variables_and_one_profile_half_alone() {
    let is_runs = |path: &Path| path.starts_with("/kit/run");
    let in_protected = |path: &Path| path.starts_with("/owner");
    let judges = Judges {
        windows: false,
        is_runs: &is_runs,
        in_protected: &in_protected,
        kit: &[],
    };
    let env = run_homes("/kit/run/home");
    // Each of the door's own variables, given by a caller.
    for name in [
        super::MADE_ROOTS,
        super::account::PROTECTED_FILE,
        super::account::PROTECTED_DIGEST,
    ] {
        let mut given = env.clone();
        // A value that names no store: only the check of the door's own variables refuses it.
        given.push((name.into(), "/kit/run/roots.json".into()));
        let refused = spawn::seal(given, &judges).unwrap_err();
        assert!(refused.contains(name), "{name}: {refused}");
    }
    // One profile half without the other, either one.
    for half in ["HOMEDRIVE", "HOMEPATH"] {
        let mut one = run_homes("/kit/run/home");
        one.push((
            half.into(),
            if half == "HOMEDRIVE" {
                "C:"
            } else {
                "/kit/run/home"
            }
            .into(),
        ));
        let refused = spawn::seal(one, &judges).unwrap_err();
        assert!(
            refused.contains("one without the other"),
            "{half}: {refused}"
        );
    }
}

// ---- T-2219: a program reached through a shell is judged by what the shell is told to run. ----

/// A window whose program is a shell is judged by the program its command starts: a harness's gets
/// its switches, the Keychain's utility and cargo are refused whatever shell reaches them, shells
/// inside shells are followed, and a command the shell would read for itself (an operator, a
/// substitution, a variable) is refused, since the door cannot say what it starts.
#[test]
fn a_window_started_through_a_shell_is_judged_by_the_program_the_shell_is_told_to_run() {
    let root = made("k3-shell-window-").unwrap();
    let host = window_host(&root.path().join("home"));
    let muse = root.path().join("opt").join("muse-bin-1.4.14");
    let admitted = |argv: serde_json::Value, env: serde_json::Value| {
        super::window_admitted(&host, &json!({ "id": "p", "argv": argv, "env": env }))
    };
    // A POSIX shell reads a backslash as an escape: the path is spelled with `/`.
    let posix = muse.display().to_string().replace('\\', "/");
    let command = format!("{posix} --version");
    // A harness through a shell wants its switches, as it does started alone.
    let refused = admitted(json!(["/bin/sh", "-c", command]), json!({})).unwrap_err();
    assert!(refused.contains("MUSE"), "{refused}");
    assert_eq!(
        admitted(json!(["/bin/sh", "-c", command]), switches_of("muse")),
        Ok(())
    );
    for argv in [
        json!(["bash", "-lc", format!("exec {posix}")]),
        json!(["/bin/zsh", "-c", format!("'{posix}' x")]),
        json!(["sh", "-c", format!("sh -c '{posix}'")]),
        json!(["cmd.exe", "/d", "/c", muse.display().to_string()]),
        json!([
            "C:\\Windows\\System32\\cmd.exe",
            "/c",
            muse.display().to_string()
        ]),
        json!([
            "powershell.exe",
            "-NoProfile",
            "-Command",
            muse.display().to_string()
        ]),
        json!(["pwsh", "-c", muse.display().to_string()]),
    ] {
        let refused = admitted(argv.clone(), json!({})).unwrap_err();
        assert!(refused.contains("MUSE"), "{argv}: {refused}");
        assert_eq!(
            admitted(argv.clone(), switches_of("muse")),
            Ok(()),
            "{argv}"
        );
    }
    // The Keychain's utility and cargo, through any shell.
    for argv in [
        json!(["/bin/sh", "-c", "/usr/bin/security dump-keychain"]),
        json!(["sh", "-c", "sh -c 'security list-keychains'"]),
        json!(["cmd", "/c", "security x"]),
    ] {
        let refused = admitted(argv.clone(), json!({})).unwrap_err();
        assert!(refused.contains("Keychain"), "{argv}: {refused}");
    }
    for argv in [
        json!(["/bin/sh", "-c", "cargo build"]),
        json!(["bash", "-c", "exec /opt/x/cargo test"]),
        json!(["powershell", "-Command", "cargo build"]),
    ] {
        let refused = admitted(argv.clone(), json!({})).unwrap_err();
        assert!(refused.contains("cargo"), "{argv}: {refused}");
    }
    // A command that cannot be read is refused, and so is one that starts a shell it cannot read.
    for argv in [
        json!(["/bin/sh", "-c", "echo ready && cargo build"]),
        json!(["/bin/sh", "-c", "$PROGRAM --version"]),
        json!(["sh", "-c", "sh -c 'a; b'"]),
        json!(["cmd", "/c", "echo a & cargo build"]),
        json!(["powershell", "-Command", "& cargo build"]),
        json!(["powershell", "-EncodedCommand", "AAAA"]),
    ] {
        let refused = admitted(argv.clone(), json!({})).unwrap_err();
        assert!(
            refused.contains("cannot be read") || refused.contains("encoded command"),
            "{argv}: {refused}"
        );
    }
    // A shell with no command, one running a script file, and a command that is plain: opened.
    for argv in [
        json!(["/bin/sh"]),
        json!(["/bin/sh", "./start.sh", "-c", "claude"]),
        json!(["/bin/sh", "-c", "echo hello"]),
        json!(["cmd"]),
        json!(["powershell", "-NoProfile", "-File", "x.ps1"]),
    ] {
        assert_eq!(admitted(argv.clone(), json!({})), Ok(()), "{argv}");
    }
}

/// A start of a program the kit makes through a shell is judged the same way: a plain start of
/// `sh -c "cargo …"` is a start of cargo, and one the door cannot read is not started.
#[cfg(unix)]
#[test]
fn a_plain_start_through_a_shell_is_judged_by_the_program_the_shell_is_told_to_run() {
    use crate::process::Run;
    let refused = Run::new("/bin/sh")
        .args(["-c", "cargo --version"])
        .run()
        .unwrap_err();
    assert!(refused.to_string().contains("cargo"), "{refused}");
    let refused = Run::new("/bin/sh")
        .args(["-c", "/usr/bin/security list-keychains"])
        .run()
        .unwrap_err();
    assert!(refused.to_string().contains("Keychain"), "{refused}");
    let refused = Run::new("/bin/sh")
        .args(["-c", "echo ready; cargo --version"])
        .run()
        .unwrap_err();
    assert!(refused.to_string().contains("cannot be read"), "{refused}");
    // A script an offline fixture runs goes through its own start, and is refused there too where
    // a word of it names a program with a door.
    let ran = Run::fixture_script("echo ready; echo again").run().unwrap();
    assert_eq!(ran.stdout.trim(), "ready\nagain");
    for script in [
        "echo a; cargo build",
        "/usr/bin/security dump-keychain | cat",
        "x=1; Claude --version",
    ] {
        let refused = Run::fixture_script(script).run().unwrap_err();
        assert!(
            refused.to_string().contains("door of its own"),
            "{script}: {refused}"
        );
    }
    let ran = Run::new("/bin/sh")
        .args(["-c", "echo ready"])
        .run()
        .unwrap();
    assert_eq!(ran.stdout.trim(), "ready");
}
