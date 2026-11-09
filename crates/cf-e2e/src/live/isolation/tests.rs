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
            ".config/other-tool/config.json",
            ".local/share/devin/credentials.toml",
            ".local/share/devin/cli/trusted_workspaces.json",
            ".local/share/devin/cli/sessions.db",
            ".local/share/other-tool/auth.json",
            ".config/muse/auth.json",
            ".config/muse/settings.json",
            ".config/muse/trust.json",
            ".local/share/muse/session-index.db",
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

    /// The `cliproxyapi` provider's files, as the owner's Mac keeps them in its agent folder:
    /// the login (the only one the Mac's Pi has), the config pointed at the proxy `base_url`,
    /// the models the proxy served last, `settings.json` naming the package among the owner's
    /// other one, npm's manifests, and the package below `npm/node_modules` Pi loads it from.
    fn cliproxyapi(&self, base_url: &str) {
        let agent = self.path(".pi/agent");
        write(&agent.join("auth.json"), &auth_cliproxyapi());
        write(
            &agent.join("cliproxyapi.json"),
            &json!({
                "baseUrl": base_url,
                "apiKey": PROXY_KEY,
                "providerId": "cliproxyapi",
                "providerName": "CLIProxyAPI",
            })
            .to_string(),
        );
        write(
            &agent.join("cliproxyapi-models.json"),
            &json!({ "models": [{ "id": "muse-spark-1.3-contributor" }], "fetchedAt": at(NOW) })
                .to_string(),
        );
        write(
            &agent.join("settings.json"),
            &json!({ "packages": ["npm:pi-web-access", "npm:@router-for-me/pi-cliproxyapi-provider"] })
                .to_string(),
        );
        write(
            &agent.join("npm/package.json"),
            &json!({ "dependencies": {
                "pi-web-access": "^0.46.0",
                "@router-for-me/pi-cliproxyapi-provider": "^1.4.24",
            } })
            .to_string(),
        );
        write(&agent.join("npm/package-lock.json"), "{}");
        let package = agent.join("npm/node_modules/@router-for-me/pi-cliproxyapi-provider");
        write(
            &package.join("package.json"),
            &json!({
                "name": "@router-for-me/pi-cliproxyapi-provider",
                "version": "1.4.24",
                "pi": { "extensions": ["./extensions/index.ts", "./extensions/tps.ts"] },
            })
            .to_string(),
        );
        write(&package.join("LICENSE"), "the package's license");
        write(&package.join("README.md"), "the package's readme");
        write(
            &package.join("extensions/index.ts"),
            "the provider's extension",
        );
        write(
            &package.join("extensions/lib.ts"),
            "the extension's library",
        );
        write(
            &package.join("extensions/tps.ts"),
            "the provider's second extension",
        );
    }

    /// Pi's, on the models `models`, for a run of `run`.
    fn pi_on(&self, models: &[&str], run: Duration) -> Isolation {
        let models: Vec<String> = models.iter().map(|model| (*model).to_owned()).collect();
        let mut made = Isolation::pi_on(self.home.path(), &self.path(".pi/agent"), &models, run);
        made.parent = self.parent.path().to_path_buf();
        made
    }

    fn codex(&self) -> Isolation {
        let mut made = Isolation::codex(self.home.path(), &self.path(".codex"));
        made.parent = self.parent.path().to_path_buf();
        made
    }

    fn muse(&self) -> Isolation {
        let mut made = Isolation::muse(
            self.home.path(),
            &self.path(".config"),
            &self.path(".local/share"),
        );
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
    // The variable Pi reads its folder by, naming the run's, and the one that keeps Pi from
    // installing at its start.
    let vars = isolation.vars_in(&home);
    assert_eq!(
        vars,
        [
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                home.join("pi/agent").display().to_string()
            ),
            ("PI_OFFLINE".to_owned(), "1".to_owned())
        ]
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

// ------------------------------------------------------------- the provider's files (T-1172)

/// The key CLIProxyAPI is asked its models with: `access` of the `cliproxyapi` login and
/// `apiKey` of `cliproxyapi.json`. The login's `refresh` is the endpoint's own metadata,
/// still not a thing a run is given.
const PROXY_KEY: &str = "cli-proxy-key-fedcba987654";
const PROXY_META: &str = "proxy-meta-zyxwvutsrqponm";

/// The `auth.json` of a Mac whose Pi has the `cliproxyapi` login alone, as the provider's
/// extension writes it: the key in `access`, the endpoint's metadata in `refresh`, and an
/// `expires` a hundred years on.
fn auth_cliproxyapi() -> String {
    json!({
        "cliproxyapi": { "type": "oauth", "access": PROXY_KEY, "refresh": PROXY_META, "expires": ends(hours(876_600)) },
    })
    .to_string()
}

/// A stand-in for CLIProxyAPI: answers every HTTP request it gets with `401`, its answer to a
/// keyless one, on a port of its own. Its `http://` URL.
fn proxy_answering() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                return;
            };
            use std::io::Write;
            let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
        }
    });
    url
}

/// A base URL nothing answers on: a port the system just let go of.
fn proxy_gone() -> String {
    format!(
        "http://{}",
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
    )
}

