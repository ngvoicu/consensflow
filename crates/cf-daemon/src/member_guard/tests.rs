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
