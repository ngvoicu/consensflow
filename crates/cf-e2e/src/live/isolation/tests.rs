// The tests are for the machines the logins are copied on: the Unix ones.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use serde_json::json;

use super::*;
use crate::process::Run;

/// What a login holds that must never be said or kept in a home that is not its owner's.
const ACCESS: &str = "pi-access-0123456789";
const REFRESH: &str = "pi-refresh-9876543210";
const KEY: &str = "sk-or-KEY-abcdefghij";
const OTHER: &str = "other-provider-SECRET-zzzz";
const SENTINEL: &str = "SENTINEL-the-owners-own-0001";
const SECRETS: [&str; 4] = [ACCESS, REFRESH, KEY, OTHER];

const NOW: u64 = 1_800_000_000;

/// `seconds` after the epoch, in milliseconds.
fn at(seconds: u64) -> i64 {
    i64::try_from(seconds * 1000).unwrap()
}

fn hours(hours: u64) -> Duration {
    Duration::from_secs(hours * 3600)
}

/// The expiry, in milliseconds, of a login that lasts `lasting` after `NOW`.
fn ends(lasting: Duration) -> i64 {
    at(NOW + lasting.as_secs())
}

fn oauth(secret: &str, lasting: Duration) -> Value {
    json!({ "type": "oauth", "access": secret, "refresh": format!("r-{secret}"), "expires": ends(lasting) })
}

/// The owner's `auth.json`: a key for `openrouter` and OAuth logins for `openai` (the run's
/// models', lasting `openai`) and `anthropic` (another provider's).
fn auth(openai: Duration) -> String {
    json!({
        "openrouter": { "type": "api_key", "key": KEY },
        "openai": { "type": "oauth", "access": ACCESS, "refresh": REFRESH, "expires": ends(openai) },
        "anthropic": oauth(OTHER, hours(9)),
    })
    .to_string()
}

