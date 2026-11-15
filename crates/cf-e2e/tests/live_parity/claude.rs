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

use cf_e2e::door::Door;
#[cfg(unix)]
use cf_e2e::door::Grounded;
use cf_e2e::live::ask;
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

/// Copies a Claude tree without walking its links.
fn copy_tree(from: &Path, to: &Path, outside: cf_e2e::door::OutsideLinks) -> Result {
    let meta = match fs::symlink_metadata(from) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(Error::File {
                action: "read",
                path: from.to_owned(),
                source,
            });
        }
    };
    if !meta.is_dir() && !meta.file_type().is_symlink() {
        return Ok(());
    }
    cf_e2e::door::copy_tree_with_links(from, to, &|_| false, outside)
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
    let required_json = |path: &Path, name: &str| -> Result<Map<String, Value>> {
        let failed = || Error::Daemon(format!("Claude parity cannot read or parse {name}"));
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Map::new());
            }
            Err(_) => return Err(failed()),
        };
        serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .ok_or_else(failed)
    };
    let mut settings = required_json(&own.join("settings.json"), "settings.json")?;
    let mut configuration = required_json(own_json, ".claude.json")?;
    cf_e2e::door::make_dir(throwaway)?;
    let mut login = login.clone();
    cf_e2e::live::login::without_refresh(&mut login);
    cf_e2e::door::write(
        throwaway.join(".credentials.json"),
        json!({ "claudeAiOauth": login }).to_string(),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = cf_e2e::door::set_permissions(throwaway, fs::Permissions::from_mode(0o700));
        let _ = cf_e2e::door::set_permissions(
            throwaway.join(".credentials.json"),
            fs::Permissions::from_mode(0o600),
        );
    }
    settings.remove("hooks");
    settings.remove("statusLine");
    let mut settings = Value::Object(settings);
    cf_e2e::live::login::without_refresh(&mut settings);
    cf_e2e::door::write(throwaway.join("settings.json"), settings.to_string())?;
    for part in ["skills", "plugins", "agents", "commands"] {
        let outside = if part == "plugins" {
            cf_e2e::door::OutsideLinks::Refuse
        } else {
            cf_e2e::door::OutsideLinks::Preserve
        };
        copy_tree(&own.join(part), &throwaway.join(part), outside)?;
    }
    rewrite_plugin_paths(&throwaway.join("plugins"), own, throwaway)?;
    let entry = json!({ "hasTrustDialogAccepted": true, "hasCompletedProjectOnboarding": true, "enabledMcpServers": ["computer-use"] });
    let mut projects = Map::new();
    let name = folder.to_string_lossy().into_owned();
    projects.insert(name.replace('\\', "/"), entry.clone());
    projects.insert(name, entry);
    configuration.insert("projects".to_owned(), Value::Object(projects));
    configuration.insert("hasCompletedOnboarding".to_owned(), Value::Bool(true));
    let mut configuration = Value::Object(configuration);
    cf_e2e::live::login::without_refresh(&mut configuration);
    cf_e2e::door::write(throwaway.join(".claude.json"), configuration.to_string())
}

/// Plugin metadata can name either a linked Claude folder or its real path.
fn rewrite_plugin_paths(plugins: &Path, own: &Path, throwaway: &Path) -> Result {
    let mut names = vec![own.to_string_lossy().into_owned()];
    if let Ok(real) = fs::canonicalize(own) {
        let real = real.to_string_lossy().into_owned();
        #[cfg(windows)]
        if let Some(unc) = real.strip_prefix(r"\\?\UNC\") {
            names.push(format!(r"\\{unc}"));
        } else if let Some(plain) = real.strip_prefix(r"\\?\") {
            names.push(plain.to_owned());
        }
        names.push(real);
    }
    #[cfg(windows)]
    names.extend(
        names
            .clone()
            .into_iter()
            .map(|name| name.replace('\\', "/")),
    );
    // Replace the longest spelling first, so one alias cannot consume a prefix of another.
    names.sort_by_key(|name| std::cmp::Reverse(name.len()));
    names.dedup();
    for name in names {
        rename_in_json_under(plugins, &name, &throwaway.to_string_lossy())?;
    }
    Ok(())
}

