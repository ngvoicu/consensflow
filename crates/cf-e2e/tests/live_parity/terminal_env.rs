//! The terminal's environment in a real member window (T-540, Calliope's review T-590,
//! P1): the daemon alone is given what the app takes of the human's terminal, as the app
//! gives it (an rc's `LANG` and one export, and their names in `CONSENSFLOW_TERMINAL_ENV`);
//! the pane host and its windows start without them, from an environment of their own.
//! A real Claude Code worker is given a one-word task, and its window's own process must
//! have both, as the same Claude opened in that terminal would. Its environment is read
//! with `ps eww` (macOS shows its owner a process's environment), compared by value and
//! said by name only.
//!
//! The run's Claude config is a folder of its own (`--claude-config <folder>`), into which
//! the case copies the machine's login and nothing else, and from which it takes the copy
//! away at the end, so nothing of the machine's `~/.claude` is used.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::agents;
use cf_e2e::live::machine::home;
use cf_e2e::live::members::{self, find, is_over, Members, Plan};
use cf_e2e::live::{self, text, Harness, Log};
use cf_e2e::process::Run;
use cf_e2e::{files, Error, Result};
use serde_json::json;

use crate::claude::{login, now_ms, LOGIN_LEFT_MS};

/// The stand-in for the chief's window, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the human's rc says, which the app takes: a locale no other part of the run
/// gives (the rig gives every window `en_US.UTF-8`), and an export of its own.
const TAKEN: [(&str, &str); 2] = [("LANG", "en_IE.UTF-8"), ("PARITY_RC_EXPORT", "rc-1")];

/// The variable the app names what it took in (`cf_base::env::TERMINAL_ENV`).
const TERMINAL_ENV: &str = "CONSENSFLOW_TERMINAL_ENV";

/// What the worker is given to do.
const TASK: &str = "Reply with only the word ok.";

/// How long the worker's window has to open, and its task to end.
const OPENS: Duration = Duration::from_secs(180);
const WORK: Duration = Duration::from_secs(300);
const LOOK: Duration = Duration::from_secs(1);

/// The run's own Claude config, `--claude-config`, given the machine's login.
fn claude_config() -> Result<PathBuf> {
    let folder = live::option("claude-config").map(PathBuf::from).ok_or_else(|| {
        Error::Daemon(
            "give the run a Claude config folder of its own: cargo xtask live parity --claude-config <empty folder> -- a_real_claude_member".to_owned(),
        )
    })?;
    files::make_dir(&folder)?;
    let login = login(&home()?.join(".claude"))?;
    let left = login["expiresAt"].as_i64().unwrap_or(0) - now_ms();
    if left < LOGIN_LEFT_MS {
        return Err(Error::Daemon(format!(
            "the machine's Claude login has {} min left: wait for Claude to refresh it",
            left / 60_000
        )));
    }
    files::write(
        &folder.join(".credentials.json"),
        json!({ "claudeAiOauth": login }).to_string(),
    )?;
    files::write(
        &folder.join(".claude.json"),
        json!({ "hasCompletedOnboarding": true }).to_string(),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&folder, fs::Permissions::from_mode(0o700));
        let _ = fs::set_permissions(
            folder.join(".credentials.json"),
            fs::Permissions::from_mode(0o600),
        );
    }
    Ok(folder)
}

/// The value the process `pid` has of `name`, by `ps eww`: none where it has none.
fn of_process(pid: u32, name: &str) -> Result<Option<String>> {
    let shown = Run::new("ps")
        .args(["eww", "-o", "command=", "-p", &pid.to_string()])
        .inheriting_env()
        .run()?
        .stdout;
    let key = format!(" {name}=");
    Ok(shown.find(&key).map(|at| {
        shown[at + key.len()..]
            .split(' ')
            .next()
            .unwrap_or_default()
            .to_owned()
    }))
}

pub fn run(log: &Log) -> Result {
    if !cfg!(target_os = "macos") {
        return Err(Error::Daemon(
            "this case reads a process's environment as macOS shows it".to_owned(),
        ));
    }
    let config = claude_config()?;
    let outcome = checked(log);
    // The copy of the login goes with the run, whatever came of it.
    let _ = fs::remove_file(config.join(".credentials.json"));
    outcome
}

fn checked(log: &Log) -> Result {
    let mut plan = Plan::new("parity-terminal", members::stand_in_chief())
        .with(members::member(Harness::Claude));
    for (name, value) in TAKEN {
        plan = plan.with_daemon_var(name, value);
    }
    let names: Vec<&str> = TAKEN.iter().map(|(name, _)| *name).collect();
    plan = plan.with_daemon_var(TERMINAL_ENV, json!(names).to_string());
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    let agent = agents::id(Harness::Claude);
    members.add(&agent, &[])?;
    let number = members.give(&agent, TASK)?;
    log.say(format!(
        "T-{number} is given; the daemon alone was given {names:?}"
    ));
    let window = find(log, "the worker's window to open", OPENS, LOOK, || {
        Ok(members
            .window_of(&agent)?
            .and_then(|window| members.window_process(&window)))
    })?
    .ok_or_else(|| Error::Daemon("the worker's window never opened".to_owned()))?;
    let mut missing = Vec::new();
    for (name, value) in TAKEN {
        match of_process(window.pid, name)? {
            Some(held) if held == value => log.say(format!(
                "the worker's process has {name}, as the rc says it"
            )),
            Some(_) => missing.push(format!("{name} is another value than the rc's")),
            None => missing.push(format!("{name} is not there")),
        }
    }
    let _ = find(log, "the worker's task to end", WORK, LOOK, || {
        Ok(is_over(&members.task(number)?).then_some(()))
    });
    log.say(format!(
        "T-{number} is {}",
        text(&members.task(number)?["state"])
    ));
    let closed = members.close();
    if !missing.is_empty() {
        return Err(Error::Daemon(format!(
            "the member's window lacks what the human's terminal gives:\n  - {}",
            missing.join("\n  - ")
        )));
    }
    closed?;
    log.say("ok: a real member window has the terminal's LANG and the rc's export");
    Ok(())
}
