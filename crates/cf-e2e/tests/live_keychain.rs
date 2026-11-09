//! Live: what Claude Code leaves in the owner's login Keychain (T-1102). It is opt-in (`cargo xtask
//! live keychain`): it runs the machine's Claude Code, so it is ignored unless asked for and is no
//! part of `check` or of CI. It opens no window, sends no prompt and spends no quota: Claude Code
//! is given a model server of its own that answers 401, and a login that is made up.
//!
//! On a Mac Claude Code keeps what it stores securely (its login, and what MCP servers ask of OAuth)
//! in the login Keychain, under a name made of its config folder's path, so a run on a throwaway
//! folder leaves an item that outlives the folder; and if the folder held a login, the same write
//! carries it into the item and takes it out of the folder ([`cf_e2e::live::keychain`]). The kit
//! keeps its runs off it by starting Claude Code with `--strict-mcp-config`, and a switch is worth
//! what the binary does with it, so this shows it on the Claude Code the machine has, against MCP
//! servers of its own that ask for OAuth (a 401 and the discovery documents; no token anywhere):
//!
//! 1. **The controls**, which must leave an item or the run proves nothing: `claude mcp list` on a
//!    folder with no login; a session on a folder with a made-up login (the item must then hold
//!    it, and the folder's `.credentials.json` must be gone); and a session that is given a
//!    plugin whose server asks for OAuth. Each item is deleted by the name the kit computes for
//!    the folder, and by no other.
//! 2. **The kit's flags** ([`cf_e2e::live::ISOLATION`]): the same session with a made-up login and
//!    servers in the folder's config, in the project's `.mcp.json` and in a plugin: no server is
//!    asked, no item is made, the login stays in the folder.
//! 3. **A probe that cannot take the flag** (`claude mcp list` connects to every server on
//!    purpose), with [`cf_e2e::live::keychain::without_keychain`] first on its `PATH`: the servers
//!    are asked, no item is made, and what Claude Code stored is in the folder.
//! 4. **The guard of the owner's launchers** ([`cf_e2e::live::launcher::Guard`]) watched the whole
//!    run, and finds the Keychain as it was when the run ended: the controls deleted their own.
//!
//! ```text
//! cargo xtask live keychain
//! ```

#![cfg(target_os = "macos")]

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cf_e2e::live::keychain::{self, forget, item_of, without_keychain};
use cf_e2e::live::launcher::{updates_off, Guard};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, Harness, Installed, Log, ISOLATION};
use cf_e2e::process::{own_var, Run};
use cf_e2e::rig::HarnessHome;
use cf_e2e::{Error, Result};

/// How long a Claude Code run is given: it ends in seconds when the model server answers 401.
const LIMIT: Duration = Duration::from_secs(90);

/// A made-up login: nothing of it is the owner's, and nothing of it is valid anywhere.
const FAKE_LOGIN: &str = r#"{"claudeAiOauth":{"accessToken":"FAKE-ACCESS","refreshToken":"FAKE-REFRESH","expiresAt":9999999999999,"scopes":["user:inference"]}}"#;

/// An io error as the run's, saying what it was doing to what.
fn io<'a>(action: &'a str, path: &'a Path) -> impl FnOnce(std::io::Error) -> Error + 'a {
    move |failed| Error::Daemon(format!("could not {action} {}: {failed}", path.display()))
}

/// A server that asks for OAuth and answers nothing else, on 127.0.0.1: a 401 that points at the
/// protected resource's document, the two discovery documents, and a count of what it was asked.
struct Server {
    port: u16,
    asked: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

impl Server {
    fn start() -> Result<Self> {
        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(io("listen on", Path::new("127.0.0.1")))?;
        listener
            .set_nonblocking(true)
            .map_err(io("poll", Path::new("127.0.0.1")))?;
        let port = listener
            .local_addr()
            .map_err(io("name", Path::new("127.0.0.1")))?
            .port();
        let asked = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (count, done) = (Arc::clone(&asked), Arc::clone(&stop));
        thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let count = Arc::clone(&count);
                        thread::spawn(move || answer(stream, port, &count));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(20)),
                }
            }
        });
        Ok(Self { port, asked, stop })
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// What it was asked since it started.
    fn asked(&self) -> usize {
        self.asked.load(Ordering::Relaxed)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Answers one request on `stream` as the server above does.
fn answer(mut stream: TcpStream, port: u16, count: &AtomicUsize) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|rest| rest.trim().to_owned())
        })
        .and_then(|length| length.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; length];
    let _ = stream.read_exact(&mut body);
    count.fetch_add(1, Ordering::Relaxed);
    let path = head
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");
    let base = format!("http://127.0.0.1:{port}");
    let (status, extra, text) = if path.starts_with("/.well-known/oauth-protected-resource") {
        (
            "200 OK",
            String::new(),
            format!(r#"{{"resource":"{base}/mcp","authorization_servers":["{base}"]}}"#),
        )
    } else if path.starts_with("/.well-known/oauth-authorization-server")
        || path.starts_with("/.well-known/openid-configuration")
    {
        (
            "200 OK",
            String::new(),
            format!(
                r#"{{"issuer":"{base}","authorization_endpoint":"{base}/authorize","token_endpoint":"{base}/token","response_types_supported":["code"],"code_challenge_methods_supported":["S256"],"grant_types_supported":["authorization_code","refresh_token"]}}"#
            ),
        )
    } else {
        (
            "401 Unauthorized",
            format!(
                "WWW-Authenticate: Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource\"\r\n"
            ),
            r#"{"error":"unauthorized"}"#.to_owned(),
        )
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n{text}",
        text.len()
    );
    let _ = stream.write_all(reply.as_bytes());
}

