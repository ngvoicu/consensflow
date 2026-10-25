//! Claude Code's half of `cargo xtask live parity`: a window opened as ConsensFlow opens a
//! member's has what the same Claude Code has opened plainly in that folder (T-540). Both
//! run in a terminal of ConsensFlow's own (`cf-panes`, a ConPTY on Windows), in a folder of
//! the run's, on a throwaway Claude config holding a copy of the machine's own (its login,
//! settings without their hooks and status line, skills, plugins, agents and commands, and
//! the user's MCP servers), so that nothing of the machine's `~/.claude` is written. Each
//! writes Claude's debug log (`--debug-file`), which says what it loaded as it started: the
//! MCP servers that connected, the plugins admitted, the skills (`getSkills returning: …`),
//! and Claude in Chrome. The member's must say the same, with one skill more: its role's.
//!
//! The member is launched with the words ConsensFlow's adapter gives a worker
//! (`crates/cf-harness/tests/goldens/launch/scenarios.*.json`, the first Claude scenario):
//! its settings file, its role folder and prompt, its own session, model, effort and full
//! permission, and the terminal ConsensFlow says it is (`TERM_PROGRAM=ConsensFlow`).

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::time::Duration;

use cf_e2e::live::machine::{home, live_folder};
#[cfg(unix)]
use cf_e2e::live::members::{self, Machine};
use cf_e2e::live::{self, Harness, Log};
use cf_e2e::process::{own_var, Run};
use cf_e2e::{files, Error, Result};
use cf_panes::pty::PaneTable;
use portable_pty::PtySize;
use serde_json::{json, Map, Value};

/// How long each window has to start and settle what it loads.
const SETTLES: Duration = Duration::from_secs(45);

/// The least life the login copy must have left, in milliseconds.
pub(crate) const LOGIN_LEFT_MS: i64 = 2 * 60 * 60 * 1000;

/// The cheap model both windows are opened on: neither is given a message, so neither
/// spends a turn.
const MODEL: &str = "haiku";

/// What a Claude window loaded as it started, by its debug log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    /// The MCP servers that connected.
    pub servers: BTreeSet<String>,
    /// The plugins admitted.
    pub plugins: BTreeSet<String>,
    /// The skills, by kind: `skill dir commands`, `plugin skills`, `bundled skills`,
    /// `builtin plugin skills`, the last time it counted them.
    pub skills: Vec<(String, u64)>,
    /// Whether Claude in Chrome was set up.
    pub chrome: bool,
}

/// What `debug` (a Claude debug log) says was loaded.
pub fn loaded(debug: &str) -> Loaded {
    let mut found = Loaded::default();
    for line in debug.lines() {
        if let Some(name) = between(line, "MCP server \"", "\": Successfully connected") {
            found.servers.insert(name.to_owned());
        }
        if let Some(rest) = line.split("plugin.register: ").nth(1) {
            if rest.contains("admitted") {
                if let Some(name) = rest.split(' ').next() {
                    found.plugins.insert(name.to_owned());
                }
            }
        }
        if let Some(counts) = line.split("getSkills returning: ").nth(1) {
            found.skills = counts
                .split(", ")
                .filter_map(|part| {
                    let (number, kind) = part.split_once(' ')?;
                    Some((kind.trim().to_owned(), number.parse().ok()?))
                })
                .collect();
        }
        if line.contains("In-process Chrome MCP server started") {
            found.chrome = true;
        }
    }
    found
}

/// The text of `line` between `open` and `close`.
fn between<'a>(line: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = line.find(open)? + open.len();
    let end = line[start..].find(close)? + start;
    Some(&line[start..end])
}

/// The machine's Claude login as its own `.credentials.json` holds it (`claudeAiOauth`):
/// on macOS from the Keychain, by the user's account; elsewhere from the file in the
/// config folder. Never printed.
pub(crate) fn login(config: &Path) -> Result<Value> {
    let read = |what: &str, text: String| -> Result<Value> {
        let said: Value = serde_json::from_str(&text)
            .map_err(|_| Error::Daemon(format!("{what} holds no login")))?;
        said.get("claudeAiOauth")
            .cloned()
            .ok_or_else(|| Error::Daemon(format!("{what} holds no claude.ai login")))
    };
    if cfg!(target_os = "macos") {
        let user = own_var("USER").unwrap_or_default();
        let found = Run::new("security")
            .args([
                "find-generic-password",
                "-s",
                "Claude Code-credentials",
                "-a",
                &user,
                "-w",
            ])
            .inheriting_env()
            .run()?;
        read("the Keychain", found.stdout)
    } else {
        read(
            ".credentials.json",
            files::read_string(&config.join(".credentials.json"))?,
        )
    }
}

