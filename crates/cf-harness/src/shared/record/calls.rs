//! The last tool call a record shows, as what a note may say of it (T-823): what a window was
//! doing when it closed under its task, which no screen of it says (kluris T-153: a Claude
//! Code worker's window closed with exit code 143, and its record showed the `cargo test
//! --workspace` the project's own tests had just killed).
//!
//! A reader hands over the call as the record has it, in pieces: the tool, and what it was
//! given where that is a shell command or a path. It decides nothing of whether a note may say
//! it: the engine does, on the whole of it (`cf-engine`'s `shown/call.rs`), because a command
//! carries credentials in more shapes than a reader can list (`curl -u user:pass`, a URL's
//! user, a `--with-token`), and what cannot be told safe is not said (the review of
//! `a26e0848e`). A reader bounds what it keeps, and keeps nothing of a command it cannot keep
//! whole: a command cut before the engine looked is a command the engine never saw the end of.
//! What a tool was given as text of its own (a script, a prompt, a query, a URL) is not kept.

use std::sync::Arc;

use serde_json::Value;

/// The most characters of a shell command a call keeps. A longer one is kept as the tool alone.
const COMMAND_CHARS: usize = 4_096;

/// The most characters of a path a call keeps.
const TARGET_CHARS: usize = 512;

/// The inputs that hold a command, in the order they are looked for.
const COMMANDS: [&str; 2] = ["command", "cmd"];

/// The inputs that hold a file a tool works on. No pattern, URL, query or prompt: those hold
/// what a person typed or a model made up, and a secret can be any of it.
const TARGETS: [&str; 2] = ["file_path", "path"];

/// The last tool call a record shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastCall {
    /// The tool, as the record names it: empty where it names none.
    pub tool: String,
    /// The shell command it was given, whole, where it was given one that fits the bound.
    pub command: Option<String>,
    /// The file it works on (`file_path`, `path`), where it was given one.
    pub target: Option<String>,
}

impl LastCall {
    /// A call of `tool` that was given nothing a note may say. Calls are shared: a record is
    /// read again at every look, and a command can be 4 KB.
    pub fn tool(tool: &str) -> Arc<Self> {
        Arc::new(Self {
            tool: tool.to_owned(),
            command: None,
            target: None,
        })
    }

    /// A call of `tool` that was given the shell command `command`.
    pub fn shell(tool: &str, command: &str) -> Arc<Self> {
        Arc::new(Self {
            command: Some(command.to_owned()),
            tool: tool.to_owned(),
            target: None,
        })
    }

    /// A call of `tool` on the file `target`.
    pub fn on(tool: &str, target: &str) -> Arc<Self> {
        Arc::new(Self {
            target: Some(target.to_owned()),
            tool: tool.to_owned(),
            command: None,
        })
    }
}

/// The call of tool `name` with `input`: none where the call names no tool and holds no
/// command. `input` is the object the harness recorded, text that holds one as JSON (Codex
/// writes a function call's `arguments` so), or text (a custom tool's input), which is kept
/// as nothing.
pub(crate) fn describe(name: Option<&str>, input: Option<&Value>) -> Option<Arc<LastCall>> {
    let parsed;
    let input = match input {
        Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
            Ok(object @ Value::Object(_)) => {
                parsed = object;
                Some(&parsed)
            }
            _ => None,
        },
        other => other,
    };
    let tool = name.map(str::trim).unwrap_or_default();
    let (mut command, mut target) = (None, None);
    if let Some(Value::Object(fields)) = input {
        command = COMMANDS
            .iter()
            .find_map(|key| command_of(fields.get(*key)))
            .filter(|command| command.chars().count() <= COMMAND_CHARS);
        if command.is_none() {
            target = TARGETS
                .iter()
                .find_map(|key| fields.get(*key).and_then(Value::as_str))
                .map(str::trim)
                .filter(|target| !target.is_empty() && target.chars().count() <= TARGET_CHARS)
                .map(str::to_owned);
        }
    }
    (!tool.is_empty() || command.is_some()).then(|| {
        Arc::new(LastCall {
            tool: tool.to_owned(),
            command,
            target,
        })
    })
}