fn said_anywhere(text: &str) -> bool {
    SECRETS.iter().any(|secret| text.contains(secret))
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Every file below `folder`, relative to it (a link is a file, not followed), sorted.
fn files_below(folder: &Path) -> Vec<String> {
    fn walk(folder: &Path, base: &Path, into: &mut Vec<String>) {
        for entry in fs::read_dir(folder).unwrap().flatten() {
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                walk(&entry.path(), base, into);
            } else {
                into.push(
                    entry
                        .path()
                        .strip_prefix(base)
                        .unwrap()
                        .display()
                        .to_string(),
                );
            }
        }
    }
    let mut found = Vec::new();
    walk(folder, folder, &mut found);
    found.sort();
    found
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// The owner: a home with the setups of the three harnesses in it, each file holding the
/// sentinel, and the scratch folder their homes are made in.
struct Owner {
    home: tempfile::TempDir,
    parent: tempfile::TempDir,
}

impl Owner {
    fn new() -> Self {
        let owner = Self {
            home: tempfile::tempdir().unwrap(),
            parent: tempfile::tempdir().unwrap(),
        };
        let h = owner.home.path();
        for file in [
            ".pi/agent/settings.json",
            ".pi/agent/models.json",
            ".pi/agent/mcp-auth.json",
            ".pi/agent/extensions/ext.js",
            ".codex/config.toml",
            ".codex/history.jsonl",
            ".config/devin/config.json",
            ".config/opencode/opencode.json",
            ".local/share/devin/credentials.toml",
            ".local/share/devin/cli/trusted_workspaces.json",
            ".local/share/devin/cli/sessions.db",
            ".local/share/opencode/auth.json",
        ] {
            write(&h.join(file), SENTINEL);
        }
        write(&h.join(".pi/agent/auth.json"), &auth(hours(9)));
        write(&h.join(".codex/auth.json"), &auth(hours(9)));
        owner
    }

    fn path(&self, below: &str) -> PathBuf {
        self.home.path().join(below)
    }

    /// Pi's, on the model `model`, for a run of `run`.
    fn pi(&self, model: &str, run: Duration) -> Isolation {
        let mut made = Isolation::pi(self.home.path(), &self.path(".pi/agent"), model, run);
        made.parent = self.parent.path().to_path_buf();
        made
    }

    fn codex(&self) -> Isolation {
        let mut made = Isolation::codex(self.home.path(), &self.path(".codex"));
        made.parent = self.parent.path().to_path_buf();
        made
    }

    fn devin(&self) -> Isolation {
        let mut made = Isolation::devin(
            self.home.path(),
            &self.path(".config"),
            &self.path(".local/share"),
        );
        made.parent = self.parent.path().to_path_buf();
        made
    }

    /// What is in the temporary folder the homes are made in.
    fn made(&self) -> usize {
        fs::read_dir(self.parent.path()).unwrap().count()
    }
}

// ---------------------------------------------------------------------- Pi's login

#[test]
fn a_pi_model_is_on_the_provider_before_its_first_slash() {
    assert_eq!(
        pi::provider_of("openrouter/meta/muse-spark-1.3").as_deref(),
        Some("openrouter")
    );
    assert_eq!(
        pi::provider_of("openai-codex/gpt-5.6-luna:high").as_deref(),
        Some("openai-codex")
    );
    // Pi finds the provider of a name with none, which nothing here can.
    assert_eq!(pi::provider_of("gpt-6-luna"), None);
    assert_eq!(pi::provider_of("/gpt-6-luna"), None);
}

#[test]
fn only_the_entries_of_the_provider_the_run_names_are_copied() {
    let providers = ["openrouter".to_owned()];
    let copied = pi::login(&auth(hours(9)), Some(&providers), at(NOW), LEFT_2H, "auth").unwrap();
    assert_eq!(
        copied,
        json!({ "openrouter": { "type": "api_key", "key": KEY } })
    );
    // The other providers' logins (an OAuth one among them) are nowhere in it.
    for secret in [ACCESS, REFRESH, OTHER] {
        assert!(!copied.to_string().contains(secret), "{copied}");
    }
    let providers = ["openai".to_owned(), "openrouter".to_owned()];
    let both = pi::login(&auth(hours(9)), Some(&providers), at(NOW), LEFT_2H, "auth").unwrap();
    assert_eq!(both.as_object().unwrap().len(), 2);
}

const LEFT_2H: Duration = Duration::from_secs(2 * 3600);

#[test]
fn an_oauth_login_that_ends_within_the_run_and_its_margin_is_refused_and_one_that_lasts_longer_is_not(
) {
    let providers = ["openai".to_owned()];
    let near = auth(LEFT_2H - Duration::from_secs(60));
    let said = pi::login(
        &near,
        Some(&providers),
        at(NOW),
        LEFT_2H,
        "~/.pi/agent/auth.json",
    )
    .unwrap_err()
    .to_string();
    assert!(said.contains("`openai` expires in 1 h 59 min"), "{said}");
    assert!(said.contains("at least 2 h more"), "{said}");
    assert!(said.contains("~/.pi/agent/auth.json"), "{said}");
    // What to do about it: refresh it by using Pi, take a provider whose login lasts, or be shorter.
    assert!(said.contains("use Pi once on that provider"), "{said}");
    assert!(said.contains("--models pi="), "{said}");
    assert!(said.contains("make the run shorter"), "{said}");
    assert!(!said_anywhere(&said), "{said}");
    let longer = auth(LEFT_2H + Duration::from_secs(60));
    assert!(pi::login(&longer, Some(&providers), at(NOW), LEFT_2H, "auth").is_ok());
}

#[test]
fn an_api_key_kept_as_an_oauth_entry_with_no_refresh_token_and_no_end_is_copied() {
    // OpenRouter's login, as Pi writes it: the key is the access token, the refresh token is empty and
    // the expiry is the largest number JavaScript has. Nothing refreshes it, whatever the run's length.
    let text = json!({
        "openrouter": { "type": "oauth", "access": KEY, "refresh": "", "expires": 9_007_199_254_740_991_i64 },
    })
    .to_string();
    let providers = ["openrouter".to_owned()];
    let copied = pi::login(&text, Some(&providers), at(NOW), hours(48), "auth").unwrap();
    assert_eq!(copied["openrouter"]["access"], KEY);
    // One that ends in the run is refused all the same, refresh token or none.
    let soon = json!({
        "openrouter": { "type": "oauth", "access": KEY, "refresh": "", "expires": ends(hours(1)) },
    })
    .to_string();
    let said = pi::login(&soon, Some(&providers), at(NOW), LEFT_2H, "auth")
        .unwrap_err()
        .to_string();
    assert!(said.contains("`openrouter` expires in 1 h"), "{said}");
    assert!(!said_anywhere(&said), "{said}");
}

#[test]
fn an_oauth_login_that_has_expired_is_never_copied_so_nothing_refreshes_it() {
    let providers = ["openai".to_owned()];
    let ended = auth(Duration::ZERO);
    let said = pi::login(&ended, Some(&providers), at(NOW + 10), LEFT_2H, "auth")
        .unwrap_err()
        .to_string();
    assert!(said.contains("`openai` has expired"), "{said}");
    assert!(!said_anywhere(&said), "{said}");
    // One provider of two ended is a refusal, and the other's key is not given in its place.
    let providers = ["openai".to_owned(), "openrouter".to_owned()];
    let said = pi::login(&ended, Some(&providers), at(NOW + 10), LEFT_2H, "auth")
        .unwrap_err()
        .to_string();
    assert!(said.contains("`openai` has expired"), "{said}");
    assert!(!said.contains("`openrouter`"), "{said}");
}

#[test]
fn a_provider_with_no_entry_and_an_entry_pi_cannot_refresh_are_refused_in_words() {
    let providers = ["groq".to_owned()];
    let said = pi::login(&auth(hours(9)), Some(&providers), at(NOW), LEFT_2H, "auth")
        .unwrap_err()
        .to_string();
    assert!(said.contains("`groq` has no entry"), "{said}");
    let odd = json!({
        "a": { "type": "oauth", "access": ACCESS, "refresh": REFRESH },
        "b": { "type": "cookie", "key": KEY },
        "c": { "type": "oauth", "refresh": REFRESH, "expires": ends(hours(9)) },
    })
    .to_string();
    for (provider, why) in [
        ("a", "no access token or expiry"),
        ("b", "an entry of a kind Pi does not know"),
        ("c", "no access token or expiry"),
    ] {
        let providers = [provider.to_owned()];
        let said = pi::login(&odd, Some(&providers), at(NOW), LEFT_2H, "auth")
            .unwrap_err()
            .to_string();
        assert!(said.contains(why), "{provider}: {said}");
        assert!(!said_anywhere(&said), "{said}");
    }
}

#[test]
fn a_model_with_no_provider_gets_every_login_that_is_fit_and_is_refused_where_none_is() {
    let near = auth(hours(1));
    let copied = pi::login(&near, None, at(NOW), LEFT_2H, "auth").unwrap();
    // The key and the long OAuth login; the short one is left out, not copied to refresh.
    let mut names: Vec<&String> = copied.as_object().unwrap().keys().collect();
    names.sort();
    assert_eq!(names, ["anthropic", "openrouter"]);
    for empty in [
        "{}",
        &json!({ "openai": oauth(ACCESS, hours(1)) }).to_string(),
    ] {
        let said = pi::login(empty, None, at(NOW), LEFT_2H, "auth")
            .unwrap_err()
            .to_string();
        assert!(said.contains("cannot be used for this run"), "{said}");
    }
}

#[test]
fn text_that_is_no_login_is_refused_and_what_it_held_is_not_said() {
    for text in [format!("not JSON {ACCESS}"), format!("[\"{ACCESS}\"]")] {
        let said = pi::login(&text, None, at(NOW), LEFT_2H, "auth")
            .unwrap_err()
            .to_string();
        assert!(said.contains("cannot be used"), "{said}");
        assert!(!said_anywhere(&said), "{said}");
    }
}

// ----------------------------------------------------------------------- the homes

#[test]
fn a_pi_run_is_given_a_folder_of_its_own_with_the_login_it_needs_and_nothing_else_of_the_owners() {
    let owner = Owner::new();
    let isolation = owner.pi("openrouter/meta/muse-spark-1.3", Duration::from_secs(3600));
    let home = isolation.made_at(at(NOW)).unwrap();
    // One variable, the one Pi reads its folder by, and it names the run's.
    let vars = isolation.vars_in(&home);
    assert_eq!(
        vars,
        [(
            "PI_CODING_AGENT_DIR".to_owned(),
            home.join("pi/agent").display().to_string()
        )]
    );
    assert!(!vars[0].1.contains(&owner.home.path().display().to_string()));
    // Only the login: not the settings, the models, the MCP logins, the extensions or the sessions.
    assert_eq!(files_below(&home), ["pi/agent/auth.json"]);
    let copied = fs::read_to_string(home.join("pi/agent/auth.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&copied).unwrap(),
        json!({ "openrouter": { "type": "api_key", "key": KEY } })
    );
    for secret in [ACCESS, REFRESH, OTHER, SENTINEL] {
        assert!(!copied.contains(secret), "{copied}");
    }
    // Readable by the owner alone.
    assert_eq!(mode(&home), 0o700);
    assert_eq!(mode(&home.join("pi/agent/auth.json")), 0o600);
    // The same home is the one asked again, and it goes with the isolation.
    assert_eq!(isolation.made_at(at(NOW)).unwrap(), home);
    drop(isolation);
    assert!(!home.exists());
}

#[test]
fn a_login_that_is_too_short_is_refused_before_anything_is_made_and_one_that_fits_the_run_is_not() {
    let owner = Owner::new();
    write(&owner.path(".pi/agent/auth.json"), &auth(hours(2)));
    // An hour's run needs an hour and a half of the `openai` login's two.
    let fits = owner.pi("openai/gpt-6-luna", hours(1));
    assert!(fits.made_at(at(NOW)).is_ok());
    // Two hours' run, and the margin, does not fit it.
    let longer = owner.pi("openai/gpt-6-luna", hours(2));
    let said = longer.made_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("`openai` expires in 2 h"), "{said}");
    assert!(said.contains("at least 2 h 30 min more"), "{said}");
    assert!(!said_anywhere(&said), "{said}");
    // Only the home that fit is on disk.
    assert_eq!(owner.made(), 1);
}

#[test]
fn a_codex_run_is_given_the_login_alone_and_what_it_configures_goes_in_its_own_home() {
    let owner = Owner::new();
    let isolation = owner.codex();
    let home = isolation.made_at(at(NOW)).unwrap();
    assert_eq!(
        isolation.vars_in(&home),
        [(
            "CODEX_HOME".to_owned(),
            home.join("codex").display().to_string()
        )]
    );
    assert_eq!(files_below(&home), ["codex/auth.json"]);
    assert_eq!(
        fs::read_to_string(home.join("codex/auth.json")).unwrap(),
        auth(hours(9))
    );
    assert_eq!(mode(&home.join("codex/auth.json")), 0o600);
    // A check that configures Codex writes in the run's home; the owner's config is as it was.
    isolation
        .write("CODEX_HOME", "config.toml", "[mcp_servers.x]\n")
        .unwrap();
    isolation
        .write("CODEX_HOME", "skills/s/SKILL.md", "skill")
        .unwrap();
    let said = isolation
        .write("XDG_CONFIG_HOME", "x", "y")
        .unwrap_err()
        .to_string();
    assert!(
        said.contains("windows run on no home for XDG_CONFIG_HOME"),
        "{said}"
    );
    assert!(home.join("codex/config.toml").is_file());
    assert_eq!(
        fs::read_to_string(owner.path(".codex/config.toml")).unwrap(),
        SENTINEL
    );
    assert!(isolation.finish().is_ok());
}

#[test]
fn a_devin_run_is_given_the_four_xdg_folders_with_its_login_and_reaches_the_other_programs_ones() {
    let owner = Owner::new();
    let isolation = owner.devin();
    let home = isolation.made_at(at(NOW)).unwrap();
    let vars = isolation.vars_in(&home);
    let names: Vec<&str> = vars.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME"
        ]
    );
    for (name, value) in &vars {
        assert!(
            value.starts_with(&home.display().to_string()),
            "{name}: {value}"
        );
    }
    // Devin's login and config; its sessions, logs and trusted folders are not copied.
    assert!(home.join("data/devin/credentials.toml").is_file());
    assert!(home.join("config/devin/config.json").is_file());
    assert!(!home.join("data/devin/cli").exists());
    assert_eq!(mode(&home.join("data/devin/credentials.toml")), 0o600);
    // What other programs keep there is reached through a link, as it was, and Devin's own
    // folders are not: Devin writes in the run's.
    let link = home.join("data/opencode");
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::read_link(&link).unwrap(),
        owner.path(".local/share/opencode")
    );
    assert!(fs::symlink_metadata(home.join("config/opencode")).is_ok());
    write(
        &home.join("data/devin/cli/sessions.db"),
        "the run's session",
    );
    write(
        &home.join("cache/devin/telemetry_state.json"),
        "the run's cache",
    );
    assert_eq!(
        fs::read_to_string(owner.path(".local/share/devin/cli/sessions.db")).unwrap(),
        SENTINEL
    );
    assert!(
        isolation.finish().is_ok(),
        "the run wrote in its own folders only"
    );
    // Dropping the home takes the links away, and none of what they reach.
    drop(isolation);
    assert!(owner.path(".local/share/opencode/auth.json").is_file());
}

