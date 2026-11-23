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
        spawn::program_of(&made.join("x"), Some(Path::new("/opt/muse/muse-bin-1.4.14")), false, false),
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
        ("appdata", json!({ "id": "p-2", "argv": [program], "env": { "appdata": decoy_text } })),
        ("APPDATA", json!({ "id": "p-3", "argv": [program], "env": { "APPDATA": decoy_text } })),
        ("HOME dropped", json!({ "id": "p-4", "argv": [program], "dropEnv": ["HOME"] })),
        (
            "the kit's own dropped",
            json!({ "id": "p-5", "argv": [program], "dropEnv": [super::account::PROTECTED_FILE] }),
        ),
        ("the Keychain", json!({ "id": "p-6", "argv": ["/usr/bin/security"] })),
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
    assert!(super::window_admitted(&host, &on_wrapper).is_err(), "the shim's wrapper was not seen");
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
        .map(|(name, value)| (name.to_string_lossy().into_owned(), value.to_string_lossy().into_owned()))
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
    let mut env: Vec<_> = env.into_iter().map(|(name, value)| (name.to_ascii_lowercase(), value)).collect();
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
        .chain(vars(&[("PATH", "/usr/bin:/bin"), ("TEMP", &temp.to_string_lossy())]))
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
    let names: Vec<OsString> = stored.iter().map(|(name, _)| OsString::from(*name)).collect();
    let body = json!({ "id": "p-1", "argv": ["/bin/sh"], "env": { "CF_ASKED": "1" }, "dropEnv": [] });
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
    let mut owners = json!({ "op": "models", "kind": "codex", "env": { "HOME": home, "APPDATA": decoy } });
    assert!(crate::live::ask::question_sealed(&mut owners).is_err());
    let mut elsewhere = json!({ "op": "models", "kind": "codex", "env": { "HOME": "/not/the/runs" } });
    assert!(crate::live::ask::question_sealed(&mut elsewhere).is_err());
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
        let open = fs::OpenOptions::new().read(true).write(true).open(file).unwrap();
        assert!(!write_lock(&open), "the parent's lock was gone");
        return;
    }
    let root = made("k3-lock-").unwrap();
    let file = root.path().join("sessions.db-shm");
    super::write(&file, "shared memory").unwrap();
    let held = fs::OpenOptions::new().read(true).write(true).open(&file).unwrap();
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
    .inheriting_env()
    .var(CHILD, &file)
    .run()
    .unwrap();
    assert_eq!(ran.code, Some(0), "{ran}");
    drop(held);
}
