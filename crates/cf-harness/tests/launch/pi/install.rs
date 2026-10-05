//! The extension made for Pi to load, as `tests/pi-install.test.mjs` holds
//! Node's.

use super::*;

/// The extension prepared for the environment of `home`.
fn extension(home: &Home) -> Extension {
    prepare_extension(&home.env())
}

#[test]
fn pi_absent_never_creates_an_extension() {
    let home = Home::without_pi();
    assert_eq!(extension(&home), Extension::NotInstalled);
    assert!(!Path::new(&path::join(&[home.var("CONSENSFLOW_HOME"), "extensions"])).exists());
}

#[test]
fn detected_pi_gets_an_immutable_private_extension_with_working_imports_and_no_global_edits() {
    let home = Home::new();
    let global = path::join(&[home.var("HOME"), ".pi", "agent"]);
    fs::create_dir_all(&global).unwrap();
    let settings = path::join(&[&global, "settings.json"]);
    fs::write(&settings, r#"{"extensions":["user-extension"]}"#).unwrap();
    let first = extension(&home);
    let Extension::InstalledUnverified { path: published } = &first else {
        panic!("{first:?}");
    };
    assert!(published.starts_with(home.var("CONSENSFLOW_HOME")));
    // Node imports the file, which holds its imports, in its own test of the
    // same file: what is held here is that it is the file the repository holds.
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../hosts/pi-extension/consensflow-delivery.mjs");
    assert_eq!(fs::read(published).unwrap(), fs::read(source).unwrap());
    assert_eq!(extension(&home), first, "the same folder, made once");
    assert_eq!(
        fs::read_to_string(&settings).unwrap(),
        r#"{"extensions":["user-extension"]}"#
    );
    // Never overwrite code possibly loaded by a live process.
    fs::write(published, "damaged").unwrap();
    let damaged = extension(&home);
    assert!(matches!(damaged, Extension::Error { .. }), "{damaged:?}");
    assert_eq!(fs::read_to_string(published).unwrap(), "damaged");
}

#[test]
fn a_pi_preparation_failure_is_reported_not_a_crash_or_a_false_ok() {
    let home = Home::new();
    let app = home.var("CONSENSFLOW_HOME");
    fs::create_dir_all(app).unwrap();
    fs::write(
        path::join(&[app, "extensions"]),
        "cannot create directory here",
    )
    .unwrap();
    let failed = extension(&home);
    let Extension::Error { reason } = failed else {
        panic!("{failed:?}");
    };
    assert!(
        reason.starts_with("ENOTDIR") || reason.contains("mkdir"),
        "{reason}"
    );
}

#[test]
fn opening_the_app_prepares_pi_only_when_its_executable_is_detected() {
    // `prepareApp` (`src/install.js`) is the app's own, and calls exactly
    // this for Pi: what is held here is that the extension is made once Pi
    // is on the PATH and not before.
    let home = Home::without_pi();
    assert_eq!(extension(&home), Extension::NotInstalled);
    home.install_pi();
    let after = extension(&home);
    assert!(
        matches!(after, Extension::InstalledUnverified { .. }),
        "{after:?}"
    );
}
