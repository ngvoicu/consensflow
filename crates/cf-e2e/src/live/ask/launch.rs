//! A member's Devin or Pi window whole, asked of the adapter: `start` answers the flags
//! of a bare window, and the window the daemon opens for a member has more. Its
//! `--config` is the owner's with ConsensFlow's hooks, its first message is in a
//! prompt file, and its launch adds the variables that name its wire log and its
//! role. The adapter writes the launch's files as it prepares it, under the
//! ConsensFlow home the environment names. And what the daemon's readiness makes of
//! such a window: the mode its wire log says, and what the window is given first.

use std::path::Path;

use serde_json::{json, Value};

use super::{env_object, question, start_of, Start};
use crate::{Error, Result};

/// What the daemon's readiness reads of a Devin window's wire log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevinReady {
    /// The conversation the window shows, once the log said.
    pub shown: Option<String>,
    /// The permission mode it is in, once the log said.
    pub mode: Option<String>,
    /// What the window is given before it is ready for work, where it is given one.
    pub command: Option<String>,
}

/// What the daemon's readiness reads of the Devin window whose wire log is `wire`.
pub fn devin_ready(wire: &Path) -> Result<DevinReady> {
    let answer = question(&json!({ "op": "devin_ready", "wire": wire }))?;
    let text = |name: &str| answer[name].as_str().map(str::to_owned);
    Ok(DevinReady {
        shown: text("shown"),
        mode: text("mode"),
        command: text("command"),
    })
}

/// What the daemon reads of a paste in Devin's store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevinTook {
    /// The TUI took the paste (`Window::took`): a history row of it, written since.
    pub took: bool,
    /// How many history rows the session has.
    pub prompts: u64,
}

/// What Devin's store, in the places `env` names, says of `pasted`, the text a window on
/// `session` was given as it was pasted at `at_ms`, as the daemon reads the window after
/// a paste.
pub fn devin_took(
    env: &[(String, String)],
    session: &str,
    pasted: &str,
    at_ms: i64,
) -> Result<DevinTook> {
    let answer = question(&json!({
        "op": "devin_took", "env": env_object(env), "session": session,
        "pasted": pasted, "atMs": at_ms,
    }))?;
    match (answer["took"].as_bool(), answer["prompts"].as_u64()) {
        (Some(took), Some(prompts)) => Ok(DevinTook { took, prompts }),
        _ => Err(Error::Daemon(format!(
            "harness-ask said nothing of a paste taken: {answer}"
        ))),
    }
}

/// The hooks ConsensFlow adds to the owner's in a Devin launch (`member`'s, or the
/// chief's), its session hook run by `cf`: each event with the entries added, as the
/// launch's config has them written.
pub fn devin_hooks(cf: &Path, member: bool) -> Result<Value> {
    question(&json!({ "op": "devin_hooks", "cf": cf, "member": member }))
}

/// The owner's own config of Devin, as the adapter reads it for a launch, in the config
/// folder `env` names: its comments and trailing commas left out, and an empty one where
/// there is none.
pub fn devin_config(env: &[(String, String)]) -> Result<Value> {
    question(&json!({ "op": "devin_config", "env": env_object(env) }))
}

/// What the adapter is asked to prepare a member's Devin window from.
#[derive(Debug, Clone, Copy)]
pub struct DevinLaunch<'a> {
    /// The launch's id, a lowercase uuid: the folder of its files is named by it,
    /// and each launch has one of its own.
    pub id: &'a str,
    /// What the window is of: `worker`, `reviewer`…
    pub role: &'a str,
    /// The folder it opens in.
    pub directory: &'a Path,
    /// The conversation it goes on with, where it is opened again.
    pub resume: Option<&'a str>,
    /// Its first message, which Devin reads from a prompt file.
    pub message: Option<&'a str>,
    /// The agent it runs (`kind`, `model`, `effort`).
    pub agent: &'a Value,
    /// The role's text, which a session of the window starts with.
    pub instructions: &'a str,
    /// The `cf` its session hook runs.
    pub cf: &'a Path,
    /// The environment the launch is prepared in: the ConsensFlow home among it.
    pub env: &'a [(String, String)],
}

impl DevinLaunch<'_> {
    /// The question as `harness-ask` is asked it.
    fn question(&self) -> Value {
        json!({
            "op": "devin_launch",
            "launch": self.id,
            "role": self.role,
            "directory": self.directory,
            "resume": self.resume,
            "message": self.message,
            "agent": self.agent,
            "instructions": self.instructions,
            "cf": self.cf,
            "env": env_object(self.env),
        })
    }
}

/// The window the adapter prepares: its command is the program the adapter found
/// (and not the harness's name, as `start` has it), its arguments follow it, and
/// its variables are what the launch adds to the window's. The launch's files are
/// written, so each window is asked for with an id of its own.
pub fn devin_launch(launch: &DevinLaunch) -> Result<Start> {
    let answer = question(&launch.question())?;
    start_of(&answer)
        .ok_or_else(|| Error::Daemon(format!("harness-ask prepared no Devin window: {answer}")))
}

