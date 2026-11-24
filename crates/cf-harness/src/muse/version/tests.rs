use std::fs;

use super::*;

#[test]
fn a_version_is_read_from_each_place_muse_writes_one() {
    let file = Version::of("1.4.13-R6996.1\n").unwrap();
    let said = Version::of("Muse Code 1.4.13 (1.4.13-R6996.1)").unwrap();
    assert_eq!(file, said, ".muse-version and --version");
    let semver = Version::of("1.4.13").unwrap();
    assert_eq!(semver.semver(), "1.4.13");
    assert_eq!(file.semver(), "1.4.13");
    assert!(semver < file, "a release is after the bare numbers");
    assert_eq!(Version::of("1.4.13-R6996"), Version::of("1.4.13-R6996.0"));
    for not in [
        "",
        "1.4",
        "1.4.13.2",
        "v1.4.13",
        "1.4.13-6996",
        "1.4.13-R",
        "1.4.x",
        "Muse Code 1.4.13 (1.4.14-R1)",
        "Muse Code 1.4.13",
        "Muse Code 1.4.13 [1.4.13-R1]",
    ] {
        assert_eq!(Version::of(not), None, "{not:?}");
    }
}

#[test]
fn the_minimum_is_1_4_13_and_an_older_muse_is_refused_in_plain_words() {
    let supported = |text: &str| Version::of(text).unwrap().supported();
    assert!(supported("1.4.13"));
    assert!(supported("1.4.13-R6996.1"));
    assert!(supported("1.5.0"));
    assert!(supported("2.0.0-R1"));
    assert!(!supported("1.4.12-R9999.9"));
    assert!(!supported("1.3.99"));
    assert!(!supported("0.9.0"));
    assert_eq!(
        required(),
        "Muse Code 1.4.13 or newer is required. Update Muse Code before opening a pane."
    );
}

#[test]
fn a_release_is_newer_by_its_numbers_then_by_its_release() {
    assert_eq!(newer(Some("1.4.13-R6996.1"), "1.4.13-R6996.1"), Some(false));
    assert_eq!(newer(Some("1.4.13-R6996.1"), "1.4.13-R6996.2"), Some(true));
    assert_eq!(newer(Some("1.4.13-R6996.1"), "1.4.14-R1"), Some(true));
    assert_eq!(newer(Some("1.4.13-R6996.1"), "1.4.12-R9999"), Some(false));
    assert_eq!(newer(Some("1.4.13"), "1.4.13-R1"), Some(false));
    assert_eq!(newer(Some("1.4.13"), "1.4.14-R1"), Some(true));
    assert_eq!(newer(None, "1.4.13"), None);
    assert_eq!(newer(Some("1.4.13"), "latest"), None);
}

#[test]
fn the_installed_version_is_the_line_beside_the_launcher_where_it_really_is() {
    let folder = tempfile::tempdir().unwrap();
    let bin = folder.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let launcher = bin.join("muse");
    fs::write(&launcher, "#!/bin/bash\n").unwrap();
    assert_eq!(installed(&launcher), None, "no file");
    fs::write(bin.join(".muse-version"), "not a version\n").unwrap();
    assert_eq!(installed(&launcher), None, "a file that names none");
    fs::write(bin.join(".muse-version"), "1.4.13-R6996.1\n").unwrap();
    assert_eq!(installed(&launcher).as_deref(), Some("1.4.13-R6996.1"));
    #[cfg(unix)]
    {
        let elsewhere = folder.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let link = elsewhere.join("muse");
        std::os::unix::fs::symlink(&launcher, &link).unwrap();
        assert_eq!(installed(&link).as_deref(), Some("1.4.13-R6996.1"));
    }
}

#[test]
fn a_launcher_s_answer_is_the_one_line_wholly_muse_s_own_and_nothing_else() {
    let read = |said: &str| native(said).map(Version::semver);
    let own = |version: &str| format!("Muse Code {version} ({version}-R6996.1)");
    assert_eq!(read(&own("1.4.13")).as_deref(), Some("1.4.13"));
    assert_eq!(
        read(&format!("{}\n", own("1.4.13"))).as_deref(),
        Some("1.4.13")
    );
    // A wrapper's line is passed over, before Muse's or after it.
    let wrapper = |version: &str| format!("workspace launcher {version} ({version})");
    for (said, expected) in [
        (
            format!("{}\n{}", own("1.4.12"), wrapper("1.4.24")),
            "1.4.12",
        ),
        (
            format!("{}\n{}", own("1.4.24"), wrapper("1.4.12")),
            "1.4.24",
        ),
        (
            format!("{}\r\n{}\r\n", wrapper("1.4.24"), own("1.4.12")),
            "1.4.12",
        ),
        (
            format!("{}\r\n{}\r\n", own("1.4.24"), wrapper("1.4.12")),
            "1.4.24",
        ),
    ] {
        assert_eq!(read(&said).as_deref(), Some(expected), "{said:?}");
    }
    for not in [
        wrapper("1.4.24"),
        format!("{}\n{}", own("1.4.23"), own("1.4.23")),
        "Muse Code 1.4.23-beta (1.4.23-R1)".to_owned(),
        "Muse Code 1.4.23 (1.4.24-R1)".to_owned(),
        "Muse Code 1.4.23 (1.4.23)".to_owned(),
        "Muse Code 1.4.23".to_owned(),
        "prefix Muse Code 1.4.23 (1.4.23-R1)".to_owned(),
        "Muse Code 1.4.23 (1.4.23-R1) suffix".to_owned(),
        String::new(),
    ] {
        assert_eq!(read(&not), None, "{not:?}");
    }
}

#[test]
fn the_pin_is_one_bounded_line_of_a_regular_file() {
    let folder = tempfile::tempdir().unwrap();
    let launcher = folder.path().join("muse");
    fs::write(&launcher, "#!/bin/bash\n").unwrap();
    let pin = folder.path().join(".muse-version");
    for not in [
        "1.4.13\n1.4.14\n".to_owned(),
        "1.4.13 1.4.14".to_owned(),
        format!("1.4.13-R6996.1{}", " ".repeat(300)),
    ] {
        fs::write(&pin, &not).unwrap();
        assert_eq!(installed(&launcher), None, "{not:?}");
    }
    fs::write(&pin, [0xff, 0xfe]).unwrap();
    assert_eq!(installed(&launcher), None, "not text");
    fs::remove_file(&pin).unwrap();
    fs::create_dir(&pin).unwrap();
    assert_eq!(installed(&launcher), None, "a folder");
}

#[cfg(unix)]
#[test]
// A pipe is made by the system's own `mkfifo`: nothing here is a window's program.
#[allow(clippy::disallowed_methods)]
fn a_pipe_in_the_pin_s_place_is_never_opened() {
    let folder = tempfile::tempdir().unwrap();
    let launcher = folder.path().join("muse");
    fs::write(&launcher, "#!/bin/bash\n").unwrap();
    let pipe = folder.path().join(".muse-version");
    assert!(std::process::Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .unwrap()
        .success());
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(installed(&launcher));
    });
    let answer = receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the read returned: it did not wait on the pipe");
    assert_eq!(answer, None);
}
