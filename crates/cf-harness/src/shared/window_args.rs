//! How each harness's own window opens on a conversation ConsensFlow starts or
//! resumes: the command, its arguments, the first message where the harness
//! takes it otherwise, and the keys it must not inherit. Every adapter builds
//! its pane from here.
//!
//! Every window, fresh or resumed, for every role, opens in full-permission
//! ("yolo") mode (the owner's decision, 2026-09-19). Each flag is the
//! harness's own documented one; Claude's settings file adds the mode's
//! companions. A resumed window opens on the agent's model and effort with
//! the flags a start uses: one without them ran on the harness's default.

use cf_proto::agents::Harness;

use crate::contract::Agent;

/// A harness's own window, as the pane opens it: its program and
/// arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) command: &'static str,
    pub(crate) args: Vec<String>,
    /// The first message of a harness that takes it in another way than as an
    /// argument: Devin's, in a prompt file, and a resumed Muse window's, pasted
    /// once it is up (`muse resume` takes no prompt).
    pub(crate) prompt: Option<String>,
    /// The keys whose presence would silently switch the window's billing.
    pub(crate) drop_env: &'static [&'static str],
}

// What the goldens and the live tools (`tooling`) read; the product reads none of it.
#[cfg(any(test, feature = "test-support"))]
impl Invocation {
    /// The window as Node's window code wrote it, which the goldens hold it to
    /// and the live tools read: its `env` always empty, `prompt` only where
    /// given.
    pub(crate) fn written(&self) -> serde_json::Value {
        use serde_json::{json, Map, Value};

        let mut fields = Map::new();
        fields.insert("command".to_owned(), json!(self.command));
        fields.insert("args".to_owned(), json!(self.args));
        if let Some(prompt) = &self.prompt {
            fields.insert("prompt".to_owned(), json!(prompt));
        }
        fields.insert("env".to_owned(), json!({}));
        fields.insert("dropEnv".to_owned(), json!(self.drop_env));
        Value::Object(fields)
    }
}

/// The window on a conversation that does not exist yet, `seed` its first
/// message: none where the harness needs an id and has none (Claude and Pi
/// take theirs from ConsensFlow).
pub(crate) fn start(
    harness: Harness,
    agent: Agent,
    session: Option<&str>,
    seed: Option<&str>,
) -> Option<Invocation> {
    window(harness, agent, given(session), given(seed), false)
}

/// The window on a conversation the harness already keeps, by the id it
/// recorded: none without one.
pub(crate) fn resume(
    harness: Harness,
    agent: Agent,
    session: Option<&str>,
    seed: Option<&str>,
) -> Option<Invocation> {
    let session = given(session)?;
    window(harness, agent, Some(session), given(seed), true)
}

/// A text JavaScript's `if (text)` takes: none for none and for empty.
fn given(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.is_empty())
}

