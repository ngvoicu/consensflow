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
        folder: crate::door::made("consensflow-test-").unwrap(),
        oauth: copied.clone(),
        login: copied.clone(),
        owner: owner.look(),
        services: None,
        renewed: None,
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

/// What a decoy of the login sweep's test runs: this test program again, which waits a minute
/// where the test started it (`CF_E2E_LOGIN_DECOY`) and ends at once anywhere else.
#[test]
#[ignore = "a decoy the login sweep's test starts as a program of its own"]
#[allow(clippy::disallowed_methods)] // The decoy reads the switch it was started with.
fn a_decoy_that_holds_a_login_copy_for_a_minute() {
    if std::env::var_os("CF_E2E_LOGIN_DECOY").is_some() {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

/// A process number nothing has: this test program run for no test, which ends at once and is
/// waited for.
fn a_gone_pid() -> u32 {
    let me = std::env::current_exe().unwrap();
    let mut gone = crate::process::Run::new(&me)
        .args(["--exact", "no test is called this"])
        .spawn()
        .unwrap();
    let pid = gone.id();
    gone.wait(std::time::Duration::from_secs(30)).unwrap();
    pid
}

/// A login copy whose run's process is gone is still in use while a process of that run runs on
/// it, as a detached harness does: it holds the folder only in its environment
/// (`CLAUDE_CONFIG_DIR`), as a real one does, and on Windows, whose processes' environments
/// cannot be read, in its command line. That folder is left; one that nothing uses is taken away
/// (T-1557, finding 3b).
#[test]
fn a_login_copy_of_a_run_that_is_gone_is_left_while_a_process_uses_it_and_taken_when_none_does() {
    let parent = tempfile::tempdir().unwrap();
    let gone = a_gone_pid();
    let make = |name: String| {
        let folder = parent.path().join(name);
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join(".credentials.json"), "{}").unwrap();
        folder
    };
    let used = make(format!("{PREFIX}{gone}-used111"));
    let free = make(format!("{PREFIX}{gone}-free222"));
    let me = std::env::current_exe().unwrap();
    let mut decoy = crate::process::Run::new(&me)
        .args([
            "--exact",
            "live::login::portable::a_decoy_that_holds_a_login_copy_for_a_minute",
            "--ignored",
        ])
        .args(cfg!(windows).then(|| used.display().to_string()))
        .var("CF_E2E_LOGIN_DECOY", "1")
        .var("CLAUDE_CONFIG_DIR", used.display().to_string())
        .spawn()
        .unwrap();
    // Listed with what it runs on before the sweep looks.
    let pid = decoy.id();
    let listed = (0..100).any(|_| {
        use super::super::members::Machine as _;
        let seen = super::super::members::ThisMachine
            .processes()
            .is_ok_and(|all| all.iter().any(|one| one.identity.pid == pid));
        if !seen {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        seen
    });

    sweep(parent.path(), PREFIX);
    let (used_there, free_there) = (used.exists(), free.exists());
    decoy.kill();

    assert!(listed, "the decoy was never listed");
    assert!(
        used_there,
        "a login copy a process of its run still uses was taken away"
    );
    assert!(!free_there, "a login copy nothing uses was left");
}