#[test]
fn the_variables_of_a_home_name_none_of_the_owners_folders() {
    let owner = Owner::new();
    for isolation in [
        owner.pi("openrouter/x", hours(1)),
        owner.codex(),
        owner.devin(),
    ] {
        let home = isolation.made_at(at(NOW)).unwrap();
        for (name, value) in isolation.vars_in(&home) {
            assert!(
                !value.starts_with(&owner.home.path().display().to_string()),
                "{name} is {value}"
            );
        }
    }
}

// ----------------------------------------------------------- the owner's, untouched

/// The owner's files that each harness's run is held not to change.
fn watched(owner: &Owner, which: &str) -> Vec<PathBuf> {
    let below: &[&str] = match which {
        "pi" => &[
            ".pi/agent/auth.json",
            ".pi/agent/settings.json",
            ".pi/agent/models.json",
            ".pi/agent/mcp-auth.json",
        ],
        "codex" => &[".codex/auth.json", ".codex/config.toml"],
        _ => &[
            ".local/share/devin/credentials.toml",
            ".config/devin/config.json",
            ".local/share/devin/cli/trusted_workspaces.json",
        ],
    };
    below.iter().map(|file| owner.path(file)).collect()
}

fn isolation_of(owner: &Owner, which: &str) -> Isolation {
    match which {
        "pi" => owner.pi("openrouter/x", hours(1)),
        "codex" => owner.codex(),
        _ => owner.devin(),
    }
}

