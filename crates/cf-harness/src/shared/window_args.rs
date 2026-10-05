//! How each harness's own window opens on a conversation ConsensFlow starts
//! or resumes (`hosts/lib/windows.js`): the command, its arguments, the
//! first message where the harness takes it otherwise, and the keys it must
//! not inherit. Every adapter builds its pane from here.
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
    /// Devin's first message, which it takes in another way than as an
    /// argument.
    pub(crate) prompt: Option<String>,
    /// The keys whose presence would silently switch the window's billing.
    pub(crate) drop_env: &'static [&'static str],
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
                drop_env: &["ANTHROPIC_API_KEY"],
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
    }
    flags
}

#[cfg(test)]
mod tests;
