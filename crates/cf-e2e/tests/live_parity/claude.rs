//! Claude Code's half of `cargo xtask live parity`: a member window has what the same
//! Claude Code loads in a plain terminal. Both use the kit's own Claude config, which holds
//! a copy of the login without refresh keys, and a skill and MCP server of the run's own.
//! Their debug logs say what loaded; the member also has its role's skill.
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
use cf_e2e::live::machine::live_folder;
#[cfg(unix)]
use cf_e2e::live::members::{self, Machine};
use cf_e2e::live::{self, Log};
use cf_e2e::rig::ClaudeConfig;
use cf_e2e::{files, Error, Result};
use cf_panes::pty::PaneTable;
use portable_pty::PtySize;
use serde_json::{json, Value};

use super::codex_devin::{skill, GLOBAL_SKILL, SERVER};

/// How long each window has to start and settle what it loads.
const SETTLES: Duration = Duration::from_secs(45);

/// The cheap model both windows are opened on: neither is given a message, so neither
/// spends a turn.
const MODEL: &str = "haiku";
const SERVER_NAME: &str = "cf-parity";
const ROLE_SKILL: &str = "consensflow-worker";

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
    /// Skills named in Claude's skill prompt.
    pub skill_names: BTreeSet<String>,
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
        if let Some(name) = between(line, "Skill prompt: showing \"", "\"") {
            found.skill_names.insert(name.to_owned());
        }
        if let Some((_, names)) = line.split_once("Skills and commands included in Skill tool:") {
            found.skill_names = names
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
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

/// The milliseconds since the epoch, now.
pub(crate) fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// Add the run's own user skill, MCP server and project trust to the kit's login copy.
/// `login.folder()` makes the private, refresh-free login and fresh onboarding config;
/// no owner config or skill path is an input to this setup.
fn prepare_config(login: &dyn ClaudeConfig, folder: &Path) -> Result<std::path::PathBuf> {
    let config = login.folder()?;
    if !cf_e2e::door::is_made_or_below(&config) {
        return Err(Error::Daemon(
            "Claude parity has no config of the run's own".to_owned(),
        ));
    }
    let file = config.join(".claude.json");
    if fs::symlink_metadata(&file).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(Error::Daemon("Claude parity's config is a link".to_owned()));
    }
    let mut configuration: Value = serde_json::from_str(&files::read_string(&file)?)
        .map_err(|_| Error::Daemon("Claude parity's run config is invalid".to_owned()))?;
    let object = configuration
        .as_object_mut()
        .ok_or_else(|| Error::Daemon("Claude parity's run config is not an object".to_owned()))?;
    object.insert("hasCompletedOnboarding".to_owned(), Value::Bool(true));
    let projects = object.entry("projects").or_insert_with(|| json!({}));
    let projects = projects.as_object_mut().ok_or_else(|| {
        Error::Daemon("Claude parity's run projects are not an object".to_owned())
    })?;
    let project = cf_e2e::live::machine::claude_project_key(&folder.to_string_lossy());
    projects.insert(
        project,
        json!({
            "hasTrustDialogAccepted": true,
            "hasCompletedProjectOnboarding": true
        }),
    );
    let program = config.join("server.cjs");
    object.insert(
        "mcpServers".to_owned(),
        json!({
            (SERVER_NAME): {"type": "stdio", "command": "node", "args": [program]}
        }),
    );
    let skill_dir = config.join("skills").join(GLOBAL_SKILL);
    cf_e2e::door::make_dir(&skill_dir)?;
    cf_e2e::door::write(skill_dir.join("SKILL.md"), skill(GLOBAL_SKILL))?;
    cf_e2e::door::write(&program, SERVER)?;
    // A signed-in Claude can otherwise download the owner's account skills, plugins and
    // connectors into even a fresh config. These settings are the run's own.
    cf_e2e::door::write(
        config.join("settings.json"),
        json!({
            "syncClaudeAiSkills": false,
            "syncClaudeAiPlugins": false,
            "disableClaudeAiConnectors": true
        })
        .to_string(),
    )?;
    cf_e2e::door::write(&file, configuration.to_string())?;
    Ok(config)
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

/// The words of a window's account in the report.
fn said(loaded: &Loaded) -> String {
    let skills: Vec<String> = loaded
        .skills
        .iter()
        .map(|(kind, count)| format!("{count} {kind}"))
        .collect();
    format!(
        "MCP servers {:?}; plugins {:?}; skills {} (named {:?}); Claude in Chrome {}",
        loaded.servers,
        loaded.plugins,
        skills.join(", "),
        loaded.skill_names,
        if loaded.chrome {
            "set up"
        } else {
            "not set up"
        }
    )
}

/// The baseline must prove the run's own skill and server loaded, even if both windows
/// would otherwise agree about their absence.
fn plain_requirements(plain: &Loaded) -> Vec<String> {
    let mut missing = Vec::new();
    if !plain.skill_names.contains(GLOBAL_SKILL) {
        missing.push(format!("the terminal did not load skill {GLOBAL_SKILL}"));
    }
    if !plain.servers.contains(SERVER_NAME) {
        missing.push(format!(
            "the terminal did not connect MCP server {SERVER_NAME}"
        ));
    }
    missing
}

/// How the member's account differs from the plain window's, in words; none where it
/// has what the plain one has, and its role's skill besides.
pub fn differences(plain: &Loaded, member: &Loaded) -> Vec<String> {
    let mut found = plain_requirements(plain);
    for name in plain.skill_names.difference(&member.skill_names) {
        found.push(format!("the member did not load skill {name}"));
    }
    if !member.skill_names.contains(ROLE_SKILL) {
        found.push(format!("the member did not load role skill {ROLE_SKILL}"));
    }
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

fn parity_version(claude: &live::machine::Installed, config: &Path) -> String {
    claude.version_in_claude_config(config)
}

pub fn run(log: &Log) -> Result {
    let claude = live::installed_for_claude_parity(Duration::from_secs(2 * 60 * 60))?;
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
    let made = made_here.path();
    let login = claude
        .claude_config()
        .ok_or_else(|| Error::Daemon("Claude parity needs the kit's own login copy".to_owned()))?;
    // Finish the kit's login checks even when setup or either window fails.
    let outcome = (|| -> Result {
        let config = prepare_config(login.as_ref(), &folder)?;
        log.say(format!(
            "Claude Code is {} ({}); both windows open in {} on the kit's login copy",
            claude.command().display(),
            parity_version(&claude, &config),
            folder.display()
        ));
        let plain = opened(
            claude.command(),
            &["--model".to_owned(), MODEL.to_owned()],
            &folder,
            &config,
            None,
            &made.join("debug-terminal.txt"),
        )?;
        log.say(format!("terminal: {}", said(&plain)));
        let missing = plain_requirements(&plain);
        if !missing.is_empty() {
            return Err(Error::Daemon(missing.join("; ")));
        }
        let member = opened(
            claude.command(),
            &member_words(made)?,
            &folder,
            &config,
            Some("ConsensFlow"),
            &made.join("debug-member.txt"),
        )?;
        log.say(format!("member: {}", said(&member)));
        let found = differences(&plain, &member);
        if !found.is_empty() {
            return Err(Error::Daemon(format!(
                "the member's window lacks what the terminal's has:\n  - {}",
                found.join("\n  - ")
            )));
        }
        Ok(())
    })();
    claude.finish_homes()?;
    outcome?;
    log.say("ok: the member's window loads what the terminal's does, and its role's skill");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(unix)]
    #[test]
    fn t1700_version_probe_uses_the_runs_claude_config() {
        let made = cf_e2e::door::made("cf-parity-version-").unwrap();
        let config = made.path().join("config");
        let program = made.path().join("claude");
        cf_e2e::door::make_dir(&config).unwrap();
        cf_e2e::door::write_executable(
            &program,
            "#!/bin/sh\nif [ \"$CLAUDE_CONFIG_DIR\" = \"$(dirname \"$0\")/config\" ]; then echo run-owned; else echo inherited; fi\n",
        )
        .unwrap();
        let claude = live::machine::Installed::at(live::machine::Harness::Claude, program, "home");
        assert_eq!(parity_version(&claude, &config), "run-owned");
    }

    /// A fabricated stand-in for the kit's ClaudeConfig. It returns the already-seeded
    /// login folder; no Keychain or machine home is consulted by these tests.
    #[derive(Debug)]
    struct FakeLogin {
        config: PathBuf,
        calls: AtomicUsize,
    }

    impl ClaudeConfig for FakeLogin {
        fn folder(&self) -> Result<PathBuf> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.config.clone())
        }
        fn described(&self) -> String {
            "fabricated login".to_owned()
        }
        fn finish(&self) -> Result {
            Ok(())
        }
    }

    #[test]
    fn t1700_run_config_excludes_owner_and_contains_its_own_items() {
        let root = cf_e2e::door::made("cf-parity-new-config-").unwrap();
        let owner = root.path().join("owner");
        let config = root.path().join("run");
        let project = root.path().join("project with spaces");
        fs::create_dir_all(owner.join("skills/owner-secret")).unwrap();
        fs::write(owner.join("skills/owner-secret/SKILL.md"), "owner skill").unwrap();
        for part in ["plugins", "agents", "commands"] {
            fs::create_dir(owner.join(part)).unwrap();
            fs::write(owner.join(part).join("owner-secret"), "owner content").unwrap();
        }
        // A read and parse of either fabricated owner file would fail.
        fs::write(owner.join("settings.json"), "invalid owner settings").unwrap();
        fs::write(owner.join(".claude.json"), "invalid owner config").unwrap();
        let source = json!({"claudeAiOauth": {"accessToken": "fake-access",
            "refreshToken": "fake-refresh", "nested": [{"REFRESH_key": "fake"}]}})
        .to_string();
        cf_e2e::door::make_dir_private(&config).unwrap();
        let login_copy = cf_e2e::live::login::json_copy(&source, "fabricated Claude").unwrap();
        fs::write(config.join(".credentials.json"), login_copy).unwrap();
        fs::write(
            config.join(".claude.json"),
            r#"{"hasCompletedOnboarding":true,"projects":{}}"#,
        )
        .unwrap();
        let login = FakeLogin {
            config: config.clone(),
            calls: AtomicUsize::new(0),
        };
        assert_eq!(prepare_config(&login, &project).unwrap(), config);
        assert_eq!(
            login.calls.load(Ordering::SeqCst),
            1,
            "the kit login was not asked for its folder"
        );
        assert!(
            !config.join("skills/owner-secret").exists(),
            "the owner's skill was copied"
        );
        for part in ["plugins", "agents", "commands"] {
            assert!(!config.join(part).exists(), "the owner's {part} was copied");
        }
        let skill_file = config.join("skills").join(GLOBAL_SKILL).join("SKILL.md");
        assert_eq!(fs::read_to_string(skill_file).unwrap(), skill(GLOBAL_SKILL));
        assert_eq!(
            fs::read_to_string(config.join("server.cjs")).unwrap(),
            SERVER
        );
        let json: Value =
            serde_json::from_slice(&fs::read(config.join(".claude.json")).unwrap()).unwrap();
        let server = &json["mcpServers"][SERVER_NAME];
        assert_eq!(server["type"], "stdio");
        assert_eq!(server["command"], "node");
        assert_eq!(
            server["args"][0],
            config.join("server.cjs").to_string_lossy().as_ref()
        );
        assert_eq!(json["hasCompletedOnboarding"], true);
        let project_key = cf_e2e::live::machine::claude_project_key(&project.to_string_lossy());
        assert_eq!(
            json["projects"][&project_key]["hasTrustDialogAccepted"],
            true
        );
        assert_eq!(
            json["projects"][&project_key]["hasCompletedProjectOnboarding"],
            true
        );
        let credentials: Value =
            serde_json::from_slice(&fs::read(config.join(".credentials.json")).unwrap()).unwrap();
        assert_eq!(credentials["claudeAiOauth"]["accessToken"], "fake-access");
        assert!(credentials["claudeAiOauth"].get("refreshToken").is_none());
        assert!(credentials["claudeAiOauth"]["nested"][0]
            .get("REFRESH_key")
            .is_none());
    }

    #[test]
    fn t1700_plain_missing_skill_fails_by_name() {
        let plain = loaded("MCP server \"cf-parity\": Successfully connected\ngetSkills returning: 0 skill dir commands\n");
        assert!(
            plain_requirements(&plain)
                .join(" ")
                .contains("cf-parity-global"),
            "missing run skill was not named"
        );
    }

    #[test]
    fn t1700_plain_missing_server_fails_by_name() {
        let plain = loaded("Skill prompt: showing \"cf-parity-global\" (userFacingName=\"cf-parity-global\")\nMCP server \"cf-parity\": connection failed\ngetSkills returning: 1 skill dir commands\n");
        assert!(
            plain_requirements(&plain)
                .join(" ")
                .contains("MCP server cf-parity"),
            "missing run server was not named"
        );
    }

    #[test]
    fn t1700_debug_user_skill_list_uses_the_latest_complete_snapshot() {
        let log = "MCP server \"cf-parity\": Successfully connected\n\
                   Skills and commands included in Skill tool: unrelated, cf-parity-global\n\
                   getSkills returning: 1 skill dir commands\n";
        let found = loaded(log);
        assert!(
            plain_requirements(&found).is_empty(),
            "the named user skill was missed"
        );
        let member = loaded("MCP server \"cf-parity\": Successfully connected\n\
            Skills and commands included in Skill tool: unrelated, cf-parity-global, consensflow-worker\n\
            getSkills returning: 2 skill dir commands\n");
        assert!(differences(&found, &member).is_empty());
        let vanished = loaded(&format!(
            "{log}Skills and commands included in Skill tool: cf-parity-global-other\n"
        ));
        assert!(
            plain_requirements(&vanished)
                .join(" ")
                .contains(GLOBAL_SKILL),
            "a missing or merely similar skill passed"
        );
    }

    #[test]
    fn t1700_config_refuses_a_folder_the_run_did_not_make() {
        let login = FakeLogin {
            config: std::env::temp_dir(),
            calls: AtomicUsize::new(0),
        };
        assert!(prepare_config(&login, Path::new("project"))
            .unwrap_err()
            .to_string()
            .contains("run's own"));
    }

    #[test]
    fn t1700_run_config_disables_account_skill_and_plugin_sync() {
        let root = cf_e2e::door::made("cf-parity-sync-").unwrap();
        let config = root.path().join("config");
        cf_e2e::door::make_dir_private(&config).unwrap();
        fs::write(config.join(".claude.json"), "{}").unwrap();
        let login = FakeLogin {
            config: config.clone(),
            calls: AtomicUsize::new(0),
        };
        prepare_config(&login, root.path()).unwrap();
        let settings: Value =
            serde_json::from_slice(&fs::read(config.join("settings.json")).unwrap()).unwrap();
        assert_eq!(
            settings,
            json!({"syncClaudeAiSkills": false, "syncClaudeAiPlugins": false, "disableClaudeAiConnectors": true})
        );
    }

    #[test]
    fn t1700_run_config_disables_account_connectors() {
        let root = cf_e2e::door::made("cf-parity-account-connectors-").unwrap();
        let config = root.path().join("config");
        cf_e2e::door::make_dir_private(&config).unwrap();
        fs::write(config.join(".claude.json"), "{}").unwrap();
        let login = FakeLogin {
            config: config.clone(),
            calls: AtomicUsize::new(0),
        };
        prepare_config(&login, root.path()).unwrap();
        let settings: Value =
            serde_json::from_slice(&fs::read(config.join("settings.json")).unwrap()).unwrap();
        assert_eq!(
            settings["disableClaudeAiConnectors"], true,
            "account connectors were not disabled"
        );
    }

    // Login::folder lists the real Keychain on macOS even for a fabricated login.
    // On other platforms this child has a fabricated HOME and temp folder and exercises
    // the actual kit login together with the parity setup, without touching a real home.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn t1700_actual_kit_login_composes_with_parity_setup() {
        const CHILD: &str = "CF_T1700_FAKE_HOME";
        if let Some(home) = cf_e2e::process::own_var(CHILD) {
            let home = PathBuf::from(home);
            #[cfg(unix)]
            let login = live::installed_for_claude_parity(Duration::from_secs(60))
                .unwrap()
                .claude_config()
                .unwrap()
                .clone();
            #[cfg(windows)]
            let login: std::sync::Arc<dyn ClaudeConfig> = std::sync::Arc::new(
                cf_e2e::live::login::Login::of_folder(
                    &home.join("login-source"),
                    Duration::from_secs(60),
                )
                .unwrap()
                .without_owner_config_reads(),
            );
            let config = prepare_config(login.as_ref(), &home.join("project")).unwrap();
            let credentials: Value =
                serde_json::from_slice(&fs::read(config.join(".credentials.json")).unwrap())
                    .unwrap();
            assert_eq!(credentials["claudeAiOauth"]["accessToken"], "fake-access");
            assert!(credentials["claudeAiOauth"].get("refreshToken").is_none());
            assert!(credentials["claudeAiOauth"]["nested"][0]
                .get("REFRESH_key")
                .is_none());
            assert!(config
                .join("skills")
                .join(GLOBAL_SKILL)
                .join("SKILL.md")
                .is_file());
            assert!(!config.join("skills/owner-secret").exists());
            let config_json: Value =
                serde_json::from_slice(&fs::read(config.join(".claude.json")).unwrap()).unwrap();
            assert_eq!(config_json["mcpServers"][SERVER_NAME]["command"], "node");
            login.finish().unwrap();
            return;
        }
        let root = cf_e2e::door::made("cf-parity-actual-login-").unwrap();
        let home = root.path().join("owner");
        let temp = root.path().join("temp");
        fs::create_dir_all(home.join(".claude/skills/owner-secret")).unwrap();
        fs::create_dir_all(home.join("login-source")).unwrap();
        fs::create_dir(&temp).unwrap();
        fs::write(
            home.join(".claude/skills/owner-secret/SKILL.md"),
            "owner skill",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [
                home.join(".claude/settings.json"),
                home.join(".claude.json"),
            ] {
                nix::unistd::mkfifo(&path, nix::sys::stat::Mode::S_IRUSR).unwrap();
            }
            fs::create_dir(home.join("bin")).unwrap();
            fs::write(home.join("bin/claude"), "stand-in; never executed").unwrap();
            fs::set_permissions(home.join("bin/claude"), fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        #[cfg(windows)]
        {
            fs::write(home.join(".claude/settings.json"), "invalid owner settings").unwrap();
            fs::write(home.join(".claude.json"), "invalid owner config").unwrap();
        }
        let credentials = json!({"claudeAiOauth": {"accessToken": "fake-access",
                "refreshToken": "fake-refresh", "expiresAt": now_ms() + 3_600_000,
                "nested": [{"REFRESH_key": "fake"}]}})
        .to_string();
        fs::write(home.join("login-source/.credentials.json"), &credentials).unwrap();
        fs::write(home.join(".claude/.credentials.json"), &credentials).unwrap();
        let child = cf_e2e::process::case_of(
            std::env::current_exe().unwrap(),
            "claude::tests::t1700_actual_kit_login_composes_with_parity_setup",
            Duration::from_secs(30),
        )
        .inheriting_env()
        .var(CHILD, &home)
        .var("HOME", &home)
        .var("USERPROFILE", &home)
        .var("TMPDIR", &temp)
        .var("TMP", &temp)
        .var("TEMP", &temp)
        .var("CONSENSFLOW_LIVE_CLAUDE_CONFIG", "");
        #[cfg(unix)]
        let child = child.var("PATH", home.join("bin"));
        let output = child.run().unwrap();
        assert!(
            output.code == Some(0),
            "fabricated login child failed:\n{}",
            output.output()
        );
    }

    const LOG: &str = "\
2026-10-25T08:46:13.590Z [DEBUG] MCP server \"cf-parity\": Successfully connected (transport: stdio) in 871ms
2026-10-25T08:46:13.591Z [DEBUG] MCP server \"claude.ai Claude Docs\": Successfully connected (transport: http) in 90ms
2026-10-25T08:46:13.592Z [DEBUG] plugin.register: cc-plugin-diff (builtin, cc-plugin-diff@builtin), judged by core alone: admitted
2026-10-25T08:46:13.593Z [DEBUG] getSkills returning: 1 skill dir commands, 0 plugin skills, 37 bundled skills, 1 builtin plugin skills
2026-10-25T08:46:13.594Z [DEBUG] Skill prompt: showing \"cf-parity-global\" (userFacingName=\"cf-parity-global\")
";

    #[test]
    fn a_debug_log_says_the_servers_plugins_and_last_skills_it_loaded() {
        let found = loaded(LOG);
        assert_eq!(
            found.servers.into_iter().collect::<Vec<_>>(),
            ["cf-parity", "claude.ai Claude Docs"]
        );
        assert_eq!(
            found.plugins.into_iter().collect::<Vec<_>>(),
            ["cc-plugin-diff"]
        );
        assert_eq!(found.skills[0], ("skill dir commands".to_owned(), 1));
        assert_eq!(
            found.skill_names.into_iter().collect::<Vec<_>>(),
            [GLOBAL_SKILL]
        );
        assert!(!found.chrome);
    }

    #[test]
    fn member_requires_plain_items_and_its_role_skill() {
        let plain = loaded(LOG);
        let member = loaded(&format!("{}\nSkill prompt: showing \"consensflow-worker\" (userFacingName=\"consensflow-worker\")\ngetSkills returning: 2 skill dir commands, 0 plugin skills, 37 bundled skills, 1 builtin plugin skills\n", LOG));
        assert!(plain_requirements(&plain).is_empty());
        assert_eq!(differences(&plain, &member), Vec::<String>::new());
        let without_role = loaded(&LOG.replace("1 skill dir", "2 skill dir"));
        assert!(differences(&plain, &without_role)
            .join(" ")
            .contains(ROLE_SKILL));
        let without_global = loaded("MCP server \"cf-parity\": Successfully connected\nSkill prompt: showing \"consensflow-worker\"\ngetSkills returning: 2 skill dir commands\n");
        assert!(differences(&plain, &without_global)
            .join(" ")
            .contains(GLOBAL_SKILL));
        let without_server = loaded(&format!("{}\nSkill prompt: showing \"consensflow-worker\"\ngetSkills returning: 2 skill dir commands\n", LOG.replace("Successfully connected", "connection failed")));
        assert!(differences(&plain, &without_server)
            .join(" ")
            .contains("MCP servers"));
    }
}