#[test]
fn a_run_that_changes_a_file_of_the_owners_fails_naming_it_and_saying_none_of_what_it_holds() {
    for which in ["pi", "codex", "devin"] {
        let owner = Owner::new();
        let isolation = isolation_of(&owner, which);
        isolation.made_at(at(NOW)).unwrap();
        assert!(
            isolation.finish().is_ok(),
            "{which}: nothing happened to the owner's"
        );
        for file in watched(&owner, which) {
            let before = fs::read(&file).unwrap();
            let shown = format!(
                "~/{}",
                file.strip_prefix(owner.home.path()).unwrap().display()
            );
            // The file written (a refresh, a trust table, a setting).
            fs::write(&file, format!("{SENTINEL}-written-by-the-run")).unwrap();
            let said = isolation.finish().unwrap_err().to_string();
            assert!(
                said.contains(&format!("{shown} is not as it was")),
                "{which}: {said}"
            );
            assert!(said.contains("THE RUN CHANGED THE OWNER'S"), "{said}");
            assert!(
                said.contains("(or the owner's own"),
                "the owner's own harness could have: {said}"
            );
            assert!(!said.contains(SENTINEL) && !said_anywhere(&said), "{said}");
            // The file gone.
            fs::remove_file(&file).unwrap();
            let said = isolation.finish().unwrap_err().to_string();
            assert!(
                said.contains(&format!("{shown} is gone")),
                "{which}: {said}"
            );
            // Put back as it was, the owner's setup is as it was.
            fs::write(&file, &before).unwrap();
            assert!(isolation.finish().is_ok(), "{which}: {shown} is as it was");
        }
    }
}