/// What the adapter is asked to prepare a member's Pi window from.
#[derive(Debug, Clone, Copy)]
pub struct PiLaunch<'a> {
    /// The launch's id, a lowercase uuid, as [`DevinLaunch::id`].
    pub id: &'a str,
    /// What the window is of: `worker`, `reviewer`…
    pub role: &'a str,
    /// The folder it opens in, where Pi finds the project's own files.
    pub directory: &'a Path,
    /// The conversation it goes on with, where it is opened again.
    pub resume: Option<&'a str>,
    /// Its first message, the last word of its command line.
    pub message: Option<&'a str>,
    /// The agent it runs (`kind`, `model`, `thinking`).
    pub agent: &'a Value,
    /// The role's text: its skill, and appended to Pi's system prompt.
    pub instructions: &'a str,
    /// The environment the launch is prepared in: the ConsensFlow home, and the
    /// home whose Pi folder the adapter reads, among it.
    pub env: &'a [(String, String)],
}

impl PiLaunch<'_> {
    /// The question as `harness-ask` is asked it.
    fn question(&self) -> Value {
        json!({
            "op": "pi_launch",
            "launch": self.id,
            "role": self.role,
            "directory": self.directory,
            "resume": self.resume,
            "message": self.message,
            "agent": self.agent,
            "instructions": self.instructions,
            "env": env_object(self.env),
        })
    }
}

/// The Pi window the adapter prepares, as [`devin_launch`] answers Devin's: its
/// extension and role skill are written under the ConsensFlow home, so each
/// window is asked for with an id of its own.
pub fn pi_launch(launch: &PiLaunch) -> Result<Start> {
    let answer = question(&launch.question())?;
    start_of(&answer)
        .ok_or_else(|| Error::Daemon(format!("harness-ask prepared no Pi window: {answer}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_adapter_is_asked_for_the_launch_with_all_that_makes_it_a_members() {
        let agent = json!({ "kind": "devin", "model": "swe-2", "effort": "medium" });
        let env = [("HOME".to_owned(), "/h".to_owned())];
        let launch = DevinLaunch {
            id: "11111111-1111-4111-8111-111111111111",
            role: "worker",
            directory: Path::new("/work"),
            resume: Some("mild-coin"),
            message: None,
            agent: &agent,
            instructions: "# worker",
            cf: Path::new("/bundle/cf"),
            env: &env,
        };
        assert_eq!(
            launch.question(),
            json!({
                "op": "devin_launch",
                "launch": "11111111-1111-4111-8111-111111111111",
                "role": "worker",
                "directory": "/work",
                "resume": "mild-coin",
                "message": null,
                "agent": { "kind": "devin", "model": "swe-2", "effort": "medium" },
                "instructions": "# worker",
                "cf": "/bundle/cf",
                "env": { "HOME": "/h" },
            })
        );
    }

    #[test]
    fn the_adapter_is_asked_for_a_pi_members_launch_with_its_folder_and_its_thinking() {
        let agent = json!({ "kind": "pi", "model": "openrouter/x", "thinking": "low" });
        let env = [("HOME".to_owned(), "/h".to_owned())];
        let launch = PiLaunch {
            id: "11111111-1111-4111-8111-111111111111",
            role: "worker",
            directory: Path::new("/work"),
            resume: None,
            message: Some("the task"),
            agent: &agent,
            instructions: "# worker",
            env: &env,
        };
        assert_eq!(
            launch.question(),
            json!({
                "op": "pi_launch",
                "launch": "11111111-1111-4111-8111-111111111111",
                "role": "worker",
                "directory": "/work",
                "resume": null,
                "message": "the task",
                "agent": { "kind": "pi", "model": "openrouter/x", "thinking": "low" },
                "instructions": "# worker",
                "env": { "HOME": "/h" },
            })
        );
    }

    #[test]
    fn what_the_adapter_prepared_is_a_window_on_the_program_it_found() {
        let window = start_of(&json!({
            "command": "/h/.local/bin/devin",
            "args": ["--config", "/c/config.json", "--resume", "mild-coin"],
            "env": { "CHISEL_PURE_ACP_WIRE_LOG": "/c/wire.jsonl" },
            "dropEnv": [],
            "session": "mild-coin",
        }))
        .unwrap();
        assert_eq!(window.command, "/h/.local/bin/devin");
        assert_eq!(window.args[2..], ["--resume", "mild-coin"]);
        assert_eq!(
            window.env,
            [(
                "CHISEL_PURE_ACP_WIRE_LOG".to_owned(),
                "/c/wire.jsonl".to_owned()
            )]
        );
        assert!(window.drop_env.is_empty() && window.prompt.is_none());
    }
}
