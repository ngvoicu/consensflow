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
