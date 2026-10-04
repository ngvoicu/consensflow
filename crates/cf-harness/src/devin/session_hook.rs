//! Devin's SessionStart hook for one launch: a session of the conversation
//! the window shows starts with its role text. A subagent's session, another
//! conversation's, or any other event gets nothing; and nothing the hook
//! meets (no role text, no wire log, input that is no event) stops Devin.

use std::fs;
use std::io::Read;
use std::path::Path;

use cf_base::js;
use cf_base::json::{from_slice_lossy, js_order};
use serde_json::{json, Value};

use super::wire::selected_session;

/// The most of an event the hook reads; past it, the event is not read at all.
const SESSION_EVENT_LIMIT: u64 = 1024 * 1024;

/// What `cf hook devin-session` prints for the event on `input`: the role
/// text in `role_file`, as the context a session starts with, when the event
/// starts a session of the conversation `wire_log` says the window shows.
/// Devin reads a SessionStart hook's output whole: no line break after it.
pub fn session_hook(
    input: &mut dyn Read,
    role_file: Option<&Path>,
    wire_log: Option<&Path>,
) -> Option<String> {
    context(input, role_file, wire_log).map(|said| js_order(said).to_string())
}

/// The context `session_hook` gives, as JSON.
fn context(
    input: &mut dyn Read,
    role_file: Option<&Path>,
    wire_log: Option<&Path>,
) -> Option<Value> {
    let mut bytes = Vec::new();
    input
        .take(SESSION_EVENT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > SESSION_EVENT_LIMIT {
        return None;
    }
    let event = from_slice_lossy(&bytes).ok()?;
    let instructions = match role_file {
        Some(file) => String::from_utf8_lossy(&fs::read(file).ok()?).into_owned(),
        None => String::new(),
    };
    if event.get("hook_event_name").and_then(Value::as_str) != Some("SessionStart")
        || instructions.is_empty()
        || js::truthy(event.get("agent_id"))
        || js::truthy(event.get("parent_session_id"))
    {
        return None;
    }
    let session = event
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|id| session_id(id))?;
    if selected_session(wire_log?).ok()?.as_deref() != Some(session) {
        return None;
    }
    Some(json!({
        "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": instructions }
    }))
}

/// Whether `id` can be a Devin session's: letters, digits, `_` and `-`, 1 to 200 of them.
fn session_id(id: &str) -> bool {
    (1..=200).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devin::wire::tests::shows;

    struct Launch {
        _dir: tempfile::TempDir,
        wire: std::path::PathBuf,
        role: std::path::PathBuf,
    }

    /// One launch's files: the wire log showing native-a, and the role text.
    fn launch() -> Launch {
        let dir = tempfile::tempdir().unwrap();
        let wire = dir.path().join("wire.jsonl");
        let role = dir.path().join("SKILL.md");
        fs::write(&wire, shows("native-a")).unwrap();
        fs::write(&role, "# ConsensFlow worker\n").unwrap();
        Launch {
            _dir: dir,
            wire,
            role,
        }
    }

    fn run(launch: &Launch, event: &Value) -> Option<Value> {
        context(
            &mut event.to_string().as_bytes(),
            Some(&launch.role),
            Some(&launch.wire),
        )
    }

    fn event(name: &str, session: &str) -> Value {
        json!({ "hook_event_name": name, "session_id": session })
    }

    fn role() -> Value {
        json!({ "hookSpecificOutput": {
            "hookEventName": "SessionStart", "additionalContext": "# ConsensFlow worker\n" } })
    }

    #[test]
    fn a_session_of_the_shown_conversation_starts_with_its_role_text_whatever_its_source() {
        let launch = launch();
        for source in ["startup", "resume", "clear"] {
            let mut start = event("SessionStart", "native-a");
            start["source"] = source.into();
            assert_eq!(run(&launch, &start), Some(role()), "{source}");
        }
        for name in ["UserPromptSubmit", "Stop", "SessionEnd", "PreToolUse"] {
            assert_eq!(run(&launch, &event(name, "native-a")), None, "{name}");
        }
    }

    #[test]
    fn another_conversation_a_subagent_or_a_bad_id_gets_nothing() {
        let launch = launch();
        let mut subagent = event("SessionStart", "native-a");
        subagent["agent_id"] = "subagent-1".into();
        let mut child = event("SessionStart", "native-a");
        child["parent_session_id"] = "native-a".into();
        let mut blank_parent = event("SessionStart", "native-a");
        blank_parent["parent_session_id"] = "".into();
        assert_eq!(run(&launch, &event("SessionStart", "native-b")), None);
        assert_eq!(run(&launch, &subagent), None);
        assert_eq!(run(&launch, &child), None);
        assert_eq!(run(&launch, &event("SessionStart", "../bad")), None);
        assert_eq!(
            run(&launch, &blank_parent),
            Some(role()),
            "an empty parent is none"
        );
    }

    #[test]
    fn after_a_new_conversation_its_start_is_the_one_answered() {
        let launch = launch();
        let mut log = fs::read_to_string(&launch.wire).unwrap();
        log.push_str(&shows("native-b"));
        fs::write(&launch.wire, log).unwrap();
        assert_eq!(
            run(&launch, &event("SessionStart", "native-b")),
            Some(role())
        );
        assert_eq!(run(&launch, &event("SessionStart", "native-a")), None);
    }

    #[test]
    fn nothing_it_meets_stops_devin() {
        let launch = launch();
        let start = event("SessionStart", "native-a");
        let line = start.to_string();
        let missing = launch.role.with_file_name("gone.md");
        assert_eq!(
            context(&mut line.as_bytes(), None, Some(&launch.wire)),
            None
        );
        assert_eq!(
            context(&mut line.as_bytes(), Some(&missing), Some(&launch.wire)),
            None
        );
        assert_eq!(
            context(&mut line.as_bytes(), Some(&launch.role), None),
            None
        );
        assert_eq!(
            context(&mut line.as_bytes(), Some(&launch.role), Some(&missing)),
            None
        );
        assert_eq!(
            context(
                &mut &b"{\"hook_event_name\":"[..],
                Some(&launch.role),
                Some(&launch.wire)
            ),
            None
        );
        let mut huge = start.clone();
        huge["padding"] = "x".repeat(1024 * 1024).into();
        assert_eq!(
            run(&launch, &huge),
            None,
            "past a mebibyte the event is not read"
        );
        fs::write(&launch.role, "").unwrap();
        assert_eq!(
            run(&launch, &start),
            None,
            "an empty role text says nothing"
        );
    }
}
