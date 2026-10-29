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

/// A login as the machine keeps it: the tokens a refresh changes, and what Claude Code learns of the
/// account besides them.
fn login(access: &str, refresh: &str, expires: i64) -> Value {
    json!({
        "accessToken": access, "refreshToken": refresh, "expiresAt": expires,
        "scopes": ["user:inference"], "subscriptionType": "max",
    })
}

fn wrapped(oauth: &Value) -> Value {
    json!({ "claudeAiOauth": oauth })
}

#[test]
fn what_a_run_left_of_the_copy_is_told_by_the_names_of_the_fields_that_differ() {
    let copied = login("ACCESS-1", "REFRESH-1", 1_800_000_000_000);
    let of = |now: &Value| Copy::of(&copied, Some(&wrapped(now)));
    assert_eq!(of(&copied), Copy::Kept);
    // Claude Code learns the account's plan and its limit tier and writes them beside the tokens:
    // a login whose tokens are as copied was not refreshed.
    let mut learned = copied.clone();
    learned["rateLimitTier"] = json!("default_claude_max_5x");
    learned["subscriptionType"] = json!("pro");
    assert_eq!(
        of(&learned),
        Copy::Rewritten(vec![
            "rateLimitTier".to_owned(),
            "subscriptionType".to_owned()
        ])
    );
    // A refresh: a new access token, when it ends, and a new refresh token.
    let refreshed = login("ACCESS-2", "REFRESH-2", 1_800_000_999_000);
    assert_eq!(
        of(&refreshed),
        Copy::Refreshed(vec![
            "accessToken".to_owned(),
            "expiresAt".to_owned(),
            "refreshToken".to_owned()
        ])
    );
    // One token is enough, and what else was written is named after the tokens.
    let mut one = copied.clone();
    one["expiresAt"] = json!(1_800_000_999_000_i64);
    one["rateLimitTier"] = json!("x");
    assert_eq!(
        of(&one),
        Copy::Refreshed(vec!["expiresAt".to_owned(), "rateLimitTier".to_owned()])
    );
    // A token taken out is a change of it.
    let mut gone = copied.clone();
    gone.as_object_mut().unwrap().remove("refreshToken");
    assert_eq!(of(&gone), Copy::Refreshed(vec!["refreshToken".to_owned()]));
    // A file with no login in it, and one that is gone.
    assert_eq!(
        Copy::of(&copied, Some(&json!({ "mcpOAuth": {} }))),
        Copy::Refreshed(Vec::new())
    );
    assert_eq!(Copy::of(&copied, None), Copy::Lost);
}

#[test]
fn a_copy_only_rewritten_beside_its_tokens_is_no_failure_and_a_refreshed_one_names_its_fields_and_no_value(
) {
    let home = tempfile::tempdir().unwrap();
    let owner = Owner {
        home: home.path().to_path_buf(),
    };
    let copied = login("ACCESS-SECRET-1", "REFRESH-SECRET-1", 1_800_000_000_000);
    let made = Made {
        folder: tempfile::tempdir().unwrap(),
        oauth: copied.clone(),
        owner: owner.look(),
        services: None,
    };
    let write = |now: &Value| {
        std::fs::write(
            made.folder.path().join(".credentials.json"),
            wrapped(now).to_string(),
        )
        .unwrap();
    };
    write(&copied);
    assert_eq!((made.refreshed(), made.rewritten()), (None, None));
    let mut learned = copied.clone();
    learned["rateLimitTier"] = json!("default_claude_max_5x");
    write(&learned);
    assert_eq!(made.refreshed(), None);
    let note = made.rewritten().unwrap();
    assert!(note.contains("rateLimitTier"), "{note}");
    assert!(note.contains("not refreshed"), "{note}");
    write(&login(
        "ACCESS-SECRET-2",
        "REFRESH-SECRET-2",
        1_800_000_999_000,
    ));
    let said = made.refreshed().unwrap();
    assert!(
        said.contains("it changed accessToken, expiresAt, refreshToken"),
        "{said}"
    );
    assert_eq!(made.rewritten(), None);
    for text in [said, note] {
        assert!(!text.contains("SECRET"), "{text}");
    }
}