/// Pi loads `cliproxyapi` as the package `settings.json`'s `packages` names, an `npm:` spec
/// resolved below `npm/node_modules`, which reads `cliproxyapi.json` (the proxy's base URL
/// and key) and `cliproxyapi-models.json` (the models it served last), and logs in by
/// `auth.json`. A run on the provider's model is given all of it, the login without its
/// `refresh`, and the owner's files are hashed with the provider's among them.
#[test]
fn a_pi_run_on_a_provider_the_agent_folder_gives_pi_carries_its_files_and_its_login() {
    let owner = Owner::new();
    owner.cliproxyapi(&proxy_answering());
    let isolation = owner.pi("cliproxyapi/muse-spark-1.3-contributor", hours(1));
    let home = isolation.made_at(at(NOW)).unwrap();
    // The login, the provider's own files and its package, and a `settings.json` that names
    // the provider's package alone: the owner's other package and npm's manifests are left.
    assert_eq!(
        files_below(&home),
        [
            "pi/agent/auth.json",
            "pi/agent/cliproxyapi-models.json",
            "pi/agent/cliproxyapi.json",
            "pi/agent/npm/node_modules/@router-for-me/pi-cliproxyapi-provider/LICENSE",
            "pi/agent/npm/node_modules/@router-for-me/pi-cliproxyapi-provider/README.md",
            "pi/agent/npm/node_modules/@router-for-me/pi-cliproxyapi-provider/extensions/index.ts",
            "pi/agent/npm/node_modules/@router-for-me/pi-cliproxyapi-provider/extensions/lib.ts",
            "pi/agent/npm/node_modules/@router-for-me/pi-cliproxyapi-provider/extensions/tps.ts",
            "pi/agent/npm/node_modules/@router-for-me/pi-cliproxyapi-provider/package.json",
            "pi/agent/settings.json",
        ]
    );
    let copied = fs::read_to_string(home.join("pi/agent/auth.json")).unwrap();
    let held: Value = serde_json::from_str(&copied).unwrap();
    assert_eq!(
        held,
        json!({ "cliproxyapi": {
            "type": "oauth",
            "access": PROXY_KEY,
            "expires": ends(hours(876_600)),
        } })
    );
    assert!(
        !copied.contains(PROXY_META),
        "the refresh was copied: {copied}"
    );
    // The config and the models come whole; the settings name the provider's package alone.
    assert_eq!(
        fs::read_to_string(home.join("pi/agent/cliproxyapi.json")).unwrap(),
        fs::read_to_string(owner.path(".pi/agent/cliproxyapi.json")).unwrap()
    );
    assert_eq!(
        serde_json::from_str::<Value>(
            &fs::read_to_string(home.join("pi/agent/settings.json")).unwrap()
        )
        .unwrap(),
        json!({ "packages": ["npm:@router-for-me/pi-cliproxyapi-provider"] })
    );
    // The provider's files of the owner's are hashed, before and after, as the rest are.
    let said = isolation.evidence().unwrap();
    for file in [
        "cliproxyapi.json",
        "cliproxyapi-models.json",
        "npm/package.json",
        "npm/package-lock.json",
        "npm/node_modules/@router-for-me/pi-cliproxyapi-provider/package.json",
    ] {
        assert!(
            said.contains(&format!("~/.pi/agent/{file}")),
            "{file}: {said}"
        );
    }
    assert!(isolation.finish().is_ok());
}

/// Pi installs a `git:` package below `git/<host>/<path>`, the ref left out, and loads it from
/// there: the owner's Mac's Pi has named `cliproxyapi`'s package so since 2026-11-09 (a branch
/// of a fork, as a string or as an object's `source`), and its npm copy, still there, is named no
/// more. A run on the provider's model is given that package whole and a `settings.json` that
/// names the same source, and the owner's copy of it is hashed; npm's is neither (T-1358).
#[test]
fn a_pi_run_on_a_provider_whose_package_pi_has_from_git_carries_that_package_and_names_it() {
    const SOURCE: &str = "git:github.com/ngvoicu/pi-cliproxyapi-provider@feat/claude-messages";
    for named in [
        json!({ "source": SOURCE, "extensions": ["extensions/index.ts"] }),
        json!(SOURCE),
    ] {
        let owner = Owner::new();
        owner.cliproxyapi(&proxy_answering());
        let package = owner.path(".pi/agent/git/github.com/ngvoicu/pi-cliproxyapi-provider");
        write(
            &package.join("package.json"),
            &json!({
                "name": "@router-for-me/pi-cliproxyapi-provider",
                "version": "1.4.25",
                "pi": { "extensions": ["./extensions/index.ts", "./extensions/tps.ts"] },
            })
            .to_string(),
        );
        write(&package.join("extensions/index.ts"), "the fork's extension");
        let commit = git_checkout(&package);
        write(
            &owner.path(".pi/agent/settings.json"),
            &json!({ "packages": ["npm:pi-web-access", named, "git:github.com/ngvoicu/pi-cached-compaction@main"] })
                .to_string(),
        );
        let isolation = owner.pi("cliproxyapi/muse-spark-1.3-contributor", hours(1));
        let home = isolation.made_at(at(NOW)).unwrap();
        let copied: Vec<String> = files_below(&home)
            .into_iter()
            .filter(|file| !file.contains("/.git/") || file.ends_with("/.git/HEAD"))
            .collect();
        assert_eq!(
            copied,
            [
                "pi/agent/auth.json",
                "pi/agent/cliproxyapi-models.json",
                "pi/agent/cliproxyapi.json",
                "pi/agent/git/github.com/ngvoicu/pi-cliproxyapi-provider/.git/HEAD",
                "pi/agent/git/github.com/ngvoicu/pi-cliproxyapi-provider/extensions/index.ts",
                "pi/agent/git/github.com/ngvoicu/pi-cliproxyapi-provider/package.json",
                "pi/agent/settings.json",
            ],
            "{named}"
        );
        assert_eq!(
            serde_json::from_str::<Value>(
                &fs::read_to_string(home.join("pi/agent/settings.json")).unwrap()
            )
            .unwrap(),
            // The entry as the owner wrote it: an object's filter of the package's resources is
            // kept, or Pi would load every resource the package lists.
            json!({ "packages": [named] })
        );
        let copy = isolation.said().join("\n");
        assert!(
            copy.contains(&commit) && copy.contains("clean"),
            "the run says the checkout's commit: {copy}"
        );
        let said = isolation.evidence().unwrap();
        assert!(
            said.contains(
                "~/.pi/agent/git/github.com/ngvoicu/pi-cliproxyapi-provider/package.json"
            ),
            "{said}"
        );
        assert!(
            !said.contains("npm/node_modules") && !said.contains("npm/package.json"),
            "{said}"
        );
        assert!(isolation.finish().is_ok());
    }
}

