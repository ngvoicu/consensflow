//! What the history draws of a rollout's calls besides their outputs (chief-harness
//! CHF-15): a call's header, from what it was given (a command, `apply_patch`'s patch); how
//! it ended and how long it took, from what its output says (Codex's `metadata`, or the
//! lines a command's output opens with); and the change a patch made. And a reasoning's
//! summary, the only words of its thinking Codex keeps readable.

use std::sync::Arc;

use serde_json::Value;

use super::fields::field;
use crate::shared::record::drawn::{self, Action, Detail, Status};

/// What the history draws of a `function_call` or a `custom_tool_call` while it runs: its
/// tool's name and its header, from its `arguments` (JSON text) or, for `apply_patch`, the
/// patch it was given (`input`), and the change the patch makes.
pub(super) fn called(payload: Option<&Value>) -> Detail {
    let name = field(payload, "name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("tool");
    let patch = field(payload, "input")
        .and_then(Value::as_str)
        .filter(|_| name == "apply_patch");
    let script = field(payload, "input")
        .and_then(Value::as_str)
        .filter(|_| name == "exec");
    let (header, diff) = match (patch, script) {
        (Some(patch), _) => patched(patch),
        (None, Some(script)) => scripted(script),
        (None, None) => {
            let given = field(payload, "arguments").or_else(|| field(payload, "input"));
            (drawn::guess(name, given), None)
        }
    };
    Detail {
        diff,
        ..drawn::tool(name, header, Some(Status::Running))
    }
}

/// The header of a call of Codex's `exec`, which runs JavaScript that calls its tools (Codex
/// 0.166, as it wrote it live: `await tools.exec_command({cmd:"…", workdir:…})`, `await
/// tools.apply_patch(patch)` with the patch a string the script holds): the first command it
/// runs, or the patch it applies, and its change; else `exec`.
fn scripted(script: &str) -> (Arc<str>, Option<Arc<str>>) {
    if script.contains("apply_patch") {
        let unescaped = script.replace("\\n", "\n").replace("\\\"", "\"");
        if unescaped.contains("*** Update File: ") || unescaped.contains("*** Add File: ") {
            return patched(&unescaped);
        }
    }
    let command = ["cmd:", "\"cmd\":"].into_iter().find_map(|key| {
        let after = script.split_once(key)?.1.trim_start();
        let quoted = after.strip_prefix('"')?;
        let mut said = String::new();
        let mut chars = quoted.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(said),
                '\\' => match chars.next()? {
                    'n' => said.push('\n'),
                    't' => said.push('\t'),
                    other => said.push(other),
                },
                c => said.push(c),
            }
        }
        None
    });
    match command {
        Some(command) => (drawn::header(Action::Shell(&command)), None),
        None => (
            drawn::header(Action::Other {
                name: "exec",
                about: None,
            }),
            None,
        ),
    }
}

/// A patch's header (`edit` the first file it changes, `write` one it adds) and its change:
/// its hunks' lines, each file's after the line that names it.
fn patched(patch: &str) -> (Arc<str>, Option<Arc<str>>) {
    let mut header = None;
    let mut diff = Vec::new();
    for line in patch.lines() {
        if let Some(path) = line.strip_prefix("*** Update File: ") {
            header.get_or_insert(Action::Edit(path.trim()));
        } else if let Some(path) = line.strip_prefix("*** Add File: ") {
            header.get_or_insert(Action::Write(path.trim()));
        } else if line.starts_with("***") {
            continue;
        } else if line.starts_with(['@', '+', '-', ' ']) {
            diff.push(line);
        }
    }
    let header = drawn::header(header.unwrap_or(Action::Other {
        name: "apply_patch",
        about: None,
    }));
    (
        header,
        (!diff.is_empty()).then(|| Arc::from(diff.join("\n"))),
    )
}

/// How a call ended and how long it took, as its output (`output`) says: Codex's own
/// `metadata` (`exit_code`, `duration_seconds`), where the output is JSON that holds it;
/// else the lines a command's output opens with (`Process exited with code N`, `Exit
/// code: N`, `Wall time: S seconds`). An output that says neither is one that ended well.
pub(super) fn ended(output: Option<&Value>) -> (Status, Option<u64>) {
    let parsed;
    let output = match output {
        Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
            Ok(json @ Value::Object(_)) => {
                parsed = json;
                Some(&parsed)
            }
            _ => output,
        },
        other => other,
    };
    let metadata = field(output, "metadata");
    let code = field(metadata, "exit_code").and_then(Value::as_i64);
    let seconds = field(metadata, "duration_seconds").and_then(Value::as_f64);
    let text = match output {
        Some(Value::String(text)) => Some(text.as_str()),
        _ => field(output, "output").and_then(Value::as_str),
    };
    let said = |prefix: &str| {
        text.into_iter()
            .flat_map(str::lines)
            .take(8)
            .find_map(|line| line.trim().strip_prefix(prefix).map(str::trim))
    };
    let code = code.or_else(|| {
        said("Process exited with code ")
            .or_else(|| said("Exit code:"))
            .and_then(|code| code.parse().ok())
    });
    let seconds = seconds.or_else(|| {
        said("Wall time:").and_then(|time| time.trim_end_matches("seconds").trim().parse().ok())
    });
    let status = match code {
        Some(code) if code != 0 => Status::Failed,
        _ => Status::Ok,
    };
    (status, seconds.and_then(millis))
}

