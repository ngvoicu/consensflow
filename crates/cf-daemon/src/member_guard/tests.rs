use cf_base::env::Env;

use super::*;
use crate::testing::scene;

fn log_in(home: &Path) -> Log {
    Log::new(home)
}

#[test]
fn every_window_has_the_guards_folder_first_on_its_path_before_the_bundle() {
    let scene = scene();
    let home = tempfile_dir("first");
    let cf = home.join("cf");
    std::fs::write(&cf, b"cf").unwrap();
    let env = Env::from_vars([("PATH", "/usr/bin:/bin")]);
    let windows = WindowEnv::new(&env, "http://127.0.0.1:1", "/bundle/bin");
    let guarded = guard(windows, &home, &cf, &log_in(&home));
    let participant = scene.project.participants.first().unwrap();
    let started = cf_engine::seams::PaneEnv::env(&guarded, participant, &scene.project);
    let path = &started.iter().find(|(name, _)| name == "PATH").unwrap().1;
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    assert_eq!(
        *path,
        format!(
            "{}{delimiter}/bundle/bin{delimiter}/usr/bin:/bin",
            folder(&home).display()
        )
    );
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn the_guards_folder_holds_cf_under_each_name_of_the_platform_and_is_made_again_when_wrong() {
    let home = tempfile_dir("names");
    let cf = home.join(if cfg!(windows) { "cf.exe" } else { "cf" });
    std::fs::write(&cf, b"cf").unwrap();
    let folder = install(&home, &cf).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let expected: &[&str] = if cfg!(windows) {
        &["taskkill.exe", "tskill.exe"]
    } else {
        &["killall", "pkill"]
    };
    assert_eq!(names, expected);
    // Again, with the files in place, and with one gone.
    install(&home, &cf).unwrap();
    std::fs::remove_file(folder.join(expected[0])).unwrap();
    install(&home, &cf).unwrap();
    for name in expected {
        assert_eq!(std::fs::read(folder.join(name)).unwrap(), b"cf");
    }
    std::fs::remove_dir_all(&home).unwrap();
}

#[cfg(unix)]
#[test]
fn a_file_that_is_no_link_of_ours_is_kept_and_the_failure_named() {
    let home = tempfile_dir("foreign");
    let cf = home.join("cf");
    std::fs::write(&cf, b"cf").unwrap();
    std::fs::create_dir_all(folder(&home)).unwrap();
    std::fs::write(folder(&home).join("pkill"), b"someone's").unwrap();
    let (file, _) = install(&home, &cf).unwrap_err();
    assert_eq!(file, folder(&home).join("pkill"));
    assert_eq!(std::fs::read(&file).unwrap(), b"someone's");
    assert!(
        folder(&home).join("killall").exists(),
        "the others are made"
    );
    std::fs::remove_dir_all(&home).unwrap();
}

/// A new folder of the test's own, named by `what`, in the system's temp folder.
fn tempfile_dir(what: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cf-member-guard-{what}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An `etc` as the system's, with what `path_helper` reads of it.
#[cfg(unix)]
fn system_etc(what: &str) -> PathBuf {
    let etc = tempfile_dir(what);
    std::fs::create_dir_all(etc.join("paths.d")).unwrap();
    std::fs::write(etc.join("paths"), "/usr/bin\n/bin\n").unwrap();
    std::fs::write(etc.join("manpaths"), "/usr/share/man\n").unwrap();
    std::fs::write(etc.join("paths.d/10-one"), "/one\n").unwrap();
    etc
}

#[cfg(unix)]
#[test]
fn the_root_mirrors_the_systems_files_with_the_guards_folder_first_and_follows_them() {
    let etc = system_etc("mirror-etc");
    let home = tempfile_dir("mirror-home");
    let root = path_helper_root(&home);
    mirror(&etc, &root, &folder(&home)).unwrap();
    let read = |name: &str| std::fs::read_to_string(root.join("etc").join(name)).ok();
    assert_eq!(
        read("paths"),
        Some(format!("{}\n/usr/bin\n/bin\n", folder(&home).display()))
    );
    assert_eq!(read("manpaths").as_deref(), Some("/usr/share/man\n"));
    assert_eq!(read("paths.d/10-one").as_deref(), Some("/one\n"));
    assert_eq!(read("manpaths.d/x"), None);
    // The system's files change: one gone, one new, one changed.
    std::fs::remove_file(etc.join("paths.d/10-one")).unwrap();
    std::fs::write(etc.join("paths.d/20-two"), "/two\n").unwrap();
    std::fs::remove_file(etc.join("manpaths")).unwrap();
    mirror(&etc, &root, &folder(&home)).unwrap();
    assert_eq!(read("paths.d/10-one"), None);
    assert_eq!(read("paths.d/20-two").as_deref(), Some("/two\n"));
    assert_eq!(read("manpaths"), None);
    let mut listed: Vec<_> = std::fs::read_dir(root.join("etc/paths.d"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    listed.sort();
    assert_eq!(listed, ["20-two"], "no file of ours is left among them");
    std::fs::remove_dir_all(&etc).unwrap();
    std::fs::remove_dir_all(&home).unwrap();
}

#[cfg(unix)]
#[test]
fn a_link_at_a_name_in_the_root_is_replaced_never_written_through_and_a_linked_folder_is_refused() {
    let etc = system_etc("link-etc");
    let home = tempfile_dir("link-home");
    let root = path_helper_root(&home);
    mirror(&etc, &root, &folder(&home)).unwrap();
    let own = home.join("persons-file");
    std::fs::write(&own, "the person's\n").unwrap();
    std::fs::remove_file(root.join("etc/paths")).unwrap();
    std::os::unix::fs::symlink(&own, root.join("etc/paths")).unwrap();
    mirror(&etc, &root, &folder(&home)).unwrap();
    assert_eq!(std::fs::read_to_string(&own).unwrap(), "the person's\n");
    assert!(std::fs::symlink_metadata(root.join("etc/paths"))
        .unwrap()
        .is_file());
    std::fs::remove_dir_all(root.join("etc/paths.d")).unwrap();
    std::os::unix::fs::symlink(etc.join("paths.d"), root.join("etc/paths.d")).unwrap();
    assert!(mirror(&etc, &root, &folder(&home)).is_err());
    std::fs::remove_dir_all(&etc).unwrap();
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_person_with_a_path_helper_root_of_their_own_keeps_it() {
    let root = Path::new("/ours");
    let given = |vars: Vec<(&str, &str)>| {
        WindowEnv::new(&Env::from_vars(vars), "http://127.0.0.1:1", "/bundle/bin")
            .with_path_helper_root(root)
            .guard_env()
            .into_iter()
            .find(|(name, _)| name == "PATH_HELPER_ROOT")
            .map(|(_, value)| value)
    };
    assert_eq!(given(vec![]), Some("/ours".to_owned()));
    assert_eq!(given(vec![("PATH_HELPER_ROOT", "/theirs")]), None);
    // A root that is the guard's, as a window gave it to a ConsensFlow started inside it, is
    // no one's own.
    assert_eq!(
        given(vec![
            ("PATH_HELPER_ROOT", "/other/member-guard-root"),
            ("CONSENSFLOW_PATH_HELPER_ROOT", "/other/member-guard-root"),
        ]),
        Some("/ours".to_owned())
    );
    // As the app's terminal said it.
    assert_eq!(
        given(vec![
            (cf_base::env::TERMINAL_ENV, r#"["PATH_HELPER_ROOT"]"#),
            ("PATH_HELPER_ROOT", "/theirs"),
        ]),
        None
    );
}

/// The system's own `path_helper`, on the root the daemon makes: the system's answer with the
/// guard's folder first, and the same MANPATH. It fails where an OS stops reading
/// `PATH_HELPER_ROOT`, which the guard's hold in a Mac's login shells stands on (CI's Mac
/// runners run it).
#[cfg(target_os = "macos")]
#[test]
#[allow(clippy::disallowed_methods)] // The test asks the system's own helper.
fn macos_path_helper_reads_path_helper_root_and_answers_as_the_system_with_the_guards_folder_first()
{
    let home = tempfile_dir("helper home");
    let root = path_helper_root(&home);
    mirror(Path::new("/etc"), &root, &folder(&home)).unwrap();
    let asked = |helper_root: Option<&Path>| {
        let mut helper = std::process::Command::new("/usr/libexec/path_helper");
        helper.arg("-s").env_clear().env("MANPATH", "/x");
        if let Some(helper_root) = helper_root {
            helper.env("PATH_HELPER_ROOT", helper_root);
        }
        let said = String::from_utf8(helper.output().unwrap().stdout).unwrap();
        let value = |name: &str| {
            let start = format!("{name}=\"");
            said.lines()
                .find_map(|line| line.strip_prefix(&start))
                .and_then(|line| line.split('"').next())
                .map(str::to_owned)
        };
        (value("PATH"), value("MANPATH"))
    };
    let (system_path, system_manpath) = asked(None);
    let (path, manpath) = asked(Some(&root));
    let system_path = system_path.expect("the system's PATH");
    assert_eq!(
        path,
        Some(format!("{}:{system_path}", folder(&home).display())),
        "path_helper no longer reads PATH_HELPER_ROOT, or reads it differently"
    );
    assert_eq!(manpath, system_manpath);
    std::fs::remove_dir_all(&home).unwrap();
}

/// An update replaces `cf.exe` and leaves the old hard link on the old file, which may have the
/// new one's size and time: the content is what says it is stale.
#[cfg(windows)]
#[test]
fn a_stand_in_of_cfs_size_and_time_and_other_content_is_made_again() {
    let home = tempfile_dir("stale");
    let cf = home.join("cf.exe");
    std::fs::write(&cf, b"ONE").unwrap();
    let folder = install(&home, &cf).unwrap();
    let stamp = std::fs::metadata(&cf).unwrap().modified().unwrap();
    // Replaced, as an update does it: a new file at the name, the links on the old one.
    let new = home.join("cf.new");
    std::fs::write(&new, b"TWO").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&new)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    std::fs::rename(&new, &cf).unwrap();
    assert_eq!(std::fs::read(folder.join("taskkill.exe")).unwrap(), b"ONE");
    install(&home, &cf).unwrap();
    for name in ["taskkill.exe", "tskill.exe"] {
        assert_eq!(std::fs::read(folder.join(name)).unwrap(), b"TWO", "{name}");
    }
    std::fs::remove_dir_all(&home).unwrap();
}