#[test]
fn a_file_the_run_makes_in_the_owners_folder_is_named_too() {
    let owner = Owner::new();
    fs::remove_file(owner.path(".pi/agent/models.json")).unwrap();
    let isolation = owner.pi("openrouter/x", hours(1));
    isolation.made_at(at(NOW)).unwrap();
    write(&owner.path(".pi/agent/models.json"), "{}");
    let said = isolation.finish().unwrap_err().to_string();
    assert!(said.contains("~/.pi/agent/models.json was made"), "{said}");
}

#[test]
fn a_session_the_run_leaves_in_the_owners_pi_folder_is_the_runs_and_the_owners_own_is_not() {
    let owner = Owner::new();
    let isolation = owner.pi("openrouter/x", hours(1));
    isolation.made_at(at(NOW)).unwrap();
    let sessions = owner.path(".pi/agent/sessions");
    // The owner's own Pi writes all day.
    for own in ["--Users-me-Projects-app--", "--Users-me-.consensflow--"] {
        write(&sessions.join(own).join("s.jsonl"), "{}");
    }
    assert!(isolation.finish().is_ok());
    for run in [
        "--Users-me-.consensflow-candidate-live-pi-parity--",
        "--Users-me-.consensflow-candidate-evals-workspace--",
        "--var-folders-ab-T-consensflow-integration-AbC-workspace--",
    ] {
        write(&sessions.join(run).join("s.jsonl"), "{}");
        let said = isolation.finish().unwrap_err().to_string();
        assert!(
            said.contains(&format!("~/.pi/agent/sessions gained {run}")),
            "{said}"
        );
        fs::remove_dir_all(sessions.join(run)).unwrap();
        assert!(isolation.finish().is_ok());
    }
}