/// A git checkout of `folder`'s files, committed, as Pi's install leaves one: its commit.
fn git_checkout(folder: &Path) -> String {
    let git = |args: &[&str]| {
        let ran = Run::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args.iter().copied())
            .cwd(folder)
            .finding_programs()
            .inheriting_env()
            .run()
            .unwrap();
        assert_eq!(ran.code, Some(0), "git {args:?}: {}", ran.stderr);
        ran.stdout.trim().to_owned()
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "the package"]);
    git(&["rev-parse", "HEAD"])
}

/// A git checkout with a change of its own, or a file that is none of its commit's, is not what
/// the source names: Pi would load bytes no commit holds. Refused before anything is made.
#[test]
fn a_pi_run_on_a_package_from_git_is_refused_where_its_checkout_is_not_clean() {
    const SOURCE: &str = "git:github.com/ngvoicu/pi-cliproxyapi-provider@feat/claude-messages";
    let owner = Owner::new();
    owner.cliproxyapi(&proxy_answering());
    let package = owner.path(".pi/agent/git/github.com/ngvoicu/pi-cliproxyapi-provider");
    write(
        &package.join("package.json"),
        &json!({ "name": "@router-for-me/pi-cliproxyapi-provider", "version": "1.4.25" })
            .to_string(),
    );
    write(&package.join("extensions/index.ts"), "the fork's extension");
    git_checkout(&package);
    write(
        &owner.path(".pi/agent/settings.json"),
        &json!({ "packages": [SOURCE] }).to_string(),
    );
    for (dirty, how) in [
        ("extensions/index.ts", "changed"),
        ("extensions/extra.ts", "a file of no commit"),
    ] {
        write(&package.join(dirty), "code no commit holds");
        let said = owner
            .pi("cliproxyapi/muse-spark-1.3-contributor", hours(1))
            .made_at(at(NOW))
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("not clean") && said.contains(dirty),
            "{how}: {said}"
        );
        assert_eq!(owner.made(), 0, "{how}");
        fs::remove_file(package.join(dirty)).ok();
        git_checkout_reset(&package);
    }
}

/// The checkout as its commit has it again.
fn git_checkout_reset(folder: &Path) {
    let ran = Run::new("git")
        .args(["checkout", "-q", "--", "."])
        .cwd(folder)
        .finding_programs()
        .inheriting_env()
        .run()
        .unwrap();
    assert_eq!(ran.code, Some(0), "{}", ran.stderr);
}

/// An `npm:` spec that names a version Pi has not got on disk makes Pi install it at its start,
/// from the network, into the run's home: refused before anything is made. A spec with no
/// version is what is on disk; one with an exact version must be that version; a range or a
/// tag, which the kit cannot hold to what is on disk, is refused too. The run says the version.
#[test]
fn a_pi_run_is_refused_where_the_npm_package_on_disk_is_not_the_version_its_spec_asks() {
    let owner = Owner::new();
    owner.cliproxyapi(&proxy_answering());
    let settings = |spec: &str| {
        write(
            &owner.path(".pi/agent/settings.json"),
            &json!({ "packages": [spec] }).to_string(),
        );
    };
    let made = |owner: &Owner| {
        let isolation = owner.pi("cliproxyapi/muse-spark-1.3-contributor", hours(1));
        isolation
            .made_at(at(NOW))
            .map(|_| isolation.said().join("\n"))
    };
    settings("npm:@router-for-me/pi-cliproxyapi-provider@1.4.24");
    let said = made(&owner).unwrap();
    assert!(said.contains("1.4.24"), "{said}");
    for (spec, words) in [
        (
            "npm:@router-for-me/pi-cliproxyapi-provider@1.4.25",
            "1.4.25",
        ),
        ("npm:@router-for-me/pi-cliproxyapi-provider@^1.4.0", "exact"),
        ("npm:@router-for-me/pi-cliproxyapi-provider@latest", "exact"),
    ] {
        settings(spec);
        let refused = made(&owner).unwrap_err().to_string();
        assert!(
            refused.contains(words) && refused.contains("1.4.24"),
            "{spec}: {refused}"
        );
        assert_eq!(owner.made(), 0, "{spec}: refused before anything is made");
    }
}

/// A proxy URL can carry a key (a user and password, a query, a fragment): the refusal of a
/// proxy that does not answer says its scheme, host, port and path alone.
#[test]
fn a_proxy_that_does_not_answer_is_said_with_no_part_of_its_url_that_can_hold_a_secret() {
    let owner = Owner::new();
    let port = proxy_gone().rsplit(':').next().unwrap().to_owned();
    owner.cliproxyapi(&format!(
        "http://user:SECRET-PASSWORD@127.0.0.1:{port}/v1?key=SECRET-KEY#SECRET-FRAGMENT"
    ));
    let said = owner
        .pi("cliproxyapi/muse-spark-1.3-contributor", hours(1))
        .made_at(at(NOW))
        .unwrap_err()
        .to_string();
    assert!(said.contains("does not answer"), "{said}");
    assert!(
        said.contains(&format!("http://127.0.0.1:{port}/v1")),
        "{said}"
    );
    assert!(
        !said.contains("SECRET") && !said.contains("user:"),
        "{said}"
    );
}

/// Pi installs a package its settings name and it has not got, and updates and downloads at its
/// start (Pi 1.5.0, `dist/core/package-manager.js`: `installMissing` returns before installing
/// when `PI_OFFLINE` is set): every window of the kit's Pi starts with it, so what runs is what
/// the kit copied.
#[test]
fn a_pi_window_of_the_kit_starts_offline() {
    let owner = Owner::new();
    write(&owner.path(".pi/agent/auth.json"), &auth(hours(2)));
    let isolation = owner.pi("openai/gpt-6-luna", hours(1));
    let vars = HarnessHome::vars(&isolation).unwrap();
    assert!(
        vars.iter()
            .any(|(name, value)| name == "PI_OFFLINE" && value == "1"),
        "{vars:?}"
    );
}

