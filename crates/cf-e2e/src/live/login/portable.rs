//! What of the login is the same on every machine, or only differs by the paths it is given,
//! so that the Mac's gate checks the Windows forms too. (The rest of the login's tests are for
//! the Unix machines: they read a file's mode.)

use super::*;

#[test]
fn the_login_is_in_the_keychain_on_a_mac_and_in_a_file_of_the_users_claude_folder_elsewhere() {
    assert_eq!(
        Source::on(true, Some(PathBuf::from("/Users/me"))),
        Some(Source::Keychain)
    );
    // The Keychain needs no home to be found; the run gives the folder one all the same.
    assert_eq!(Source::on(true, None), Some(Source::Keychain));
    assert_eq!(
        Source::on(false, Some(PathBuf::from("/home/me"))),
        Some(Source::File(PathBuf::from(
            "/home/me/.claude/.credentials.json"
        )))
    );
    // Windows' profile folder: the file is below it as on Linux, and the one the file names
    // is the machine's whatever separator the run's home was written with.
    let profile = PathBuf::from(r"C:\Users\AGV");
    assert_eq!(
        Source::on(false, Some(profile.clone())),
        Some(Source::File(
            profile.join(".claude").join(".credentials.json")
        ))
    );
    // A run with no home has nowhere to look.
    assert_eq!(Source::on(false, None), None);
}

#[test]
fn a_folder_of_a_run_is_known_by_the_separator_windows_wrote_it_with_as_well_as_by_ours() {
    for folder in [
        "/home/me/.consensflow-candidate/live/first-message",
        "C:/Users/AGV/.consensflow-candidate/live/first-message",
        r"C:\Users\AGV\.consensflow-candidate\live\first-message",
        r"C:\Users\AGV\.consensflow-candidate\evals\run-1",
        r"C:\Users\AGV\AppData\Local\Temp\consensflow-integration-ab12",
    ] {
        assert!(is_run_folder(folder), "{folder}");
    }
    for folder in [
        r"C:\Users\AGV\Projects\mine",
        "C:/Users/AGV/.consensflow-candidate",
        "/home/me/work",
    ] {
        assert!(!is_run_folder(folder), "{folder}");
    }
    // The name Claude Code gives such a folder below `projects`, as it writes it on Windows.
    assert!(is_run_name(
        "C--Users-AGV--consensflow-candidate-live-first-message"
    ));
    assert!(!is_run_name("C--Users-AGV-Projects-mine"));
}

#[test]
fn what_a_run_on_windows_left_in_the_owners_setup_is_found_out_by_the_folders_written_there() {
    let home = tempfile::tempdir().unwrap();
    let owner = Owner {
        home: home.path().to_path_buf(),
    };
    let before = owner.look();
    let folder = r"C:\Users\AGV\.consensflow-candidate\live\first-message";
    std::fs::write(
        home.path().join(".claude.json"),
        json!({ "projects": { folder: { "hasTrustDialogAccepted": true } } }).to_string(),
    )
    .unwrap();
    let typed = format!("{}\n", json!({ "display": "hi", "project": folder }));
    let found = changes(&before, &owner.look(), &typed);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found[0].contains("gained the project") && found[0].contains(folder),
        "{found:?}"
    );
    assert!(found[1].contains("1 prompt(s)"), "{found:?}");
    // A project of the owner's own is not the run's, whatever its separators.
    std::fs::write(
        home.path().join(".claude.json"),
        json!({ "projects": { r"C:\Users\AGV\Projects\mine": {} } }).to_string(),
    )
    .unwrap();
    assert!(owner.changes_since(&before).is_empty());
}