/// The milliseconds since the epoch, now.
pub(crate) fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// Copies the folder `from` into `to`, whole, where it is there.
fn copy_tree(from: &Path, to: &Path) -> Result {
    if !from.is_dir() {
        return Ok(());
    }
    files::make_dir(to)?;
    for entry in fs::read_dir(from).map_err(|source| Error::File {
        action: "read",
        path: from.to_owned(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::File {
            action: "read",
            path: from.to_owned(),
            source,
        })?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else if source.is_file() {
            files::copy(&source, &target)?;
        }
    }
    Ok(())
}

/// A throwaway Claude config at `throwaway`, a copy of the machine's own at `own` that
/// Claude can write in freely: the login (`login`), the settings without their hooks and
/// status line (which would run the person's own commands), the skills, plugins (their
/// paths written to name the copy), agents and commands, and the person's configuration
/// with no project but `folder`'s, trusted and with Claude's computer use on.
fn throwaway_config(
    own: &Path,
    own_json: &Path,
    throwaway: &Path,
    login: &Value,
    folder: &Path,
) -> Result {
    files::make_dir(throwaway)?;
    files::write(
        &throwaway.join(".credentials.json"),
        json!({ "claudeAiOauth": login }).to_string(),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(throwaway, fs::Permissions::from_mode(0o700));
        let _ = fs::set_permissions(
            throwaway.join(".credentials.json"),
            fs::Permissions::from_mode(0o600),
        );
    }
    if let Ok(text) = files::read_string(&own.join("settings.json")) {
        if let Ok(Value::Object(mut settings)) = serde_json::from_str::<Value>(&text) {
            settings.remove("hooks");
            settings.remove("statusLine");
            files::write(
                &throwaway.join("settings.json"),
                Value::Object(settings).to_string(),
            )?;
        }
    }
    for part in ["skills", "plugins", "agents", "commands"] {
        copy_tree(&own.join(part), &throwaway.join(part))?;
    }
    rename_in_json_under(
        &throwaway.join("plugins"),
        &own.to_string_lossy(),
        &throwaway.to_string_lossy(),
    )?;
    let mut configuration = files::read_string(own_json)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let entry = json!({ "hasTrustDialogAccepted": true, "hasCompletedProjectOnboarding": true, "enabledMcpServers": ["computer-use"] });
    let mut projects = Map::new();
    let name = folder.to_string_lossy().into_owned();
    projects.insert(name.replace('\\', "/"), entry.clone());
    projects.insert(name, entry);
    configuration.insert("projects".to_owned(), Value::Object(projects));
    configuration.insert("hasCompletedOnboarding".to_owned(), Value::Bool(true));
    files::write(
        &throwaway.join(".claude.json"),
        Value::Object(configuration).to_string(),
    )
}

/// Writes `to` for `from` in every JSON file under `folder`, as JSON writes both.
fn rename_in_json_under(folder: &Path, from: &str, to: &str) -> Result {
    if !folder.is_dir() {
        return Ok(());
    }
    let (from_json, to_json) = (from.replace('\\', "\\\\"), to.replace('\\', "\\\\"));
    for entry in fs::read_dir(folder).map_err(|source| Error::File {
        action: "read",
        path: folder.to_owned(),
        source,
    })? {
        let path = entry
            .map_err(|source| Error::File {
                action: "read",
                path: folder.to_owned(),
                source,
            })?
            .path();
        if path.is_dir() {
            rename_in_json_under(&path, from, to)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let text = files::read_string(&path)?;
            if text.contains(&from_json) {
                files::write(&path, text.replace(&from_json, &to_json))?;
            }
        }
    }
    Ok(())
}

/// The words ConsensFlow's adapter gives a Claude worker, after the executable: its
/// settings file (as `claude/install.rs` writes it), its role folder and prompt, no
/// snapshot of that prompt, its own session, the model and effort, and full permission.
fn member_words(made: &Path) -> Result<Vec<String>> {
    let role = made.join("role");
    let skill = role
        .join(".claude")
        .join("skills")
        .join("consensflow-worker")
        .join("SKILL.md");
    files::make_dir(skill.parent().unwrap_or(&role))?;
    files::write(
        &skill,
        "# ConsensFlow worker\n\nThe live parity check's role.\n",
    )?;
    let settings = made.join("settings.json");
    files::write(
        &settings,
        json!({
            "permissions": { "defaultMode": "bypassPermissions" },
            "skipDangerousModePermissionPrompt": true,
            "tui": "default",
            "hooks": {
                "PreToolUse": [{ "matcher": "AskUserQuestion", "hooks": [{ "type": "command", "command": "cf hook claude", "timeout": 3600 }] }],
                "Stop": [{ "hooks": [{ "type": "command", "command": "exit 0" }] }],
            },
        })
        .to_string(),
    )?;
    let session = format!("{:08x}-1f26-4d34-bb42-4950575e656c", now_ms() & 0xffff_ffff);
    Ok([
        "--settings",
        &settings.to_string_lossy(),
        "--add-dir",
        &role.to_string_lossy(),
        "--append-system-prompt-file",
        &skill.to_string_lossy(),
        "--system-prompt-snapshot",
        "off",
        "--session-id",
        &session,
        "--model",
        MODEL,
        "--effort",
        "low",
        "--permission-mode",
        "bypassPermissions",
    ]
    .map(str::to_owned)
    .to_vec())
}

/// Opens `claude` with `words` in a terminal of ConsensFlow's, in `folder`, on the config
/// `config`, as the terminal `terminal` (what `TERM_PROGRAM` says), lets it settle and ends
/// it: what its debug log says it loaded.
fn opened(
    claude: &Path,
    words: &[String],
    folder: &Path,
    config: &Path,
    terminal: Option<&str>,
    debug: &Path,
) -> Result<Loaded> {
    let mut argv = vec![
        claude.to_string_lossy().into_owned(),
        "--debug-file".to_owned(),
        debug.to_string_lossy().into_owned(),
    ];
    argv.extend(words.iter().cloned());
    let mut env = HashMap::from([
        (
            "CLAUDE_CONFIG_DIR".to_owned(),
            config.to_string_lossy().into_owned(),
        ),
        ("TERM".to_owned(), "xterm-256color".to_owned()),
    ]);
    // What a terminal says it is: the plain window's is the system's own terminal (the
    // pane host would say ConsensFlow, which it is not), the member's ConsensFlow's.
    let (program, bundle) = match terminal {
        Some(terminal) => (terminal.to_owned(), "dev.ngvoicu.consensflow"),
        None if cfg!(windows) => (String::new(), ""),
        None => ("Apple_Terminal".to_owned(), "com.apple.Terminal"),
    };
    env.insert("TERM_PROGRAM".to_owned(), program);
    env.insert("COLORTERM".to_owned(), "truecolor".to_owned());
    if cfg!(target_os = "macos") {
        env.insert("__CFBundleIdentifier".to_owned(), bundle.to_owned());
    }
    if cfg!(windows) && terminal.is_none() {
        env.insert(
            "WT_SESSION".to_owned(),
            "5ac0c0a8-1d0b-4e3a-9a52-0d2c3f1f0001".to_owned(),
        );
    }
    let table = PaneTable::new();
    let size = PtySize {
        rows: 40,
        cols: 140,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pane = table
        .open(folder, &argv, &env, size)
        .map_err(|failed| Error::Daemon(format!("the window did not open: {failed}")))?;
    // Its output is read and let go, as a terminal reads it.
    let mut reader = pane.reader;
    let drain = std::thread::spawn(move || {
        let mut sink = [0_u8; 8192];
        while matches!(std::io::Read::read(&mut reader, &mut sink), Ok(read) if read > 0) {}
    });
    std::thread::sleep(SETTLES);
    let screen = table.tail(&pane.key, 40).unwrap_or_default();
    // Claude and what it started end first, through the descent: an MCP server that starts
    // a session of its own is no process the pane table may signal (`cf-panes`' test guard).
    #[cfg(unix)]
    let ended = ended_by_its_debug_log(debug);
    let _ = table.kill(&pane.key);
    let _ = drain.join();
    #[cfg(unix)]
    ended?;
    let log = files::read_string(debug).unwrap_or_default();
    if log.is_empty() {
        return Err(Error::Daemon(format!(
            "the window wrote no debug log; its screen:\n{}",
            screen.join("\n")
        )));
    }
    Ok(loaded(&log))
}

/// Ends the Claude this case started with `--debug-file debug`, and what it started, by
/// the one way a live check ends a process ([`members::end_chosen`]): chosen from this
/// test's own tree of processes by that path, which no other command names. What may
/// still run is the error.
#[cfg(unix)]
fn ended_by_its_debug_log(debug: &Path) -> Result {
    let machine = members::ThisMachine;
    let me = machine
        .identity(std::process::id())?
        .ok_or_else(|| Error::Daemon("this test's own process is not listed".to_owned()))?;
    let all = machine.processes()?;
    let chosen = members::holding(&all, me, &format!("--debug-file {}", debug.display()));
    let problems = members::end_chosen(&chosen).problems();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Error::Daemon(problems.join("\n")))
    }
}

