//! Who names the folder the runtime is unpacked into, and whether they name the
//! one the app unpacks into.
//!
//! The runtime goes under a parent of its own ([`RUNTIME_PARENT`]), out of reach
//! of the collector of the apps before the flip release, which empties
//! `runtime`. What looks for the runtime by name has to name that parent: the
//! workflow that starts the portable exe once, and the release page, whose text
//! is the publisher's.

use std::fs;
use std::path::Path;

use super::RUNTIME_PARENT;

/// The names of the folders a text puts under the app's local data folder, as
/// `dev.ngvoicu.consensflow\<name>` (a doubled backslash, as a string in
/// JavaScript has it, is one) where the name ends in `runtime`.
fn runtime_folders_named(text: &str) -> Vec<String> {
    const APP: &str = "dev.ngvoicu.consensflow";
    text.match_indices(APP)
        .filter_map(|(at, _)| {
            let after = &text[at + APP.len()..];
            let name = after.trim_start_matches('\\');
            let named = name
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                .collect::<String>();
            (name.len() < after.len() && named.ends_with("runtime")).then_some(named)
        })
        .collect()
}

#[test]
fn the_workflow_and_the_release_page_name_the_folder_the_runtime_is_unpacked_into() {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the checkout");
    assert_ne!(RUNTIME_PARENT, "runtime", "the old apps empty `runtime`");
    for file in [
        ".github/workflows/windows-build.yml",
        "tools/cf-publish/src/publish.rs",
    ] {
        let text = fs::read_to_string(checkout.join(file)).expect("the file");
        let folders = runtime_folders_named(&text);
        assert!(
            folders.iter().any(|folder| folder == RUNTIME_PARENT),
            "{file} does not name {RUNTIME_PARENT}"
        );
        // The one mention of the old folder is the workflow's check that
        // nothing went into it.
        let others = folders
            .iter()
            .filter(|folder| *folder != RUNTIME_PARENT)
            .collect::<Vec<_>>();
        let expected: &[&str] = if file.ends_with("windows-build.yml") {
            &["runtime"]
        } else {
            &[]
        };
        assert_eq!(
            others, expected,
            "{file} looks in a folder other than {RUNTIME_PARENT}"
        );
    }
}

#[test]
fn the_folders_a_text_names_are_read_off_a_powershell_path_and_a_javascript_string() {
    let text = "'dev.ngvoicu.consensflow\\portable-runtime\\{0}-{1:x8}' and \
                '`%LOCALAPPDATA%\\\\dev.ngvoicu.consensflow\\\\portable-runtime`' and \
                'dev.ngvoicu.consensflow\\runtime' and \
                'dev.ngvoicu.consensflow.candidate' and 'dev.ngvoicu.consensflow\\logs'";
    assert_eq!(
        runtime_folders_named(text),
        ["portable-runtime", "portable-runtime", "runtime"]
    );
}