/// The forms of `packages` the kit finds the provider's package in, and where Pi keeps each:
/// an `npm:` spec of its name (a version or not), a `git:` source of a repository of its name
/// (a ref or not, a fork's or not), a string or an object's `source`. Not a package of another
/// name, a repository of another name, a URL or a path, whose folder the kit does not know.
#[test]
fn the_providers_package_is_found_by_its_npm_spec_or_its_git_source_and_nothing_else() {
    let provider = pi::provider("cliproxyapi").unwrap();
    let found = |entries: Value| provider.source_in(entries.as_array().unwrap());
    let folder = |entries: Value| found(entries).map(|source| source.folder);
    let npm = Path::new("npm/node_modules/@router-for-me/pi-cliproxyapi-provider");
    assert_eq!(
        folder(json!(["npm:@router-for-me/pi-cliproxyapi-provider"])).as_deref(),
        Some(npm)
    );
    assert_eq!(
        folder(json!(["npm:@router-for-me/pi-cliproxyapi-provider@1.4.24"])).as_deref(),
        Some(npm)
    );
    let git = Path::new("git/github.com/ngvoicu/pi-cliproxyapi-provider");
    for spec in [
        "git:github.com/ngvoicu/pi-cliproxyapi-provider",
        "git:github.com/ngvoicu/pi-cliproxyapi-provider@feat/claude-messages",
        "git:github.com/ngvoicu/pi-cliproxyapi-provider.git@v1",
    ] {
        assert_eq!(folder(json!([spec])).as_deref(), Some(git), "{spec}");
        assert_eq!(
            folder(json!([{ "source": spec }])).as_deref(),
            Some(git),
            "{spec}"
        );
    }
    let source = found(json!([
        7,
        null,
        "npm:pi-web-access",
        "git:github.com/router-for-me/pi-cliproxyapi-provider@main"
    ]))
    .unwrap();
    assert_eq!(
        source.spec,
        "git:github.com/router-for-me/pi-cliproxyapi-provider@main"
    );
    assert!(!source.npm);
    for none in [
        "npm:@router-for-me/pi-cliproxyapi-providers",
        "npm:pi-cliproxyapi-provider",
        "git:github.com/ngvoicu/pi-cached-compaction@main",
        "git:https://github.com/ngvoicu/pi-cliproxyapi-provider",
        "git:TOKEN@github.com/ngvoicu/pi-cliproxyapi-provider",
        "git:github.com/../pi-cliproxyapi-provider",
        "git:/opt/pi-cliproxyapi-provider",
        "git:./pi-cliproxyapi-provider",
        "git:../forks/pi-cliproxyapi-provider",
        "git:github.com:ngvoicu/pi-cliproxyapi-provider",
        "./pi-cliproxyapi-provider",
        "/opt/pi-cliproxyapi-provider",
    ] {
        assert_eq!(found(json!([none])), None, "{none}");
    }
}

/// Refused before anything is made, in words naming what is missing and what the human can
/// do: where the provider's config is missing, where its package is, where `settings.json`
/// does not name it, and where the proxy it is set for does not answer.
#[test]
fn a_pi_run_is_refused_before_a_window_where_the_provider_or_the_proxy_is_missing() {
    let owner = Owner::new();
    let refused = |owner: &Owner| {
        owner
            .pi("cliproxyapi/muse-spark-1.3-contributor", hours(1))
            .made_at(at(NOW))
            .unwrap_err()
            .to_string()
    };
    // The provider's own config is missing.
    owner.cliproxyapi(&proxy_gone());
    fs::remove_file(owner.path(".pi/agent/cliproxyapi.json")).unwrap();
    let said = refused(&owner);
    assert!(said.contains("`cliproxyapi`"), "{said}");
    assert!(said.contains("cliproxyapi.json"), "{said}");
    assert!(said.contains("--models pi="), "{said}");
    assert!(!said_anywhere(&said) && !said.contains(PROXY_KEY), "{said}");
    // The package Pi loads the provider from is missing.
    owner.cliproxyapi(&proxy_gone());
    fs::remove_dir_all(owner.path(".pi/agent/npm/node_modules")).unwrap();
    let said = refused(&owner);
    assert!(
        said.contains("npm/node_modules/@router-for-me/pi-cliproxyapi-provider"),
        "{said}"
    );
    assert!(!said_anywhere(&said) && !said.contains(PROXY_KEY), "{said}");
    // `settings.json` does not name the package in `packages`: Pi would never load it.
    owner.cliproxyapi(&proxy_gone());
    write(
        &owner.path(".pi/agent/settings.json"),
        &json!({ "packages": ["npm:pi-web-access"] }).to_string(),
    );
    let said = refused(&owner);
    assert!(said.contains("settings.json"), "{said}");
    assert!(
        said.contains("npm:@router-for-me/pi-cliproxyapi-provider"),
        "{said}"
    );
    assert!(!said_anywhere(&said) && !said.contains(PROXY_KEY), "{said}");
    // All of it is there, and the proxy does not answer.
    owner.cliproxyapi(&proxy_gone());
    let said = refused(&owner);
    assert!(said.contains("does not answer"), "{said}");
    assert!(said.contains("http://127.0.0.1:"), "{said}");
    assert!(said.contains("--models pi="), "{said}");
    assert!(!said_anywhere(&said) && !said.contains(PROXY_KEY), "{said}");
    // No home was made anywhere along the refusals.
    assert_eq!(owner.made(), 0);
}

#[test]
fn a_login_that_is_too_short_is_refused_before_anything_is_made_and_one_that_fits_the_run_is_not() {
    let owner = Owner::new();
    write(&owner.path(".pi/agent/auth.json"), &auth(hours(2)));
    // An hour's run needs an hour and ten minutes of the `openai` login's two.
    let fits = owner.pi("openai/gpt-6-luna", hours(1));
    assert!(fits.made_at(at(NOW)).is_ok());
    // Two hours' run, and the margin, does not fit it.
    let longer = owner.pi("openai/gpt-6-luna", hours(2));
    let said = longer.made_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("`openai` expires in 2 h"), "{said}");
    assert!(said.contains("at least 2 h 10 min more"), "{said}");
    assert!(!said_anywhere(&said), "{said}");
    // Only the home that fit is on disk.
    assert_eq!(owner.made(), 1);
}

