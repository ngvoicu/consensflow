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
