//! Claude Code's half of `cargo xtask live parity`: a member window has what the same
//! Claude Code loads in a plain terminal. Both use the kit's own Claude config, which holds
//! a copy of the login without refresh keys, and a skill and MCP server of the run's own.
//! Their debug logs say what loaded; the member also has its role's skill.
//!
//! The member is launched with the words ConsensFlow's adapter gives a worker
//! (`crates/cf-harness/tests/goldens/launch/scenarios.*.json`, the first Claude scenario):
//! its settings file, its role folder and prompt, its own session, model, effort and full
//! permission, and the terminal ConsensFlow says it is (`TERM_PROGRAM=ConsensFlow`).
//!
//! Both windows open on a rig's own pane host (`Rig::open_pane`), the one door a live window
//! opens through: the environment each has is the rig's, judged by the seal, with every home the
//! run's own and Claude Code's update switches given (T-2119 X1: the case opened the machine's
//! Claude Code on a pane table of its own, on the test process's whole environment).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

#[cfg(unix)]
use cf_e2e::door::{Door, Grounded};
use cf_e2e::live::ask;
use cf_e2e::live::machine::live_folder_in;
#[cfg(unix)]
use cf_e2e::live::members::{self, Machine};
use cf_e2e::live::{self, Log};
use cf_e2e::rig::{ClaudeConfig, Rig};
use cf_e2e::{files, Error, Result};
use serde_json::{json, Value};

use super::codex_devin::{skill, GLOBAL_SKILL, SERVER};

/// The stand-in for the chief's window: the rig the two windows open on seats it, and no
/// message is given to anyone.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

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

/// Where and as what one window opens: its pane's name, its folder, the run's config folder it
/// loads, the terminal it says it is (`TERM_PROGRAM`) and the debug log it writes.
#[derive(Clone, Copy)]
struct Spot<'a> {
    id: &'a str,
    folder: &'a Path,
    config: &'a Path,
    terminal: Option<&'a str>,
    debug: &'a Path,
}

/// The `pane.open` request for `program` (the argv that starts Claude Code, as `ask::program` says
/// it) with `words` in `folder`, as the terminal `terminal` (what
/// `TERM_PROGRAM` says): the run's own config folder and the words of a terminal, and no other
/// variable. The home variables and the update switches are the rig's pane host's, which
/// [`Rig::open_pane`] judges the whole environment of, with this over it.
fn window_request(program: &[String], words: &[String], spot: &Spot) -> Value {
    let Spot {
        id,
        folder,
        config,
        terminal,
        debug,
    } = *spot;
    let mut argv = program.to_vec();
    argv.extend([
        "--debug-file".to_owned(),
        debug.to_string_lossy().into_owned(),
    ]);
    argv.extend(words.iter().cloned());
    let mut env = serde_json::Map::from_iter([
        (
            "CLAUDE_CONFIG_DIR".to_owned(),
            json!(config.to_string_lossy()),
        ),
        ("TERM".to_owned(), json!("xterm-256color")),
    ]);
    // What a terminal says it is: the plain window's is the system's own terminal (the
    // pane host would say ConsensFlow, which it is not), the member's ConsensFlow's.
    let (program, bundle) = match terminal {
        Some(terminal) => (terminal.to_owned(), "dev.ngvoicu.consensflow"),
        None if cfg!(windows) => (String::new(), ""),
        None => ("Apple_Terminal".to_owned(), "com.apple.Terminal"),
    };
    env.insert("TERM_PROGRAM".to_owned(), json!(program));
    env.insert("COLORTERM".to_owned(), json!("truecolor"));
    if cfg!(target_os = "macos") {
        env.insert("__CFBundleIdentifier".to_owned(), json!(bundle));
    }
    if cfg!(windows) && terminal.is_none() {
        env.insert(
            "WT_SESSION".to_owned(),
            json!("5ac0c0a8-1d0b-4e3a-9a52-0d2c3f1f0001"),
        );
    }
    json!({
        "id": id, "generation": 1, "cwd": folder, "argv": argv,
        "env": env, "dropEnv": [],
        "size": { "rows": 40, "cols": 140 },
    })
}