/// An eval's chief and staff may be on Pi models of two providers (T-785): the home holds the
/// login of each, and a run is refused where one of them does not last it. A model that names no
/// provider leaves it to Pi, which gets every login that is fit.
#[test]
fn a_pi_run_on_models_of_two_providers_needs_the_login_of_each_and_is_refused_where_one_ends_in_it()
{
    let owner = Owner::new();
    write(&owner.path(".pi/agent/auth.json"), &auth(hours(2)));
    let both = owner.pi_on(&["openrouter/deepseek/v4", "openai/gpt-6-luna"], hours(1));
    let home = both.made_at(at(NOW)).unwrap();
    let copied: Value =
        serde_json::from_str(&fs::read_to_string(home.join("pi/agent/auth.json")).unwrap())
            .unwrap();
    let mut providers: Vec<&String> = copied.as_object().unwrap().keys().collect();
    providers.sort();
    assert_eq!(providers, ["openai", "openrouter"]);
    assert!(!copied.to_string().contains(OTHER), "{copied}");
    // The `openai` login lasts two hours: a run of two, and its margin, is refused for it.
    let longer = owner.pi_on(&["openrouter/deepseek/v4", "openai/gpt-6-luna"], hours(2));
    let said = longer.made_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("`openai` expires in 2 h"), "{said}");
    assert!(!said.contains("`openrouter`"), "{said}");
    // One model that names no provider: every fit login, the short one left out.
    let any = owner.pi_on(&["openai/gpt-6-luna", "deepseek-v4"], hours(2));
    let home = any.made_at(at(NOW)).unwrap();
    let copied = fs::read_to_string(home.join("pi/agent/auth.json")).unwrap();
    assert!(
        copied.contains("openrouter") && !copied.contains(ACCESS),
        "{copied}"
    );
}

/// A home that keeps what it finds (an eval's, T-785) never fails the rig that ends on it: each
/// change is kept once, however many rigs end (a long run restarts the app), with the owner's
/// hashes before the run and after it, and none of what a file holds.
#[test]
fn a_home_that_keeps_what_it_finds_never_fails_the_rig_and_keeps_each_change_once_with_the_hashes()
{
    for which in ["pi", "codex", "devin", "muse"] {
        let owner = Owner::new();
        let isolation = isolation_of(&owner, which).keeping_what_it_finds();
        assert_eq!(isolation.found(), Some(Found::default()), "{which}");
        isolation.made_at(at(NOW)).unwrap();
        assert!(isolation.finish().is_ok(), "{which}");
        let found = isolation.found().unwrap();
        assert!(found.changed.is_empty(), "{which}: {found:?}");
        let checked = found.checked.unwrap();
        assert!(
            checked.contains("before the run and after it: ~/"),
            "{which}: {checked}"
        );
        let file = watched(&owner, which).remove(0);
        let shown = format!(
            "~/{}",
            file.strip_prefix(owner.home.path()).unwrap().display()
        );
        fs::write(&file, format!("{SENTINEL}-written-by-the-run")).unwrap();
        for _ in 0..2 {
            assert!(isolation.finish().is_ok(), "{which}: kept, not failed");
        }
        let found = isolation.found().unwrap();
        assert_eq!(
            found.changed,
            [format!("{shown} is not as it was")],
            "{which}"
        );
        let checked = found.checked.unwrap();
        let (before, after) = checked
            .strip_prefix(&format!(
                "the owner's {} setup before the run: ",
                proper(isolation.harness())
            ))
            .and_then(|hashes| hashes.split_once("; after it: "))
            .unwrap_or_else(|| panic!("{which}: {checked}"));
        let hash_of = |summary: &str| {
            summary
                .split(", ")
                .find_map(|entry| entry.strip_prefix(&format!("{shown} ")))
                .map(str::to_owned)
        };
        assert_ne!(hash_of(before), hash_of(after), "{which}: {checked}");
        assert!(hash_of(before).is_some(), "{which}: {checked}");
        assert!(
            !checked.contains(SENTINEL) && !said_anywhere(&checked),
            "{checked}"
        );
    }
    // A home that does not keep them fails the rig, as it always has.
    let owner = Owner::new();
    let plain = owner.codex();
    assert_eq!(plain.found(), None);
    plain.made_at(at(NOW)).unwrap();
    fs::write(owner.path(".codex/config.toml"), "written").unwrap();
    assert!(plain.finish().is_err());
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

#[cfg(unix)]
#[test]
fn a_home_made_in_a_folder_reached_through_a_link_is_named_through_the_link() {
    // `live linked-folder`'s leg A (T-1038): the folders a Devin window is given are spelled
    // as the folder the home is made in spells them, a Mac's `/var/folders/…` kept so, not
    // resolved to `/private/var/folders/…`.
    let owner = Owner::new();
    let linked = owner.home.path().join("linked-temp");
    std::os::unix::fs::symlink(owner.parent.path(), &linked).unwrap();
    let isolation = owner.devin().made_in(&linked);
    let home = isolation.made_at(at(NOW)).unwrap();
    assert!(home.starts_with(&linked), "{}", home.display());
    for (name, value) in isolation.vars_in(&home) {
        assert!(Path::new(&value).starts_with(&linked), "{name}: {value}");
        assert_eq!(
            fs::canonicalize(&value)
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap(),
            fs::canonicalize(owner.parent.path()).unwrap(),
            "{name}: the same folder as the one it links to"
        );
    }
    assert_eq!(owner.made(), 1);
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
    let link = home.join("data/other-tool");
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::read_link(&link).unwrap(),
        owner.path(".local/share/other-tool")
    );
    assert!(fs::symlink_metadata(home.join("config/other-tool")).is_ok());
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
    assert!(owner.path(".local/share/other-tool/auth.json").is_file());
}

