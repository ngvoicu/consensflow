use std::fs;
use std::path::Path;

use super::*;
use crate::testing::fake_executable;

#[test]
fn a_launch_is_refused_in_node_s_sentence_for_each_way_the_plugin_failed() {
    let installed = Extension::InstalledUnverified {
        path: "/ext/consensflow-session.mjs".to_owned(),
        config: "/ext/tui.json".to_owned(),
    };
    assert_eq!(installed.into_config(), Ok("/ext/tui.json".to_owned()));
    let failed = Extension::Error {
        reason: "EACCES: permission denied".to_owned(),
    };
    assert_eq!(
        failed.into_config(),
        Err(
            "ConsensFlow's OpenCode plugin could not be installed: EACCES: permission denied"
                .to_owned()
        )
    );
    assert_eq!(
        Extension::NotInstalled.into_config(),
        Err("ConsensFlow's OpenCode plugin could not be installed: undefined".to_owned())
    );
}

#[test]
fn the_settings_name_the_plugin_by_the_file_url_it_has_in_the_folder_they_are_written_to() {
    let folder = if cfg!(windows) {
        r"C:\cf\extensions\opencode\ab12"
    } else {
        "/cf/extensions/opencode/ab12"
    };
    let [(name, bytes)] = &settings(folder)[..] else {
        panic!("one settings file");
    };
    assert_eq!(name, "hosts/opencode-extension/tui.json");
    let expected = if cfg!(windows) {
        r#"{"plugin":["file:///C:/cf/extensions/opencode/ab12/hosts/opencode-extension/consensflow-session.mjs"]}"#
    } else {
        r#"{"plugin":["file:///cf/extensions/opencode/ab12/hosts/opencode-extension/consensflow-session.mjs"]}"#
    };
    assert_eq!(String::from_utf8_lossy(bytes), expected);
}

#[test]
fn a_folder_that_is_not_whole_has_no_file_url_so_the_plugin_is_refused_and_nothing_made() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    fake_executable(&bin.join("opencode"));
    let env = Env::from_vars([
        ("PATH", bin.to_string_lossy().into_owned()),
        ("HOME", dir.path().to_string_lossy().into_owned()),
        ("CONSENSFLOW_HOME", "cf-opencode-relative-home".to_owned()),
    ]);
    assert_eq!(
        prepare_extension(&env),
        Extension::Error {
            reason: "ConsensFlow's folder is not an absolute path".to_owned()
        }
    );
    assert!(!Path::new("cf-opencode-relative-home").exists());
}
