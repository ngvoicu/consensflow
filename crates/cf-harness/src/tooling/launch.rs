//! A member's Devin or Pi window as the adapter prepares it. Devin's: its
//! `--config` (the owner's, with the hooks ConsensFlow adds to it), its first
//! message in a prompt file, the conversation it goes on with, and the variables
//! its launch gives it. Pi's: its extension, its role skill, the texts it appends
//! to Pi's system prompt (the user's own `APPEND_SYSTEM.md` among them) and the
//! variables of its channel.
//! `start` answers the flags of a bare window, which is all a check of the window
//! needs; a check of what a member's window does with the launch the daemon gives
//! it needs the launch whole, so the adapter prepares it here as the daemon has it
//! do (`Adapter::prepare`). It runs on the machine's own programs: Devin is asked
//! its version, once, as it is at every launch, and the launch's files are written
//! in the ConsensFlow home the question's environment names.

use std::path::Path;
use std::rc::Rc;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use serde_json::{json, Map, Value};

use super::{env, optional, text};
use crate::contract::{Agent, Launch, LaunchId};
use crate::launch::adapter;
use crate::seams::{Bundle, Services, SystemProcesses};
use crate::testing::{Fakes, FIRST_MESSAGE_MS};

/// The bundle whose `cf` is the program at `cf`: the folder it is in, and the
/// name a window gives it (forward slashes, which Git Bash keeps).
fn bundle_of(cf: &Path) -> Bundle {
    Bundle {
        bin: cf.parent().map(Path::to_path_buf).unwrap_or_default(),
        cf: cf.to_path_buf(),
        pane_cf: cf.to_string_lossy().replace('\\', "/"),
    }
}

/// What the adapter is given to prepare a launch with: the fakes the other
/// questions are answered with, but for the programs it runs (the system's, which
/// Devin's version is asked of) and the bundle (the question's `cf`).
fn system_services(env: &Env, cf: &Path) -> Services {
    Services {
        processes: Rc::new(SystemProcesses::new(Env::from_process())),
        bundle: bundle_of(cf),
        ..Fakes::new(env).services(env, Path::new(""))
    }
}

/// The Devin window of a member the question names, as the daemon opens it: the
/// program the adapter found, its arguments, the variables the launch adds and the
/// ones it drops, and the conversation it names (the one resumed, none for a new
/// one: Devin names its own).
pub(super) fn devin_launch(question: &Value) -> Result<Value, String> {
    let services = system_services(&env(question)?, Path::new(text(question, "cf")?));
    prepared(Harness::Devin, question, &services)
}

/// The Pi window of a member the question names, as the daemon opens it: the
/// program the adapter found, its extension, role skill and system prompt
/// texts, and the variables of its channel. Pi is asked nothing to prepare it.
pub(super) fn pi_launch(question: &Value) -> Result<Value, String> {
    let env = env(question)?;
    prepared(
        Harness::Pi,
        question,
        &Fakes::new(&env).services(&env, Path::new("")),
    )
}

/// What the daemon's readiness reads of a Devin window's wire log (the question's
/// `wire`): the conversation it shows, the mode that conversation is in, and the
/// command the window is given before it is ready for work, where it is given one
/// (`/bypass`, where the wire says the normal mode).
pub(super) fn devin_ready(question: &Value) -> Result<Value, String> {
    let read = crate::devin::wire_readiness(Path::new(text(question, "wire")?))?;
    Ok(json!({ "shown": read.shown, "mode": read.mode, "command": read.command }))
}

/// Whether Devin's store of the question's `env` says the TUI of a window on `session`
/// took the text `pasted`, handed over at `atMs` (a history row of it, written since), as
/// the engine asks the window after the paste; and how many history rows the session has.
pub(super) fn devin_took(question: &Value) -> Result<Value, String> {
    let at_ms = question
        .get("atMs")
        .and_then(Value::as_i64)
        .ok_or("the question names no atMs")?;
    let (took, prompts) = crate::devin::store_took(
        &super::env(question)?,
        text(question, "session")?,
        text(question, "pasted")?,
        at_ms,
    );
    Ok(json!({ "took": took, "prompts": prompts }))
}

/// The hooks ConsensFlow adds to the owner's in a Devin launch (`member`: a member's,
/// whose question tool is answered from the board; else the chief's), its session hook
/// run by the question's `cf`: each event with the entries added, as the launch's config
/// has them written.
pub(super) fn devin_hooks(question: &Value) -> Result<Value, String> {
    let cf = bundle_of(Path::new(text(question, "cf")?)).pane_cf;
    let member = question
        .get("member")
        .and_then(Value::as_bool)
        .ok_or("the question does not say whether the launch is a member's")?;
    let mut hooks = Map::new();
    for (event, entry) in crate::devin::added_hooks(&cf, member) {
        hooks.insert(
            event.to_owned(),
            json!(entry.into_iter().collect::<Vec<_>>()),
        );
    }
    Ok(Value::Object(hooks))
}

/// The launch the question names, prepared by `harness`'s adapter over `services`.
fn prepared(harness: Harness, question: &Value, services: &Services) -> Result<Value, String> {
    let id = LaunchId::new(text(question, "launch")?)
        .ok_or("the question's launch is not a launch id (a lowercase uuid)")?;
    let agent = question.get("agent").ok_or("the question names no agent")?;
    let launch = Launch {
        id: &id,
        project: 0,
        handle: "live",
        role: text(question, "role")?,
        directory: text(question, "directory")?,
        resume: optional(question, "resume"),
        previous_gone: false,
        message: optional(question, "message"),
        agent: Some(Agent {
            model: optional(agent, "model"),
            effort: optional(agent, "effort"),
            thinking: optional(agent, "thinking"),
            designer: false,
        }),
        instructions: text(question, "instructions")?,
        specs: None,
        first_message_ms: FIRST_MESSAGE_MS,
        starts_given_up: 0,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|failed| failed.to_string())?;
    let adapter = adapter(harness, services);
    let prepared = runtime.block_on(adapter.prepare(&launch))?;
    let (command, args) = prepared
        .argv
        .split_first()
        .ok_or("the adapter prepared no command")?;
    let vars: Map<String, Value> = prepared
        .env
        .iter()
        .map(|(name, value)| (name.clone(), json!(value)))
        .collect();
    Ok(json!({
        "command": command,
        "args": args,
        "env": vars,
        "dropEnv": prepared.drop_env,
        "session": prepared.native_session,
    }))
}

#[cfg(test)]
mod tests;