/// T-1081: Devin's home moves the XDG folders, and Muse keeps its settings and sessions below
/// them too. A home that reached the owner's Muse through a link put the owner's sessions where
/// the daemon looks for a rig's Muse window's, and the owner's settings where it reads a
/// launch's hooks: the folders of the harnesses on the XDG layout are reached by no home.
#[test]
fn a_devin_home_reaches_none_of_the_owners_muse() {
    let owner = Owner::new();
    write(&owner.path(".config/muse/settings.json"), SENTINEL);
    write(&owner.path(".local/share/muse/session-index.db"), SENTINEL);
    let isolation = owner.devin();
    let home = isolation.made_at(at(NOW)).unwrap();
    for below in ["config/muse", "data/muse"] {
        assert!(
            fs::symlink_metadata(home.join(below)).is_err(),
            "{below} is in the run's home"
        );
    }
    // What another program keeps there is still reached.
    assert!(fs::symlink_metadata(home.join("data/other-tool")).is_ok());
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
        "muse" => &[
            ".config/muse/auth.json",
            ".config/muse/settings.json",
            ".config/muse/trust.json",
            ".local/share/muse/session-index.db",
        ],
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
        "muse" => owner.muse(),
        _ => owner.devin(),
    }
}

#[test]
fn a_run_that_changes_a_file_of_the_owners_fails_naming_it_and_saying_none_of_what_it_holds() {
    for which in ["pi", "codex", "devin", "muse"] {
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
    assert!(
        Isolation::of_machine_lasting(Harness::Claude, home, hours(1))
            .unwrap()
            .is_none()
    );
    for harness in [Harness::Codex, Harness::Devin] {
        assert!(
            Isolation::of_machine_lasting(harness, home, hours(1))
                .unwrap()
                .is_some(),
            "{harness:?}"
        );
    }
}

/// Every harness the kit can launch runs on a home of the run's own, or is named here with why
/// it has none. T-724's test named the harnesses of its day, and `of_machine_lasting` took any
/// other for one with no home, in silence: Muse Code, which came after, ran the live pacing run's
/// chief on the owner's own `~/.config/muse` and `~/.local/share/muse` (T-946).
#[test]
fn every_harness_the_kit_can_launch_has_a_home_of_the_runs_own_or_a_reason_it_has_none() {
    let home = Path::new("/home/me");
    // Claude Code's config folder is the kit's copy of the login (`super::login`).
    let elsewhere = [Harness::Claude];
    for harness in Harness::ALL {
        if harness == Harness::Pi {
            // Pi's home needs the run's model, which this test does not ask.
            continue;
        }
        let made = Isolation::of_machine_lasting(harness, home, hours(1)).unwrap();
        assert_eq!(made.is_some(), !elsewhere.contains(&harness), "{harness:?}");
    }
}

// ---------------------------------------------------------------------- the renewal (T-1067)

use super::super::renew::tests::stand_in;
use super::super::renew::TestMachine;

/// The access token an entry that Pi renewed holds.
const RENEWED: &str = "pi-RENEWED-5555555555";

/// A test machine, as the runner says it.
fn zeewin2() -> TestMachine {
    TestMachine::signalled(Some("zeewin2")).unwrap()
}

/// The owner's `auth.json` with an `openai` entry (`access`) that ends at `expires`.
fn auth_ending(expires: i64, access: &str) -> String {
    json!({
        "openrouter": { "type": "api_key", "key": KEY },
        "openai": { "type": "oauth", "access": access, "refresh": REFRESH, "expires": expires },
        "anthropic": oauth(OTHER, hours(9)),
    })
    .to_string()
}

/// A `pi` that stands in for the owner's: it writes down how it was started, does what `body` says,
/// and ends with `status`.
struct Renewing {
    owner: Owner,
    tools: tempfile::TempDir,
}

impl Renewing {
    /// An owner whose `openai` login ended an hour before `NOW`.
    fn new() -> Self {
        let renewing = Self {
            owner: Owner::new(),
            tools: tempfile::tempdir().unwrap(),
        };
        write(
            &renewing.owner.path(".pi/agent/auth.json"),
            &auth_ending(at(NOW - 3600), ACCESS),
        );
        renewing
    }

    fn log(&self) -> PathBuf {
        self.tools.path().join("log")
    }

    /// What the stand-in did, line by line: its words, then the folder it was started in. None
    /// where it never ran.
    fn started(&self) -> Option<Vec<String>> {
        let text = fs::read_to_string(self.log()).ok()?;
        Some(text.lines().map(str::to_owned).collect())
    }

    /// What the stand-in does when it renews the entry as Pi does: writes an entry that ends
    /// eight hours after `NOW`.
    fn renews(&self) -> String {
        let text = auth_ending(at(NOW + 8 * 3600), RENEWED);
        format!(
            "cat > '{}' <<'JSON'\n{text}\nJSON",
            self.owner.path(".pi/agent/auth.json").display()
        )
    }

    /// Pi's home for a run on `model`, with a renewal on `machine` (none on a machine that is not
    /// a test machine) that starts the stand-in.
    fn pi(&self, model: &str, machine: Option<TestMachine>, body: &str, status: i32) -> Isolation {
        let mut made = self.owner.pi(model, hours(1));
        let program = stand_in(self.tools.path(), &self.log(), body, status);
        made.renewal = machine.map(|machine| {
            pi::renewal(
                machine,
                self.owner.home.path(),
                &self.owner.path(".pi/agent"),
                model,
                move || Ok(program.clone()),
            )
        });
        made
    }
}

#[test]
fn a_pi_entry_that_ended_on_a_test_machine_is_renewed_by_pi_once_and_the_run_goes_on() {
    let run = Renewing::new();
    let isolation = run.pi("openai/gpt-6-luna", Some(zeewin2()), &run.renews(), 0);
    let home = isolation.made_at(at(NOW)).unwrap();
    // The run is given the entry Pi renewed, and only the run's provider's.
    let held: Value =
        serde_json::from_str(&fs::read_to_string(home.join("pi/agent/auth.json")).unwrap())
            .unwrap();
    assert_eq!(held["openai"]["access"], RENEWED);
    assert_eq!(held.as_object().unwrap().len(), 1, "the one provider's");
    // Pi was run once, in its print mode, on the run's model, with nothing to reach out for.
    let started = run.started().unwrap();
    assert_eq!(
        started[0],
        "-p Reply with the single word ok. --model openai/gpt-6-luna --thinking off --no-session \
         --no-tools --no-extensions --no-mcp --no-skills --no-context-files --no-prompt-templates"
    );
    assert!(
        started[1].contains("consensflow-integration-renew-"),
        "{started:?}"
    );
    // The record is the file and when the entry ended before and after, and no entry.
    let renewed = isolation.renewed.lock().unwrap();
    assert_eq!(renewed.len(), 1);
    assert_eq!(
        (renewed[0].before, renewed[0].after),
        (Some(at(NOW - 3600)), Some(at(NOW + 8 * 3600)))
    );
    assert_eq!(renewed[0].file, "~/.pi/agent/auth.json");
    drop(renewed);
    // The owner's file is what was copied, so nothing is stale and the run ends well.
    isolation.finish().unwrap();
}

#[test]
fn a_renewal_that_fails_refuses_the_pi_run_in_words_that_say_so_and_makes_no_home() {
    let run = Renewing::new();
    let isolation = run.pi(
        "openai/gpt-6-luna",
        Some(zeewin2()),
        "echo 'Error: token refresh failed' >&2",
        1,
    );
    let said = isolation.made_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("`openai` has expired"), "{said}");
    assert!(
        said.contains(
            "On zeewin2 the kit ran Pi once as the owner to renew it (T-1067), and the renewal \
             ran and failed: it ended 1 (Error: token refresh failed)"
        ),
        "{said}"
    );
    assert!(!said_anywhere(&said), "{said}");
    assert_eq!(run.owner.made(), 0);
}

