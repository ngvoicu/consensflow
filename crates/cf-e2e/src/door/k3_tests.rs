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
    // A `..` among the absent names.
    assert!(judge(&root.path().join("absent/../x"), Act::Write).is_err());
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
