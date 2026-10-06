use std::fs;

use super::*;

#[test]
fn the_names_are_the_apps_and_the_short_one_and_a_cmd_where_windows_has_them() {
    assert_eq!(names(false), ["consensflow", "cf"]);
    assert_eq!(names(true), ["consensflow.cmd", "cf.cmd"]);
}

#[test]
fn by_default_the_only_place_is_the_bin_of_the_home_whatever_a_project_says() {
    let env = Env::from_vars([
        ("CONSENSFLOW_HOME", "/work/cf"),
        ("CONSENSFLOW_BIN_DIR", "/project/bin"),
        ("HOME", "/home/me"),
    ]);
    assert_eq!(
        Places::default().folders(&env).unwrap(),
        [PathBuf::from(path::join(&["/work/cf", "bin"]))]
    );
    let plain = Env::from_vars([("HOME", "/home/me")]);
    assert_eq!(
        Places::default().folders(&plain).unwrap(),
        [PathBuf::from(path::join(&[
            "/home/me",
            ".consensflow",
            "bin"
        ]))]
    );
}

#[test]
fn without_a_home_there_is_no_place_and_the_sentence_says_so() {
    assert_eq!(
        Places::default().folders(&Env::default()).unwrap_err(),
        "missing home in env"
    );
}

#[test]
fn the_places_given_are_the_places_as_they_are_given() {
    let given = vec![PathBuf::from("/a/./b"), PathBuf::from("c/")];
    let places = Places::at(given.clone());
    assert_eq!(places.folders(&Env::default()).unwrap(), given);
}

#[test]
fn a_system_folder_is_one_that_begins_with_usr_or_opt_and_nothing_else_is() {
    for system in ["/usr", "/usr/local/bin", "/opt/x", "/usrfoo", "/optional"] {
        assert!(is_system(system), "{system}");
    }
    for user in [
        "/home/me/usr",
        "/tmp/opt",
        "usr",
        "/Users/me/.consensflow/bin",
        "",
    ] {
        assert!(!is_system(user), "{user}");
    }
}

#[test]
fn a_missing_folder_of_the_user_is_made_with_every_level_above_it() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("a").join("b").join("bin");
    make_missing(std::slice::from_ref(&bin));
    assert!(bin.is_dir());
    // One that is there is left as it is, and one that cannot be made is not said.
    fs::write(root.path().join("file"), "").unwrap();
    make_missing(&[bin, root.path().join("file").join("bin")]);
    assert!(root.path().join("file").is_file());
}

#[test]
fn a_folder_the_user_may_write_is_writable_and_a_missing_one_is_not() {
    let root = tempfile::tempdir().unwrap();
    assert!(writable(root.path()));
    assert!(!writable(&root.path().join("missing")));
    assert!(!writable(&root.path().join("file").join("bin")));
}

#[test]
fn a_file_is_as_writable_as_the_system_says_so_that_the_write_is_where_it_fails() {
    // `access` says nothing of what a path is: a file may be written, and a
    // write beneath it fails with its own error.
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("bin");
    fs::write(&file, "not a folder").unwrap();
    assert!(writable(&file));
}

#[cfg(unix)]
#[test]
fn a_folder_the_user_may_not_write_is_not_writable() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let locked = root.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    // Root may write where it likes.
    let said = writable(&locked);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if fs::write(locked.join("probe"), "").is_err() {
        assert!(!said);
    }
}