#[test]
fn a_renewal_that_moves_nothing_refuses_the_pi_run_and_says_when_pi_renews_an_entry() {
    let run = Renewing::new();
    // Four minutes left of the run's length and its margin: Pi is run, as it renews an entry with
    // five minutes left, and the stand-in moves nothing.
    write(
        &run.owner.path(".pi/agent/auth.json"),
        &auth_ending(at(NOW + 4 * 60), ACCESS),
    );
    let isolation = run.pi("openai/gpt-6-luna", Some(zeewin2()), "", 0);
    let said = isolation.made_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("`openai` expires in 4 min"), "{said}");
    assert!(said.contains("the renewal ran and failed"), "{said}");
    assert!(
        said.contains("the login still ends 2027-01-15T08:04:00Z (in 4 min): Pi renews a token only once the entry has ended"),
        "{said}"
    );
    assert!(!said_anywhere(&said), "{said}");
    assert_eq!(run.owner.made(), 0);
}

/// Pi renews an entry when five minutes of it are left or it has ended, so running it on an entry
/// that has more is a call of the model for nothing (T-1136).
#[test]
fn an_entry_with_more_than_five_minutes_left_is_not_renewed_by_running_pi() {
    let run = Renewing::new();
    write(
        &run.owner.path(".pi/agent/auth.json"),
        &auth_ending(at(NOW + 29 * 60), ACCESS),
    );
    let isolation = run.pi("openai/gpt-6-luna", Some(zeewin2()), &run.renews(), 0);
    let said = isolation.made_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("`openai` expires in 29 min"), "{said}");
    assert!(!said.contains("the renewal ran and failed"), "{said}");
    assert_eq!(run.started(), None, "Pi was not run");
    assert_eq!(run.owner.made(), 0);
}

#[test]
fn a_renewal_that_changes_the_owners_pi_files_beside_the_login_fails_and_names_the_file() {
    let run = Renewing::new();
    let body = format!(
        "{}\necho '{{}}' > '{}'",
        run.renews(),
        run.owner.path(".pi/agent/settings.json").display()
    );
    let isolation = run.pi("openai/gpt-6-luna", Some(zeewin2()), &body, 0);
    let said = isolation.made_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("the renewal ran and failed"), "{said}");
    assert!(
        said.contains("it changed the owner's Pi files beside its login: ~/.pi/agent/settings.json is not as it was"),
        "{said}"
    );
    assert_eq!(run.owner.made(), 0);
}

#[test]
fn an_entry_that_lasts_the_run_or_cannot_be_renewed_is_never_renewed() {
    let run = Renewing::new();
    // A key does not end; the run is on `openrouter`.
    let isolation = run.pi(
        "openrouter/meta/muse-spark-1.3",
        Some(zeewin2()),
        &run.renews(),
        0,
    );
    assert!(isolation.made_at(at(NOW)).is_ok());
    assert_eq!(run.started(), None, "Pi was not run");
    // An entry that lasts nine hours.
    write(&run.owner.path(".pi/agent/auth.json"), &auth(hours(9)));
    let isolation = run.pi("openai/gpt-6-luna", Some(zeewin2()), &run.renews(), 0);
    assert!(isolation.made_at(at(NOW)).is_ok());
    assert_eq!(run.started(), None, "Pi was not run");
    // An ended entry with no refresh token: Pi cannot renew it.
    let no_refresh = json!({
        "openai": { "type": "oauth", "access": ACCESS, "expires": at(NOW - 3600) },
    })
    .to_string();
    write(&run.owner.path(".pi/agent/auth.json"), &no_refresh);
    let isolation = run.pi("openai/gpt-6-luna", Some(zeewin2()), &run.renews(), 0);
    let said = isolation.made_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("`openai` has expired"), "{said}");
    assert!(!said.contains("T-1067"), "{said}");
    // A model with no provider has no entry to renew, whatever the owner's file holds.
    let only_ended = json!({
        "openai": { "type": "oauth", "access": ACCESS, "refresh": REFRESH, "expires": at(NOW - 3600) },
    })
    .to_string();
    write(&run.owner.path(".pi/agent/auth.json"), &only_ended);
    let isolation = run.pi("gpt-6-luna", Some(zeewin2()), &run.renews(), 0);
    assert!(isolation.made_at(at(NOW)).is_err());
    assert_eq!(run.started(), None, "Pi was not run");
}

