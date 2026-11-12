// The tests are for the machines the login is copied on: the Unix ones.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use super::*;

/// What a login holds that must never be said or kept.
const ACCESS: &str = "sk-ant-oat01-ACCESS-0123456789";
const REFRESH: &str = "sk-ant-ort01-REFRESH-9876543210";
const MCP: &str = "mcp-token-ZZZZ";
const SECRETS: [&str; 3] = [ACCESS, REFRESH, MCP];

/// How long a login must last for a case planned at an hour and a half, with its margin.
const LEFT: Duration = Duration::from_secs(2 * 60 * 60);

/// `seconds` after the epoch, in milliseconds.
fn at(seconds: u64) -> i64 {
    i64::try_from(seconds * 1000).unwrap()
}

/// The text the machine keeps for a login whose access token ends `lasting` after `NOW`.
const NOW: u64 = 1_800_000_000;

fn machine_text(lasting: Duration) -> String {
    let expires = (NOW + lasting.as_secs()) * 1000;
    json!({
        "mcpOAuth": { "plugin:x:x|1": { "accessToken": MCP, "refreshToken": MCP } },
        "claudeAiOauth": {
            "accessToken": ACCESS, "refreshToken": REFRESH, "expiresAt": expires,
            "scopes": ["user:inference"], "subscriptionType": "max",
        },
    })
    .to_string()
}

fn hours(hours: u64) -> Duration {
    Duration::from_secs(hours * 3600)
}

/// A login that copies `text` from a file, for the owner whose home is `home`, into folders
/// of `parent`.
fn login(text: &str, home: &Path, parent: &Path) -> Login {
    let file = home.join("machine-login.json");
    std::fs::write(&file, text).unwrap();
    Login::new(
        Source::File(file),
        home.to_path_buf(),
        parent.to_path_buf(),
        LEFT,
    )
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn said_anywhere(text: &str) -> bool {
    SECRETS.iter().any(|secret| text.contains(secret))
}

#[test]
fn only_the_login_of_what_the_machine_keeps_is_copied() {
    let copied = oauth(&machine_text(hours(5)), at(NOW), LEFT).unwrap();
    assert_eq!(copied["accessToken"], ACCESS);
    assert_eq!(copied["refreshToken"], REFRESH);
    // The text holds the OAuth tokens of the person's MCP servers too; the run is given none.
    assert!(!copied.to_string().contains(MCP), "{copied}");
}

#[test]
fn a_login_that_cannot_be_used_is_refused_and_what_the_text_held_is_not_said() {
    let with_no_login = json!({ "mcpOAuth": { "x": { "accessToken": MCP } } }).to_string();
    let no_refresh =
        json!({ "claudeAiOauth": { "accessToken": ACCESS, "expiresAt": 1 } }).to_string();
    let no_expiry =
        json!({ "claudeAiOauth": { "accessToken": ACCESS, "refreshToken": REFRESH } }).to_string();
    for (text, why) in [
        (format!("not JSON {ACCESS}"), "it is not JSON"),
        (with_no_login, "no `claudeAiOauth`"),
        (no_refresh, "no `refreshToken`"),
        (no_expiry, "no `expiresAt`"),
    ] {
        let said = oauth(&text, at(NOW), LEFT).unwrap_err().to_string();
        assert!(said.contains(why), "{why}: {said}");
        assert!(!said_anywhere(&said), "{said}");
    }
}

#[test]
fn a_login_that_ends_within_the_two_hours_is_refused_and_one_that_lasts_longer_is_not() {
    let near = machine_text(LEFT - Duration::from_secs(60));
    let said = oauth(&near, at(NOW), LEFT).unwrap_err().to_string();
    assert!(said.contains("expires in 1 h 59 min"), "{said}");
    assert!(said.contains("at least 2 h more"), "{said}");
    // Using Claude Code now does not refresh a token with two hours left: it is refreshed when
    // five minutes of it are left, so the run is told to wait for that, or to be shorter.
    assert!(
        said.contains("only when five minutes of it are left"),
        "{said}"
    );
    assert!(said.contains("make the run shorter"), "{said}");
    assert!(!said.contains("yourself"), "{said}");
    assert!(!said_anywhere(&said), "{said}");
    assert!(oauth(&machine_text(LEFT + Duration::from_secs(60)), at(NOW), LEFT).is_ok());
    // One that has ended already.
    let ended = oauth(&machine_text(Duration::ZERO), at(NOW + 10), LEFT)
        .unwrap_err()
        .to_string();
    assert!(ended.contains("has expired"), "{ended}");
}

#[test]
fn a_run_that_states_its_own_length_is_refused_a_login_that_would_end_before_it_does() {
    let home = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    // A login with five hours left: a live suite (two hours) takes it, a six-hour eval does not.
    let text = machine_text(hours(5));
    let suite = login(&text, home.path(), parent.path());
    assert!(suite.folder_at(at(NOW)).is_ok());
    let mut eval = login(&text, home.path(), parent.path());
    eval.left = hours(6);
    let said = eval.folder_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("expires in 5 h"), "{said}");
    assert!(said.contains("at least 6 h more"), "{said}");
    assert!(!said_anywhere(&said), "{said}");
    // One that fits the run's length is taken, and no folder was made for the refused one.
    let mut fits = login(&text, home.path(), parent.path());
    fits.left = hours(4);
    assert!(fits.folder_at(at(NOW)).is_ok());
    assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 2);
}

#[test]
fn a_login_asks_what_it_is_told_a_case_or_an_eval_plans_and_nothing_of_its_own() {
    let source = Source::File(PathBuf::from("/nowhere/login.json"));
    let made = Login::new(
        source,
        PathBuf::from("/home/me"),
        PathBuf::from("/tmp"),
        hours(3),
    );
    assert_eq!(made.left, hours(3));
    // On this machine, if it has a login to copy at all.
    for left in [Duration::from_secs(40 * 60), hours(7)] {
        if let Some(login) = Login::of_machine_lasting(left) {
            assert_eq!(login.left, left);
        }
    }
}

#[test]
fn a_span_is_said_in_hours_and_minutes() {
    assert_eq!(span(Duration::from_secs(45 * 60)), "45 min");
    assert_eq!(span(hours(2)), "2 h");
    assert_eq!(span(Duration::from_secs(4300)), "1 h 11 min");
    assert_eq!(span(Duration::ZERO), "0 min");
}