fn window(
    harness: Harness,
    agent: Agent,
    session: Option<&str>,
    seed: Option<&str>,
    resumed: bool,
) -> Option<Invocation> {
    let seeded = |mut args: Vec<String>| {
        args.extend(seed.map(str::to_owned));
        args
    };
    let words = |words: &[&str]| {
        words
            .iter()
            .map(|&word| word.to_owned())
            .collect::<Vec<_>>()
    };
    Some(match harness {
        Harness::Claude => {
            let session = session?;
            let mut args = words(&[if resumed { "--resume" } else { "--session-id" }, session]);
            args.extend(model_and_effort(harness, agent));
            args.extend(words(&["--permission-mode", "bypassPermissions"]));
            Invocation {
                command: "claude",
                args: seeded(args),
                prompt: None,
                // Taken off whatever else of the human's shell the window keeps: a
                // leftover export would move the window off the human's subscription
                // login onto per-token API billing, with no word on the board.
                drop_env: &["ANTHROPIC_API_KEY"],
            }
        }
        // `muse [PROMPT]` opens the TUI with its first turn begun; `muse resume <id>`
        // takes no prompt (Muse 1.4.13: "unknown argument"), so a resumed window's goes
        // in once it is up. `--yolo` is the full-permission mode: no approval, no
        // sandbox, the workspace trusted for the run (not saved). Contributor mode, in
        // which Meta trains on what the window sees, is off for the run unless the
        // agent's model is a contributor model, which only the human picks.
        Harness::Muse => {
            let mut args = match (resumed, session) {
                (true, Some(session)) => words(&["resume", session]),
                _ => Vec::new(),
            };
            args.extend(model_and_effort(harness, agent));
            args.push("--yolo".to_owned());
            if !given(agent.model).is_some_and(|model| model.ends_with(CONTRIBUTOR)) {
                args.extend(words(&["--contributor-mode", "off"]));
            }
            let (args, prompt) = if resumed {
                (args, seed.map(str::to_owned))
            } else {
                (seeded(args), None)
            };
            Invocation {
                command: "muse",
                args,
                prompt,
                // As Claude's key: Muse uses a `META_API_KEY` in its environment before
                // the human's login (Muse's docs), which would move the window onto that
                // key's billing.
                drop_env: &["META_API_KEY"],
            }
        }
        // `--session-id` creates the session the first time and resumes it after.
        Harness::Pi => {
            let session = session?;
            let mut args = words(&["--session-id", session]);
            args.extend(model_and_effort(harness, agent));
            args.push("--approve".to_owned());
            Invocation {
                command: "pi",
                args: seeded(args),
                prompt: None,
                drop_env: &[],
            }
        }
        // `codex [PROMPT]` opens the real window seeded with that prompt; it
        // announces no id, so the broker names the thread once Codex starts it.
        Harness::Codex => {
            let mut args = match (resumed, session) {
                (true, Some(session)) => words(&["resume", session]),
                _ => Vec::new(),
            };
            args.extend(model_and_effort(harness, agent));
            args.push("--dangerously-bypass-approvals-and-sandbox".to_owned());
            Invocation {
                command: "codex",
                args: seeded(args),
                prompt: None,
                // As Claude's key: a leftover export would move the window off the
                // human's ChatGPT login onto per-token API billing.
                drop_env: &["OPENAI_API_KEY"],
            }
        }
        Harness::Devin => {
            let mut args = match (resumed, session) {
                (true, Some(session)) => words(&["--resume", session]),
                _ => Vec::new(),
            };
            args.extend(model_and_effort(harness, agent));
            args.extend(words(&[
                "--permission-mode",
                "dangerous",
                "--respect-workspace-trust",
                "false",
            ]));
            Invocation {
                command: "devin",
                args,
                prompt: seed.map(str::to_owned),
                drop_env: &[],
            }
        }
        // OpenCode keeps the model and its variant with the session, and
        // they are read back for a resumed one: only a new session is given
        // them. Its first message goes through its own API once it starts.
        Harness::Opencode => {
            let mut args = session.map_or_else(Vec::new, |session| words(&["--session", session]));
            if !resumed {
                args.extend(model_and_effort(harness, agent));
            }
            args.push("--auto".to_owned());
            Invocation {
                command: "opencode",
                args,
                prompt: None,
                drop_env: &[],
            }
        }
    })
}

/// How Muse names a model in its contributor mode, where Meta trains on what the
/// model sees: `muse-spark-1.3-contributor`.
const CONTRIBUTOR: &str = "-contributor";

/// The flags that put a window on its agent's model and effort.
fn model_and_effort(harness: Harness, agent: Agent) -> Vec<String> {
    let model = given(agent.model);
    let effort = given(agent.effort);
    let mut flags = Vec::new();
    let mut flag = |name: &str, value: String| {
        flags.push(name.to_owned());
        flags.push(value);
    };
    match harness {
        Harness::Claude => {
            if let Some(model) = model {
                flag("--model", model.to_owned());
            }
            if let Some(effort) = effort {
                flag("--effort", effort.to_owned());
            }
        }
        Harness::Codex => {
            if let Some(model) = model {
                flag("--model", model.to_owned());
            }
            if let Some(effort) = effort {
                flag("-c", format!("model_reasoning_effort=\"{effort}\""));
            }
        }
        Harness::Pi => {
            if let Some(model) = model {
                flag("--model", model.to_owned());
            }
            if let Some(thinking) = given(agent.thinking) {
                flag("--thinking", thinking.to_owned());
            }
        }
        // Devin writes the level into the id (claude-opus-5-5-max): an agent
        // names the family and its effort, joined here. No model is Devin's
        // own setting.
        Harness::Devin => {
            if let Some(model) = model {
                let id =
                    effort.map_or_else(|| model.to_owned(), |effort| format!("{model}-{effort}"));
                flag("--model", id);
            }
        }
        Harness::Opencode => {
            if let Some(model) = model {
                flag("--model", model.to_owned());
            }
        }
        Harness::Muse => {
            if let Some(model) = model {
                flag("--model", model.to_owned());
            }
            if let Some(effort) = effort {
                flag("--reasoning-effort", effort.to_owned());
            }
        }
    }
    flags
}

#[cfg(test)]
mod tests;