/// A command as an input holds it: text, or the words of a command line, which a shell run
/// with `-c` or `-lc` holds as its script alone.
fn command_of(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(command) if !command.trim().is_empty() => Some(command.clone()),
        Value::Array(words) => {
            let words: Vec<&str> = words.iter().filter_map(Value::as_str).collect();
            match words.as_slice() {
                [] => None,
                [_, flag, script] if flag.starts_with('-') && flag.ends_with('c') => {
                    Some((*script).to_owned())
                }
                _ => Some(words.join(" ")),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_shell_call_is_its_command_whole_and_any_other_its_tool_and_file() {
        let said = |name, input| describe(Some(name), Some(&input));
        assert_eq!(
            said(
                "Bash",
                json!({ "command": "cargo fmt --check &&\n  cargo test --workspace", "timeout": 600000 })
            ),
            Some(LastCall::shell(
                "Bash",
                "cargo fmt --check &&\n  cargo test --workspace"
            ))
        );
        assert_eq!(
            said("exec_command", json!({ "cmd": "ls -la", "workdir": "/w" })),
            Some(LastCall::shell("exec_command", "ls -la"))
        );
        assert_eq!(
            said("Read", json!({ "file_path": "src/main.rs", "limit": 20 })),
            Some(LastCall::on("Read", "src/main.rs"))
        );
        assert_eq!(
            said("read", json!({ "path": "src/lib.rs" })),
            Some(LastCall::on("read", "src/lib.rs"))
        );
        // A pattern, a URL, a query, a prompt: what someone typed is not kept.
        for input in [
            json!({ "pattern": "hunter2" }),
            json!({ "url": "https://user:pass@example.test/" }),
            json!({ "query": "my password is hunter2" }),
            json!({ "prompt": "use the key abcd" }),
            json!({ "duration_ms": 5000 }),
        ] {
            assert_eq!(
                said("tool", input.clone()),
                Some(LastCall::tool("tool")),
                "{input}"
            );
        }
    }

    #[test]
    fn a_command_line_run_through_a_shell_is_its_script() {
        let said = |command: Value| describe(Some("shell"), Some(&json!({ "command": command })));
        assert_eq!(
            said(json!(["bash", "-lc", "cargo test -p cf-engine"])),
            Some(LastCall::shell("shell", "cargo test -p cf-engine"))
        );
        assert_eq!(
            said(json!(["git", "status", "--short"])),
            Some(LastCall::shell("shell", "git status --short"))
        );
    }

    #[test]
    fn input_written_as_json_text_is_read_and_text_of_a_tools_own_is_kept_as_nothing() {
        // Codex's function call: `arguments` is JSON text.
        assert_eq!(
            describe(
                Some("shell"),
                Some(&json!(r#"{"command":["bash","-lc","make test"]}"#))
            ),
            Some(LastCall::shell("shell", "make test"))
        );
        // A custom tool's input is a script or a patch: its name alone.
        assert_eq!(
            describe(Some("exec"), Some(&json!("await tools.run('x')"))),
            Some(LastCall::tool("exec"))
        );
        assert_eq!(describe(None, Some(&json!({ "limit": 3 }))), None);
        assert_eq!(describe(None, None), None);
        assert_eq!(describe(Some("  "), None), None);
        // A command with no tool named is a call all the same.
        assert_eq!(
            describe(None, Some(&json!({ "command": "ls" }))),
            Some(LastCall::shell("", "ls"))
        );
    }

    #[test]
    fn a_command_too_long_to_keep_whole_is_not_kept_cut() {
        // Cut at the bound, a credential past it would never be looked at.
        let long = format!("cargo test {}", "x".repeat(5_000));
        assert_eq!(
            describe(Some("Bash"), Some(&json!({ "command": long }))),
            Some(LastCall::tool("Bash"))
        );
        let kept = "y".repeat(COMMAND_CHARS);
        assert_eq!(
            describe(Some("Bash"), Some(&json!({ "command": kept.clone() }))),
            Some(LastCall::shell("Bash", &kept))
        );
    }
}