#[test]
fn the_keychain_is_asked_for_the_item_of_the_users_own_account() {
    // A lookup that names no account returns the item of the account `unknown`, which holds
    // no refresh token and ended long ago.
    assert_eq!(
        keychain_args("me"),
        [
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-a",
            "me",
            "-w"
        ]
    );
}

#[test]
fn the_items_claude_code_keeps_are_the_ones_of_its_service_and_the_ones_named_after_a_folder() {
    let dump = "keychain: \"/Users/me/Library/Keychains/login.keychain-db\"\n\
                class: \"genp\"\n    \"svce\"<blob>=\"Claude Code-credentials\"\n\
                class: \"genp\"\n    \"svce\"<blob>=\"Claude Code-credentials-ee80e5e3\"\n\
                class: \"genp\"\n    \"svce\"<blob>=\"Claude Safe Storage\"\n\
                class: \"genp\"\n    \"svce\"<blob>=\"AIR Claude Credentials\"\n\
                class: \"genp\"\n    \"svce\"<blob>=<NULL>\n";
    assert_eq!(
        services(dump),
        BTreeSet::from([
            "Claude Code-credentials".to_owned(),
            "Claude Code-credentials-ee80e5e3".to_owned()
        ])
    );
}

#[test]
fn the_item_claude_code_made_for_the_runs_folder_is_the_runs_and_a_failure_with_the_command_that_deletes_it(
) {
    let folder = Path::new("/tmp/cf-keychain-vector-1");
    let own = "Claude Code-credentials-b0d4aa31";
    let had = BTreeSet::from(["Claude Code-credentials".to_owned()]);
    assert_eq!(keychain_of(folder, &had, &had), InKeychain::default());
    let now = BTreeSet::from(["Claude Code-credentials".to_owned(), own.to_owned()]);
    let kept = keychain_of(folder, &had, &now);
    assert_eq!(kept.own.len(), 1, "{kept:?}");
    assert!(kept.own[0].contains(&format!("`{own}`")), "{kept:?}");
    assert!(
        kept.own[0].contains(&format!("security delete-generic-password -s \"{own}\"")),
        "{kept:?}"
    );
    assert!(kept.own[0].contains("--strict-mcp-config"), "{kept:?}");
    assert!(kept.others.is_empty(), "{kept:?}");
}

#[test]
fn an_item_another_process_made_is_a_note_of_this_run_and_not_its_failure() {
    // T-1044: an eval was failed, and told its Claude Code had refreshed its login, for an item that
    // another run on the machine made (the Keychain is the whole machine's).
    let folder = Path::new("/tmp/cf-keychain-vector-1");
    let had = BTreeSet::from(["Claude Code-credentials".to_owned()]);
    let now = BTreeSet::from([
        "Claude Code-credentials".to_owned(),
        "Claude Code-credentials-f2cee134".to_owned(),
    ]);
    let kept = keychain_of(folder, &had, &now);
    assert!(kept.own.is_empty(), "{kept:?}");
    assert_eq!(kept.others.len(), 1, "{kept:?}");
    assert!(
        kept.others[0].contains("Claude Code-credentials-f2cee134"),
        "{kept:?}"
    );
    assert!(!kept.others[0].contains("refresh"), "{kept:?}");
    // An item the Keychain had before the run is no one's of the run, whatever its name.
    let both = BTreeSet::from(["Claude Code-credentials-b0d4aa31".to_owned()]);
    assert_eq!(keychain_of(folder, &both, &both), InKeychain::default());
}

/// A Keychain item of the name Claude Code gives `folder`, made here and deleted by that name when this
/// is dropped (the Keychain is the machine's: a test of the run's own folder is the only way to see
/// `finish` look at it).
#[cfg(target_os = "macos")]
struct Item {
    name: String,
    user: String,
}

#[cfg(target_os = "macos")]
impl Item {
    fn make(folder: &Path) -> Self {
        let name = keychain::item_of(folder);
        let user = own_var("USER").unwrap();
        let made = Run::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-s",
                &name,
                "-a",
                &user,
                "-w",
                "a made-up value",
            ])
            .inheriting_env()
            .run()
            .unwrap();
        assert_eq!(made.code, Some(0), "{}", made.stderr);
        Self { name, user }
    }
}

#[cfg(target_os = "macos")]
impl Drop for Item {
    fn drop(&mut self) {
        let _ = Run::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-s",
                &self.name,
                "-a",
                &self.user,
            ])
            .inheriting_env()
            .run();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_run_whose_claude_code_moved_the_login_into_the_keychain_is_told_so_and_not_that_it_refreshed_it(
) {
    let person = Person::new();
    let parent = tempfile::tempdir().unwrap();
    let login = login(&machine_text(hours(5)), person.home.path(), parent.path());
    let folder = login.folder_at(at(NOW)).unwrap();
    // What Claude Code did to a folder in the probe of T-1102: its secure storage into the
    // folder's item, and the login out of the folder's file.
    let item = Item::make(&folder);
    std::fs::remove_file(folder.join(".credentials.json")).unwrap();
    let said = login.finish().unwrap_err().to_string();
    assert!(
        said.contains("LEFT ITS SECURE STORAGE IN THE OWNER'S KEYCHAIN"),
        "{said}"
    );
    assert!(said.contains(&format!("`{}`", item.name)), "{said}");
    // The login did not go stale, and the file is not "gone": it is in the item, said once.
    assert!(!said.contains("REFRESHED"), "{said}");
    assert!(!said.contains("gone or no longer holds a login"), "{said}");
    for secret in SECRETS {
        assert!(!said.contains(secret), "{said}");
    }
}

#[test]
fn the_folder_holds_the_login_alone_for_the_user_alone_and_is_the_same_each_time_it_is_asked_for() {
    let home = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let login = login(&machine_text(hours(5)), home.path(), parent.path());
    let folder = login.folder_at(at(NOW)).unwrap();
    assert!(folder.starts_with(parent.path()));
    let name = folder.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with(&format!("{PREFIX}{}-", std::process::id())),
        "{name}"
    );
    assert_eq!(mode(&folder), 0o700);
    let credentials = folder.join(".credentials.json");
    assert_eq!(mode(&credentials), 0o600);
    let text = std::fs::read_to_string(&credentials).unwrap();
    let held: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(held["claudeAiOauth"]["accessToken"], ACCESS);
    assert_eq!(held.as_object().unwrap().len(), 1, "the login alone");
    assert!(
        !text.contains(MCP),
        "the MCP servers' tokens are not copied"
    );
    let config = folder.join(".claude.json");
    assert_eq!(mode(&config), 0o600);
    let answered: Value = serde_json::from_str(&std::fs::read_to_string(config).unwrap()).unwrap();
    assert_eq!(answered["hasCompletedOnboarding"], true);
    // Asked for again, even at a time the login would be refused at, it is the folder it was.
    assert_eq!(login.folder_at(at(NOW + 100 * 3600)).unwrap(), folder);
    // What is said of the login names the folder and holds none of what it holds.
    let debug = format!("{login:?}");
    assert!(debug.contains(&name), "{debug}");
    assert!(!said_anywhere(&debug), "{debug}");
}

