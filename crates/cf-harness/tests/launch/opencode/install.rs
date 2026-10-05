//! The plugin made for OpenCode to load, as `tests/opencode-install.test.mjs`
//! holds Node's: an immutable private bundle with a `tui.json` that names
//! it, never an edit of OpenCode's own settings, and a launch that loads it
//! with credentials of its own. Node's `Harnesses` case runs the app's own
//! preparation (`HarnessAdmin`, `prepareApp`), which is not here; what is
//! held is that the plugin is made once OpenCode is on the PATH and not
//! before.

use std::fs;
use std::path::Path;

use cf_base::path::{self, to_file_url};
use cf_harness::contract::Prepared;
use cf_harness::opencode::{prepare_extension, Extension};
use serde_json::Value;

use super::stage::{planned, Home, Stage, Wanted, LAUNCH};

/// The plugin prepared for the environment of `home`.
fn extension(home: &Home) -> Extension {
    prepare_extension(&home.env())
}

#[test]
fn opencode_absent_never_creates_a_plugin() {
    let home = Home::without_opencode();
    assert_eq!(extension(&home), Extension::NotInstalled);
    assert!(!Path::new(&path::join(&[home.var("CONSENSFLOW_HOME"), "extensions"])).exists());
}

#[test]
fn detected_opencode_gets_an_immutable_private_plugin_with_working_imports_and_no_global_edits() {
    let home = Home::new();
    let global = path::join(&[home.var("HOME"), ".config", "opencode", "tui.json"]);
    fs::create_dir_all(Path::new(&global).parent().unwrap()).unwrap();
    fs::write(&global, r#"{"plugin":["user-plugin"]}"#).unwrap();
    let first = extension(&home);
    let Extension::InstalledUnverified {
        path: published, ..
    } = &first
    else {
        panic!("{first:?}");
    };
    assert!(published.starts_with(home.var("CONSENSFLOW_HOME")));
    // Node imports the file, which holds its imports, in its own test of the
    // same file: what is held here is that it is the file the repository
    // holds, and the one it imports beside it.
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../hosts");
    assert_eq!(
        fs::read(published).unwrap(),
        fs::read(source.join("opencode-extension/consensflow-session.mjs")).unwrap()
    );
    let door = Path::new(published)
        .parent()
        .unwrap()
        .join("../lib/question-door.js");
    assert_eq!(
        fs::read(door).unwrap(),
        fs::read(source.join("lib/question-door.js")).unwrap()
    );
    assert_eq!(extension(&home), first, "the same folder, made once");
    assert_eq!(
        fs::read_to_string(&global).unwrap(),
        r#"{"plugin":["user-plugin"]}"#
    );
    // Never overwrite code possibly loaded by a live process.
    fs::write(published, "drifted").unwrap();
    let drifted = extension(&home);
    assert!(matches!(drifted, Extension::Error { .. }), "{drifted:?}");
    assert_eq!(fs::read_to_string(published).unwrap(), "drifted");
}

#[test]
fn a_preparation_failure_is_reported_not_a_crash_or_a_false_ok() {
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
fn opening_the_app_prepares_opencode_only_when_its_executable_is_detected() {
    let home = Home::without_opencode();
    assert_eq!(extension(&home), Extension::NotInstalled);
    home.install_opencode();
    let after = extension(&home);
    assert!(
        matches!(after, Extension::InstalledUnverified { .. }),
        "{after:?}"
    );
}

#[test]
fn a_launch_loads_the_bundled_tui_integration_with_unique_private_admission_credentials() {
    let stage = Stage::new();
    stage.serves_creation("ses_abc123");
    stage.serves_creation("ses_other456");
    let first = stage.prepare(&Wanted::default()).unwrap();
    let second = stage.prepare(&Wanted::default()).unwrap();
    let tui = planned(&first, "OPENCODE_TUI_CONFIG").unwrap();
    let config: Value = serde_json::from_str(&fs::read_to_string(tui).unwrap()).unwrap();
    let plugins = config["plugin"].as_array().unwrap();
    assert_eq!(plugins.len(), 1);
    let plugin = plugins[0].as_str().unwrap();
    assert!(plugin.ends_with("consensflow-session.mjs"), "{plugin}");
    // It is the plugin's own file, as a URL.
    let folder = Path::new(tui).parent().unwrap();
    let file = folder
        .join("consensflow-session.mjs")
        .to_string_lossy()
        .into_owned();
    assert_eq!(to_file_url(&file, cfg!(windows)).as_deref(), Some(plugin));
    let options = |plan: &Prepared| -> Value {
        serde_json::from_str(planned(plan, "CF_OPENCODE_SESSION_BRIDGE").unwrap()).unwrap()
    };
    assert_eq!(options(&first)["launchId"], LAUNCH);
    assert_ne!(options(&first)["token"], options(&second)["token"]);
    assert_ne!(options(&first)["port"], options(&second)["port"]);
    assert_eq!(options(&first)["token"].as_str().unwrap().len(), 32);
    // The home's own environment names no settings file, and still does not.
    assert_eq!(stage.home.env().text("OPENCODE_TUI_CONFIG"), None);
}