#[test]
fn a_login_the_run_wrote_again_is_said_and_one_written_the_same_is_not() {
    let owner = Owner::new();
    let isolation = owner.pi("openrouter/x", hours(1));
    let home = isolation.made_at(at(NOW)).unwrap();
    let login = home.join("pi/agent/auth.json");
    // Written again with nothing in it changed (another layout of the same JSON).
    let same: Value = serde_json::from_str(&fs::read_to_string(&login).unwrap()).unwrap();
    fs::write(&login, serde_json::to_string_pretty(&same).unwrap()).unwrap();
    assert!(isolation.finish().is_ok());
    // A token refreshed in the run's copy.
    fs::write(
        &login,
        json!({ "openrouter": { "type": "api_key", "key": "refreshed-ZZZ" } }).to_string(),
    )
    .unwrap();
    let said = isolation.finish().unwrap_err().to_string();
    assert!(
        said.contains("THE RUN'S PI WROTE ITS COPY OF THE LOGIN AGAIN"),
        "{said}"
    );
    assert!(
        said.contains("the run's Pi wrote a different login in the home's pi/agent/auth.json"),
        "{said}"
    );
    assert!(
        !said.contains("refreshed-ZZZ") && !said_anywhere(&said),
        "{said}"
    );
    // The owner's own was not touched by it.
    fs::write(&login, "gone").unwrap();
    assert!(isolation.finish().is_err());
    fs::remove_file(&login).unwrap();
    let said = isolation.finish().unwrap_err().to_string();
    assert!(said.contains("pi/agent/auth.json"), "{said}");
}