/// Writes `to` for `from` in every JSON file under `folder`, as JSON writes both.
fn rename_in_json_under(folder: &Path, from: &str, to: &str) -> Result {
    if fs::symlink_metadata(folder).is_ok_and(|meta| meta.file_type().is_symlink())
        || !folder.is_dir()
    {
        return Ok(());
    }
    for entry in fs::read_dir(folder).map_err(|source| Error::File {
        action: "read",
        path: folder.to_owned(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::File {
            action: "read",
            path: folder.to_owned(),
            source,
        })?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|source| Error::File {
            action: "read",
            path: path.clone(),
            source,
        })?;
        if kind.is_dir() {
            rename_in_json_under(&path, from, to)?;
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
        {
            let text = files::read_string(&path)?;
            if let Some(rewritten) = rename_json_strings(&text, from, to)? {
                cf_e2e::door::write(&path, rewritten)?;
            }
        }
    }
    Ok(())
}

/// Replace paths in JSON string tokens, including escaped spellings, without changing other bytes.
fn rename_json_strings(text: &str, from: &str, to: &str) -> Result<Option<String>> {
    let bytes = text.as_bytes();
    let mut next = 0;
    let mut copied = 0;
    let mut rewritten = String::new();
    let mut changed = false;
    while next < bytes.len() {
        if bytes[next] != b'"' {
            next += 1;
            continue;
        }
        let start = next;
        next += 1;
        let mut closed = false;
        while next < bytes.len() {
            match bytes[next] {
                b'\\' => next = (next + 2).min(bytes.len()),
                b'"' => {
                    next += 1;
                    closed = true;
                    break;
                }
                _ => next += 1,
            }
        }
        if !closed {
            break;
        }
        let Ok(value) = serde_json::from_str::<String>(&text[start..next]) else {
            continue;
        };
        if value.contains(from) {
            let value = value.replace(from, to);
            let encoded = serde_json::to_string(&value)
                .map_err(|_| Error::Daemon("Claude parity cannot rewrite a plugin path".into()))?;
            rewritten.push_str(&text[copied..start]);
            rewritten.push_str(&encoded);
            copied = next;
            changed = true;
        }
    }
    if changed {
        rewritten.push_str(&text[copied..]);
        Ok(Some(rewritten))
    } else {
        Ok(None)
    }
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
    cf_e2e::door::make_dir(skill.parent().unwrap_or(&role))?;
    cf_e2e::door::write(
        &skill,
        "# ConsensFlow worker\n\nThe live parity check's role.\n",
    )?;
    // The settings file is the one the adapter writes (a member's, its question tool answered
    // from the board), in a ConsensFlow folder of this run's own, and not a copy of it: the
    // hook in it is run by a `cf` by its whole path, as the daemon's launches say it.
    let home = made.join("consensflow").to_string_lossy().into_owned();
    let launch = format!("{:08x}-4a2b-4c3d-8e4f-0a1b2c3d4e5f", now_ms() & 0xffff_ffff);
    let settings = ask::claude_settings(
        &[("CONSENSFLOW_HOME".to_owned(), home)],
        &launch,
        true,
        &made.join("bin").join("cf"),
    )?;
    let session = format!("{:08x}-1f26-4d34-bb42-4950575e656c", now_ms() & 0xffff_ffff);
    let mut words = settings;
    words.extend(
        [
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
        .map(str::to_owned),
    );
    Ok(words)
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
    let named = format!("pane {}#{}", pane.key.id, pane.key.generation);
    let closed = Door::of_run().close_own_terminal(&named, || {
        let _ = table.kill(&pane.key);
    });
    if closed {
        let _ = drain.join();
    } else {
        // A say-only door left the pane open: the table is not dropped, which would close it
        // (the system hangs a terminal's program up with it), so what the audit says is what
        // the case did.
        std::mem::forget(table);
    }
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
/// the one way a live check ends a process ([`Door::end`]): chosen from this
/// test's own tree of processes by that path, which no other command names. What may
/// still run is the error.
#[cfg(unix)]
fn ended_by_its_debug_log(debug: &Path) -> Result {
    let machine = members::ThisMachine;
    let me = machine
        .identity(std::process::id())?
        .ok_or_else(|| Error::Daemon("this test's own process is not listed".to_owned()))?;
    let all = machine.processes()?;
    let chosen = Grounded::holding(&all, me, &format!("--debug-file {}", debug.display()));
    let problems = Door::of_run().end(&chosen).problems();
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
    cf_e2e::door::make_dir(Path::new(env!("CARGO_TARGET_TMPDIR")))?;
    let made_here = cf_e2e::door::made_in(
        Path::new(env!("CARGO_TARGET_TMPDIR")),
        "live-parity-claude-",
    )
    .map_err(|source| Error::File {
        action: "make a folder in",
        path: Path::new(env!("CARGO_TARGET_TMPDIR")).to_path_buf(),
        source,
    })?;
    let made = made_here.path().to_path_buf();
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
        let _ = Door::of_run().remove_in_made(&config.join(".credentials.json"));
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
    let _ = Door::of_run().remove_in_made(&made.join("config-terminal"));
    let _ = Door::of_run().remove_in_made(&made.join("config-member"));
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

    fn link_dir(from: &Path, to: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(from, to).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(from, to).unwrap();
    }

    fn link_file(from: &Path, to: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(from, to).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(from, to).unwrap();
    }

    fn make_parity_copy(owner: &Path, config: &Path, run: &Path) -> Result {
        throwaway_config(owner, config, run, &json!({"accessToken":"fake"}), run)
    }

    #[test]
    fn kit_round4_missing_settings_is_empty() {
        let root = cf_e2e::door::made("cf-parity-missing-settings-").unwrap();
        let owner = root.path().join("owner");
        fs::create_dir(&owner).unwrap();
        let config = root.path().join("account.json");
        fs::write(&config, "{}").unwrap();
        let run = root.path().join("run");
        make_parity_copy(&owner, &config, &run).expect("missing settings must be accepted");
        assert_eq!(fs::read_to_string(run.join("settings.json")).unwrap(), "{}");
    }

    #[test]
    fn kit_round4_missing_account_is_empty() {
        let root = cf_e2e::door::made("cf-parity-missing-account-").unwrap();
        let owner = root.path().join("owner");
        fs::create_dir(&owner).unwrap();
        fs::write(owner.join("settings.json"), "{}").unwrap();
        let run = root.path().join("run");
        make_parity_copy(&owner, &root.path().join("missing.json"), &run)
            .expect("missing account config must be accepted");
        let config: Value =
            serde_json::from_slice(&fs::read(run.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(config["hasCompletedOnboarding"], true);
    }

    #[test]
    fn kit_round4_linked_settings_is_read() {
        let root = cf_e2e::door::made("cf-parity-linked-settings-").unwrap();
        let real_owner = root.path().join("real-owner");
        fs::create_dir(&real_owner).unwrap();
        let settings = root.path().join("settings.json");
        fs::write(&settings, r#"{"theme":"dark"}"#).unwrap();
        link_file(&settings, &real_owner.join("settings.json"));
        let owner = real_owner;
        let config = root.path().join("account.json");
        fs::write(&config, "{}").unwrap();
        let run = root.path().join("run");
        make_parity_copy(&owner, &config, &run).expect("linked settings must be accepted");
        let copied: Value =
            serde_json::from_slice(&fs::read(run.join("settings.json")).unwrap()).unwrap();
        assert_eq!(copied["theme"], "dark");
    }

    #[test]
    fn kit_round4_linked_config_folder_is_read() {
        let root = cf_e2e::door::made("cf-parity-linked-owner-").unwrap();
        let real_owner = root.path().join("real-owner");
        fs::create_dir(&real_owner).unwrap();
        fs::write(real_owner.join("settings.json"), r#"{"theme":"dark"}"#).unwrap();
        let owner = root.path().join("owner-link");
        link_dir(&real_owner, &owner);
        let config = root.path().join("account.json");
        fs::write(&config, "{}").unwrap();
        let run = root.path().join("run");
        make_parity_copy(&owner, &config, &run).expect("linked config folder must be accepted");
        let copied: Value =
            serde_json::from_slice(&fs::read(run.join("settings.json")).unwrap()).unwrap();
        assert_eq!(copied["theme"], "dark");
    }

    #[test]
    fn kit_round4_linked_skill_keeps_the_real_target() {
        let root = cf_e2e::door::made("cf-parity-linked-skill-").unwrap();
        let owner = root.path().join("owner");
        let skills = owner.join("skills");
        fs::create_dir_all(&skills).unwrap();
        let foreign = tempfile::tempdir().unwrap();
        let shared = foreign.path().join("shared-skill");
        fs::create_dir(&shared).unwrap();
        fs::write(shared.join("SKILL.md"), "fabricated skill").unwrap();
        link_dir(&shared, &skills.join("mine"));
        fs::write(owner.join("settings.json"), "{}").unwrap();
        let config = root.path().join("account.json");
        fs::write(&config, "{}").unwrap();
        let run = root.path().join("run");
        make_parity_copy(&owner, &config, &run).unwrap();
        let copied = run.join("skills/mine");
        assert!(
            fs::symlink_metadata(&copied).is_ok_and(|m| m.file_type().is_symlink()),
            "outside skill must remain a link"
        );
        assert_eq!(
            fs::canonicalize(&copied).unwrap(),
            fs::canonicalize(&shared).unwrap()
        );
        assert_eq!(
            fs::read_to_string(copied.join("SKILL.md")).unwrap(),
            "fabricated skill"
        );
    }

    #[test]
    fn kit_round4_linked_skills_root_keeps_the_real_target() {
        let root = cf_e2e::door::made("cf-parity-linked-root-").unwrap();
        let owner = root.path().join("owner");
        fs::create_dir(&owner).unwrap();
        let foreign = tempfile::tempdir().unwrap();
        let shared = foreign.path().join("shared-skills");
        fs::create_dir(&shared).unwrap();
        link_dir(&shared, &owner.join("skills"));
        fs::write(owner.join("settings.json"), "{}").unwrap();
        let config = root.path().join("account.json");
        fs::write(&config, "{}").unwrap();
        let run = root.path().join("run");
        make_parity_copy(&owner, &config, &run).unwrap();
        let copied = run.join("skills");
        assert!(
            fs::symlink_metadata(&copied).is_ok_and(|m| m.file_type().is_symlink()),
            "linked skills root must remain a link"
        );
        assert_eq!(
            fs::canonicalize(copied).unwrap(),
            fs::canonicalize(shared).unwrap()
        );
    }

    #[test]
    fn kit_round4_external_plugin_link_is_refused_by_name() {
        let root = cf_e2e::door::made("cf-parity-linked-plugin-").unwrap();
        let owner = root.path().join("owner");
        let plugins = owner.join("plugins");
        fs::create_dir_all(&plugins).unwrap();
        let foreign = tempfile::tempdir().unwrap();
        let shared = foreign.path().join("shared-plugin");
        fs::create_dir(&shared).unwrap();
        let link = plugins.join("mine");
        link_dir(&shared, &link);
        fs::write(owner.join("settings.json"), "{}").unwrap();
        let config = root.path().join("account.json");
        fs::write(&config, "{}").unwrap();
        let error = make_parity_copy(&owner, &config, &root.path().join("run"))
            .expect_err("external plugin link must be refused");
        assert!(
            error
                .to_string()
                .contains(&link.to_string_lossy().to_string()),
            "{error}"
        );
    }

    #[test]
    fn kit_round4_linked_account_is_read_and_bad_objects_refuse_generically() {
        let root = cf_e2e::door::made("cf-parity-account-link-").unwrap();
        let owner = root.path().join("owner");
        fs::create_dir(&owner).unwrap();
        fs::write(owner.join("settings.json"), "{}").unwrap();
        let account = root.path().join("real-account");
        fs::write(&account, r#"{"custom":"kept"}"#).unwrap();
        let link = root.path().join("account-link");
        link_file(&account, &link);
        let run = root.path().join("run");
        make_parity_copy(&owner, &link, &run).unwrap();
        let copied: Value =
            serde_json::from_slice(&fs::read(run.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(copied["custom"], "kept");
        for text in ["[]", "{broken"] {
            fs::write(&account, text).unwrap();
            let error = make_parity_copy(&owner, &link, &root.path().join("other")).unwrap_err();
            assert_eq!(
                error.to_string(),
                "Claude parity cannot read or parse .claude.json"
            );
        }
        fs::write(owner.join("settings.json"), "[]").unwrap();
        let error = make_parity_copy(&owner, &link, &root.path().join("other")).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Claude parity cannot read or parse settings.json"
        );
    }

    #[test]
    fn kit_round4_parity_skips_dangling_links_and_retargets_internal_links() {
        let root = cf_e2e::door::made("cf-parity-inside-").unwrap();
        let source = root.path().join("skills");
        fs::create_dir_all(source.join("real")).unwrap();
        fs::write(source.join("real/SKILL.md"), "inside").unwrap();
        link_dir(&source.join("real"), &source.join("inside"));
        link_dir(&root.path().join("missing"), &source.join("dangling"));
        let target = root.path().join("copy");
        copy_tree(&source, &target, cf_e2e::door::OutsideLinks::Preserve).unwrap();
        assert!(fs::symlink_metadata(target.join("inside"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::canonicalize(target.join("inside")).unwrap(),
            fs::canonicalize(target.join("real")).unwrap()
        );
        assert!(fs::symlink_metadata(target.join("dangling")).is_err());
    }

    #[test]
    fn kit_round4_plugin_root_link_is_refused_and_rewriter_skips_links() {
        let root = cf_e2e::door::made("cf-parity-plugin-root-").unwrap();
        let foreign = tempfile::tempdir().unwrap();
        let source = root.path().join("plugins");
        let external = foreign.path().join("plugins");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("paths.json"), r#"{"path":"old"}"#).unwrap();
        link_dir(&external, &source);
        let error = copy_tree(
            &source,
            &root.path().join("copy"),
            cf_e2e::door::OutsideLinks::Refuse,
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains(&source.to_string_lossy().to_string()));
        rename_in_json_under(&source, "old", "new").unwrap();
        assert_eq!(
            fs::read_to_string(external.join("paths.json")).unwrap(),
            r#"{"path":"old"}"#
        );
    }

    #[test]
    fn kit_round4_plugin_paths_use_copy_when_owner_config_is_linked() {
        let root = cf_e2e::door::made("cf-parity-linked-plugin-path-").unwrap();
        let real_owner = root.path().join("real-owner");
        let plugins = real_owner.join("plugins");
        fs::create_dir_all(plugins.join("package")).unwrap();
        let real_path = fs::canonicalize(&plugins).unwrap().join("package");
        let original = json!({"installPath": real_path.to_string_lossy()}).to_string();
        fs::write(plugins.join("installed.json"), &original).unwrap();
        let owner = root.path().join("owner-link");
        link_dir(&real_owner, &owner);
        let run = root.path().join("run");
        make_parity_copy(&owner, &root.path().join("missing.json"), &run).unwrap();
        let copied: Value =
            serde_json::from_slice(&fs::read(run.join("plugins/installed.json")).unwrap()).unwrap();
        assert_eq!(
            copied["installPath"],
            run.join("plugins/package").to_string_lossy().as_ref(),
            "plugin path still reaches the owner's folder"
        );
        assert_eq!(
            fs::read_to_string(plugins.join("installed.json")).unwrap(),
            original
        );
    }

    #[test]
    fn kit_round4_plugin_paths_with_json_escapes_are_rewritten() {
        let root = cf_e2e::door::made("cf-parity-escaped-plugin-").unwrap();
        let owner = root.path().join("owner-escaped");
        let plugins = owner.join("plugins");
        fs::create_dir_all(plugins.join("package")).unwrap();
        let real_path = fs::canonicalize(&plugins).unwrap().join("package");
        let quoted = serde_json::to_string(&real_path.to_string_lossy().to_string())
            .unwrap()
            .replace("owner-escaped", r"\u006fwner-escaped");
        let original = format!(r#"{{"installPath":{quoted}}}"#);
        fs::write(plugins.join("installed.json"), &original).unwrap();
        let run = root.path().join("run");
        make_parity_copy(&owner, &root.path().join("missing.json"), &run).unwrap();
        let copied: Value =
            serde_json::from_slice(&fs::read(run.join("plugins/installed.json")).unwrap()).unwrap();
        assert_eq!(
            copied["installPath"],
            run.join("plugins/package").to_string_lossy().as_ref(),
            "escaped plugin path still reaches the owner's folder"
        );
        assert_eq!(
            fs::read_to_string(plugins.join("installed.json")).unwrap(),
            original
        );
    }

    #[test]
    fn kit_round4_removing_copy_leaves_external_skill_untouched() {
        let root = cf_e2e::door::made("cf-parity-cleanup-").unwrap();
        let foreign = tempfile::tempdir().unwrap();
        fs::write(foreign.path().join("SKILL.md"), "unchanged").unwrap();
        let owner = root.path().join("owner");
        fs::create_dir_all(owner.join("skills")).unwrap();
        link_dir(foreign.path(), &owner.join("skills/mine"));
        let config = root.path().join("missing.json");
        let run = root.path().join("run");
        make_parity_copy(&owner, &config, &run).unwrap();
        let removed = Door::of_run().remove_in_made(&run);
        assert!(!run.exists(), "copy was not removed: {removed:?}");
        assert_eq!(
            fs::read_to_string(foreign.path().join("SKILL.md")).unwrap(),
            "unchanged"
        );
    }

    #[cfg(unix)]
    fn linked_out_parity_tree(part: &str) {
        let root = cf_e2e::door::made("cf-parity-tree-").unwrap();
        let foreign = tempfile::tempdir().unwrap();
        fs::write(
            foreign.path().join("login.json"),
            "fabricated refresh token",
        )
        .unwrap();
        let source = root.path().join(part);
        fs::create_dir(&source).unwrap();
        std::os::unix::fs::symlink(foreign.path().join("login.json"), source.join("leak.json"))
            .unwrap();
        let target = root.path().join("copy");
        let outside = if part == "plugins" {
            cf_e2e::door::OutsideLinks::Refuse
        } else {
            cf_e2e::door::OutsideLinks::Preserve
        };
        let copied = copy_tree(&source, &target, outside);
        if part == "plugins" {
            assert!(copied.is_err(), "plugins accepted a linked-out file");
        } else {
            copied.unwrap();
            let link = target.join("leak.json");
            assert!(
                fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "{part} copied instead of linking the outside file"
            );
            assert_eq!(
                fs::canonicalize(link).unwrap(),
                fs::canonicalize(foreign.path().join("login.json")).unwrap()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn kit_round3_claude_skills_keep_links_out() {
        linked_out_parity_tree("skills");
    }
    #[cfg(unix)]
    #[test]
    fn kit_round3_claude_plugins_refuse_links_out() {
        linked_out_parity_tree("plugins");
    }
    #[cfg(unix)]
    #[test]
    fn kit_round3_claude_agents_keep_links_out() {
        linked_out_parity_tree("agents");
    }
    #[cfg(unix)]
    #[test]
    fn kit_round3_claude_commands_keep_links_out() {
        linked_out_parity_tree("commands");
    }

    fn parity_rejects_bad_config(settings: &str, config: &str) {
        let root = cf_e2e::door::made("cf-parity-invalid-").unwrap();
        let owner = root.path().join("owner");
        fs::create_dir(&owner).unwrap();
        fs::write(owner.join("settings.json"), settings).unwrap();
        fs::write(owner.join(".claude.json"), config).unwrap();
        let result = throwaway_config(
            &owner,
            &owner.join(".claude.json"),
            &root.path().join("run"),
            &json!({"accessToken":"fake"}),
            root.path(),
        );
        assert!(result.is_err(), "invalid parity configuration was accepted");
    }

    #[test]
    fn kit_round3_claude_parity_refuses_invalid_settings() {
        parity_rejects_bad_config("{invalid", "{}");
    }
    #[test]
    fn kit_round3_claude_parity_refuses_invalid_account_config() {
        parity_rejects_bad_config("{}", "[]");
    }

    #[test]
    fn parity_config_copies_no_refresh_keys() {
        let root = cf_e2e::door::made("cf-parity-login-").unwrap();
        let owner = root.path().join("owner");
        let target = root.path().join("run");
        let login = json!({"accessToken":"fake-access","refreshToken":"fake-refresh","nested":[{"REFRESH_key":"fake"}]});
        fs::create_dir(&owner).unwrap();
        fs::write(
            owner.join(".claude.json"),
            r#"{"oauth":{"refreshToken":"fake","accessToken":"kept"}}"#,
        )
        .unwrap();
        fs::write(
            owner.join("settings.json"),
            r#"{"nested":{"refresh_token":"fake","display":"kept"}}"#,
        )
        .unwrap();
        throwaway_config(
            &owner,
            &owner.join(".claude.json"),
            &target,
            &login,
            root.path(),
        )
        .unwrap();
        let copied: Value =
            serde_json::from_str(&fs::read_to_string(target.join(".credentials.json")).unwrap())
                .unwrap();
        assert!(
            copied["claudeAiOauth"].get("refreshToken").is_none(),
            "refresh key was copied"
        );
        assert!(copied["claudeAiOauth"]["nested"][0]
            .get("REFRESH_key")
            .is_none());
        assert_eq!(copied["claudeAiOauth"]["accessToken"], "fake-access");
        let config: Value =
            serde_json::from_slice(&fs::read(target.join(".claude.json")).unwrap()).unwrap();
        assert!(
            config["oauth"].get("refreshToken").is_none(),
            "a refresh key was copied in configuration"
        );
        let settings: Value =
            serde_json::from_slice(&fs::read(target.join("settings.json")).unwrap()).unwrap();
        assert!(settings["nested"].get("refresh_token").is_none());
    }

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
