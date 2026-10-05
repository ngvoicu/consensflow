use std::fs;
use std::path::Path;

use super::*;

/// A file at `path` this user can start.
fn startable(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn a_cli_on_path_comes_first_then_the_places_it_installs_itself() {
    let root = tempfile::tempdir().unwrap();
    let (home, bin) = (root.path().join("home"), root.path().join("bin"));
    let name = |command: &str| {
        if cfg!(windows) {
            format!("{command}.exe")
        } else {
            command.to_owned()
        }
    };
    let env = Env::from_vars([
        ("HOME", home.to_str().unwrap()),
        ("PATH", bin.to_str().unwrap()),
    ]);
    assert_eq!(harness_path(Harness::Claude, &env), None);
    let local = home.join(".claude").join("local").join(name("claude"));
    startable(&local);
    assert_eq!(harness_path(Harness::Claude, &env), Some(local.clone()));
    let common = home.join(".volta").join("bin").join(name("claude"));
    startable(&common);
    assert_eq!(
        harness_path(Harness::Claude, &env),
        Some(local),
        "the harness's own place before the common ones"
    );
    let on_path = bin.join(name("claude"));
    startable(&on_path);
    assert_eq!(harness_path(Harness::Claude, &env), Some(on_path));
    // Another harness's place is not this one's.
    startable(&home.join(".codex").join("bin").join(name("pi")));
    assert_eq!(harness_path(Harness::Pi, &env), None);
}

#[test]
fn every_harness_is_known_in_the_order_harnesses_js_lists_them_not_the_rosters() {
    assert_eq!(
        known_harnesses(),
        [
            Harness::Devin,
            Harness::Claude,
            Harness::Codex,
            Harness::Opencode,
            Harness::Pi
        ]
    );
    let mut sorted = known_harnesses().map(Harness::as_str);
    sorted.sort_unstable();
    let mut all = Harness::ALL.map(Harness::as_str);
    all.sort_unstable();
    assert_eq!(sorted, all, "the same five harnesses, in another order");
}

#[test]
fn the_installed_and_the_missing_are_told_apart_each_in_the_order_of_the_list() {
    let root = tempfile::tempdir().unwrap();
    let (home, bin) = (root.path().join("home"), root.path().join("bin"));
    let name = |command: &str| {
        if cfg!(windows) {
            format!("{command}.exe")
        } else {
            command.to_owned()
        }
    };
    let env = Env::from_vars([
        ("HOME", home.to_str().unwrap()),
        ("PATH", bin.to_str().unwrap()),
    ]);
    assert_eq!(missing_harnesses(&env), known_harnesses());
    assert!(detect_harnesses(&env).is_empty());
    // Pi where it installs itself, Codex and Devin on PATH.
    startable(&home.join(".pi").join("bin").join(name("pi")));
    startable(&bin.join(name("codex")));
    startable(&bin.join(name("devin")));
    assert_eq!(
        missing_harnesses(&env),
        [Harness::Claude, Harness::Opencode]
    );
    let detected = detect_harnesses(&env);
    assert_eq!(
        detected.iter().map(|each| each.id).collect::<Vec<_>>(),
        [Harness::Devin, Harness::Codex, Harness::Pi]
    );
    assert_eq!(
        serde_json::to_string(&detected).unwrap(),
        r#"[{"id":"devin","command":"devin"},{"id":"codex","command":"codex"},{"id":"pi","command":"pi"}]"#
    );
}