#[test]
fn a_run_that_left_the_owners_setup_as_it_was_says_which_files_it_checked_by_a_hash_and_none_of_what_they_hold(
) {
    let owner = Owner::new();
    let isolation = owner.pi("openrouter/x", hours(1));
    assert_eq!(isolation.evidence(), None, "no run used it");
    isolation.made_at(at(NOW)).unwrap();
    let said = isolation.evidence().unwrap();
    assert!(said.contains("the owner's Pi setup is as it was"), "{said}");
    for file in ["auth.json", "settings.json", "models.json", "mcp-auth.json"] {
        assert!(
            said.contains(&format!("~/.pi/agent/{file} ")),
            "{file}: {said}"
        );
    }
    // Twelve digits of a hash after each name: the same one for the same file, another once it changed.
    let hash = |said: &str, file: &str| {
        let after = said.split(&format!("~/.pi/agent/{file} ")).nth(1).unwrap();
        after.chars().take(12).collect::<String>()
    };
    let before = hash(&said, "settings.json");
    assert!(
        before.chars().all(|digit| digit.is_ascii_hexdigit()),
        "{said}"
    );
    fs::write(owner.path(".pi/agent/settings.json"), "changed").unwrap();
    // Evidence is of a run that passed: a changed file is a failure's, not evidence's.
    assert_ne!(
        hash(&isolation.evidence().unwrap(), "settings.json"),
        before
    );
    for secret in [ACCESS, REFRESH, KEY, OTHER, SENTINEL] {
        assert!(!said.contains(secret), "{said}");
    }
}

#[test]
fn a_home_never_made_has_nothing_to_say_and_the_words_never_hold_a_login() {
    let owner = Owner::new();
    let unmade = owner.codex();
    assert!(unmade.finish().is_ok(), "no run used it");
    assert_eq!(owner.made(), 0);
    let made = owner.pi("openrouter/x", hours(1));
    made.made_at(at(NOW)).unwrap();
    let debug = format!("{made:?}");
    assert!(
        debug.contains("Isolation") && debug.contains("Pi"),
        "{debug}"
    );
    assert!(
        !said_anywhere(&debug) && !debug.contains(SENTINEL),
        "{debug}"
    );
}

#[test]
fn a_home_a_killed_run_left_is_taken_away_by_the_next_that_is_made() {
    let owner = Owner::new();
    // A process id nothing has: a child that ended and was waited for.
    let mut child = Run::new("/usr/bin/true").spawn().unwrap();
    let gone = child.id();
    child.wait(Duration::from_secs(10)).unwrap();
    let left = owner.parent.path().join(format!("{PREFIX}{gone}-abc123"));
    write(&left.join("pi/agent/auth.json"), "{}");
    let running = owner
        .parent
        .path()
        .join(format!("{PREFIX}{}-xyz789", std::process::id()));
    write(&running.join("codex/auth.json"), "{}");
    owner.codex().made_at(at(NOW)).unwrap();
    assert!(
        !left.exists(),
        "the home of a run that is gone is taken away"
    );
    assert!(running.exists(), "a process that is there keeps its own");
}

#[test]
fn a_harness_with_no_home_of_its_own_to_make_has_none() {
    let home = Path::new("/home/me");
    for harness in [Harness::Claude, Harness::OpenCode] {
        assert!(
            Isolation::of_machine(harness, home).unwrap().is_none(),
            "{harness:?}"
        );
    }
    for harness in [Harness::Codex, Harness::Devin] {
        assert!(
            Isolation::of_machine(harness, home).unwrap().is_some(),
            "{harness:?}"
        );
    }
}
