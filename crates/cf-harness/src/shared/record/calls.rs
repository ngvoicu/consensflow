//! A tool call as the one line a note can say of it (T-823): the last call a record shows is
//! what a window was doing when it closed under its task, which no screen of it says (kluris
//! T-153: a Claude Code worker's window closed with exit code 143, and its record showed the
//! `cargo test --workspace` the project's own tests had just killed).
//!
//! A shell tool's call is its command, as it was run; any other tool's, its name and its main
//! argument (a path, a pattern, a URL). The line is whole, on one line: the note that tells it
//! masks it and cuts it short.

use serde_json::Value;

/// The most characters a line keeps: a call's input can hold a file's whole text.
const LINE_CHARS: usize = 2_000;

/// The inputs that hold a command, in the order they are looked for.
const COMMANDS: [&str; 2] = ["command", "cmd"];

/// The inputs that hold what any other tool works on, in the order they are looked for.
const ARGUMENTS: [&str; 7] = [
    "file_path",
    "pattern",
    "url",
    "query",
    "path",
    "description",
    "prompt",
];

/// The line a call of tool `name` with `input` makes: none where the call names no tool and
/// holds no command. `input` is the object the harness recorded, text that holds one as JSON
/// (Codex writes a function call's `arguments` so), or text (a custom tool's input).
pub(crate) fn describe(name: Option<&str>, input: Option<&Value>) -> Option<String> {
    let parsed;
    let input = match input {
        Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
            Ok(object @ Value::Object(_)) => {
                parsed = object;
                Some(&parsed)
            }
            _ => input,
        },
        other => other,
    };
    let name = name.map(str::trim).filter(|name| !name.is_empty());
    let line = match input {
        Some(Value::Object(fields)) => {
            if let Some(command) = COMMANDS.iter().find_map(|key| command_of(fields.get(*key))) {
                command
            } else {
                let argument = ARGUMENTS.iter().find_map(|key| {
                    fields
                        .get(*key)
                        .and_then(Value::as_str)
                        .filter(|argument| !argument.trim().is_empty())
                });
                match (name, argument) {
                    (Some(name), Some(argument)) => format!("{name} {argument}"),
                    (Some(name), None) => name.to_owned(),
                    (None, _) => return None,
                }
            }
        }
        Some(Value::String(text)) if !text.trim().is_empty() => match name {
            Some(name) => format!("{name} {text}"),
            None => text.clone(),
        },
        _ => name?.to_owned(),
    };
    let one = line.split_whitespace().collect::<Vec<_>>().join(" ");
    (!one.is_empty()).then(|| one.chars().take(LINE_CHARS).collect())
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
    fn a_shell_call_is_its_command_and_any_other_its_name_and_main_argument() {
        let said = |name, input| describe(Some(name), Some(&input));
        assert_eq!(
            said("Bash", json!({ "command": "cargo fmt --check &&\n  cargo test --workspace", "timeout": 600000 })).as_deref(),
            Some("cargo fmt --check && cargo test --workspace")
        );
        assert_eq!(
            said("exec_command", json!({ "cmd": "ls -la", "workdir": "/w" })).as_deref(),
            Some("ls -la")
        );
        assert_eq!(
            said("Read", json!({ "file_path": "src/main.rs", "limit": 20 })).as_deref(),
            Some("Read src/main.rs")
        );
        assert_eq!(
            said("grep", json!({ "pattern": "fn main", "path": "src" })).as_deref(),
            Some("grep fn main")
        );
        assert_eq!(
            said("sleep", json!({ "duration_ms": 5000 })).as_deref(),
            Some("sleep")
        );
    }

    #[test]
    fn a_command_line_run_through_a_shell_is_its_script() {
        let said = |command: Value| describe(Some("shell"), Some(&json!({ "command": command })));
        assert_eq!(
            said(json!(["bash", "-lc", "cargo test -p cf-engine"])).as_deref(),
            Some("cargo test -p cf-engine")
        );
        assert_eq!(
            said(json!(["git", "status", "--short"])).as_deref(),
            Some("git status --short")
        );
    }

    #[test]
    fn input_written_as_json_text_or_as_plain_text_is_read_for_what_it_holds() {
        // Codex's function call: `arguments` is JSON text.
        assert_eq!(
            describe(
                Some("shell"),
                Some(&json!(r#"{"command":["bash","-lc","make test"]}"#))
            )
            .as_deref(),
            Some("make test")
        );
        // A custom tool's input is text.
        assert_eq!(
            describe(Some("exec"), Some(&json!("await tools.run('x')"))).as_deref(),
            Some("exec await tools.run('x')")
        );
        assert_eq!(describe(None, Some(&json!({ "limit": 3 }))), None);
        assert_eq!(describe(None, None), None);
        assert_eq!(describe(Some("  "), None), None);
    }

    #[test]
    fn a_line_is_bounded() {
        let long = "x".repeat(5_000);
        let line = describe(Some("Bash"), Some(&json!({ "command": long }))).unwrap();
        assert_eq!(line.chars().count(), LINE_CHARS);
    }
}