/// What of the machine's own Claude config a run must leave as it was: its settings'
/// bytes, the prompts its history holds of the run's folder, and whether it knows the
/// folder. (The history's length is no measure: every Claude of the machine writes it,
/// the session running this case among them.)
fn own_state(own: &Path, own_json: &Path, folder: &Path) -> (Vec<u8>, usize, bool) {
    let folder = folder.to_string_lossy();
    let history = files::read_string(&own.join("history.jsonl")).map_or(0, |text| {
        text.lines().filter(|line| line.contains(&*folder)).count()
    });
    let knows = files::read_string(own_json).is_ok_and(|text| text.contains(&*folder));
    (
        files::read(&own.join("settings.json")).unwrap_or_default(),
        history,
        knows,
    )
}

/// The words of a window's account in the report.
fn said(loaded: &Loaded) -> String {
    let skills: Vec<String> = loaded
        .skills
        .iter()
        .map(|(kind, count)| format!("{count} {kind}"))
        .collect();
    format!(
        "MCP servers {:?}; plugins {:?}; skills {}; Claude in Chrome {}",
        loaded.servers,
        loaded.plugins,
        skills.join(", "),
        if loaded.chrome {
            "set up"
        } else {
            "not set up"
        }
    )
}

/// How the member's account differs from the plain window's, in words; none where it
/// has what the plain one has, and its role's skill besides.
pub fn differences(plain: &Loaded, member: &Loaded) -> Vec<String> {
    let mut found = Vec::new();
    if member.servers != plain.servers {
        found.push(format!(
            "MCP servers {:?} where the terminal's has {:?}",
            member.servers, plain.servers
        ));
    }
    if member.plugins != plain.plugins {
        found.push(format!(
            "plugins {:?} where the terminal's has {:?}",
            member.plugins, plain.plugins
        ));
    }
    if member.chrome != plain.chrome {
        found.push(format!(
            "Claude in Chrome {} where the terminal's {}",
            member.chrome, plain.chrome
        ));
    }
    let count = |loaded: &Loaded, kind: &str| {
        loaded
            .skills
            .iter()
            .find(|(held, _)| held == kind)
            .map(|(_, count)| *count)
    };
    for (kind, _) in &plain.skills {
        let (theirs, ours) = (count(plain, kind), count(member, kind));
        // The role is a skill of the window's own (`--add-dir`), one more of its folders'.
        let expected = if kind == "skill dir commands" {
            theirs.map(|count| count + 1)
        } else {
            theirs
        };
        if ours != expected {
            found.push(format!("{ours:?} {kind} where {expected:?} were meant"));
        }
    }
    if plain.skills.is_empty() {
        found.push("the terminal's window counted no skills".to_owned());
    }
    found
}