/// The throwaway folders of a run, whose Keychain items are deleted (those, by name, that the
/// controls made) when the run ends, however it ends.
struct Folders {
    parent: tempfile::TempDir,
    made: Vec<PathBuf>,
}

impl Folders {
    fn new() -> Result<Self> {
        Ok(Self {
            parent: tempfile::tempdir().map_err(io("make a folder", Path::new("tmp")))?,
            made: Vec::new(),
        })
    }

    /// A config folder with the onboarding answered, the servers of `servers` in its own config
    /// (`<name>` at `<server>/<name>`), and a made-up login where `login` says so.
    fn config(&mut self, name: &str, servers: &[(&str, String)], login: bool) -> Result<PathBuf> {
        let folder = self.parent.path().join(name);
        fs::create_dir_all(&folder).map_err(io("make", &folder))?;
        let mut config =
            serde_json::json!({"hasCompletedOnboarding": true, "hasSeenAutoDefaultNotice": true});
        if !servers.is_empty() {
            config["mcpServers"] = servers
                .iter()
                .map(|(name, url)| {
                    (
                        (*name).to_owned(),
                        serde_json::json!({"type": "http", "url": url}),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
                .into();
        }
        fs::write(folder.join(".claude.json"), config.to_string()).map_err(io("write", &folder))?;
        if login {
            let file = folder.join(".credentials.json");
            fs::write(&file, FAKE_LOGIN).map_err(io("write", &file))?;
        }
        self.made.push(folder.clone());
        Ok(folder)
    }

    /// A project folder, with `.mcp.json` naming the servers of `servers` where there are any.
    fn project(&self, name: &str, servers: &[(&str, String)]) -> Result<PathBuf> {
        let folder = self.parent.path().join(name);
        fs::create_dir_all(&folder).map_err(io("make", &folder))?;
        if !servers.is_empty() {
            let listed: serde_json::Map<_, _> = servers
                .iter()
                .map(|(name, url)| {
                    (
                        (*name).to_owned(),
                        serde_json::json!({"type": "http", "url": url}),
                    )
                })
                .collect();
            fs::write(
                folder.join(".mcp.json"),
                serde_json::json!({"mcpServers": listed}).to_string(),
            )
            .map_err(io("write", &folder))?;
        }
        Ok(folder)
    }

    /// A plugin whose one server is `url`.
    fn plugin(&self, name: &str, url: &str) -> Result<PathBuf> {
        let folder = self.parent.path().join(name);
        fs::create_dir_all(folder.join(".claude-plugin")).map_err(io("make", &folder))?;
        fs::write(
            folder.join(".claude-plugin/plugin.json"),
            r#"{"name":"probe","version":"0.0.1","description":"T-1102"}"#,
        )
        .map_err(io("write", &folder))?;
        fs::write(
            folder.join(".mcp.json"),
            serde_json::json!({"mcpServers": {"plugsrv": {"type": "http", "url": url}}})
                .to_string(),
        )
        .map_err(io("write", &folder))?;
        Ok(folder)
    }
}

impl Drop for Folders {
    fn drop(&mut self) {
        for folder in &self.made {
            let _ = forget(folder);
        }
    }
}

/// What a run of Claude Code came to.
struct Came {
    /// The Keychain items gained while it ran.
    gained: BTreeSet<String>,
    /// What the server was asked while it ran.
    asked: usize,
    /// Whether the folder's `.credentials.json` is there after.
    login_kept: bool,
    /// The folder's item is the one gained, by the name the kit computes.
    named_for_the_folder: bool,
    /// The names of the top-level keys of the item gained, never a value.
    holds: Vec<String>,
}

/// Runs `claude` with `args` in `project`, with `folder` as its config folder, and `model` as the
/// server of its model (which answers 401 to whatever it is asked), and says what the Keychain and
/// the folder came to, and how often `mcp` (the servers that ask for OAuth) was asked. The made-up
/// login is the only secret in play. `front` is put first on the `PATH`.
fn run(
    claude: &Path,
    args: &[&str],
    folder: &Path,
    project: &Path,
    (model, mcp): (&Server, &Server),
    front: Option<&Path>,
) -> Result<Came> {
    let before = keychain::names().unwrap_or_default();
    let asked = mcp.asked();
    let path = front.map_or_else(
        || "/usr/bin:/bin:/usr/sbin:/sbin".to_owned(),
        |front| format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", front.display()),
    );
    let dead = "http://127.0.0.1:9";
    let mut vars: Vec<(String, String)> = vec![
        ("PATH".to_owned(), path),
        (
            "HOME".to_owned(),
            live::machine::home()?.to_string_lossy().into_owned(),
        ),
        ("USER".to_owned(), own_var("USER").unwrap_or_default()),
        (
            "TMPDIR".to_owned(),
            std::env::temp_dir().to_string_lossy().into_owned(),
        ),
        (
            "CLAUDE_CONFIG_DIR".to_owned(),
            folder.to_string_lossy().into_owned(),
        ),
        ("ANTHROPIC_BASE_URL".to_owned(), model.url("")),
        ("DISABLE_TELEMETRY".to_owned(), "1".to_owned()),
        (
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_owned(),
            "1".to_owned(),
        ),
        ("HTTPS_PROXY".to_owned(), dead.to_owned()),
        ("HTTP_PROXY".to_owned(), dead.to_owned()),
        ("NO_PROXY".to_owned(), "127.0.0.1,localhost".to_owned()),
    ];
    // The updater is off here as in every run of the kit: the owner's launcher is the guard's.
    vars.extend(updates_off().map(|(name, value)| (name.to_owned(), value.to_owned())));
    Run::new(claude)
        .args(args)
        .vars(vars)
        .cwd(project)
        .limit(LIMIT)
        .run()?;
    thread::sleep(Duration::from_millis(500));
    let now = keychain::names().unwrap_or_default();
    let gained: BTreeSet<String> = now.difference(&before).cloned().collect();
    let own = item_of(folder);
    Ok(Came {
        named_for_the_folder: gained.contains(&own),
        holds: if gained.contains(&own) {
            shape_of(&own)
        } else {
            Vec::new()
        },
        gained,
        asked: mcp.asked() - asked,
        login_kept: folder.join(".credentials.json").is_file(),
    })
}

/// The names of the top-level keys that the item `name` (made by this case, holding made-up data
/// alone) holds, and the number of entries of each that is an object; never a value.
fn shape_of(name: &str) -> Vec<String> {
    let user = own_var("USER").unwrap_or_default();
    let Ok(ran) = Run::new("/usr/bin/security")
        .args(["find-generic-password", "-s", name, "-a", &user, "-w"])
        .inheriting_env()
        .run()
    else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(ran.stdout.trim()) else {
        return Vec::new();
    };
    value
        .as_object()
        .map(|keys| {
            keys.iter()
                .map(|(key, inner)| match inner.as_object() {
                    Some(entries) => format!("{key}({})", entries.len()),
                    None => key.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What a run came to, in a line for a check's detail.
fn line(came: &Came) -> String {
    format!(
        "server asked {} time(s); items gained: {:?}; the folder's item is among them: {}; it holds {:?}; the folder's login is {}",
        came.asked,
        came.gained,
        came.named_for_the_folder,
        came.holds,
        if came.login_kept { "still there" } else { "gone" }
    )
}

#[test]
#[ignore = "opt-in: runs the machine's Claude Code against servers of its own, spends no quota; cargo xtask live keychain"]
fn claude_code_leaves_no_keychain_item_for_a_throwaway_folder_when_the_kit_starts_it_and_the_control_does(
) -> Result {
    let log = Log::own_agents("live keychain", "it opens no window and runs no model")?;
    let command = live::command("claude")?;
    let home = live::machine::home()?;
    let version = Installed::at(Harness::Claude, &command, &home).version();
    // The binary itself, so that this run follows no link the owner's updater could move.
    let claude = fs::canonicalize(&command).map_err(io("read the link", &command))?;
    log.say(format!("claude is {} ({version})", claude.display()));
    if keychain::names().is_none() {
        return Err(Error::Daemon(
            "the login Keychain cannot be listed here".to_owned(),
        ));
    }
    let guard = Guard::new(Harness::Claude, &command, &home);
    guard.vars()?;
    // One server is the model's (it answers 401 to all), the other is the MCP servers' that ask for OAuth.
    let model = Server::start()?;
    let server = Server::start()?;
    let both = (&model, &server);
    let mut folders = Folders::new()?;
    let mut checks = Vec::new();
    let servers = [
        ("fake-oauth-0", server.url("/mcp-0")),
        ("fake-oauth-1", server.url("/mcp-1")),
    ];
    let session = ["-p", "say ok"];
    let flags: Vec<&str> = session.iter().copied().chain(ISOLATION).collect();

    // 1. The controls.
    let folder = folders.config("control-list", &servers, false)?;
    let project = folders.project("project-control-list", &[])?;
    let came = run(&claude, &["mcp", "list"], &folder, &project, both, None)?;
    checks.push(Check::new(
        "claude",
        "the control: `mcp list` on a folder with no login makes the folder's Keychain item, of the name the kit computes, holding the servers' OAuth state alone",
        came.named_for_the_folder && came.holds == ["mcpOAuth(2)"] && !came.login_kept && came.asked > 0,
        line(&came),
    ));
    let _ = forget(&folder)?;

    let folder = folders.config("control-login", &servers, true)?;
    let project = folders.project("project-control-login", &[])?;
    let came = run(&claude, &session, &folder, &project, both, None)?;
    checks.push(Check::new(
        "claude",
        "the control: a session on a folder with a made-up login makes the item hold the login and the servers' state, and takes the login out of the folder",
        came.named_for_the_folder
            && came.holds.iter().any(|key| key.starts_with("claudeAiOauth("))
            && came.holds.contains(&"mcpOAuth(2)".to_owned())
            && !came.login_kept,
        line(&came),
    ));
    let _ = forget(&folder)?;

    let folder = folders.config("control-plugin", &[], true)?;
    let project = folders.project("project-control-plugin", &[])?;
    let plugin = folders.plugin("plugin-control", &server.url("/plug"))?;
    let plugin_flag = plugin.to_string_lossy().into_owned();
    let with_plugin: Vec<&str> = session
        .iter()
        .copied()
        .chain(["--plugin-dir", plugin_flag.as_str()])
        .collect();
    let came = run(&claude, &with_plugin, &folder, &project, both, None)?;
    checks.push(Check::new(
        "claude",
        "the control: a plugin's server that asks for OAuth makes the item too",
        came.named_for_the_folder && came.asked > 0,
        line(&came),
    ));
    let _ = forget(&folder)?;

    // 2. The kit's flags.
    for (what, in_config, in_project, with_a_plugin) in [
        ("the folder's own servers", true, false, false),
        ("the project's .mcp.json", false, true, false),
        ("a plugin's server", false, false, true),
    ] {
        let name = what.replace(' ', "-").replace(['\'', '.'], "");
        let folder = folders.config(
            &format!("kit-{name}"),
            if in_config { &servers[..] } else { &[] },
            true,
        )?;
        let project = folders.project(
            &format!("project-kit-{name}"),
            if in_project { &servers[..] } else { &[] },
        )?;
        let plugin = folders.plugin(&format!("plugin-kit-{name}"), &server.url("/plug"))?;
        let plugin_flag = plugin.to_string_lossy().into_owned();
        let mut args = flags.clone();
        if with_a_plugin {
            args.extend(["--plugin-dir", plugin_flag.as_str()]);
        }
        let came = run(&claude, &args, &folder, &project, both, None)?;
        checks.push(Check::new(
            "claude",
            &format!("with the kit's flags, a session reaches none of {what}, makes no item, and leaves the login in the folder"),
            came.asked == 0 && came.gained.is_empty() && came.login_kept,
            line(&came),
        ));
        let _ = forget(&folder)?;
    }

    // 3. A probe that cannot take the flag.
    let bare = tempfile::tempdir().map_err(io("make a folder", Path::new("tmp")))?;
    let front = without_keychain(bare.path())?
        .ok_or_else(|| Error::Daemon("this machine has no Keychain to do without".to_owned()))?;
    let folder = folders.config("probe-no-keychain", &servers, false)?;
    let project = folders.project("project-probe", &[])?;
    let came = run(
        &claude,
        &["mcp", "list"],
        &folder,
        &project,
        both,
        Some(&front),
    )?;
    checks.push(Check::new(
        "claude",
        "`mcp list` with a `security` that finds no item connects to the servers, makes no item, and keeps what it stored in the folder",
        came.asked > 0 && came.gained.is_empty() && came.login_kept,
        line(&came),
    ));
    let _ = forget(&folder)?;

    // 4. The guard saw the whole run.
    let closed = guard.finish();
    checks.push(Check::new(
        "claude",
        "the guard of the owner's launchers and Keychain finds both as they were when the run ended",
        closed.is_ok(),
        closed.as_ref().map_or_else(ToString::to_string, |()| "clean".to_owned()),
    ));
    drop(folders);
    report::conclude(&log, "what Claude Code left in the Keychain", &checks)
}
