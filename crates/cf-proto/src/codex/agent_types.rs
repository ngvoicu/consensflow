//! Codex's own agent types: those a `spawn_agent` call may name with no configuration of the
//! owner's, each with the words Codex shows the model for it (the spawn tool lists every
//! type with its description). Codex 0.166.0 has three (`codex-rs/core/src/agent/role.rs`,
//! `built_in::configs`, at `rust-v0.166.0`): `default` and `worker` with no configuration of
//! their own, and `explorer` with `explorer.toml`, which is empty. None sets a model, an
//! effort, tools or instructions: a sub-agent of a built-in type starts from its parent's
//! configuration, the parent's developer instructions with it.
//!
//! So a window's sub-agent of one of these types, defined again with these words and a file
//! that sets its developer instructions alone (the supervisor's `-c agents.<type>.…`), starts
//! as Codex would start it in all but those instructions, and the spawn tool lists the types
//! as Codex lists them: the same words, and no model or effort said to be fixed. The record
//! of what 0.166.0 says is `codex-0.166.0-agent-types.json` beside this file, which a test
//! holds this table to; the live sub-agent suite holds it to the Codex the machine runs.

/// One of Codex's own agent types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentType {
    /// What a `spawn_agent` call names it (`agent_type`; none names `default`).
    pub name: &'static str,
    /// The words Codex shows the model for it, verbatim.
    pub description: &'static str,
}

/// The Codex whose agent types [`AGENT_TYPES`] are.
pub const AGENT_TYPES_OF: &str = "0.166.0";

/// Codex's own agent types, as Codex 0.166.0 defines them, in its order.
pub const AGENT_TYPES: [AgentType; 3] = [
    AgentType {
        name: "default",
        description: r#"Default agent."#,
    },
    AgentType {
        name: "explorer",
        description: r#"Use `explorer` for specific codebase questions.
Explorers are fast and authoritative.
They must be used to ask specific, well-scoped questions on the codebase.
Rules:
- In order to avoid redundant work, you should avoid exploring the same problem that explorers have already covered. Typically, you should trust the explorer results without additional verification. You are still allowed to inspect the code yourself to gain the needed context!
- You are encouraged to spawn up multiple explorers in parallel when you have multiple distinct questions to ask about the codebase that can be answered independently. This allows you to get more information faster without waiting for one question to finish before asking the next. While waiting for the explorer results, you can continue working on other local tasks that do not depend on those results. This parallelism is a key advantage of delegation, so use it whenever you have multiple questions to ask.
- Reuse existing explorers for related questions."#,
    },
    AgentType {
        name: "worker",
        description: r#"Use for execution and production work.
Typical tasks:
- Implement part of a feature
- Fix tests or bugs
- Split large refactors into independent chunks
Rules:
- Explicitly assign **ownership** of the task (files / responsibility). When the subtask involves code changes, you should clearly specify which files or modules the worker is responsible for. This helps avoid merge conflicts and ensures accountability. For example, you can say "Worker 1 is responsible for updating the authentication module, while Worker 2 will handle the database layer." By defining clear ownership, you can delegate more effectively and reduce coordination overhead.
- Always tell workers they are **not alone in the codebase**, and they should not revert the edits made by others, and they should adjust their implementation to accommodate the changes made by others. This is important because there may be multiple workers making changes in parallel, and they need to be aware of each other's work to avoid conflicts and ensure a cohesive final product."#,
    },
];

/// The agent type `name` is, where it is one of Codex's own.
pub fn agent_type(name: &str) -> Option<&'static AgentType> {
    AGENT_TYPES.iter().find(|kind| kind.name == name)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    /// What Codex 0.166.0's source says of its agent types, recorded from it.
    const RECORD: &str = include_str!("codex-0.166.0-agent-types.json");

    #[test]
    fn the_types_are_codex_0_166_0_s_own_word_for_word_and_set_nothing_else() {
        let record: Value = serde_json::from_str(RECORD).unwrap();
        assert_eq!(record["codex"], AGENT_TYPES_OF);
        let recorded = record["types"].as_array().unwrap();
        assert_eq!(recorded.len(), AGENT_TYPES.len());
        for (kind, recorded) in AGENT_TYPES.iter().zip(recorded) {
            assert_eq!(kind.name, recorded["name"], "{recorded}");
            assert_eq!(kind.description, recorded["description"], "{}", kind.name);
            // No configuration of its own sets anything: none, or an empty file.
            assert!(
                recorded["config_file"].is_null() || recorded["config_file_text"] == "",
                "{}: {recorded}",
                kind.name
            );
        }
        assert_eq!(agent_type("worker").map(|kind| kind.name), Some("worker"));
        assert_eq!(agent_type("awaiter"), None, "0.166.0 took it out");
    }
}