pub fn run(log: &Log) -> Result {
    let claude = live::installed(Harness::Claude)?;
    let own = home()?.join(".claude");
    let own_json = home()?.join(".claude.json");
    let folder = live_folder("parity-claude")?;
    let made = Path::new(env!("CARGO_TARGET_TMPDIR")).join("live-parity-claude");
    let _ = fs::remove_dir_all(&made);
    files::make_dir(&made)?;
    let before = own_state(&own, &own_json, &folder);
    let login = login(&own)?;
    let left = login["expiresAt"].as_i64().unwrap_or(0) - now_ms();
    if left < LOGIN_LEFT_MS {
        return Err(Error::Daemon(format!(
            "the machine's Claude login has {} min left: wait for Claude to refresh it",
            left / 60_000
        )));
    }
    log.say(format!(
        "Claude Code is {} ({}); both windows open in {}",
        claude.command().display(),
        claude.version(),
        folder.display()
    ));
    let arms: [(&str, Vec<String>, Option<&str>); 2] = [
        (
            "terminal",
            vec!["--model".to_owned(), MODEL.to_owned()],
            None,
        ),
        ("member", member_words(&made)?, Some("ConsensFlow")),
    ];
    let mut accounts = Vec::new();
    let mut outcome = Ok(());
    for (name, words, terminal) in arms {
        let config = made.join(format!("config-{name}"));
        let result = throwaway_config(&own, &own_json, &config, &login, &folder).and_then(|()| {
            opened(
                claude.command(),
                &words,
                &folder,
                &config,
                terminal,
                &made.join(format!("debug-{name}.txt")),
            )
        });
        // The copy of the login goes with the run, whatever came of it.
        let _ = fs::remove_file(config.join(".credentials.json"));
        match result {
            Ok(loaded) => {
                log.say(format!("{name}: {}", said(&loaded)));
                accounts.push(loaded);
            }
            Err(failed) => {
                outcome = Err(failed);
                break;
            }
        }
    }
    let _ = fs::remove_dir_all(made.join("config-terminal"));
    let _ = fs::remove_dir_all(made.join("config-member"));
    if own_state(&own, &own_json, &folder) != before {
        return Err(Error::Daemon(
            "!!! the machine's own Claude config changed during the run".to_owned(),
        ));
    }
    outcome?;
    let found = differences(&accounts[0], &accounts[1]);
    if !found.is_empty() {
        return Err(Error::Daemon(format!(
            "the member's window lacks what the terminal's has:\n  - {}",
            found.join("\n  - ")
        )));
    }
    log.say("ok: the member's window loads what the terminal's does, and its role's skill");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
2026-10-25T08:46:13.590Z [DEBUG] MCP server \"computer-use\": Successfully connected (transport: stdio) in 871ms
2026-10-25T08:46:13.591Z [DEBUG] MCP server \"claude.ai Claude Docs\": Successfully connected (transport: http) in 90ms
2026-10-25T08:46:13.592Z [DEBUG] plugin.register: cc-plugin-diff (builtin, cc-plugin-diff@builtin), judged by core alone: admitted
2026-10-25T08:46:13.593Z [DEBUG] getSkills returning: 3 skill dir commands, 0 plugin skills, 37 bundled skills, 1 builtin plugin skills
2026-10-25T08:46:13.594Z [DEBUG] getSkills returning: 4 skill dir commands, 0 plugin skills, 37 bundled skills, 1 builtin plugin skills
";

    #[test]
    fn a_debug_log_says_the_servers_plugins_and_last_skills_it_loaded() {
        let found = loaded(LOG);
        assert_eq!(
            found.servers.into_iter().collect::<Vec<_>>(),
            ["claude.ai Claude Docs", "computer-use"]
        );
        assert_eq!(
            found.plugins.into_iter().collect::<Vec<_>>(),
            ["cc-plugin-diff"]
        );
        assert_eq!(found.skills[0], ("skill dir commands".to_owned(), 4));
        assert!(!found.chrome);
    }

    #[test]
    fn a_member_with_its_roles_skill_besides_differs_in_nothing_and_one_without_a_server_does() {
        let plain = loaded(&LOG.replace("4 skill dir", "3 skill dir"));
        let member = loaded(LOG);
        assert_eq!(differences(&plain, &member), Vec::<String>::new());
        let without = loaded(
            &LOG.replace("\"computer-use\"", "\"other\"")
                .replace("4 skill dir", "3 skill dir"),
        );
        let found = differences(&plain, &without);
        assert_eq!(found.len(), 2, "{found:?}");
    }
}