#[test]
fn a_login_refused_leaves_no_folder_and_a_login_that_cannot_be_read_says_where_it_looked() {
    let home = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let refused = login(
        &machine_text(Duration::from_secs(600)),
        home.path(),
        parent.path(),
    );
    assert!(refused.folder_at(at(NOW)).is_err());
    assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
    let missing = Login::new(
        Source::File(home.path().join("none.json")),
        home.path().to_path_buf(),
        parent.path().to_path_buf(),
        LEFT,
    );
    let said = missing.folder_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("none.json"), "{said}");
    assert!(said.contains("log in by running `claude`"), "{said}");
}

#[test]
fn the_folder_goes_when_the_login_is_dropped_and_when_the_run_panics() {
    let home = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let kept = login(&machine_text(hours(5)), home.path(), parent.path());
    let folder = kept.folder_at(at(NOW)).unwrap();
    assert!(folder.join(".credentials.json").is_file());
    drop(kept);
    assert!(!folder.exists(), "{}", folder.display());
    assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);

    let panicked = std::cell::RefCell::new(PathBuf::new());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let login = login(&machine_text(hours(5)), home.path(), parent.path());
        *panicked.borrow_mut() = login.folder_at(at(NOW)).unwrap();
        panic!("a check of the run failed");
    }));
    assert!(outcome.is_err());
    let folder = panicked.into_inner();
    assert!(
        !folder.as_os_str().is_empty() && !folder.exists(),
        "{}",
        folder.display()
    );
}

#[test]
fn a_folder_a_killed_run_left_is_taken_away_and_a_folder_of_a_run_that_is_there_is_not() {
    let parent = tempfile::tempdir().unwrap();
    // A process id nothing has: a child that ended and was waited for.
    let mut child = Run::new("/usr/bin/true").spawn().unwrap();
    let gone = child.id();
    child.wait(Duration::from_secs(10)).unwrap();
    let make = |name: String| {
        let folder = parent.path().join(name);
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join(".credentials.json"), "{}").unwrap();
        folder
    };
    let left = make(format!("{PREFIX}{gone}-abc123"));
    let running = make(format!("{PREFIX}{}-xyz789", std::process::id()));
    let others = make("consensflow-integration-AAA".to_owned());
    let no_process = make(format!("{PREFIX}not-a-number-1"));
    let file = parent.path().join(format!("{PREFIX}{gone}-file"));
    std::fs::write(&file, "").unwrap();
    sweep(parent.path(), PREFIX);
    assert!(
        !left.exists(),
        "the folder of a run that is gone is taken away"
    );
    for kept in [&running, &others, &no_process, &file] {
        assert!(kept.exists(), "{} is nobody's to take away", kept.display());
    }
}

/// A person's home with a setup in it, and what `Owner` sees of it.
struct Person {
    owner: Owner,
    home: tempfile::TempDir,
}

impl Person {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let owner = Owner {
            home: home.path().to_path_buf(),
        };
        let person = Self { home, owner };
        person.write(".claude/settings.json", r#"{"model":"opus"}"#);
        person.write(
            ".claude.json",
            &json!({
                "pluginUsage": { "a": 1 },
                "projects": {
                    "/Users/me/work": { "hasTrustDialogAccepted": true },
                    "/var/T/consensflow-integration-OLD/workspace": { "hasTrustDialogAccepted": true },
                },
            })
            .to_string(),
        );
        person.write(
            ".claude/history.jsonl",
            "{\"display\":\"fix it\",\"project\":\"/Users/me/work\",\"sessionId\":\"s1\"}\n",
        );
        std::fs::create_dir_all(person.home.path().join(".claude/projects/-Users-me-work"))
            .unwrap();
        person
    }

    fn write(&self, relative: &str, text: &str) {
        files::write(&self.home.path().join(relative), text).unwrap();
    }

    fn append(&self, relative: &str, text: &str) {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(self.home.path().join(relative))
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }
}

#[test]
fn what_the_persons_own_sessions_write_all_day_is_not_the_runs() {
    let person = Person::new();
    let before = person.owner.look();
    // Their Claude Code writes the rest of ~/.claude.json, a prompt in a project of theirs and
    // a conversation of theirs.
    person.write(
        ".claude.json",
        &json!({
            "pluginUsage": { "a": 2 },
            "projects": {
                "/Users/me/work": { "hasTrustDialogAccepted": true, "lastCost": 1 },
                "/Users/me/other": {},
                "/var/T/consensflow-integration-OLD/workspace": { "hasTrustDialogAccepted": true },
            },
        })
        .to_string(),
    );
    person.append(
        ".claude/history.jsonl",
        "{\"display\":\"more\",\"project\":\"/Users/me/other\",\"sessionId\":\"s2\"}\n",
    );
    std::fs::create_dir_all(person.home.path().join(".claude/projects/-Users-me-other")).unwrap();
    assert_eq!(person.owner.changes_since(&before), Vec::<String>::new());
}

