//! The folders a Claude window's own tools are kept out of (T-1194). On a Mac a gated library
//! that a window's command or search touches raises a macOS dialog in the app's name and holds the
//! command until someone answers at the Mac; the window's settings file (the launch's own, never
//! the owner's) denies Claude's reads of the two that no coding task needs: the Apple Music
//! library and the Photos libraries. Claude Code applies a deny rule even in the permission mode
//! the windows run in, to its Read, Grep and Glob and to the path commands of a Bash call that
//! name the folder (not to a `find ~` that walks into it: the role's sentence is for that). A
//! project that lives in one of them is not denied its own folder, and Windows' file is as it was.

#[cfg(not(windows))]
use super::compaction_window::settings_of;
#[cfg(windows)]
use super::compaction_window::settings_text;
use super::*;

/// What the deny rules of a Mac's window say, in the order they are written.
#[cfg(not(windows))]
const DENIED: [&str; 4] = [
    "Read(~/Music)",
    "Read(~/Music/**)",
    "Read(~/Pictures/*.photoslibrary)",
    "Read(~/Pictures/*.photoslibrary/**)",
];

#[cfg(not(windows))]
fn denied(plan: &Prepared) -> Option<Vec<String>> {
    settings_of(plan)["permissions"]
        .get("deny")
        .and_then(Value::as_array)
        .map(|rules| {
            rules
                .iter()
                .map(|rule| rule.as_str().unwrap().to_owned())
                .collect()
        })
}

#[cfg(not(windows))]
#[test]
fn a_window_on_a_mac_denies_the_music_library_and_the_photos_libraries_to_clauds_reads() {
    let home = Home::new();
    let adapter = home.adapter();
    for request in [Request::default(), Request::chief()] {
        let plan = prepare(&adapter, &request).unwrap();
        assert_eq!(denied(&plan), Some(DENIED.map(str::to_owned).to_vec()));
        // Nothing else of the file moved: it is what it always was, but for the rules.
        let mut without = settings_of(&plan);
        without["permissions"]
            .as_object_mut()
            .unwrap()
            .remove("deny");
        let always: Value = serde_json::from_str(&if request.role == "chief" {
            CHIEF_SETTINGS_BEFORE.to_owned()
        } else {
            worker_settings_before(&home)
        })
        .unwrap();
        assert_eq!(without, always);
    }
}

#[cfg(not(windows))]
#[test]
fn the_rules_are_the_launchs_own_and_the_owners_settings_are_not_touched() {
    let home = Home::new();
    let adapter = home.adapter();
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let at = plan
        .argv
        .iter()
        .position(|arg| arg == "--settings")
        .unwrap();
    // The file is under the ConsensFlow home, in the launch's own folder; Claude's config
    // folder (the owner's settings.json among its files) was not written to.
    assert!(plan.argv[at + 1].starts_with(home.var("CONSENSFLOW_HOME")));
    assert!(!std::path::Path::new(home.var("CLAUDE_CONFIG_DIR"))
        .join("settings.json")
        .exists());
}

#[cfg(not(windows))]
#[test]
fn a_project_that_lives_in_the_music_folder_is_not_denied_its_own_folder_and_nor_is_one_in_a_photos_library(
) {
    let home = Home::new();
    let adapter = home.adapter();
    let music = path::join(&[home.var("HOME"), "Music", "songs"]);
    let plan = prepare(
        &adapter,
        &Request {
            directory: music,
            ..Request::default()
        },
    )
    .unwrap();
    assert_eq!(
        denied(&plan),
        Some(DENIED[2..].iter().map(|rule| (*rule).to_owned()).collect())
    );
    let library = path::join(&[
        home.var("HOME"),
        "Pictures",
        "Photos Library.photoslibrary",
        "x",
    ]);
    let plan = prepare(
        &adapter,
        &Request {
            directory: library,
            ..Request::default()
        },
    )
    .unwrap();
    assert_eq!(
        denied(&plan),
        Some(DENIED[..2].iter().map(|rule| (*rule).to_owned()).collect())
    );
    // A folder that merely starts as Music does is an ordinary folder.
    let plan = prepare(
        &adapter,
        &Request {
            directory: path::join(&[home.var("HOME"), "Musical"]),
            ..Request::default()
        },
    )
    .unwrap();
    assert_eq!(denied(&plan), Some(DENIED.map(str::to_owned).to_vec()));
}

#[cfg(windows)]
#[test]
fn a_window_on_windows_has_the_settings_file_it_always_had() {
    let home = Home::new();
    let adapter = home.adapter();
    assert_eq!(
        settings_text(&prepare(&adapter, &Request::default()).unwrap()),
        worker_settings_before(&home)
    );
}

const WORKER_SETTINGS_BEFORE: &str = r#"{"permissions":{"defaultMode":"bypassPermissions"},"skipDangerousModePermissionPrompt":true,"tui":"default","hooks":{"PreToolUse":[{"matcher":"AskUserQuestion","hooks":[{"type":"command","command":"{cf} hook claude","timeout":3600}]}],"Stop":[{"hooks":[{"type":"command","command":"exit 0"}]}]}}"#;

/// [`WORKER_SETTINGS_BEFORE`] as `home`'s adapter writes it: the question hook runs its bundle's
/// own `cf`, quoted (T-1049).
fn worker_settings_before(home: &Home) -> String {
    let cf = format!(
        "{}/bundle/bin/cf{}",
        home.root.replace('\\', "/"),
        if cfg!(windows) { ".exe" } else { "" }
    );
    WORKER_SETTINGS_BEFORE.replacen("{cf}", &format!("'{cf}'"), 1)
}

#[cfg(not(windows))]
const CHIEF_SETTINGS_BEFORE: &str = r#"{"permissions":{"defaultMode":"bypassPermissions"},"skipDangerousModePermissionPrompt":true,"tui":"default","hooks":{"PreToolUse":[],"Stop":[{"hooks":[{"type":"command","command":"exit 0"}]}]}}"#;
