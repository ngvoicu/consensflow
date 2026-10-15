//! A member's Devin window whole, asked of the adapter: `start` answers the flags
//! of a bare window, and the window the daemon opens for a member has more. Its
//! `--config` is the owner's with ConsensFlow's hooks, its first message is in a
//! prompt file, and its launch adds the variables that name its wire log and its
//! role. The adapter writes the launch's files as it prepares it, under the
//! ConsensFlow home the environment names.

use std::path::Path;

use serde_json::{json, Value};

use super::{env_object, question, start_of, Start};
use crate::{Error, Result};

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