/// Opens `claude` with `words` in a terminal of ConsensFlow's, in `folder`, on the config
/// `config`, as the terminal `terminal` (what `TERM_PROGRAM` says), on `rig`'s pane host: lets it
/// settle and ends it: what its debug log says it loaded.
fn opened(rig: &Rig, claude: &Path, words: &[String], spot: &Spot) -> Result<Loaded> {
    let (id, debug) = (spot.id, spot.debug);
    let program = ask::program(&[claude.to_string_lossy().into_owned()], rig.environment())?;
    let request = window_request(&program, words, spot);
    let opened = rig.open_pane(request)?;
    if opened["ok"] != true {
        return Err(Error::Daemon(format!("the window did not open: {opened}")));
    }
    std::thread::sleep(SETTLES);
    let screen: Vec<String> = rig
        .host(
            "pane.snapshot",
            json!({ "id": id, "generation": 1, "tail": 40 }),
        )
        .ok()
        .and_then(|answer| answer["tail"].as_array().cloned())
        .map(|lines| {
            lines
                .iter()
                .map(|line| line.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default();
    // Claude and what it started end first, through the descent: an MCP server that starts
    // a session of its own is no process the pane host may signal.
    #[cfg(unix)]
    let ended = ended_by_its_debug_log(debug);
    // The window ends through the rig's pane host, the door's one way to close a pane; a pane that
    // is gone already is no matter.
    let _ = rig.close_pane(json!({ "id": id, "generation": 1 }));
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
    let made_here = work_folder()?;
    let made = made_here.path();
    let login = claude
        .claude_config()
        .ok_or_else(|| Error::Daemon("Claude parity needs the kit's own login copy".to_owned()))?;
    // Finish the kit's login checks even when setup or either window fails.
    let outcome = (|| -> Result {
        // Both windows open on this rig's pane host, which judges each one's whole environment.
        let rig = Rig::start(live::rig(std::slice::from_ref(&claude), FAKE_AGENT))?;
        let compared = compared(&rig, &claude, login.as_ref(), made, log);
        let closed = rig.close();
        compared?;
        closed
    })();
    claude.finish_homes()?;
    outcome?;
    log.say("ok: the member's window loads what the terminal's does, and its role's skill");
    Ok(())
}

/// The two windows on `rig`, and what each loaded, compared.
fn compared(
    rig: &Rig,
    claude: &live::machine::Installed,
    login: &dyn ClaudeConfig,
    made: &Path,
    log: &Log,
) -> Result {
    let home = rig
        .project_home()
        .ok_or_else(|| Error::Daemon("the rig gives its windows no project home".to_owned()))?;
    let folder = live_folder_in(Path::new(home), "parity-claude")?;
    let config = prepare_config(login, &folder)?;
    log.say(format!(
        "Claude Code is {} ({}); both windows open in {} on the kit's login copy",
        claude.command().display(),
        parity_version(claude, &config),
        folder.display()
    ));
    let plain = opened(
        rig,
        claude.command(),
        &["--model".to_owned(), MODEL.to_owned()],
        &Spot {
            id: "parity-terminal",
            folder: &folder,
            config: &config,
            terminal: None,
            debug: &made.join("debug-terminal.txt"),
        },
    )?;
    log.say(format!("terminal: {}", said(&plain)));
    let missing = plain_requirements(&plain);
    if !missing.is_empty() {
        return Err(Error::Daemon(missing.join("; ")));
    }
    let member = opened(
        rig,
        claude.command(),
        &member_words(made)?,
        &Spot {
            id: "parity-member",
            folder: &folder,
            config: &config,
            terminal: Some("ConsensFlow"),
            debug: &made.join("debug-member.txt"),
        },
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
}

fn work_folder() -> Result<cf_e2e::door::Made> {
    cf_e2e::door::made("live-parity-claude-").map_err(|source| Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn t1744_parity_work_folder_is_owned_by_the_run() {
        let folder = work_folder().unwrap();
        assert!(cf_e2e::door::is_made_or_below(folder.path()));
        cf_e2e::door::write(folder.path().join("debug.txt"), "own debug").unwrap();
    }

    /// T-2119 X1: a parity window asks for the run's config folder and a terminal's words and
    /// nothing else of the environment; the homes and the update switches are its rig's pane
    /// host's, which the rig judges whole (`Rig::open_pane`).
    #[test]
    fn a_parity_window_asks_for_the_runs_config_and_a_terminals_words_alone() {
        let allowed = [
            "CLAUDE_CONFIG_DIR",
            "TERM",
            "TERM_PROGRAM",
            "COLORTERM",
            "__CFBundleIdentifier",
            "WT_SESSION",
        ];
        for terminal in [None, Some("ConsensFlow")] {
            let request = window_request(
                &["claude".to_owned()],
                &["--model".to_owned(), MODEL.to_owned()],
                &Spot {
                    id: "parity-x",
                    folder: Path::new("folder"),
                    config: Path::new("run-config"),
                    terminal,
                    debug: Path::new("debug.txt"),
                },
            );
            let env = request["env"].as_object().unwrap();
            for name in env.keys() {
                assert!(allowed.contains(&name.as_str()), "{name} is asked for");
            }
            assert_eq!(env["CLAUDE_CONFIG_DIR"], "run-config");
            assert_eq!(request["dropEnv"], json!([]));
            assert_eq!(
                request["argv"],
                json!(["claude", "--debug-file", "debug.txt", "--model", MODEL])
            );
        }
    }

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
        const CHILD: &str = "CF_T1700_FABRICATED_SKILL_HOME";
        let Some(home) = cf_e2e::process::own_var(CHILD) else {
            let home_root = cf_e2e::door::made("cf-parity-owner-home-").unwrap();
            let home = home_root.path().join("owner");
            cf_e2e::door::make_dir(&home).unwrap();
            let output = cf_e2e::process::case_of(
                std::env::current_exe().unwrap(),
                "claude::tests::t1700_run_config_excludes_owner_and_contains_its_own_items",
                Duration::from_secs(30),
            )
            .var(CHILD, &home)
            .var("HOME", &home)
            .var("USERPROFILE", &home)
            .run()
            .unwrap();
            assert!(
                output.code == Some(0),
                "fabricated owner child failed:\n{}",
                output.output()
            );
            return;
        };
        let home = PathBuf::from(home);
        let root = cf_e2e::door::made("cf-parity-new-config-").unwrap();
        let owner = home.join(".claude");
        let config = root.path().join("run");
        let project = root.path().join("project with spaces");
        cf_e2e::door::make_dir(owner.join("skills/owner-secret")).unwrap();
        cf_e2e::door::write(owner.join("skills/owner-secret/SKILL.md"), "owner skill").unwrap();
        for part in ["plugins", "agents", "commands"] {
            cf_e2e::door::make_dir(owner.join(part)).unwrap();
            cf_e2e::door::write(owner.join(part).join("owner-secret"), "owner content").unwrap();
        }
        // A read and parse of either fabricated owner file would fail.
        cf_e2e::door::write(owner.join("settings.json"), "invalid owner settings").unwrap();
        cf_e2e::door::write(home.join(".claude.json"), "invalid owner config").unwrap();
        let source = json!({"claudeAiOauth": {"accessToken": "fake-access",
            "refreshToken": "fake-refresh", "nested": [{"REFRESH_key": "fake"}]}})
        .to_string();
        cf_e2e::door::make_dir_private(&config).unwrap();
        let login_copy = cf_e2e::live::login::json_copy(&source, "fabricated Claude").unwrap();
        cf_e2e::door::write(config.join(".credentials.json"), login_copy).unwrap();
        cf_e2e::door::write(
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
        cf_e2e::door::write(config.join(".claude.json"), "{}").unwrap();
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
        cf_e2e::door::write(config.join(".claude.json"), "{}").unwrap();
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
        cf_e2e::door::make_dir(home.join(".claude/skills/owner-secret")).unwrap();
        cf_e2e::door::make_dir(home.join("login-source")).unwrap();
        cf_e2e::door::make_dir(&temp).unwrap();
        cf_e2e::door::write(
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
            cf_e2e::door::make_dir(home.join("bin")).unwrap();
            cf_e2e::door::write(home.join("bin/claude"), "stand-in; never executed").unwrap();
            cf_e2e::door::set_permissions(
                home.join("bin/claude"),
                fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
        #[cfg(windows)]
        {
            cf_e2e::door::write(home.join(".claude/settings.json"), "invalid owner settings")
                .unwrap();
            cf_e2e::door::write(home.join(".claude.json"), "invalid owner config").unwrap();
        }
        let credentials = json!({"claudeAiOauth": {"accessToken": "fake-access",
                "refreshToken": "fake-refresh", "expiresAt": now_ms() + 3_600_000,
                "nested": [{"REFRESH_key": "fake"}]}})
        .to_string();
        cf_e2e::door::write(home.join("login-source/.credentials.json"), &credentials).unwrap();
        cf_e2e::door::write(home.join(".claude/.credentials.json"), &credentials).unwrap();
        let child = cf_e2e::process::case_of(
            std::env::current_exe().unwrap(),
            "claude::tests::t1700_actual_kit_login_composes_with_parity_setup",
            Duration::from_secs(30),
        )
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