#[test]
fn a_run_that_rewrote_the_settings_file_is_found_out_whatever_it_changed() {
    let person = Person::new();
    let before = person.owner.look();
    person.write(".claude/settings.json", r#"{"model":"claude-sonnet-5-5"}"#);
    let said = person.owner.changes_since(&before);
    assert_eq!(said, ["~/.claude/settings.json is not as it was"]);
    // The same bytes are not a change; a file that is gone is.
    person.write(".claude/settings.json", r#"{"model":"opus"}"#);
    assert!(person.owner.changes_since(&before).is_empty());
    std::fs::remove_file(person.home.path().join(".claude/settings.json")).unwrap();
    assert_eq!(person.owner.changes_since(&before).len(), 1);
}

#[test]
fn a_project_of_the_runs_folders_in_the_persons_claude_json_is_found_out_new_or_changed() {
    let person = Person::new();
    let before = person.owner.look();
    person.write(
        ".claude.json",
        &json!({
            "pluginUsage": { "a": 1 },
            "projects": {
                "/Users/me/work": { "hasTrustDialogAccepted": true },
                "/var/T/consensflow-integration-OLD/workspace": { "hasTrustDialogAccepted": true, "lastCost": 3 },
                "/Users/me/.consensflow-candidate/live/asks": { "hasTrustDialogAccepted": true },
            },
        })
        .to_string(),
    );
    let said = person.owner.changes_since(&before);
    assert_eq!(
        said,
        [
            "~/.claude.json gained the project /Users/me/.consensflow-candidate/live/asks",
            "~/.claude.json changed the project /var/T/consensflow-integration-OLD/workspace",
        ]
    );
}

#[test]
fn a_prompt_typed_in_a_folder_of_the_run_and_a_conversation_kept_of_it_are_found_out() {
    let person = Person::new();
    let before = person.owner.look();
    person.append(
        ".claude/history.jsonl",
        "{\"display\":\"a\",\"project\":\"/Users/me/.consensflow-candidate/live/paste\",\"sessionId\":\"s\"}\n\
         {\"display\":\"b\",\"project\":\"/Users/me/work\",\"sessionId\":\"s\"}\n\
         not JSON\n\
         {\"display\":\"c\",\"project\":\"/var/T/consensflow-integration-X/workspace\",\"sessionId\":\"s\"}\n",
    );
    std::fs::create_dir_all(
        person
            .home
            .path()
            .join(".claude/projects/-Users-me--consensflow-candidate-live-paste"),
    )
    .unwrap();
    let said = person.owner.changes_since(&before);
    assert_eq!(
        said,
        [
            "~/.claude/projects gained -Users-me--consensflow-candidate-live-paste",
            "~/.claude/history.jsonl gained 2 prompt(s) typed in a folder of the run",
        ]
    );
}

#[test]
fn the_evals_workspace_is_a_runs_folder_too_and_what_a_run_of_theirs_left_in_the_persons_config_is_found_out(
) {
    let person = Person::new();
    let before = person.owner.look();
    person.append(
        ".claude/history.jsonl",
        "{\"display\":\"x\",\"project\":\"/Users/me/.consensflow-candidate/evals/workspace\",\"sessionId\":\"e\"}\n",
    );
    std::fs::create_dir_all(
        person
            .home
            .path()
            .join(".claude/projects/-Users-me--consensflow-candidate-evals-workspace"),
    )
    .unwrap();
    person.write(
        ".claude.json",
        &json!({
            "projects": { "/Users/me/.consensflow-candidate/evals/workspace": { "hasTrustDialogAccepted": true } },
        })
        .to_string(),
    );
    assert_eq!(
        person.owner.changes_since(&before),
        [
            "~/.claude.json gained the project /Users/me/.consensflow-candidate/evals/workspace",
            "~/.claude/projects gained -Users-me--consensflow-candidate-evals-workspace",
            "~/.claude/history.jsonl gained 1 prompt(s) typed in a folder of the run",
        ]
    );
    assert!(is_run_folder(
        "/Users/me/.consensflow-candidate/evals/workspace"
    ));
    assert!(is_run_folder("/Users/me/.consensflow-candidate/live/paste"));
    assert!(is_run_folder("/var/T/consensflow-integration-AB/workspace"));
    assert!(is_run_name(
        "-Users-me--consensflow-candidate-evals-workspace"
    ));
    assert!(is_run_name("-Users-me--consensflow-candidate-live-paste"));
    // The person's own folders are none of a run's.
    assert!(!is_run_folder("/Users/me/work"));
    assert!(!is_run_name("-Users-me-work"));
}

#[test]
fn a_person_with_no_claude_setup_has_nothing_a_run_could_change_and_nothing_to_look_at() {
    let home = tempfile::tempdir().unwrap();
    let owner = Owner {
        home: home.path().to_path_buf(),
    };
    let before = owner.look();
    assert_eq!(before.settings, None);
    assert_eq!(before.history, 0);
    assert!(owner.changes_since(&before).is_empty());
}

#[test]
fn a_run_that_left_the_persons_setup_and_its_login_be_ends_with_nothing_to_say() {
    let person = Person::new();
    let parent = tempfile::tempdir().unwrap();
    let login = login(&machine_text(hours(5)), person.home.path(), parent.path());
    // A rig that never asked for the folder has nothing to be asked about.
    assert!(login.finish().is_ok());
    login.folder_at(at(NOW)).unwrap();
    assert!(login.finish().is_ok());
    assert!(login.finish().is_ok(), "it can be asked again");
}

#[test]
fn a_run_that_wrote_in_the_persons_setup_fails_with_where_and_says_it_loudly() {
    let person = Person::new();
    let parent = tempfile::tempdir().unwrap();
    let login = login(&machine_text(hours(5)), person.home.path(), parent.path());
    login.folder_at(at(NOW)).unwrap();
    person.write(".claude/settings.json", r#"{"model":"changed"}"#);
    let said = login.finish().unwrap_err().to_string();
    assert!(
        said.contains("THE RUN CHANGED THE OWNER'S CLAUDE SETUP"),
        "{said}"
    );
    assert!(
        said.contains("~/.claude/settings.json is not as it was"),
        "{said}"
    );
    assert!(!said.contains("REFRESHED"), "{said}");
    assert!(!said_anywhere(&said), "{said}");
}

#[test]
fn a_copy_that_was_refreshed_or_lost_fails_with_a_warning_that_the_machines_login_may_be_stale() {
    let person = Person::new();
    let parent = tempfile::tempdir().unwrap();
    let login = login(&machine_text(hours(5)), person.home.path(), parent.path());
    let folder = login.folder_at(at(NOW)).unwrap();
    let credentials = folder.join(".credentials.json");
    let copied = std::fs::read_to_string(&credentials).unwrap();
    // Claude Code rewrote the file with the same login: not a refresh.
    std::fs::write(&credentials, format!(" {copied}\n")).unwrap();
    assert!(login.finish().is_ok());
    // A refresh: another access token and another refresh token.
    let refreshed = json!({ "claudeAiOauth": {
        "accessToken": "sk-ant-oat01-NEW-ACCESS", "refreshToken": "sk-ant-ort01-NEW-REFRESH",
        "expiresAt": (NOW + 99_999) * 1000,
    } });
    std::fs::write(&credentials, refreshed.to_string()).unwrap();
    let said = login.finish().unwrap_err().to_string();
    assert!(
        said.contains("THE RUN'S CLAUDE CODE REFRESHED ITS COPY OF THE LOGIN"),
        "{said}"
    );
    // The copy holds no refresh token, so it cannot have used up the machine's.
    assert!(said.contains("The copy holds no refresh token"), "{said}");
    assert!(!said.contains("may now be stale"), "{said}");
    assert!(!said.contains("CHANGED THE OWNER'S"), "{said}");
    for secret in SECRETS.iter().copied().chain(["NEW-ACCESS", "NEW-REFRESH"]) {
        assert!(!said.contains(secret), "{said}");
    }
    // A file that is gone.
    std::fs::remove_file(&credentials).unwrap();
    let said = login.finish().unwrap_err().to_string();
    assert!(said.contains("gone or no longer holds a login"), "{said}");
}

#[test]
fn the_login_the_machine_keeps_changed_while_the_run_went_on_is_said_and_the_mcp_tokens_beside_it_are_not_held_to_it(
) {
    let person = Person::new();
    let parent = tempfile::tempdir().unwrap();
    let text = machine_text(hours(5));
    let login = login(&text, person.home.path(), parent.path());
    login.folder_at(at(NOW)).unwrap();
    let machine = person.home.path().join("machine-login.json");
    // The person's MCP servers refresh their own tokens in the same item, all day.
    std::fs::write(&machine, text.replace(MCP, "mcp-token-REFRESHED")).unwrap();
    assert!(login.finish().is_ok());
    // Their own Claude Code writes what it learns of the account beside the tokens
    // (`subscriptionTypeSource`, Claude Code 2.1.310): the login is held to its tokens, as
    // the copy is, so that is no change.
    std::fs::write(
        &machine,
        text.replace(
            "\"subscriptionType\":\"max\"",
            "\"subscriptionType\":\"max\",\"subscriptionTypeSource\":\"oauth\"",
        ),
    )
    .unwrap();
    assert!(login.finish().is_ok());
    // Their own Claude Code refreshed the login (or the run's rotated it): not as it was.
    std::fs::write(&machine, text.replace(ACCESS, "sk-ant-oat01-ROTATED")).unwrap();
    let said = login.finish().unwrap_err().to_string();
    assert!(
        said.contains(&format!(
            "the owner's Claude login in {} is not as it was",
            machine.display()
        )),
        "{said}"
    );
    assert!(
        said.contains("or the owner's own Claude Code did"),
        "{said}"
    );
    assert!(!said.contains("ROTATED") && !said_anywhere(&said), "{said}");
    // The login gone from where the machine keeps it is a change too.
    std::fs::remove_file(&machine).unwrap();
    assert!(login
        .finish()
        .unwrap_err()
        .to_string()
        .contains("is not as it was"));
}

#[test]
fn what_is_said_of_a_run_that_did_both_says_both() {
    let said = said(
        &["~/.claude/settings.json is not as it was".to_owned()],
        &["the copy changed".to_owned()],
        &["the item `Claude Code-credentials-b0d4aa31` is the run's".to_owned()],
    );
    assert!(
        said.contains("THE RUN CHANGED THE OWNER'S CLAUDE SETUP"),
        "{said}"
    );
    assert!(
        said.contains("  - ~/.claude/settings.json is not as it was"),
        "{said}"
    );
    assert!(said.contains("REFRESHED ITS COPY"), "{said}");
    assert!(said.contains("  - the copy changed"), "{said}");
    assert!(
        said.contains("LEFT ITS SECURE STORAGE IN THE OWNER'S KEYCHAIN"),
        "{said}"
    );
    assert!(
        said.contains("  - the item `Claude Code-credentials-b0d4aa31` is the run's"),
        "{said}"
    );
    assert!(said.contains(&"!".repeat(78)), "{said}");
}

#[test]
fn where_the_login_is_read_from_is_said_without_what_it_holds() {
    assert_eq!(
        Source::Keychain.described(),
        "the Keychain item Claude Code-credentials"
    );
    assert_eq!(
        Source::File(PathBuf::from("/home/me/.claude/.credentials.json")).described(),
        "/home/me/.claude/.credentials.json"
    );
}

/// Every file below `folder`, as paths relative to it, sorted.
fn files_below(folder: &Path) -> Vec<String> {
    fn walk(folder: &Path, at: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(folder, &path, found);
            } else {
                found.push(
                    path.strip_prefix(folder)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    let mut found = Vec::new();
    if folder.exists() {
        walk(folder, folder, &mut found);
    }
    found.sort();
    found
}

/// A made folder with what a run's Claude Code leaves in it besides the login.
fn after_a_run(login: &Login) -> PathBuf {
    let folder = login.folder_at(at(NOW)).unwrap();
    for (name, text) in [
        ("projects/-Users-me-work/s1.jsonl", "{\"type\":\"user\"}\n"),
        ("projects/-Users-me-work/memory/MEMORY.md", "- a note\n"),
        ("sessions/4242.json", "{\"pid\":4242}"),
        ("sessions/4242.0123abcd.key", "a secret of a socket"),
        ("shell-snapshots/snapshot.sh", "export X=1\n"),
        ("settings.json", "{}"),
    ] {
        let file = folder.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    folder
}

#[test]
fn a_kept_home_is_given_the_conversations_and_the_sessions_and_never_the_login() {
    let home = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let login = login(&machine_text(hours(5)), home.path(), parent.path());
    let folder = after_a_run(&login);
    // The login and the rest of the folder are there to be left out.
    assert!(folder.join(".credentials.json").is_file());
    assert!(folder.join(".claude.json").is_file());

    let kept = tempfile::tempdir().unwrap();
    let into = kept.path().join("home").join(".claude");
    login.keep_records(&into).unwrap();
    assert_eq!(
        files_below(&into),
        [
            "projects/-Users-me-work/memory/MEMORY.md",
            "projects/-Users-me-work/s1.jsonl",
            "sessions/4242.json",
        ],
        "the conversations and the sessions, but for a socket's secret"
    );
    for file in files_below(kept.path()) {
        let text = std::fs::read_to_string(kept.path().join(&file)).unwrap();
        assert!(!said_anywhere(&text), "{file}");
    }
    assert_eq!(
        std::fs::read_to_string(into.join("projects/-Users-me-work/s1.jsonl")).unwrap(),
        "{\"type\":\"user\"}\n"
    );

    // Asked again after the run went on, it is the same copy, grown.
    std::fs::write(
        folder.join("projects/-Users-me-work/s1.jsonl"),
        "{\"type\":\"user\"}\n{\"type\":\"assistant\"}\n",
    )
    .unwrap();
    std::fs::write(folder.join("projects/-Users-me-work/s2.jsonl"), "{}\n").unwrap();
    login.keep_records(&into).unwrap();
    assert_eq!(files_below(&into).len(), 4);
    assert_eq!(
        std::fs::read_to_string(into.join("projects/-Users-me-work/s1.jsonl"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn a_run_with_no_folder_or_a_folder_with_no_records_has_nothing_to_keep() {
    let home = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let kept = tempfile::tempdir().unwrap();
    let into = kept.path().join("home").join(".claude");
    // The folder was never asked for.
    let login = login(&machine_text(hours(5)), home.path(), parent.path());
    login.keep_records(&into).unwrap();
    assert!(files_below(kept.path()).is_empty());
    // One that holds the login alone.
    login.folder_at(at(NOW)).unwrap();
    login.keep_records(&into).unwrap();
    assert!(files_below(kept.path()).is_empty());
}

#[cfg(unix)]
#[test]
fn a_link_in_the_folder_is_not_followed_out_of_it() {
    let home = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let login = login(&machine_text(hours(5)), home.path(), parent.path());
    let folder = after_a_run(&login);
    // A link to the login, in a place that is kept.
    std::os::unix::fs::symlink(
        folder.join(".credentials.json"),
        folder.join("projects/-Users-me-work/link.jsonl"),
    )
    .unwrap();
    let kept = tempfile::tempdir().unwrap();
    login.keep_records(kept.path()).unwrap();
    assert!(!files_below(kept.path())
        .iter()
        .any(|file| file.ends_with("link.jsonl")));
}

// ---------------------------------------------------------------------- the renewal (T-1067)

use super::super::renew::tests::stand_in;
use super::super::renew::TestMachine;

/// The access token a login that Claude Code renewed holds.
const RENEWED: &str = "sk-ant-oat01-RENEWED-5555555555";

/// The text the machine keeps for a login whose access token (`access`) ends at `expires`.
fn login_ending(expires: i64, access: &str) -> String {
    json!({
        "mcpOAuth": { "plugin:x:x|1": { "accessToken": MCP, "refreshToken": MCP } },
        "claudeAiOauth": {
            "accessToken": access, "refreshToken": REFRESH, "expiresAt": expires,
            "scopes": ["user:inference"], "subscriptionType": "max",
        },
    })
    .to_string()
}

/// A test machine, as the runner says it.
fn zeewin2() -> TestMachine {
    TestMachine::signalled(Some("zeewin2")).unwrap()
}

/// A person whose Claude Code login ended an hour before `NOW`, and a `claude` that stands in for
/// theirs: it writes down how it was started, does what `body` says, and ends with `status`.
struct Renewing {
    person: Person,
    parent: tempfile::TempDir,
    tools: tempfile::TempDir,
}

impl Renewing {
    fn new() -> Self {
        Self {
            person: Person::new(),
            parent: tempfile::tempdir().unwrap(),
            tools: tempfile::tempdir().unwrap(),
        }
    }

    fn log(&self) -> PathBuf {
        self.tools.path().join("log")
    }

    /// What the stand-in did, line by line: its words, then the folder it was started in. None
    /// where it never ran.
    fn started(&self) -> Option<Vec<String>> {
        let text = std::fs::read_to_string(self.log()).ok()?;
        Some(text.lines().map(str::to_owned).collect())
    }

    /// The machine's login file.
    fn file(&self) -> PathBuf {
        self.person.home.path().join("machine-login.json")
    }

    /// What the stand-in does when it renews the login as Claude Code does: writes a login that
    /// ends eight hours after `NOW`.
    fn renews(&self) -> String {
        let text = login_ending(at(NOW + 8 * 3600), RENEWED);
        format!("cat > '{}' <<'JSON'\n{text}\nJSON", self.file().display())
    }

    /// A login on `text`, with a renewal on `machine` (none on a machine that is not a test
    /// machine) that starts the stand-in.
    fn login(&self, text: &str, machine: Option<TestMachine>, body: &str, status: i32) -> Login {
        let mut login = login(text, self.person.home.path(), self.parent.path());
        let program = stand_in(self.tools.path(), &self.log(), body, status);
        login.renewal = machine.map(|machine| {
            claude_renewal(machine, self.person.home.path().to_path_buf(), move || {
                Ok(program.clone())
            })
        });
        login
    }

    /// Whether the run made a folder.
    fn made(&self) -> usize {
        std::fs::read_dir(self.parent.path()).unwrap().count()
    }
}

#[test]
fn a_login_that_ended_on_a_test_machine_is_renewed_by_claude_code_once_and_the_run_goes_on() {
    let run = Renewing::new();
    let ended = login_ending(at(NOW - 3600), ACCESS);
    let login = run.login(&ended, Some(zeewin2()), &run.renews(), 0);
    let folder = login.folder_at(at(NOW)).unwrap();
    // The run starts on the login Claude Code renewed, not on the one that had ended.
    let held: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join(".credentials.json")).unwrap())
            .unwrap();
    assert_eq!(held["claudeAiOauth"]["accessToken"], RENEWED);
    assert_eq!(held["claudeAiOauth"]["expiresAt"], at(NOW + 8 * 3600));
    assert_eq!(held.as_object().unwrap().len(), 1, "the login alone");
    // Claude Code was run once, in its print mode, on the model the run uses, with nothing to
    // reach out for, in a folder named as a folder of a run.
    let started = run.started().unwrap();
    assert_eq!(
        started[0],
        "-p Reply with the single word ok. --model claude-sonnet-5-5 --no-session-persistence \
         --strict-mcp-config --no-chrome --disable-slash-commands --tools "
    );
    assert!(
        started[1].contains("consensflow-integration-renew-"),
        "{started:?}"
    );
    // What the renewal came to is kept for the run's report: the file, and when it ended before and after.
    let made = login.made.lock().unwrap();
    let renewed = made.as_ref().unwrap().renewed.as_ref().unwrap();
    assert_eq!(
        (renewed.before, renewed.after),
        (Some(at(NOW - 3600)), Some(at(NOW + 8 * 3600)))
    );
    assert!(
        renewed.file.ends_with("machine-login.json"),
        "{}",
        renewed.file
    );
    drop(made);
    // The login the machine keeps is the one that was copied: nothing is stale, and the run ends well.
    login.finish().unwrap();
}

#[test]
fn a_renewal_that_fails_refuses_the_run_in_words_that_say_so_and_makes_no_folder() {
    let run = Renewing::new();
    let ended = login_ending(at(NOW - 3600), ACCESS);
    let login = run.login(&ended, Some(zeewin2()), "echo 'Not logged in' >&2", 1);
    let said = login.folder_at(at(NOW)).unwrap_err().to_string();
    // Today's refusal, and then that the renewal ran and how it failed.
    assert!(said.contains("Claude Code's login has expired"), "{said}");
    assert!(
        said.contains(
            "On zeewin2 the kit ran Claude Code once as the owner to renew it (T-1067), and the \
             renewal ran and failed: it ended 1 (Not logged in)"
        ),
        "{said}"
    );
    assert!(!said_anywhere(&said), "{said}");
    assert_eq!(run.made(), 0);
}

#[test]
fn a_renewal_that_moves_nothing_refuses_the_run_and_says_when_claude_code_renews_a_token() {
    let run = Renewing::new();
    // Four minutes left of the two hours the run needs: Claude Code is run, as it renews a token
    // with five minutes left, and the stand-in moves nothing.
    let near = login_ending(at(NOW + 4 * 60), ACCESS);
    let login = run.login(&near, Some(zeewin2()), "", 0);
    let said = login.folder_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("expires in 4 min"), "{said}");
    assert!(said.contains("the renewal ran and failed"), "{said}");
    assert!(
        said.contains("the login still ends 2027-01-15T08:04:00Z (in 4 min): Claude Code renews a token only when five minutes of it are left"),
        "{said}"
    );
    assert!(!said_anywhere(&said), "{said}");
    assert_eq!(run.started().unwrap().len(), 2, "the stand-in ran once");
    assert_eq!(run.made(), 0);
}

/// Claude Code renews a token when five minutes of it are left, so running it on a login that has
/// more is a call of the model for nothing, and its record in the owner's projects besides
/// (zeewin2, 2026-11-03: 29 minutes left, the kit ran it, it moved nothing and left a project folder
/// that the kit then took for a change of the owner's setup, T-1136).
#[test]
fn a_login_with_more_than_five_minutes_left_is_not_renewed_by_running_claude_code() {
    let run = Renewing::new();
    // 29 minutes left of the two hours the run needs.
    let near = login_ending(at(NOW + 29 * 60), ACCESS);
    let login = run.login(&near, Some(zeewin2()), &run.renews(), 0);
    let said = login.folder_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("expires in 29 min"), "{said}");
    assert!(
        said.contains("only when five minutes of it are left"),
        "the refusal says when Claude Code renews a token: {said}"
    );
    assert!(!said.contains("the renewal ran and failed"), "{said}");
    assert_eq!(run.started(), None, "Claude Code was not run");
    assert_eq!(run.made(), 0);
    // Four minutes left: Claude Code is run, and renews it.
    let ending = login_ending(at(NOW + 4 * 60), ACCESS);
    let login = run.login(&ending, Some(zeewin2()), &run.renews(), 0);
    assert!(login.folder_at(at(NOW)).is_ok());
    assert_eq!(run.started().unwrap().len(), 2, "the stand-in ran once");
}

/// What Claude Code keeps of the folder it was started in, in the owner's projects, is the
/// renewal's own: it is taken away when the renewal is over, and nothing else of the owner's is
/// (zeewin2: `projects/C--Users-AGV-AppData-Local-Temp-consensflow-integration-renew-…/memory`).
/// The stand-in names it as Claude Code does, by the folder's real path, every character that
/// is no letter or digit a dash.
#[test]
fn a_renewal_takes_away_the_record_claude_code_kept_of_its_own_folder_and_no_other() {
    let run = Renewing::new();
    let projects = run.person.home.path().join(".claude/projects");
    let body = format!(
        "{}\nown=\"{}/$(pwd -P | sed 's/[^A-Za-z0-9]/-/g')\"\nmkdir -p \"$own\" && echo x > \"$own/memory\"",
        run.renews(),
        projects.display()
    );
    let ended = login_ending(at(NOW - 3600), ACCESS);
    let login = run.login(&ended, Some(zeewin2()), &body, 0);
    let folder = login.folder_at(at(NOW)).unwrap();
    assert!(folder.join(".credentials.json").is_file());
    let left: Vec<String> = std::fs::read_dir(&projects)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        left.iter().all(|name| !name.contains("renew")),
        "the renewal's own record is gone: {left:?}"
    );
    assert!(
        left.iter().any(|name| name == "-Users-me-work"),
        "and nothing else of the owner's: {left:?}"
    );
    // A record of another run's folder is not the renewal's, and is the owner's setup changed.
    let body = format!(
        "{}\nmkdir -p '{}/C--tmp-consensflow-integration-renew-OTHER'",
        run.renews(),
        projects.display()
    );
    let login = run.login(&ended, Some(zeewin2()), &body, 0);
    let said = login.folder_at(at(NOW)).unwrap_err().to_string();
    assert!(
        said.contains("it changed the owner's setup beside its login"),
        "{said}"
    );
    assert!(said.contains("OTHER"), "{said}");
}

#[test]
fn a_renewal_that_changes_the_owners_setup_beside_the_login_fails_and_names_the_file() {
    let run = Renewing::new();
    let body = format!(
        "{}\necho '{{\"model\":\"haiku\"}}' > '{}'",
        run.renews(),
        run.person
            .home
            .path()
            .join(".claude/settings.json")
            .display()
    );
    let ended = login_ending(at(NOW - 3600), ACCESS);
    let login = run.login(&ended, Some(zeewin2()), &body, 0);
    let said = login.folder_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("the renewal ran and failed"), "{said}");
    assert!(
        said.contains("it changed the owner's setup beside its login: ~/.claude/settings.json is not as it was"),
        "{said}"
    );
    assert_eq!(run.made(), 0);
}

#[test]
fn a_login_that_lasts_the_run_or_cannot_be_renewed_is_never_renewed() {
    let run = Renewing::new();
    // Five hours left of the two the run needs.
    let lasts = login_ending(at(NOW + 5 * 3600), ACCESS);
    assert!(run
        .login(&lasts, Some(zeewin2()), &run.renews(), 0)
        .folder_at(at(NOW))
        .is_ok());
    assert_eq!(run.started(), None, "Claude Code was not run");
    // An ended login with no refresh token cannot be renewed by Claude Code: today's refusal.
    let no_refresh = json!({
        "claudeAiOauth": { "accessToken": ACCESS, "expiresAt": at(NOW - 3600) }
    })
    .to_string();
    let said = run
        .login(&no_refresh, Some(zeewin2()), &run.renews(), 0)
        .folder_at(at(NOW))
        .unwrap_err()
        .to_string();
    assert!(said.contains("no `refreshToken`"), "{said}");
    assert_eq!(run.started(), None, "Claude Code was not run");
}

#[test]
fn off_a_test_machine_an_ended_login_is_not_renewed_but_refused_as_it_always_was_and_nothing_is_run(
) {
    let run = Renewing::new();
    let ended = login_ending(at(NOW - 3600), ACCESS);
    // No machine, no renewal: the login the owner's Claude Code leaves ended is refused.
    let login = run.login(&ended, None, &run.renews(), 0);
    let said = login.folder_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("Claude Code's login has expired"), "{said}");
    assert!(
        !said.contains("renew it") && !said.contains("T-1067"),
        "{said}"
    );
    assert_eq!(run.started(), None);
    assert_eq!(run.made(), 0);
    // A login made for the machine renews only where the runner said it is on a test machine.
    assert!(Login::of_machine_on(None, LEFT).is_none_or(|login| login.renewal.is_none()));
    assert!(Login::of_machine_on(Some(zeewin2()), LEFT).is_none_or(|login| login.renewal.is_some()));
    assert!(Login::new(
        Source::Keychain,
        PathBuf::from("/home"),
        PathBuf::from("/tmp"),
        LEFT
    )
    .renewal
    .is_none());
}

// ---------------------------------------------------------------------- the case's own length (T-1066)

use super::super::isolation::lasting;
use super::super::PLANNED;

#[test]
fn a_claude_login_with_forty_two_minutes_left_runs_a_case_of_ten_minutes_and_not_one_of_an_hour() {
    let text = machine_text(Duration::from_secs(42 * 60));
    assert!(oauth(&text, at(NOW), lasting(PLANNED)).is_ok());
    let said = oauth(&text, at(NOW), lasting(hours(1)))
        .unwrap_err()
        .to_string();
    // The refusal says how long is left, and that Claude Code renews a token itself, once it has ended.
    assert!(said.contains("expires in 42 min"), "{said}");
    assert!(said.contains("at least 1 h 10 min more"), "{said}");
    assert!(
        said.contains(
            "Claude Code renews a token itself, but only when five minutes of it are left"
        ),
        "{said}"
    );
    assert!(said.contains("run again once it has ended"), "{said}");
    assert!(!said_anywhere(&said), "{said}");
}

// ---------------------------------------------------------------------- a copy that cannot renew (T-1067)

/// Every file below `folder`, with what it holds.
fn everything_in(folder: &Path) -> String {
    files_below(folder)
        .iter()
        .map(|name| std::fs::read_to_string(folder.join(name)).unwrap_or_default())
        .collect()
}

#[test]
fn a_run_is_given_the_access_token_and_nothing_that_renews_it_and_the_machines_login_is_held_to_all_of_it(
) {
    // Measured on zeewin2 (Claude Code 2.1.316): a copy with no refresh token runs, is never
    // refreshed, and fails with a 401 where the server refuses its access token.
    let person = Person::new();
    let parent = tempfile::tempdir().unwrap();
    let text = json!({
        "claudeAiOauth": {
            "accessToken": ACCESS, "refreshToken": REFRESH, "refreshTokenExpiresAt": at(NOW + 90 * 86_400),
            "expiresAt": at(NOW + 5 * 3600), "scopes": ["user:inference"], "subscriptionType": "max",
        },
    })
    .to_string();
    let login = login(&text, person.home.path(), parent.path());
    let folder = login.folder_at(at(NOW)).unwrap();
    let held: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join(".credentials.json")).unwrap())
            .unwrap();
    let oauth = held["claudeAiOauth"].as_object().unwrap();
    assert_eq!(oauth["accessToken"], ACCESS);
    assert_eq!(oauth["expiresAt"], at(NOW + 5 * 3600));
    assert_eq!(oauth["subscriptionType"], "max");
    for renewing in ["refreshToken", "refreshTokenExpiresAt"] {
        assert!(!oauth.contains_key(renewing), "{renewing} is in the copy");
    }
    assert!(!everything_in(&folder).contains(REFRESH));
    // The machine's login still has its refresh token, and the run ends well: held to the login it
    // read, not to the copy.
    login.finish().unwrap();
    // The machine's file changed (its refresh token used by another): the run says so.
    let machine = person.home.path().join("machine-login.json");
    std::fs::write(&machine, text.replace(REFRESH, "sk-ant-ort01-USED")).unwrap();
    let said = login.finish().unwrap_err().to_string();
    assert!(said.contains("CHANGED THE OWNER'S CLAUDE SETUP"), "{said}");
    assert!(!said.contains("USED"), "{said}");
}

#[test]
fn a_claude_code_that_ended_badly_after_it_renewed_its_login_has_renewed_it_and_the_run_goes_on() {
    let run = Renewing::new();
    // Its model refused the owner's own session after the login moved: what Claude Code on the
    // owner's config on zeewin2 did with Sonnet 5.5 (exit 1, 2026-11-01), on a login that needed no
    // renewal, so the renewal and the refusal were not seen together.
    let body = format!("{}\necho 'API Error: refused' >&2", run.renews());
    let ended = login_ending(at(NOW - 3600), ACCESS);
    let login = run.login(&ended, Some(zeewin2()), &body, 1);
    let folder = login.folder_at(at(NOW)).unwrap();
    let held: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join(".credentials.json")).unwrap())
            .unwrap();
    assert_eq!(held["claudeAiOauth"]["accessToken"], RENEWED);
    let made = login.made.lock().unwrap();
    let renewed = made.as_ref().unwrap().renewed.as_ref().unwrap();
    assert_eq!(
        renewed.ended.as_deref(),
        Some("it ended 1 (API Error: refused)")
    );
    assert!(renewed.moved());
}

#[test]
fn a_login_runs_a_case_with_its_plan_and_ten_minutes_left_and_is_refused_with_less() {
    // The margin is Pi's five minutes of a token it will not use, and five minutes more.
    let plan = Duration::from_secs(10 * 60);
    let need = lasting(plan);
    assert_eq!(need, Duration::from_secs(20 * 60));
    assert!(oauth(&machine_text(Duration::from_secs(25 * 60)), at(NOW), need).is_ok());
    let said = oauth(&machine_text(Duration::from_secs(15 * 60)), at(NOW), need)
        .unwrap_err()
        .to_string();
    assert!(said.contains("expires in 15 min"), "{said}");
    assert!(said.contains("at least 20 min more"), "{said}");
}
