//! The terminal's environment in a real member window (T-540, Calliope's review T-590,
//! P1): the daemon alone is given what the app takes of the human's terminal, as the app
//! gives it (an rc's `LANG` and one export, and their names in `CONSENSFLOW_TERMINAL_ENV`);
//! the pane host and its windows start without them, from an environment of their own.
//! A real Claude Code worker is given a one-word task, and its window's own process must
//! have both, as the same Claude opened in that terminal would. Its environment is read
//! with `ps eww` (macOS shows its owner a process's environment), compared by value and
//! said by name only.
//!
//! The run's Claude config is a folder of the kit's own (`CLAUDE_CONFIG_DIR`, given every
//! real Claude window of a rig: a copy of the machine's login and nothing else, removed with
//! the run), so nothing of the machine's `~/.claude` is used.

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::agents;
use cf_e2e::live::members::{self, find, is_over, Members, Plan};
use cf_e2e::live::{text, Harness, Log};
use cf_e2e::process::Run;
use cf_e2e::{Error, Result};
use serde_json::json;

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
    checked(log)
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