/// Seconds as whole milliseconds, none where they are no time.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn millis(seconds: f64) -> Option<u64> {
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1_000.0).round() as u64)
}

/// What a `CommandExecution` Codex completed says of itself: its header (`$` its command,
/// text or words), how it ended (`exit_code`, or a `status` of `failed`), and how long it
/// took (`duration`, `{secs, nanos}` as Codex 0.166 writes it, or `duration_ms`).
pub(super) fn command(native: Option<&Value>) -> Detail {
    let header = drawn::guess(
        "shell",
        Some(&serde_json::json!({ "command": field(native, "command") })),
    );
    let code = field(native, "exit_code").and_then(Value::as_i64);
    let failed = code.is_some_and(|code| code != 0)
        || field(native, "status").and_then(Value::as_str) == Some("failed");
    let duration = field(native, "duration");
    let took_ms = field(native, "duration_ms")
        .and_then(Value::as_u64)
        .or_else(|| {
            let secs = field(duration, "secs").and_then(Value::as_u64)?;
            let nanos = field(duration, "nanos")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            Some(secs.saturating_mul(1_000).saturating_add(nanos / 1_000_000))
        });
    Detail {
        took_ms,
        ..drawn::tool(
            "shell",
            header,
            Some(if failed { Status::Failed } else { Status::Ok }),
        )
    }
}