#[test]
fn off_a_test_machine_an_ended_pi_entry_is_not_renewed_but_refused_as_it_always_was_and_nothing_is_run(
) {
    let run = Renewing::new();
    let isolation = run.pi("openai/gpt-6-luna", None, &run.renews(), 0);
    let said = isolation.made_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("`openai` has expired"), "{said}");
    assert!(
        !said.contains("renew it") && !said.contains("T-1067"),
        "{said}"
    );
    assert_eq!(run.started(), None);
    assert_eq!(run.owner.made(), 0);
    // A home made for the machine renews only where the runner said it is on a test machine.
    let home = run.owner.home.path();
    let models = ["openai/gpt-6-luna".to_owned()];
    let on = |machine| Isolation::of_machine_on(machine, Harness::Pi, home, hours(1), &models);
    assert!(on(None).unwrap().unwrap().renewal.is_none());
    assert!(on(Some(zeewin2())).unwrap().unwrap().renewal.is_some());
    // Codex is not covered by the rule: its logins are not renewed anywhere.
    let codex = Isolation::of_machine_on(Some(zeewin2()), Harness::Codex, home, hours(1), &[]);
    assert!(codex.unwrap().unwrap().renewal.is_none());
}

// ---------------------------------------------------------------------- the case's own length (T-1066)

use super::super::PLANNED;

#[test]
fn a_login_must_last_the_cases_planned_length_and_the_margin_whatever_the_harness() {
    assert_eq!(lasting(PLANNED), Duration::from_secs(20 * 60));
    assert_eq!(lasting(hours(1)), Duration::from_secs(70 * 60));
    assert_eq!(
        lasting(Duration::from_secs(90 * 60)),
        Duration::from_secs(100 * 60)
    );
}

#[test]
fn a_pi_login_with_forty_two_minutes_left_runs_a_case_of_ten_minutes_and_not_one_of_an_hour() {
    // T-1066: Pi's `openai` login was refused at 42 minutes left, about an hour after the owner
    // used it, while the kit asked every case for two hours.
    let owner = Owner::new();
    write(
        &owner.path(".pi/agent/auth.json"),
        &auth_ending(ends(Duration::from_secs(42 * 60)), ACCESS),
    );
    let case = owner.pi("openai/gpt-6-luna", PLANNED);
    assert!(case.made_at(at(NOW)).is_ok());
    let longer = owner.pi("openai/gpt-6-luna", hours(1));
    let said = longer.made_at(at(NOW)).unwrap_err().to_string();
    assert!(said.contains("`openai` expires in 42 min"), "{said}");
    assert!(said.contains("at least 1 h 10 min more"), "{said}");
    // The refusal says the harness renews an entry itself once it has ended.
    assert!(
        said.contains("Pi renews an entry itself once it has ended"),
        "{said}"
    );
    assert!(!said_anywhere(&said), "{said}");
}

// ---------------------------------------------------------------------- a copy that cannot renew (T-1067)

#[test]
fn a_pi_run_is_given_the_access_token_and_no_refresh_token_and_the_owners_file_is_held_to_all_of_it(
) {
    // Measured on zeewin2 (Pi 1.4.0): a copy with no refresh token runs; where less than five
    // minutes of its access token are left, Pi tries to renew it, is refused (`invalid_grant`) and
    // the run fails loudly, the owner's file untouched.
    let owner = Owner::new();
    let isolation = owner.pi("openai/gpt-6-luna", hours(1));
    let home = isolation.made_at(at(NOW)).unwrap();
    let held: Value =
        serde_json::from_str(&fs::read_to_string(home.join("pi/agent/auth.json")).unwrap())
            .unwrap();
    assert_eq!(held["openai"]["access"], ACCESS);
    assert_eq!(held["openai"]["type"], "oauth");
    assert!(held["openai"].get("refresh").is_none(), "{held}");
    assert!(!held.to_string().contains(REFRESH));
    // The owner's file is as it was, refresh token and all.
    isolation.finish().unwrap();
    assert!(fs::read_to_string(owner.path(".pi/agent/auth.json"))
        .unwrap()
        .contains(REFRESH));
    // A Pi that failed to renew what it could not, and wrote its copy: said, without the old fear.
    fs::write(
        home.join("pi/agent/auth.json"),
        json!({ "openai": { "type": "oauth", "access": ACCESS, "expires": 1 } }).to_string(),
    )
    .unwrap();
    let said = isolation.finish().unwrap_err().to_string();
    assert!(said.contains("WROTE ITS COPY OF THE LOGIN AGAIN"), "{said}");
    assert!(said.contains("The copy holds no refresh token"), "{said}");
    assert!(!said.contains("may now be stale"), "{said}");
    assert!(!said_anywhere(&said), "{said}");
}

#[test]
fn a_codex_copy_keeps_the_warning_that_it_may_have_used_up_the_machines_refresh_token() {
    let said = said(
        Harness::Codex,
        &[],
        &["the run's Codex wrote a different login".to_owned()],
    );
    assert!(said.contains("may now be stale"), "{said}");
    assert!(!said.contains("holds no refresh token"), "{said}");
}

#[test]
fn a_pi_entry_with_twenty_five_minutes_left_runs_a_case_of_ten_minutes_and_one_with_fifteen_does_not(
) {
    let owner = Owner::new();
    for (left, runs) in [(25, true), (15, false)] {
        write(
            &owner.path(".pi/agent/auth.json"),
            &auth_ending(ends(Duration::from_secs(left * 60)), ACCESS),
        );
        let made = owner.pi("openai/gpt-6-luna", PLANNED).made_at(at(NOW));
        assert_eq!(made.is_ok(), runs, "{left} min left: {made:?}");
    }
}

#[test]
fn a_pi_harness_that_ended_badly_after_it_renewed_its_entry_has_renewed_it_and_the_run_goes_on() {
    let run = Renewing::new();
    let body = format!("{}\necho 'Error: model refused' >&2", run.renews());
    let isolation = run.pi("openai/gpt-6-luna", Some(zeewin2()), &body, 1);
    let home = isolation.made_at(at(NOW)).unwrap();
    let held: Value =
        serde_json::from_str(&fs::read_to_string(home.join("pi/agent/auth.json")).unwrap())
            .unwrap();
    assert_eq!(held["openai"]["access"], RENEWED);
    let renewed = isolation.renewed.lock().unwrap();
    assert_eq!(
        renewed[0].ended.as_deref(),
        Some("it ended 1 (Error: model refused)")
    );
}