/// What a `FileChange` Codex completed says of itself (Codex 0.166, the edit an
/// `apply_patch` made): its header (`edit` the first file it changes, `write` one it adds),
/// how it ended (`status`), and the change (each file's `unified_diff`).
pub(super) fn file_change(native: Option<&Value>) -> Detail {
    let changes = match field(native, "changes") {
        Some(Value::Object(changes)) => changes.iter().collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let header = match changes.first() {
        Some((path, change))
            if field(Some(change), "type").and_then(Value::as_str) == Some("add") =>
        {
            drawn::header(Action::Write(path))
        }
        Some((path, _)) => drawn::header(Action::Edit(path)),
        None => drawn::header(Action::Other {
            name: "apply_patch",
            about: None,
        }),
    };
    let diff: Vec<&str> = changes
        .iter()
        .filter_map(|(_, change)| field(Some(change), "unified_diff").and_then(Value::as_str))
        .map(|diff| diff.trim_end_matches('\n'))
        .collect();
    let failed = field(native, "status").and_then(Value::as_str) == Some("failed");
    Detail {
        diff: (!diff.is_empty()).then(|| Arc::from(diff.join("\n"))),
        ..drawn::tool(
            "apply_patch",
            header,
            Some(if failed { Status::Failed } else { Status::Ok }),
        )
    }
}

/// What a `reasoning` item's summary says: its parts' texts, a paragraph each. None where
/// it says nothing (Codex keeps the rest of its thinking encrypted).
pub(super) fn reasoning(payload: Option<&Value>) -> Option<String> {
    let Some(Value::Array(parts)) = field(payload, "summary") else {
        return None;
    };
    let said: Vec<&str> = parts
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .filter(|text| !text.trim().is_empty())
        .collect();
    (!said.is_empty()).then(|| said.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_patch_is_headed_by_the_file_it_changes_and_its_change_is_its_hunks() {
        let detail = called(Some(&json!({
            "type": "custom_tool_call", "name": "apply_patch", "call_id": "c1",
            "input": "*** Begin Patch\n*** Update File: notes.txt\n@@\n-The light is red.\n+The light is blue.\n The door is shut.\n*** End Patch\n",
        })));
        assert_eq!(detail.header.as_deref(), Some("edit notes.txt"));
        assert_eq!(
            detail.diff.as_deref(),
            Some("@@\n-The light is red.\n+The light is blue.\n The door is shut.")
        );
        let added = called(Some(
            &json!({ "name": "apply_patch", "input": "*** Begin Patch\n*** Add File: b.md\n+hi\n*** End Patch" }),
        ));
        assert_eq!(added.header.as_deref(), Some("write b.md"));
    }

    #[test]
    fn a_script_of_codexs_exec_is_headed_by_the_command_it_runs_or_the_patch_it_applies() {
        // As Codex 0.166 wrote them live (T-728).
        let script = |input: &str| {
            called(Some(
                &json!({ "type": "custom_tool_call", "name": "exec", "input": input }),
            ))
        };
        let ran = script("const r = await tools.exec_command({cmd:\"ls no-such-folder\",workdir:\"/w\",yield_time_ms:10000}); text(r.output);\n");
        assert_eq!(ran.header.as_deref(), Some("$ ls no-such-folder"));
        assert_eq!(ran.name.as_deref(), Some("exec"));
        let patch = script("const patch = \"*** Begin Patch\\n*** Update File: /w/notes.txt\\n@@\\n-The light is red.\\n+The light is blue.\\n*** End Patch\";\ntext(await tools.apply_patch(patch));\n");
        assert_eq!(patch.header.as_deref(), Some("edit /w/notes.txt"));
        assert_eq!(
            patch.diff.as_deref(),
            Some("@@\n-The light is red.\n+The light is blue.")
        );
        let other = script("text(await tools.view_image({path: \"a.png\"}));");
        assert_eq!(other.header.as_deref(), Some("exec"));
    }

    #[test]
    fn a_file_change_says_the_file_it_changed_and_its_change() {
        let detail = file_change(Some(&json!({
            "type": "FileChange", "id": "exec-1", "status": "completed",
            "changes": { "/w/notes.txt": { "type": "update",
                "unified_diff": "@@ -1,2 +1,2 @@\n-The light is red.\n+The light is blue.\n The door is shut.\n" } },
        })));
        assert_eq!(detail.header.as_deref(), Some("edit /w/notes.txt"));
        assert_eq!(detail.status, Some(Status::Ok));
        assert_eq!(
            detail.diff.as_deref(),
            Some("@@ -1,2 +1,2 @@\n-The light is red.\n+The light is blue.\n The door is shut.")
        );
        let added = file_change(Some(
            &json!({ "status": "failed", "changes": { "b.md": { "type": "add" } } }),
        ));
        assert_eq!(
            (added.header.as_deref(), added.status),
            (Some("write b.md"), Some(Status::Failed))
        );
    }

    #[test]
    fn a_call_is_headed_by_what_its_arguments_give() {
        let detail = called(Some(&json!({
            "type": "function_call", "name": "exec_command", "call_id": "c1",
            "arguments": "{\"cmd\":\"ls no-such-folder\",\"workdir\":\"/w\"}",
        })));
        assert_eq!(detail.header.as_deref(), Some("$ ls no-such-folder"));
        assert_eq!(detail.status, Some(Status::Running));
        assert_eq!(detail.diff, None);
    }

    #[test]
    fn an_output_says_how_its_call_ended_and_how_long_it_took() {
        let said = |text: &str| ended(Some(&json!(text)));
        assert_eq!(
            said("Chunk ID: 1\nWall time: 0.0123 seconds\nProcess exited with code 1\nOutput:\nls: no-such-folder: No such file"),
            (Status::Failed, Some(12))
        );
        assert_eq!(
            said("Exit code: 0\nWall time: 1.5 seconds\nOutput:\nok"),
            (Status::Ok, Some(1_500))
        );
        assert_eq!(
            said(
                "{\"output\":\"Success\",\"metadata\":{\"exit_code\":0,\"duration_seconds\":0.2}}"
            ),
            (Status::Ok, Some(200))
        );
        assert_eq!(
            ended(Some(
                &json!({ "output": "x", "metadata": { "exit_code": 2 } })
            )),
            (Status::Failed, None)
        );
        // An output that says nothing of its end ended well, in no time known.
        assert_eq!(said("plain words"), (Status::Ok, None));
        // A line further down that says the words is the command's own output, not Codex's.
        let deep = format!("{}Process exited with code 1", "line\n".repeat(20));
        assert_eq!(said(&deep), (Status::Ok, None));
    }

    #[test]
    fn a_command_codex_completed_says_its_end_and_time() {
        let detail = command(Some(&json!({
            "type": "CommandExecution", "id": "x", "command": "ls nope", "exit_code": 1,
            "duration_ms": 40, "aggregated_output": "nope",
        })));
        assert_eq!(detail.header.as_deref(), Some("$ ls nope"));
        assert_eq!(
            (detail.status, detail.took_ms),
            (Some(Status::Failed), Some(40))
        );
        // Codex 0.166's own: its words and a duration of seconds and nanoseconds.
        let live = command(Some(
            &json!({ "command": ["/bin/zsh", "-c", "cat notes.txt"],
            "exit_code": 0, "duration": { "secs": 1, "nanos": 250_000_000 } }),
        ));
        assert_eq!(live.header.as_deref(), Some("$ cat notes.txt"));
        assert_eq!(live.took_ms, Some(1_250));
        let words = command(Some(
            &json!({ "command": ["bash", "-lc", "pwd"], "exit_code": 0 }),
        ));
        assert_eq!(words.header.as_deref(), Some("$ pwd"));
        assert_eq!(words.status, Some(Status::Ok));
    }

    #[test]
    fn a_reasonings_summary_is_its_words_and_an_empty_one_none() {
        assert_eq!(
            reasoning(Some(
                &json!({ "type": "reasoning", "summary": [{ "type": "summary_text", "text": "**Plan**" }, { "type": "summary_text", "text": "Read first." }] })
            )),
            Some("**Plan**\n\nRead first.".to_owned())
        );
        assert_eq!(
            reasoning(Some(&json!({ "type": "reasoning", "summary": [] }))),
            None
        );
        assert_eq!(
            reasoning(Some(
                &json!({ "type": "reasoning", "encrypted_content": "x" })
            )),
            None
        );
    }
}
